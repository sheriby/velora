//! 手动工作区搜索性能验证（搜索性能优化）。
//!
//! 运行：`VELORA_SEARCH_BENCH=1 cargo test --bin velora \
//!        workspace_search_cache_bench -- --ignored --nocapture`

use std::fs;
use std::time::Instant;

use super::{SearchMatcher, SearchOptions, collect_workspace_search_files, search_single_file, scan_workspace_dir, TreeSortPreference};

/// 2000 个文件、约 70KB/个。冷启动 = 全量读盘；热启动 = 内容缓存全命中。
/// 生产路径（并行分片）由 workspace_search_matches_file_names_and_contents
/// 等测试覆盖，这里内联执行同样的分片体以单独量出缓存收益。
#[test]
#[ignore = "手动性能验证；设置 VELORA_SEARCH_BENCH 后单独运行"]
fn workspace_search_cache_bench() {
    if std::env::var("VELORA_SEARCH_BENCH").is_err() {
        return;
    }
    let root = std::env::temp_dir().join(format!("velora-search-bench-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(root.join("sub")).unwrap();
    let body = "lorem ipsum dolor sit amet consectetur adipiscing elit sed do eiusmod ".repeat(1200);
    for index in 0..2000 {
        let mut content = body.clone();
        if index % 2 == 0 {
            content.push_str("needle_here\n");
        }
        let dir = if index % 3 == 0 { root.join("sub") } else { root.clone() };
        fs::write(dir.join(format!("file-{index}.md")), content).unwrap();
    }
    let tree = scan_workspace_dir(&root, TreeSortPreference::Name).unwrap();
    let matcher = std::sync::Arc::new(SearchMatcher::new("needle", SearchOptions::default()));
    let files = collect_workspace_search_files(&tree);
    assert_eq!(files.len(), 2000);
    let workers = std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4);
    let chunk_size = files.len().div_ceil(workers);
    // 镜像生产语义：分片并行扫描，按序合并，攒满 limit 后不再启动剩余分片
    // （生产里对应 drop 未开始的 Task = 取消）。
    let run_parallel = || {
        std::thread::scope(|scope| {
            let mut handles = Vec::new();
            for chunk in files.chunks(chunk_size) {
                let matcher = matcher.clone();
                handles.push(scope.spawn(move || {
                    let mut hits = Vec::new();
                    for file in chunk {
                        search_single_file(file, &matcher, 200, &mut hits);
                    }
                    hits
                }));
            }
            let mut hits = Vec::new();
            for handle in handles {
                if hits.len() >= 200 {
                    break;
                }
                hits.extend(handle.join().unwrap());
            }
            hits.truncate(200);
            hits.len()
        })
    };
    let run_sequential = || {
        let mut hits = Vec::new();
        for file in &files {
            search_single_file(file, &matcher, 200, &mut hits);
        }
        hits.len()
    };

    let cold_start = Instant::now();
    let cold_hits = run_parallel();
    let cold = cold_start.elapsed();
    let warm_start = Instant::now();
    let warm_hits = run_parallel();
    let warm = warm_start.elapsed();
    let seq_start = Instant::now();
    let seq_hits = run_sequential();
    let sequential = seq_start.elapsed();
    println!(
        "workers={workers} hits={cold_hits}/{warm_hits}/{seq_hits} \
         cold={cold:?} warm={warm:?} seq={sequential:?} \
         speedup_vs_seq={:.1}x cache_speedup={:.1}x",
        sequential.as_secs_f64() / warm.as_secs_f64(),
        cold.as_secs_f64() / warm.as_secs_f64()
    );
    assert_eq!(cold_hits, warm_hits);
    assert_eq!(cold_hits, seq_hits);
    assert_eq!(cold_hits, 200, "每文件最多 3 行命中，总数撞上限");
    let _ = fs::remove_dir_all(root);
}
