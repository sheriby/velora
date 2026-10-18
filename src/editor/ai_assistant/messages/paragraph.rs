use super::*;
use crate::components::{
    Block, BlockEvent, BlockKind, BlockRecord, InlineMathDelimiter, InlineTextTree,
};
use gpui_kit::component::text::{
    MarkdownNode, MarkdownParseContext, MarkdownPlugin, markdown_ast::Node,
};
use std::{collections::HashMap, sync::Mutex};

type Paragraphs = Arc<Mutex<HashMap<usize, Entity<ReadonlyParagraph>>>>;

#[derive(Clone)]
pub(super) struct ParagraphRenderer {
    owner: WeakEntity<MessageTranscript>,
    index: usize,
    base: Option<PathBuf>,
    state: WeakEntity<TextViewState>,
    paragraphs: Paragraphs,
}

impl ParagraphRenderer {
    pub(super) fn new(
        owner: WeakEntity<MessageTranscript>,
        state: WeakEntity<TextViewState>,
        index: usize,
        base: Option<PathBuf>,
    ) -> Self {
        Self {
            owner,
            index,
            base,
            state,
            paragraphs: Arc::default(),
        }
    }

    pub(super) fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        match self.paragraphs.lock() {
            Ok(paragraphs) => paragraphs.values().any(|paragraph| {
                paragraph
                    .read(cx)
                    .block
                    .read(cx)
                    .focus_handle
                    .is_focused(window)
            }),
            Err(error) => {
                eprintln!("读取 AI 段落状态失败：{error}");
                false
            }
        }
    }
}

struct ParagraphData {
    source: SharedString,
    tree: InlineTextTree,
    images: Arc<crate::components::ImageReferenceDefinitions>,
    links: Arc<crate::components::markdown::link::LinkReferenceDefinitions>,
}

struct ReadonlyParagraph {
    block: Entity<Block>,
    source: SharedString,
    _events: Subscription,
}

impl ReadonlyParagraph {
    fn new(
        data: &ParagraphData,
        renderer: &ParagraphRenderer,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let block = cx.new(|cx| {
            let mut block = Block::with_record(
                cx,
                BlockRecord::new(BlockKind::Paragraph, data.tree.clone()),
            );
            block.reading_mode = Some(std::rc::Rc::new(std::cell::Cell::new(true)));
            block.set_runtime_context(
                renderer.base.clone(),
                data.images.clone(),
                data.links.clone(),
                Arc::default(),
            );
            block
        });
        let owner = renderer.owner.clone();
        let index = renderer.index;
        let handle = window.window_handle();
        let events = cx.subscribe(&block, move |_, _, event: &BlockEvent, cx| {
            if let BlockEvent::RequestOpenLink { open_target } = event {
                let target = open_target.clone();
                let owner = owner.clone();
                cx.defer(move |cx| {
                    match handle.update(cx, |_, window, cx| {
                        owner.update(cx, |view, cx| view.open_link(index, &target, window, cx))
                    }) {
                        Ok(Ok(())) => {}
                        Ok(Err(error)) | Err(error) => eprintln!("打开 AI 段落链接失败：{error}"),
                    }
                });
            }
        });
        Self {
            block,
            source: data.source.clone(),
            _events: events,
        }
    }

    fn sync(&mut self, data: &ParagraphData, base: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.source.as_ptr() == data.source.as_ptr() && self.source.len() == data.source.len() {
            return;
        }
        let extends = data.source.starts_with(self.source.as_ref());
        self.source = data.source.clone();
        self.block.update(cx, |block, cx| {
            if !extends {
                block.selected_range = 0..0;
                block.marked_range = None;
            }
            block.record.set_title(data.tree.clone());
            block.set_runtime_context(
                base,
                data.images.clone(),
                data.links.clone(),
                Arc::default(),
            );
            cx.notify();
        });
    }
}

impl MarkdownPlugin for ParagraphRenderer {
    fn is_block(&self) -> bool {
        true
    }
    fn name(&self) -> &str {
        "velora-parenthesized-math"
    }
    fn parse(&self, node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        if !matches!(node, Node::Paragraph(_)) {
            return None;
        }
        let source = cx.node_source(node)?;
        if !source.contains("\\(") {
            return None;
        }
        let links = Arc::new(
            crate::components::markdown::link::parse_link_reference_definitions(cx.source()),
        );
        let tree = InlineTextTree::from_markdown_with_link_references(source, &links);
        if !tree.render_cache().spans().iter().any(|span| {
            span.math
                .as_ref()
                .is_some_and(|math| math.delimiter == InlineMathDelimiter::Paren)
        }) {
            return None;
        }
        let text = tree.visible_text();
        let data = ParagraphData {
            source: source.to_string().into(),
            tree,
            images: Arc::new(crate::components::parse_image_reference_definitions(
                cx.source(),
            )),
            links,
        };
        Some(
            MarkdownNode::new(self.name(), data)
                .text(text)
                .markdown(source.to_string()),
        )
    }
    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(data) = node.data::<ParagraphData>() else {
            return div().into_any_element();
        };
        let position = node.source_range().map_or(0, |range| range.start);
        let paragraph = match self.paragraphs.lock() {
            Ok(mut paragraphs) => paragraphs
                .entry(position)
                .or_insert_with(|| cx.new(|cx| ReadonlyParagraph::new(data, self, window, cx)))
                .clone(),
            Err(error) => {
                eprintln!("读取 AI 公式段落失败：{error}");
                return div().child(node.as_text().to_string()).into_any_element();
            }
        };
        let block = paragraph.update(cx, |paragraph, cx| {
            paragraph.sync(data, self.base.clone(), cx);
            paragraph.block.clone()
        });
        let select_state = self.state.clone();
        let select_block = block.clone();
        let native_state = self.state.clone();
        let native_block = block.clone();
        div()
            .debug_selector(|| "ai-parenthesized-math".into())
            .w_full()
            .capture_action(move |_: &crate::components::SelectAll, window, cx| {
                select_message(&select_state, &select_block, window, cx);
                cx.stop_propagation();
            })
            .on_action(move |_: &gpui_kit::base::input::SelectAll, window, cx| {
                select_message(&native_state, &native_block, window, cx);
                cx.stop_propagation();
            })
            .child(block)
            .into_any_element()
    }
}

fn select_message(
    state: &WeakEntity<TextViewState>,
    block: &Entity<Block>,
    window: &mut Window,
    cx: &mut App,
) {
    // 全选属于外层阅读结果，焦点也随选区移交，避免块的复制动作截获它。
    block.update(cx, |block, cx| {
        block.selected_range = 0..0;
        cx.notify();
    });
    if let Err(error) = state.update(cx, |state, cx| {
        state.select_all(cx);
        window.focus(state.focus_handle(), cx);
    }) {
        eprintln!("选择 AI 阅读结果失败：{error}");
    }
}
