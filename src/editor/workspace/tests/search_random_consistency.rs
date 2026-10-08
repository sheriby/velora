//! 方案文档 §7.3 的全量一致性属性测试：随机生成文档 × 随机查询与开关，断言
//! 结果列表、正文高亮、`F3` 落点、`replace_all` 计数**四处引用的是同一份命中集合**。
//!
//! 固定语料的同源测试（`the_hit_table_drives_every_search_path`）只能钉住它写过的那
//! 几种形状；这条把形状交给生成器，为的是把「四份实现靠巧合一致」这类问题在**结构上**
//! 堵住——换引擎之后四条路径读同一张表，任何一处偷算别的集合都会被新形状抓出来。
//!
//! 覆盖范围有两处有意的边界：
//! - 不生成表格块。表格单元格不注册进文档块树（跳转与锚点那里有专门的绕行），
//!   「每根有命中的块有几段高亮」这条计数在它身上不成立；表格形状的命中由
//!   `document_search_hit_inside_table_jumps` 单独钉。
//! - 查询词只取正文里的可见词。命中的是隐藏 Markdown 标记（`#`、`- [ ]`）时，
//!   换算出来的显示区间本来就会塌成空，那是设计行为，不该在这条属性里比。

use super::super::{Editor, SearchMatcher, SearchOptions, WorkspaceSearchScope, WorkspaceTab};
use gpui::TestAppContext;
use std::time::Duration;

fn init(cx: &mut TestAppContext) {
    cx.update(|cx| {
        crate::i18n::I18nManager::init(cx);
        crate::theme::ThemeManager::init(cx);
        crate::components::init(cx);
    });
}

/// 可复现的伪随机源（xorshift64）。属性测试要的是覆盖面，不是密码学强度，
/// 但失败必须能按轮次重放——轮次号打在每条断言消息里。
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn one_in(&mut self, denom: u64) -> bool {
        self.next() % denom == 0
    }

    fn pick(&mut self) -> &'static str {
        WORDS[self.below(WORDS.len())]
    }
}

/// 查询词表：都出现在正文里，且不含正则元字符，好让「随机开关」不改变可解释性。
const WORDS: [&str; 8] = [
    "alpha", "needle", "beta", "English", "正文", "标题", "段落", "中文",
];

/// 生成一段 markdown：随机挑块类型、随机填词，规模控制在几十条命中以内。
fn generate_document(rng: &mut Rng) -> String {
    let mut out = String::new();
    if rng.one_in(3) {
        out.push_str("---\ntitle: 随机样本\ntags: [alpha, needle]\n---\n\n");
    }
    let blocks = 8 + rng.below(14);
    for _ in 0..blocks {
        let a = rng.pick();
        let b = rng.pick();
        match rng.below(6) {
            0 => out.push_str(&format!("# {a} 标题 {b}\n\n")),
            1 => out.push_str(&format!("## {a}\n\n{b} 与 {a} 混排的一段。\n\n")),
            2 => out.push_str(&format!("这里既有 {a} 也有 {b}，{a} 还会再出现一次。\n\n")),
            3 => out.push_str(&format!("- 第一项 {a}\n  - 嵌套 {b}\n- 第三项 {a}\n\n")),
            4 => out.push_str(&format!("- [ ] 待办 {a}\n- [x] 已办 {b}\n\n")),
            _ => out.push_str(&format!("```text\n{a} line\n{b} line\n```\n\n")),
        }
    }
    out
}

/// 随机选项组合：大小写、单词边界、模糊、正则都进得来
/// （正则那一路只喂不含元字符的词，所以四种开关用同一份词表就够）。
fn generate_options(rng: &mut Rng) -> SearchOptions {
    SearchOptions {
        match_case: rng.one_in(2),
        whole_word: rng.one_in(3),
        use_regex: rng.one_in(2),
        fuzzy: rng.one_in(5),
    }
}

fn table_ranges(editor: &gpui::Entity<Editor>, cx: &mut TestAppContext) -> Vec<std::ops::Range<usize>> {
    editor.read_with(cx, |editor, _cx| {
        editor
            .workspace
            .document_matches
            .as_ref()
            .map(|table| {
                table
                    .hits
                    .iter()
                    .map(|hit| hit.range.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })
}

#[gpui::test]
#[ignore = "慢用例（>1s）：本地默认跳过，CI 跑"]
async fn random_documents_keep_all_four_search_paths_on_one_hit_set(
    cx: &mut TestAppContext,
) {
    init(cx);
    let mut skipped = 0usize;
    let mut per_round = Vec::new();
    for seed_index in 0..24u64 {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15 ^ seed_index.wrapping_mul(0x1000_0000_0000_0001));
        let markdown = generate_document(&mut rng);
        let options = generate_options(&mut rng);
        // 查询词从这份文档里真能命中的那些里挑：开关可能让某个词一条都搜不出
        // （中文词配单词边界就是典型），那就换一个——属性测试要每一轮都有活干。
        let mut chosen: Option<(String, Vec<std::ops::Range<usize>>)> = None;
        for _attempt in 0..WORDS.len() {
            let candidate = rng.pick().to_string();
            // 预检走的是引擎本身（与命中表同一份实现、同一批字节），扫得出命中才开窗口。
            let found: Vec<_> = SearchMatcher::new(&candidate, options)
                .find_all_in_text(&markdown)
                .into_iter()
                .map(|hit| hit.range)
                .collect();
            if !found.is_empty() && found.len() < 200 {
                chosen = Some((candidate, found));
                break;
            }
        }
        let Some((query, expected)) = chosen else {
            skipped += 1;
            continue;
        };

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, markdown.clone(), None));
        cx.run_until_parked();
        editor.update(cx, |editor, cx| {
            editor.workspace.is_open = true;
            editor.workspace.active_tab = WorkspaceTab::Search;
            editor.workspace.search_scope = WorkspaceSearchScope::Document;
            editor.workspace.search_query = query.clone();
            editor.workspace.search_match_case = options.match_case;
            editor.workspace.search_whole_word = options.whole_word;
            editor.workspace.search_use_regex = options.use_regex;
            editor.workspace.search_fuzzy = options.fuzzy;
            editor.schedule_workspace_search(cx);
        });
        cx.executor().advance_clock(Duration::from_millis(200));
        cx.run_until_parked();

        let label = format!(
            "第 {seed_index} 轮：查询 {query:?}，选项 {options:?}，命中 {} 条",
            expected.len()
        );
        // 先确认表本身就是引擎那份集合（不是缓存里的旧账）。
        let ranges = table_ranges(&editor, cx);
        assert_eq!(ranges, expected, "{label}：命中表与引擎独立扫出的集合不一致");
        per_round.push(ranges.len());

        // ① 结果列表与表逐项相等：顺序、个数、位置都得一样。
        let rows = editor.read_with(cx, |editor, _cx| {
            editor
                .workspace
                .search_results
                .iter()
                .filter_map(|hit| hit.source_range.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(rows, ranges, "{label}：结果列表与命中表不是同一份");

        // ② 高亮段数之和 == 命中数。
        let highlighted = editor.read_with(cx, |editor, cx| {
            editor
                .search_highlighted_blocks
                .iter()
                .map(|entity| entity.read(cx).search_highlight_ranges.len())
                .sum::<usize>()
        });
        assert_eq!(highlighted, ranges.len(), "{label}：高亮画出的命中数与表不符");

        // ③ 「下一个」走一圈，落点序列正好是表的内容。
        let visited = editor.update(cx, |editor, cx| {
            let mut out = Vec::new();
            for _ in 0..ranges.len() {
                editor.find_next_document_match(false, cx);
                out.push(editor.workspace.document_active_range.clone());
            }
            out
        });
        assert_eq!(
            visited.into_iter().flatten().collect::<Vec<_>>(),
            ranges,
            "{label}：循环跳转的落点与表不一致"
        );

        // ④ 替换计数 == 命中数，且替换后重扫归零。
        let replaced = cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.workspace.replace_query = "换掉".into();
                editor.replace_all_document_matches(window, cx)
            })
        });
        assert_eq!(replaced, ranges.len(), "{label}：替换计数与命中数不符");
        editor.update(cx, |editor, cx| {
            editor.document_matches(cx);
        });
        let after = table_ranges(&editor, cx);
        assert!(after.is_empty(), "{label}：全部替换后仍能扫出 {} 条命中", after.len());
    }
    eprintln!("[measure] 随机一致性 24 轮：命中数 {per_round:?}，跳过 {skipped} 轮");
    assert!(
        per_round.len() >= 20,
        "24 轮里只有 {} 轮量到命中，生成器覆盖面不够",
        per_round.len()
    );
    assert!(skipped <= 4, "{skipped} 轮一个命中都挑不出来，生成器失效了");
}
