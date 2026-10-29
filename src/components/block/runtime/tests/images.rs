
use crate::components::markdown::inline::InlineTextTree;
use crate::components::{
    Block, BlockKind, BlockRecord,
};
use std::sync::Arc;

use gpui::{
    AppContext,
    TestAppContext,
};



#[gpui::test]
async fn image_width_attribute_seeds_resize_factor(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![diagram](./assets/diagram.png){width=40%}"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.sync_render_cache();
        assert!(block.showing_rendered_image());
        assert!((block.image_width_factor - 0.4).abs() < 0.001);
    });
}

#[gpui::test]
async fn resizing_image_writes_width_attribute_back_to_markdown(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![diagram](./assets/diagram.png)"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.sync_render_cache();
        assert_eq!(block.image_width_factor, 1.0);

        block.image_width_factor = 0.4;
        block.write_image_width_back_to_source(cx);

        assert_eq!(
            block.display_text(),
            "![diagram](./assets/diagram.png){width=40%}"
        );
        assert!((block.image_width_factor - 0.4).abs() < 0.001);
        assert!(block.showing_rendered_image());
    });
}

#[gpui::test]
async fn resizing_image_back_to_full_width_removes_attribute(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![diagram](./assets/diagram.png){width=40%}"),
            ),
        )
    });

    block.update(cx, |block, cx| {
        block.sync_render_cache();
        assert!((block.image_width_factor - 0.4).abs() < 0.001);

        block.image_width_factor = 1.0;
        block.write_image_width_back_to_source(cx);

        assert_eq!(block.display_text(), "![diagram](./assets/diagram.png)");
        assert_eq!(block.image_width_factor, 1.0);
    });
}

#[gpui::test]
async fn focusing_rendered_image_does_not_auto_expand(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![diagram](./assets/diagram.png)"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.sync_render_cache();
        assert!(block.showing_rendered_image());
        assert!(!block.image_edit_expanded);

        assert!(!block.sync_image_focus_state(true));
        assert!(block.showing_rendered_image());
        assert!(!block.image_edit_expanded);
    });
}

#[gpui::test]
async fn requested_rendered_image_expansion_enters_raw_markdown_editing(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![diagram](./assets/diagram.png)"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.sync_render_cache();
        block.request_image_edit_expansion();
        assert!(block.sync_image_focus_state(true));
        assert!(block.image_edit_expanded);
        assert!(!block.showing_rendered_image());
        assert_eq!(block.cursor_offset(), block.visible_len());
    });
}

#[gpui::test]
async fn blurred_valid_rendered_image_recovers_image_presentation(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![diagram](./assets/diagram.png)"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.sync_render_cache();
        block.request_image_edit_expansion();
        assert!(block.sync_image_focus_state(true));
        assert!(block.image_edit_expanded);

        assert!(block.sync_image_focus_state(false));
        assert!(!block.image_edit_expanded);
        assert!(block.showing_rendered_image());
    });
}

#[gpui::test]
async fn broken_rendered_image_syntax_blurs_back_to_plain_text(cx: &mut TestAppContext) {
    let block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![diagram](./assets/diagram.png)"),
            ),
        )
    });

    block.update(cx, |block, _cx| {
        block.sync_render_cache();
        block.request_image_edit_expansion();
        assert!(block.sync_image_focus_state(true));

        block
            .record
            .set_title(InlineTextTree::from_markdown("not an image anymore"));
        block.sync_render_cache();
        assert!(!block.sync_image_focus_state(false));
        assert!(block.image_runtime().is_none());
        assert!(!block.image_edit_expanded);
        assert!(!block.showing_rendered_image());
        assert_eq!(block.display_text(), "not an image anymore");
    });
}


/// 无 `![` 标记的块不进图片 runtime 的解析路径（评审报告维度四建议 4 前半）：
/// set_runtime_context 对每个块无条件 visible_text() 拼接 + 两轮解析，而
/// 每次结构编辑都对全文档每块跑一遍——无图文档白付全文扫描。
/// 挡板只有一条：fragments 里没有 `![` 且当前也不是图片块，就整体跳过。
#[gpui::test]
async fn plain_blocks_skip_the_image_runtime_scan(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let plain = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("普通段落，含 [链接](https://a.test) 与 *强调*"),
            ),
        )
    });
    let before = crate::components::block::runtime::image::probe::scans();
    plain.update(cx, |block, _cx| {
        block.set_runtime_context(None, Arc::default(), Arc::default(), Arc::default());
    });
    let scanned =
        crate::components::block::runtime::image::probe::scans() - before;
    assert_eq!(scanned, 0, "无 ![ 标记的块不该进图片解析路径");
}

/// 真图块照常进解析路径；曾是的块（runtime 还挂着）也不许被挡板拦住——
/// 否则删掉 `!` 那一键图片残留。
#[gpui::test]
async fn image_blocks_still_enter_the_image_runtime_scan(cx: &mut TestAppContext) {
    let cx = cx.add_empty_window();
    let image_block = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![图](a.png)"),
            ),
        )
    });
    let before = crate::components::block::runtime::image::probe::scans();
    image_block.update(cx, |block, _cx| {
        block.set_runtime_context(None, Arc::default(), Arc::default(), Arc::default());
    });
    assert!(
        crate::components::block::runtime::image::probe::scans() > before,
        "带 ![ 的块必须进解析路径"
    );
}

/// 正文里恰好提到 `![` 的段落不是图片：廉价形状预判失败就直接返回，不该
/// 先整树拼一遍可见文本再为失败解析补一次全量序列化重解析（每键两轮扫描
/// + 一次整份序列化）。真图块照常解析。
#[gpui::test]
async fn prose_blocks_that_mention_image_syntax_skip_the_standalone_parse(
    cx: &mut TestAppContext,
) {
    use crate::components::block::runtime::image::probe;
    let cx = cx.add_empty_window();
    let prose = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("讲 ![ 语法：![图](a.png) 的写法"),
            ),
        )
    });
    let before = probe::standalone_parses();
    prose.update(cx, |block, _cx| {
        block.set_runtime_context(None, Arc::default(), Arc::default(), Arc::default());
    });
    assert_eq!(
        probe::standalone_parses() - before,
        0,
        "正文里提到 ![ 的段落不该进独立图片解析"
    );

    let image = cx.new(|cx| {
        Block::with_record(
            cx,
            BlockRecord::new(
                BlockKind::Paragraph,
                InlineTextTree::from_markdown("![图](a.png)"),
            ),
        )
    });
    let before = probe::standalone_parses();
    image.update(cx, |block, _cx| {
        block.set_runtime_context(None, Arc::default(), Arc::default(), Arc::default());
    });
    assert!(
        probe::standalone_parses() > before,
        "独立图片块必须照常解析"
    );
    image.read_with(cx, |block, _cx| {
        assert!(block.image_runtime.is_some(), "独立图片块应建出运行时");
    });
}
