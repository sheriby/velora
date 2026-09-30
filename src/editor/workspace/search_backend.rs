use super::*;

/// Match options for workspace/document search, mirroring VS Code's toggles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct SearchOptions {
    pub(super) match_case: bool,
    pub(super) whole_word: bool,
    pub(super) use_regex: bool,
    pub(super) fuzzy: bool,
}

/// Compiled search query. Regex compilation failures degrade to a plain
/// substring search so a bad pattern never silently kills search.
#[derive(Clone)]
pub(crate) struct SearchMatcher {
    query: String,
    options: SearchOptions,
    regex: Option<regex::Regex>,
}

impl SearchMatcher {
    pub(crate) fn new(query: &str, options: SearchOptions) -> Self {
        let regex = if options.use_regex {
            let mut builder = regex::RegexBuilder::new(query);
            builder.case_insensitive(!options.match_case);
            builder.build().ok()
        } else {
            None
        };
        Self {
            query: query.to_string(),
            options,
            regex,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.query.trim().is_empty() && self.regex.is_none()
    }

    /// Byte ranges of every match inside `line`.
    pub(crate) fn find_in_line(&self, line: &str) -> Vec<Range<usize>> {
        if let Some(regex) = self.regex.as_ref() {
            return regex
                .find_iter(line)
                .map(|m| m.start()..m.end())
                .collect();
        }
        if self.options.fuzzy {
            return fuzzy_subsequence_ranges(line, &self.query);
        }
        let mut ranges = if self.options.match_case {
            line.match_indices(&self.query)
                .map(|(start, matched)| start..start + matched.len())
                .collect()
        } else {
            case_insensitive_ranges(line, &self.query)
        };
        if self.options.whole_word {
            ranges.retain(|range| is_word_boundary(line, range));
        }
        ranges
    }

    /// Whether a filename matches (fuzzy subsequence or substring).
    pub(crate) fn matches_filename(&self, name: &str) -> bool {
        if self.options.fuzzy {
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
        let mut query_index = 0usize;
        let mut cursor = start_index;
        while cursor < positions.len() && query_index < query_chars.len() {
            let (_, line_char) = positions[cursor];
            let folded = line_char.to_lowercase().next().unwrap_or(line_char);
            if folded == query_chars[query_index] {
                query_index += 1;
            }
            cursor += 1;
        }
        if query_index == query_chars.len() {
            let start = positions[start_index].0;
            let end = positions
                .get(cursor)
                .map(|(offset, _)| *offset)
                .unwrap_or(line.len());
            ranges.push(start..end);
        }
    }
    ranges
}

pub(crate) fn is_word_boundary(line: &str, range: &Range<usize>) -> bool {
    let word_char = |ch: char| ch.is_alphanumeric() || ch == '_';
    let before = line[..range.start]
        .chars()
        .next_back()
        .map(word_char)
        .unwrap_or(false);
    let after = line[range.end..]
        .chars()
        .next()
        .map(word_char)
        .unwrap_or(false);
    !before && !after
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

/// 读取文件内容用于搜索：命中缓存（mtime 未变）零拷贝返回；未命中读盘一次
/// 并入缓存。非 UTF-8 文件返回 None（跳过内容搜索）。
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
    let contents: std::sync::Arc<str> = match String::from_utf8(bytes) {
        Ok(text) => std::sync::Arc::from(text),
        Err(_) => return None,
    };

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
    let mut file_hits = 0;
    for (index, raw_line) in source.split_inclusive('\n').enumerate() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        let matches = matcher.find_in_line(line);
        if let Some(first) = matches.first() {
            hits.push(WorkspaceSearchHit {
                path: file.path.clone(),
                label: file.label.clone(),
                line: Some(index + 1),
                match_range: Some(first.clone()),
                source_range: None,
                preview: line.trim().chars().take(140).collect(),
            });
            file_hits += 1;
            if file_hits == 3 || hits.len() >= limit {
                break;
            }
        }
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
    let mut hits = Vec::new();
    let mut absolute = 0usize;
    for (line_index, raw_line) in source.split_inclusive('\n').enumerate() {
        let line = raw_line.strip_suffix('\n').unwrap_or(raw_line);
        for range in matcher.find_in_line(line) {
            hits.push(WorkspaceSearchHit {
                path: path.to_path_buf(),
                label: label.to_string(),
                line: Some(line_index + 1),
                match_range: Some(range.start..range.end),
                source_range: Some(absolute + range.start..absolute + range.end),
                preview: line.trim().chars().take(140).collect(),
            });
            if hits.len() == limit {
                return hits;
            }
        }
        absolute += raw_line.len();
    }
    hits
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
