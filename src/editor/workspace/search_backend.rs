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
    ///
    /// 文件名匹配不是 ripgrep 的活（那是 `-g` 的 glob），所以这仍是自己做的子串
    /// 比较；大小写不敏感那档用标准的 `to_lowercase` 折叠，不再是手写的滑窗。
    pub(crate) fn matches_filename(&self, name: &str) -> bool {
        if self.options.fuzzy && !self.options.use_regex {
            return !fuzzy_subsequence_ranges(name, &self.query).is_empty();
        }
        if self.options.match_case {
            name.contains(&self.query)
        } else {
            name.to_lowercase().contains(&self.query.to_lowercase())
        }
    }
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

/// 把走盘名单转成待扫文件，供并行分片使用。非文本文件（图片、二进制等）沿用侧栏
/// 树时代的语义：只匹配文件名，不读内容。
// 名单来自 `collect_workspace_files_on_disk`（与侧栏同一套过滤规则），不再从树上
// 收集：树只加载到默认层数，更深一层展开前不在里面。
pub(crate) fn workspace_search_files(root: &Path, files: &[PathBuf]) -> Vec<WorkspaceSearchFile> {
    files
        .iter()
        .map(|path| WorkspaceSearchFile {
            path: path.clone(),
            label: search_file_label(path, root),
            searchable: is_markdown_file(path) || is_code_file(path),
        })
        .collect()
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
}

/// 工作区内容缓存：条目按路径查，驱逐按插入序，总字节数即时记账。
///
/// 改前的写法是每次插入都把整表求和一遍、超限再线性找最久未用的条目——2000
/// 文件 / 约 140 MB 语料顶穿 128 MB 预算后，每插一条都要走一遍全表，热跑比冷跑
/// 慢 45%（缺陷 #8）。这里把这两处都换成 O(1)。
///
/// 驱逐顺序与改前实际一致：命中路径从不更新 `last_used`，所以那套「LRU」本来就
/// 等价于插入序，`order` 队列只是把这件事写明并省去全表扫描。
pub(crate) struct SearchContentCache {
    entries: HashMap<PathBuf, SearchContentCacheEntry>,
    /// 插入序队列，元素是 (路径, 那次插入记账的字节数)。同一路径被覆盖插入时，
    /// 旧队列项弹出时对不上当前条目的字节数，跳过即可——`total_bytes` 始终等于
    /// 表内条目字节之和，不会漂。
    order: std::collections::VecDeque<(PathBuf, usize)>,
    total_bytes: usize,
    max_bytes: usize,
}

impl SearchContentCache {
    fn new(max_bytes: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: std::collections::VecDeque::new(),
            total_bytes: 0,
            max_bytes,
        }
    }

    fn get(&self, path: &Path) -> Option<&SearchContentCacheEntry> {
        self.entries.get(path)
    }

    /// 收下这个条目；超单条预算就拒收，否则按插入序逐出最早的条目直到放得下。
    fn insert(&mut self, path: &Path, mtime: std::time::SystemTime, len: u64, contents: std::sync::Arc<str>) {
        let bytes = contents.len();
        if bytes > self.max_bytes {
            return;
        }
        while self.total_bytes + bytes > self.max_bytes {
            let Some((key, accounted)) = self.order.pop_front() else {
                return;
            };
            if self.entries.get(&key).map(|entry| entry.contents.len()) == Some(accounted) {
                self.entries.remove(&key);
                self.total_bytes -= accounted;
            }
        }
        if let Some(previous) = self.entries.insert(
            path.to_path_buf(),
            SearchContentCacheEntry { mtime, len, contents },
        ) {
            self.total_bytes -= previous.contents.len();
        }
        self.total_bytes += bytes;
        self.order.push_back((path.to_path_buf(), bytes));
    }
}

pub(crate) fn search_content_cache() -> &'static std::sync::Mutex<SearchContentCache> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<SearchContentCache>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        std::sync::Mutex::new(SearchContentCache::new(SEARCH_CACHE_MAX_BYTES))
    })
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
        // 总量记账在缓存内部：超预算就按插入序逐出，直到放得下。单文件上限
        // 20MB 远小于总上限，刚插入的条目不会被自己挤掉。
        cache.insert(path, mtime, len, contents.clone());
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
            match_range: Some(in_line_match_range(&hit.range, line_start, line_end)),
            source_range: None,
            match_ordinal: Some(ordinal),
            preview: source[line_start..line_end].trim().chars().take(140).collect(),
        });
    }
}

/// 工作区搜索：文件名单由调用方给（换根后走盘那份，见
/// `collect_workspace_files_on_disk`），按 CPU 核数分片在后台线程池并行扫描；结果
/// 按分片顺序合并保持顺序稳定。上一轮读过且未变更的文件内容直接命中缓存，不再
/// 逐个重读磁盘（用户报修：大工作区搜索远慢于 VS Code）。
pub(crate) async fn search_workspace_files(
    root: &Path,
    paths: &[PathBuf],
    matcher: &SearchMatcher,
    limit: usize,
    background: &gpui::BackgroundExecutor,
) -> Vec<WorkspaceSearchHit> {
    if matcher.is_empty() || limit == 0 || paths.is_empty() {
        return Vec::new();
    }
    let files = workspace_search_files(root, paths);
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
///
/// 生产侧不再需要它：文档范围的跳转读命中表按索引取，工作区命中按 ordinal 对位
/// 也读那张表。留在这里只服务阶段 0 那批把它当被测对象的行为快照。
#[cfg(test)]
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

#[cfg(test)]
mod tests {
    // 不写 `use super::*`：父模块 glob 了 `gpui::*`，其中的 `test` 属性宏会把内建
    // 的 `#[test]` 顶掉（展开成 gpui 的测试脚手架，直接撞编译递归上限）。
    use super::SearchContentCache;
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime};

    fn contents(bytes: usize) -> Arc<str> {
        Arc::from("x".repeat(bytes))
    }

    fn path_at(index: usize) -> PathBuf {
        PathBuf::from(format!("/corpus/file-{index}.md"))
    }

    /// 记账自证：`total_bytes` 必须始终等于表内条目字节之和。
    fn accounted(cache: &SearchContentCache) -> usize {
        cache.entries.values().map(|e| e.contents.len()).sum()
    }

    #[test]
    fn eviction_follows_insertion_order_and_keeps_the_byte_accounting_exact() {
        let mut cache = SearchContentCache::new(1_000);
        for index in 0..50 {
            cache.insert(&path_at(index), SystemTime::now(), 1, contents(120));
            assert_eq!(
                cache.total_bytes,
                accounted(&cache),
                "第 {index} 次插入后记账漂了"
            );
            assert!(cache.total_bytes <= 1_000, "超预算");
        }
        // 120 字节一条，预算最多容纳 8 条；最早插入的 42 条必须已被逐出。
        for index in 0..42 {
            assert!(
                cache.get(&path_at(index)).is_none(),
                "{index} 号最早插入，应当已被逐出"
            );
        }
        for index in 42..50 {
            assert!(cache.get(&path_at(index)).is_some(), "{index} 号还该在表里");
        }
    }

    #[test]
    fn rewriting_a_path_reaccounts_instead_of_double_counting() {
        let mut cache = SearchContentCache::new(10_000);
        cache.insert(&path_at(0), SystemTime::now(), 1, contents(4_000));
        cache.insert(&path_at(0), SystemTime::now(), 2, contents(1_000));
        assert_eq!(cache.total_bytes, 1_000, "同一路径覆盖后只按最新那份记账");
        assert_eq!(cache.entries.len(), 1);

        // 队列里那条 4000 字节的旧项此时是陈迹：弹出它对不上当前条目，必须跳过
        // 而不是减出负数。逼它出来。
        cache.insert(&path_at(1), SystemTime::now(), 1, contents(9_500));
        assert_eq!(cache.total_bytes, accounted(&cache));
        assert_eq!(cache.total_bytes, 9_500);
        assert!(cache.get(&path_at(0)).is_none(), "陈迹跳过后仍要逐出真正最早的那份");
    }

    #[test]
    fn a_single_file_bigger_than_the_budget_is_not_cached() {
        let mut cache = SearchContentCache::new(100);
        cache.insert(&path_at(0), SystemTime::now(), 1, contents(101));
        assert!(cache.entries.is_empty());
        assert!(cache.order.is_empty());
        assert_eq!(cache.total_bytes, 0);
    }

    /// 缺陷 #8 的账单：旧写法每插一条都要把整表求和一遍、超限再线性找最久未用，
    /// 条目数一多开销按平方长；新写法只从队头弹，按线性长。这里在同一进程里
    /// 把旧写法照抄一遍做 A/B，钉住这条不再退步。
    #[test]
    fn eviction_does_not_walk_the_whole_table_on_every_insert() {
        const ENTRIES: usize = 1_200;
        const ENTRY_BYTES: usize = 100;
        const BUDGET: usize = ENTRY_BYTES * 300;

        let mut best = Duration::MAX;
        for _ in 0..3 {
            let started = Instant::now();
            let mut cache = SearchContentCache::new(BUDGET);
            for index in 0..ENTRIES {
                cache.insert(&path_at(index), SystemTime::now(), 1, contents(ENTRY_BYTES));
            }
            let elapsed = started.elapsed();
            best = best.min(elapsed);
        }
        let new = best;

        struct OldEntry {
            contents: Arc<str>,
            last_used: Instant,
        }
        let started = Instant::now();
        let mut old: HashMap<PathBuf, OldEntry> = HashMap::new();
        for index in 0..ENTRIES {
            let contents = contents(ENTRY_BYTES);
            let inserted = contents.len();
            while old.len() * 4 > BUDGET
                || old.values()
                    .map(|entry| entry.contents.len())
                    .sum::<usize>()
                    > BUDGET - inserted
            {
                let oldest = old
                    .iter()
                    .min_by_key(|(_, entry)| entry.last_used)
                    .map(|(key, _)| key.clone());
                match oldest {
                    Some(key) => {
                        old.remove(&key);
                    }
                    None => break,
                }
            }
            old.insert(
                path_at(index),
                OldEntry {
                    contents,
                    last_used: Instant::now(),
                },
            );
        }
        let old = started.elapsed();

        eprintln!("[measure] 驱逐 1200 次：旧写法 {old:?}，新写法 {new:?}");
        assert!(
            new * 4 <= old,
            "新写法 {new:?} 应当远快于旧写法 {old:?}——每插一条不再全表扫描才算修住了缺陷 #8"
        );
    }
}
