use super::*;

/// Match options for workspace/document search, mirroring VS Code's toggles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchOptions {
    pub(super) match_case: bool,
    pub(super) whole_word: bool,
    pub(super) use_regex: bool,
    pub(super) fuzzy: bool,
}

/// 一次搜索的编译结果。匹配本身交给 ripgrep 引擎（`search_engine::CompiledQuery`），
/// 这个类型只保留调用方惯用的「一次喂一行」的接口，并把编译诊断带出来给面板显示。
#[derive(Clone)]
pub(crate) struct SearchMatcher {
    query: String,
    options: SearchOptions,
    /// 编译失败时为 `None`：此时一切搜索都交回空结果，由 `error_message` 说明原因。
    engine: Option<CompiledQuery>,
    error: Option<QueryError>,
}

impl SearchMatcher {
    pub(crate) fn new(query: &str, options: SearchOptions) -> Self {
        let (engine, error) = match CompiledQuery::compile(query, options) {
            Ok(engine) => (Some(engine), None),
            Err(error) => (None, Some(error)),
        };
        Self {
            query: query.to_string(),
            options,
            engine,
            error,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.query.trim().is_empty()
    }

    /// 模式编译失败时引擎给的诊断原文（带出错列的 `^` 定位）。
    /// 调用方把它显示在搜索框下方，而不是继续搜出误导性的结果。
    pub(crate) fn error_message(&self) -> Option<&str> {
        self.error.as_ref().map(|error| error.message.as_str())
    }

    /// 一行内的全部命中字节区间。
    pub(crate) fn find_in_line(&self, line: &str) -> Vec<Range<usize>> {
        match self.engine.as_ref() {
            Some(engine) => engine.find_in_line(line),
            None => Vec::new(),
        }
    }

    /// 整段文本里的全部命中（绝对字节区间 + 行号）。编译失败时交回空表。
    #[cfg(test)]
    pub(crate) fn find_all_in_text(&self, source: &str) -> Vec<SearchHit> {
        match self.engine.as_ref() {
            Some(engine) => engine.find_all(source.as_bytes()),
            None => Vec::new(),
        }
    }

    /// 每根含命中的行只留第一个命中，最多 `max_lines` 行，收满引擎就停止读取。
    /// 工作区结果列表的「一行一条」口径走这里（相当于 `rg -m`）。
    pub(crate) fn find_first_line_hits(&self, source: &str, max_lines: usize) -> Vec<SearchHit> {
        match self.engine.as_ref() {
            Some(engine) => engine.find_first_per_line(source.as_bytes(), max_lines),
            None => Vec::new(),
        }
    }

    /// Whether a filename matches (fuzzy subsequence or substring).
    pub(crate) fn matches_filename(&self, name: &str) -> bool {
        if self.options.fuzzy && !self.options.use_regex {
            return !fuzzy_subsequence_ranges(name, &self.query).is_empty();
        }
        if self.options.match_case {
            name.contains(&self.query)
        } else {
            case_insensitive_contains(name, &self.query)
        }
    }
}

pub(crate) fn case_insensitive_contains(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if needle.is_ascii() {
        haystack
            .as_bytes()
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
    } else {
        !case_insensitive_ranges(haystack, needle).is_empty()
    }
}

/// Case-insensitive byte ranges for one line. ASCII needles use a fast
/// sliding compare; non-ASCII needles fall back to per-char lowercase
/// comparison (haystack byte offsets stay stable because lowercase folding
/// of a char never splits the position bookkeeping below).
pub(crate) fn case_insensitive_ranges(line: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() || line.is_empty() || line.len() < query.len() {
        return Vec::new();
    }
    if query.is_ascii() {
        let mut ranges = Vec::new();
        let last = line.len() - query.len();
        let bytes = line.as_bytes();
        let first = query.as_bytes()[0];
        let mut start = 0;
        while start <= last {
            // 先比对首字节再展开整窗：不命中位置只做一次单字节大小写不敏感
            // 比较，避免每个位置都比完整窗口。
            if bytes[start].eq_ignore_ascii_case(&first)
                && bytes[start..start + query.len()].eq_ignore_ascii_case(query.as_bytes())
            {
                ranges.push(start..start + query.len());
                start += query.len();
            } else {
                start += 1;
            }
        }
        return ranges;
    }

    let query_chars: Vec<char> = query.to_lowercase().chars().collect();
    if query_chars.is_empty() {
        return Vec::new();
    }
    let mut ranges = Vec::new();
    let char_positions: Vec<(usize, char)> = line.char_indices().collect();
    for start_index in 0..char_positions.len() {
        let mut query_index = 0usize;
        let mut cursor = start_index;
        while cursor < char_positions.len() && query_index < query_chars.len() {
            let (_, line_char) = char_positions[cursor];
            let mut folded = line_char.to_lowercase();
            let matches = match (folded.next(), folded.next()) {
                (Some(first), None) => first == query_chars[query_index],
                _ => line_char == query_chars[query_index],
            };
            if !matches {
                break;
            }
            query_index += 1;
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let start = char_positions[start_index].0;
            let end = if cursor < char_positions.len() {
                char_positions[cursor].0
            } else {
                line.len()
            };
            ranges.push(start..end);
        }
    }
    ranges
}

/// fzf-style subsequence match: every query char must appear in order
/// (case-insensitive); the returned range spans the first to last consumed
/// char so the hit can be selected on jump.
pub(crate) fn fuzzy_subsequence_ranges(line: &str, query: &str) -> Vec<Range<usize>> {
    let query_chars: Vec<char> = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .flat_map(|ch| ch.to_lowercase())
        .collect();
    if query_chars.is_empty() || line.is_empty() {
        return Vec::new();
    }
    let positions: Vec<(usize, char)> = line.char_indices().collect();
    let mut ranges = Vec::new();
    for start_index in 0..positions.len() {
        // 起点必须真的命中查询首字符：以前从这里往后扫到能补全子序列就算命中，
        // 区间会从没参与匹配的字符开始（用户报修：模糊模式下高亮落在命中词
        // 前面的字上）。
        let (start_offset, start_char) = positions[start_index];
        let folded_start = start_char.to_lowercase().next().unwrap_or(start_char);
        if folded_start != query_chars[0] {
            continue;
        }
        let mut query_index = 1usize;
        let mut cursor = start_index + 1;
        while cursor < positions.len() && query_index < query_chars.len() {
            let (_, line_char) = positions[cursor];
            let folded = line_char.to_lowercase().next().unwrap_or(line_char);
            if folded == query_chars[query_index] {
                query_index += 1;
            }
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let end = positions
                .get(cursor)
                .map(|(offset, _)| *offset)
                .unwrap_or(line.len());
            ranges.push(start_offset..end);
        }
    }
    ranges
}

/// 工作区搜索的待扫文件（树序）。`searchable` 为 false 的文件（非文本）只
/// 匹配文件名，不读内容。
#[derive(Clone)]
pub(crate) struct WorkspaceSearchFile {
    pub(super) path: PathBuf,
    pub(super) label: String,
    pub(super) searchable: bool,
}

/// 按树序收集待搜索文件，供并行分片使用。
pub(crate) fn collect_workspace_search_files(root: &WorkspaceTreeNode) -> Vec<WorkspaceSearchFile> {
    let root_path = match &root.kind {
        WorkspaceTreeKind::Directory(path) => path.as_path(),
        _ => return Vec::new(),
    };
    let mut files = Vec::new();
    pub(crate) fn visit(node: &WorkspaceTreeNode, root: &Path, files: &mut Vec<WorkspaceSearchFile>) {
        match &node.kind {
            WorkspaceTreeKind::Directory(_) => {
                for child in &node.children {
                    visit(child, root, files);
                }
            }
            WorkspaceTreeKind::MarkdownFile(path) | WorkspaceTreeKind::CodeFile(path) => {
                files.push(WorkspaceSearchFile {
                    path: path.clone(),
                    label: search_file_label(path, root),
                    searchable: true,
                });
            }
            WorkspaceTreeKind::OtherFile(path) => {
                files.push(WorkspaceSearchFile {
                    path: path.clone(),
                    label: search_file_label(path, root),
                    searchable: false,
                });
            }
            WorkspaceTreeKind::Heading { .. } => {}
        }
    }
    visit(root, root_path, &mut files);
    files
}

pub(crate) fn search_file_label(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

/// 文件内容缓存：上一轮搜过的文件本轮直接走内存，只付一次 stat 的代价校验
/// 是否过期（用户报修：工作区变大后每敲一键都全树重读磁盘，比 VS Code 慢得
/// 多）。容量超限时按最久未用驱逐。
const SEARCH_CACHE_MAX_BYTES: usize = 128 * 1024 * 1024;
const SEARCH_CACHE_MAX_FILE_BYTES: u64 = 20_000_000;

pub(crate) struct SearchContentCacheEntry {
    mtime: std::time::SystemTime,
    /// 文件长度：mtime 粒度可能粗到秒，同秒内的改写只能靠长度变
    /// 化发现（两个都同就认了，属于极端情况）。
    len: u64,
    contents: std::sync::Arc<str>,
    last_used: std::time::Instant,
}

pub(crate) fn search_content_cache() -> &'static std::sync::Mutex<
    HashMap<PathBuf, SearchContentCacheEntry>,
> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<HashMap<PathBuf, SearchContentCacheEntry>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// 读取文件内容用于搜索：命中缓存（mtime 与长度都没变）零拷贝返回；未命中读盘
/// 一次并入缓存。解码走 `decode_document_bytes`——UTF-8 原样、GB18030 回退、
/// 都不行才 lossy 兜底，与文档缓冲区拿到文本的方式**同一份实现**。
pub(crate) fn cached_file_source(path: &Path) -> Option<std::sync::Arc<str>> {
    let metadata = fs::metadata(path).ok()?;
    if metadata.len() > SEARCH_CACHE_MAX_FILE_BYTES {
        return None;
    }
    let mtime = metadata.modified().ok()?;
    let len = metadata.len();

    {
        let cache = search_content_cache();
        let Ok(cache) = cache.lock() else {
            return None;
        };
        if let Some(entry) = cache.get(path) {
            if entry.mtime == mtime && entry.len == len {
                return Some(entry.contents.clone());
            }
        }
    }

    // 读盘不持锁：并行分片时不能让一把缓存锁把所有 worker 串行化。
    let bytes = fs::read(path).ok()?;
    // 与打开文档同一个解码函数。此前这里是 `String::from_utf8(bytes)` 失败就
    // `return None`，整个文件跳过内容搜索——中文 Windows 上的 GBK/GB18030 笔记
    // 因此永远搜不到正文（缺陷 #5）。缓存里存的仍是解码后的 UTF-8 文本，
    // 所以后续的字节偏移一律按解码文本算。
    let contents: std::sync::Arc<str> =
        std::sync::Arc::from(crate::editor::encoding::decode_document_bytes(bytes));

    if let Ok(mut cache) = search_content_cache().lock() {
        if let Some(existing) = cache.get(path) {
            if existing.mtime == mtime && existing.len == len {
                return Some(existing.contents.clone());
            }
        }
        // 粗粒度总量记账：超限就把最久未用的条目逐出，直到放得下。单文件
        // 上限 20MB 远小于总上限，刚插入的条目不会被自己挤掉。
        let inserted = contents.len();
        if inserted <= SEARCH_CACHE_MAX_BYTES {
            while cache.len() * 4 > SEARCH_CACHE_MAX_BYTES
                || cache.values().map(|entry| entry.contents.len()).sum::<usize>()
                    > SEARCH_CACHE_MAX_BYTES - inserted
            {
                let oldest = cache
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(key, _)| key.clone());
                match oldest {
                    Some(key) => {
                        cache.remove(&key);
                    }
                    None => break,
                }
            }
            cache.insert(
                path.to_path_buf(),
                SearchContentCacheEntry {
                    mtime,
                    len,
                    contents: contents.clone(),
                    last_used: std::time::Instant::now(),
                },
            );
        }
    }
    Some(contents)
}

/// 单文件搜索：文件名匹配 +（文本文件的）内容匹配。与旧版逐文件逻辑一致。
pub(crate) fn search_single_file(
    file: &WorkspaceSearchFile,
    matcher: &SearchMatcher,
    limit: usize,
    hits: &mut Vec<WorkspaceSearchHit>,
) {
    if hits.len() >= limit {
        return;
    }
    if matcher.matches_filename(&file.label) {
        hits.push(WorkspaceSearchHit {
            path: file.path.clone(),
            label: file.label.clone(),
            line: None,
            match_range: None,
            source_range: None,
            match_ordinal: None,
            preview: String::new(),
        });
        if hits.len() >= limit {
            return;
        }
    }
    if !file.searchable {
        return;
    }
    let Some(source) = cached_file_source(&file.path) else {
        return;
    };
    // 行切分、字节偏移记账、模式编译与匹配全部交给 ripgrep；这里只把它交回的
    // 「每根含命中行的第一个命中」换算成侧栏一行（行内偏移 + 预览）。
    // `remaining` 是本轮还剩多少条位置：每文件全量收集（此前硬编码 3 条，用户
    // 报修「结果不全」），只受全局 limit 约束——收满引擎就停止读文件剩余部分。
    let remaining = limit - hits.len();
    let file_hits = matcher.find_first_line_hits(&source, remaining);
    for (ordinal, hit) in file_hits.iter().enumerate() {
        let (line_start, line_end) = hit_line_bounds(&source, &hit.range);
        let line_number = hit
            .line
            .unwrap_or_else(|| source[..line_start].matches('\n').count() as u64 + 1);
        hits.push(WorkspaceSearchHit {
            path: file.path.clone(),
            label: file.label.clone(),
            line: Some(line_number as usize),
            match_range: Some((hit.range.start - line_start)..(hit.range.end - line_start)),
            source_range: None,
            match_ordinal: Some(ordinal),
            preview: source[line_start..line_end].trim().chars().take(140).collect(),
        });
    }
}

/// 工作区搜索：文件列表按 CPU 核数分片，在后台线程池并行扫描；结果按分片
/// 顺序合并保持树序稳定。上一轮读过且未变更的文件内容直接命中缓存，不再
/// 逐个重读磁盘（用户报修：大工作区搜索远慢于 VS Code）。
pub(crate) async fn search_workspace_files(
    root: &WorkspaceTreeNode,
    matcher: &SearchMatcher,
    limit: usize,
    background: &gpui::BackgroundExecutor,
) -> Vec<WorkspaceSearchHit> {
    if matcher.is_empty() || limit == 0 {
        return Vec::new();
    }
    let files = collect_workspace_search_files(root);
    if files.is_empty() {
        return Vec::new();
    }
    let workers = std::thread::available_parallelism()
        .map(|parallelism| parallelism.get())
        .unwrap_or(4)
        .min(files.len());
    let chunk_size = files.len().div_ceil(workers);
    let matcher = std::sync::Arc::new(matcher.clone());
    let mut tasks = Vec::new();
    for chunk in files.chunks(chunk_size) {
        let chunk = chunk.to_vec();
        let matcher = matcher.clone();
        tasks.push(background.spawn(async move {
            // 分片体是同步扫描，panic 隔离在这里完成（一个分片炸掉只丢自己的
            // 结果，降级为"无结果"而不是拖垮整个搜索）。
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut hits = Vec::new();
                for file in &chunk {
                    search_single_file(file, &matcher, limit, &mut hits);
                }
                hits
            }))
            .unwrap_or_default()
        }));
    }
    let mut hits = Vec::new();
    for task in tasks {
        if hits.len() >= limit {
            break;
        }
        hits.extend(task.await);
    }
    hits.truncate(limit);
    hits
}

/// 扫一遍文档文本并投影成结果列表的行。
///
/// 投影本身是 `document_matches::project_hits_into_rows`——侧栏结果列表走的是
/// 同一份实现，所以这里不可能和命中表算出两样东西。
///
/// 生产侧不再需要它：文档范围的扫描已经并进 `Editor::document_matches` 那张表
/// （调度器直接要表再投影）。留在这里只服务那批把「结果列表」当被测对象的行为
/// 快照与测试。
#[cfg(test)]
pub(crate) fn search_document_source(
    source: &str,
    matcher: &SearchMatcher,
    path: &Path,
    label: &str,
    limit: usize,
) -> Vec<WorkspaceSearchHit> {
    if matcher.is_empty() || limit == 0 {
        return Vec::new();
    }
    project_hits_into_rows(&matcher.find_all_in_text(source), source, path, label, limit)
}

/// Next match at or after `from` (or before, when reversing) across the whole
/// document source, wrapping around once.
pub(crate) fn find_document_match_from(
    source: &str,
    matcher: &SearchMatcher,
    from: usize,
    reverse: bool,
) -> Option<Range<usize>> {
    if matcher.is_empty() {
        return None;
    }
    let mut from = from.min(source.len());
    while !source.is_char_boundary(from) {
        from -= 1;
    }

    let mut ranges = Vec::new();
    let mut absolute = 0usize;
    for raw_line in source.split_inclusive('\n') {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        for range in matcher.find_in_line(line) {
            ranges.push(absolute + range.start..absolute + range.end);
        }
        absolute += raw_line.len();
    }
    if ranges.is_empty() {
        return None;
    }
    if reverse {
        ranges
            .iter()
            .rev()
            .find(|range| range.start < from)
            .or_else(|| ranges.last())
            .cloned()
    } else {
        ranges
            .iter()
            .find(|range| range.start >= from)
            .or_else(|| ranges.first())
            .cloned()
    }
}

/// Detects a UTF-16 BOM (LE or BE). Such files are text, but the editor only
/// renders UTF-8 — they get a specific placeholder instead of a parse error
/// (roadmap G2).
pub(crate) fn has_utf16_bom(path: &Path) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut head = [0u8; 2];
    match file.read_exact(&mut head) {
        Ok(()) => head == [0xFF, 0xFE] || head == [0xFE, 0xFF],
        Err(_) => false,
    }
}

/// Heuristic text detection (same shape as Git's `is_text`): read up to the
/// first 8 KiB and treat the file as text when it decodes as UTF-8 (lossy
/// covers Latin-1-ish legacy files) and contains no NUL byte — the signature
/// of binary formats.
pub(crate) fn is_likely_text_file(path: &Path) -> bool {
    use std::io::Read;
    let mut file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(_) => return false,
    };
    let mut head = [0u8; 8192];
    let mut read = 0usize;
    loop {
        match file.read(&mut head[read..]) {
            Ok(0) => break,
            Ok(n) => {
                read += n;
                if read == head.len() {
                    break;
                }
            }
            Err(_) => return false,
        }
    }
    let head = &head[..read];
    // UTF-16 BOMs are text but not UTF-8; treat them as previewable anyway
    // since the editor renders UTF-8 only.
    if read >= 2 && (head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF])) {
        return true;
    }
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        // A read window that ends inside a multi-byte character is still a text
        // prefix: `error_len() == None` means the only problem is that the
        // window cut the last character (an 8 KiB window over CJK text hits
        // this constantly). Treating it as binary hid whole documents behind
        // the "can't preview" notice.
        Err(error) => error.error_len().is_none(),
    }
}

pub(crate) fn is_code_file(path: &Path) -> bool {
    const CODE_EXTENSIONS: &[&str] = &[
        "c", "cc", "cpp", "cs", "css", "go", "h", "hpp", "html", "java", "js", "json", "jsx", "kt",
        "php", "py", "rb", "rs", "sh", "sql", "swift", "toml", "ts", "tsx", "xml", "yaml", "yml",
        "zsh", "txt", "csv", "log", "ini", "conf", "lock",
    ];

    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        CODE_EXTENSIONS
            .iter()
            .any(|candidate| extension.eq_ignore_ascii_case(candidate))
    })
}
