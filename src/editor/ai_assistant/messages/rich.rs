use super::*;
use crate::components::latex::{
    DisplayMathSource, MathLayout, latex_cache_key, render_display_math_svg, render_inline_math_svg,
};
use gpui_kit::component::{
    spinner::Spinner,
    text::{
        InlineElement, InlineRenderContext, MarkdownNode, MarkdownParseContext, MarkdownPlugin,
        markdown_ast::Node,
    },
};
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    sync::Mutex,
};

pub(super) type CodeHighlighter = dyn Fn(&gpui_kit::base::text::CodeBlock) -> Vec<(std::ops::Range<usize>, HighlightStyle)>
    + Send
    + Sync;

pub(super) fn code_highlighter(theme: Arc<crate::theme::Theme>) -> Arc<CodeHighlighter> {
    use crate::components::markdown::code_highlight::{
        code_highlight_color, code_highlight_font_decoration, highlight_code_block,
    };
    Arc::new(move |block| {
        let language = block.lang();
        let source = block.code();
        let Some(result) = highlight_code_block(language.as_deref(), &source) else {
            return Vec::new();
        };
        result
            .spans
            .into_iter()
            .map(|span| {
                let (bold, italic, strike, underline) = code_highlight_font_decoration(span.class);
                let style = HighlightStyle {
                    color: Some(code_highlight_color(&theme.colors, span.class)),
                    font_weight: bold.then_some(FontWeight::BOLD),
                    font_style: italic.then_some(FontStyle::Italic),
                    strikethrough: strike.then_some(StrikethroughStyle {
                        thickness: px(1.0),
                        color: None,
                    }),
                    underline: underline.then_some(UnderlineStyle {
                        thickness: px(1.0),
                        color: None,
                        wavy: false,
                    }),
                    ..Default::default()
                };
                (span.range, style)
            })
            .collect()
    })
}

#[derive(Clone)]
struct MathSource {
    key: String,
    body: SharedString,
    color: Hsla,
    size: f32,
    layout: MathLayout,
}

impl Hash for MathSource {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.key.hash(state);
    }
}

#[derive(Clone)]
struct MathImage {
    path: PathBuf,
    width: f32,
    height: f32,
}

struct MathAsset;
impl Asset for MathAsset {
    type Source = MathSource;
    type Output = Result<MathImage, SharedString>;

    fn load(
        source: Self::Source,
        _: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move {
            let result = match source.layout {
                MathLayout::Inline => {
                    render_inline_math_svg(&source.body, source.color, source.size)
                }
                MathLayout::Display => render_display_math_svg(
                    &DisplayMathSource {
                        raw: String::new(),
                        body: source.body.to_string(),
                    },
                    source.color,
                    source.size,
                ),
            }
            .and_then(|rendered| {
                let size = crate::components::mermaid::svg_intrinsic_size(&rendered.svg)?;
                Ok(MathImage {
                    path: rendered.path,
                    width: size.width.max(1.0),
                    height: size.height.max(1.0),
                })
            });
            result.map_err(|error| {
                eprintln!("渲染 AI 公式失败：{error}");
                error.to_string().into()
            })
        }
    }
}

struct DiagramAsset;
impl Asset for DiagramAsset {
    type Source = (SharedString, bool);
    type Output = Result<Arc<crate::components::MermaidSvgRender>, SharedString>;

    fn load(
        source: Self::Source,
        _: &mut App,
    ) -> impl std::future::Future<Output = Self::Output> + Send + 'static {
        async move {
            crate::components::parse_mermaid_fence_source(&source.0)
                .ok_or_else(|| anyhow::anyhow!("无法读取图表源码"))
                .and_then(|diagram| {
                    crate::components::render_mermaid_svg_for_theme(&diagram, f32::MAX, source.1)
                })
                .map(Arc::new)
                .map_err(|error| {
                    eprintln!("渲染 AI 图表失败：{error}");
                    error.to_string().into()
                })
        }
    }
}

struct MathPlugin {
    block: bool,
    state: WeakEntity<TextViewState>,
    ready: Arc<Mutex<HashMap<usize, String>>>,
}

impl MathPlugin {
    fn image(
        &self,
        node: &MarkdownNode,
        color: Hsla,
        size: f32,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<MathImage> {
        let body = node.data::<String>()?;
        let layout = if self.block {
            MathLayout::Display
        } else {
            MathLayout::Inline
        };
        let source = MathSource {
            key: latex_cache_key(body, color, size, layout),
            body: body.clone().into(),
            color,
            size,
            layout,
        };
        let result = window.use_asset::<MathAsset>(&source, cx)?;
        // 资源完成可能改变行内宽度；延后使 TextView 的布局借用先结束。
        let position = node.source_range().map_or(0, |range| range.start);
        match self.ready.lock() {
            Ok(mut ready) => {
                if ready.get(&position) != Some(&source.key) {
                    ready.insert(position, source.key);
                    let state = self.state.clone();
                    cx.defer(move |cx| {
                        if let Err(error) =
                            state.update(cx, |state, cx| state.invalidate_inline_layout(cx))
                        {
                            eprintln!("更新 AI 公式布局失败：{error}");
                        }
                    });
                }
            }
            Err(error) => eprintln!("读取 AI 公式资源状态失败：{error}"),
        }
        result.ok()
    }
}

impl MarkdownPlugin for MathPlugin {
    fn is_block(&self) -> bool {
        self.block
    }
    fn name(&self) -> &str {
        "velora-math"
    }
    fn parse(&self, node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        let raw = cx.node_source(node)?;
        let body = if self.block {
            if !matches!(node, Node::Math(_)) {
                return None;
            }
            crate::components::latex::parse_display_math_source(raw)?.body
        } else {
            if !matches!(node, Node::InlineMath(_) | Node::Text(_)) {
                return None;
            }
            let cache = crate::components::InlineTextTree::from_markdown(raw).render_cache();
            cache
                .spans()
                .iter()
                .filter_map(|span| span.math.as_ref())
                .find(|math| math.source == raw)?
                .body
                .clone()
        };
        Some(
            MarkdownNode::new(self.name(), body.clone())
                .text(raw.to_string())
                .markdown(raw.to_string())
                .accessibility_label(body),
        )
    }
    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style = window.text_style();
        let size = crate::components::latex::display_math_font_size(f32::from(
            style.font_size.to_pixels(window.rem_size()),
        ));
        match self.image(node, style.color, size, window, cx) {
            Some(image) => div()
                .debug_selector(|| "ai-math-display".into())
                .w_full()
                .flex()
                .justify_center()
                .py_2()
                .child(
                    div()
                        .w_full()
                        .max_w(px(image.width))
                        .aspect_ratio(image.width / image.height)
                        .max_h(rems(24.0))
                        .child(img(image.path).size_full().object_fit(ObjectFit::Contain)),
                )
                .into_any_element(),
            None => div()
                .w_full()
                .child(node.as_text().to_string())
                .into_any_element(),
        }
    }
    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        let size = crate::components::latex::inline_math_font_size(f32::from(context.font_size()));
        let image = self.image(node, context.text_style().color, size, window, cx)?;
        let baseline = px((image.height + f32::from(context.font_size()) * 0.5) * 0.5);
        Some(
            InlineElement::new(
                img(image.path)
                    .debug_selector(|| "ai-math-inline".into())
                    .w(px(image.width))
                    .h(px(image.height))
                    .object_fit(ObjectFit::Contain),
            )
            .with_baseline(baseline),
        )
    }
}

struct DiagramPlugin {
    owner: WeakEntity<MessageTranscript>,
}

impl MarkdownPlugin for DiagramPlugin {
    fn is_block(&self) -> bool {
        true
    }
    fn name(&self) -> &str {
        "velora-mermaid"
    }
    fn parse(&self, node: &Node, cx: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        if !matches!(node, Node::Code(_)) {
            return None;
        }
        let raw = cx.node_source(node)?;
        crate::components::parse_mermaid_fence_source(raw)?;
        Some(
            MarkdownNode::new(self.name(), SharedString::from(raw.to_string()))
                .text(raw.to_string())
                .markdown(raw.to_string()),
        )
    }
    fn render(&self, node: &MarkdownNode, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let Some(raw) = node.data::<SharedString>() else {
            return div().into_any_element();
        };
        match window.use_asset::<DiagramAsset>(&(raw.clone(), cx.theme().is_dark()), cx) {
            Some(Ok(rendered)) => {
                let owner = self.owner.clone();
                let raw = raw.clone();
                Button::new((
                    "ai-mermaid-diagram",
                    node.source_range().map_or(0, |range| range.start),
                ))
                .debug_selector(|| "ai-mermaid-diagram".into())
                .accessibility_label(
                    cx.global::<crate::i18n::I18nManager>()
                        .strings()
                        .ai_diagram_preview
                        .clone(),
                )
                .ghost()
                .w_full()
                .h_auto()
                .p_0()
                .on_click(move |_, _, cx| {
                    let owner = owner.clone();
                    let raw = raw.clone();
                    cx.defer(move |cx| {
                        if let Err(error) = owner.update(cx, |this, cx| {
                            if let Err(error) = this
                                .owner
                                .update(cx, |editor, cx| editor.open_mermaid_preview(&raw, cx))
                            {
                                eprintln!("打开 AI 图表预览失败：{error}");
                            }
                        }) {
                            eprintln!("读取 AI 图表失败：{error}");
                        }
                    });
                })
                .child(
                    div()
                        .w_full()
                        .max_h(rems(24.0))
                        .rounded(cx.theme().radius)
                        .overflow_hidden()
                        .aspect_ratio(rendered.display_width / rendered.display_height.max(1.0))
                        .child(
                            img(rendered.path.clone())
                                .size_full()
                                .object_fit(ObjectFit::Contain),
                        ),
                )
                .into_any_element()
            }
            Some(Err(error)) => div()
                .w_full()
                .flex()
                .flex_col()
                .gap_1()
                .child(raw.clone())
                .child(div().text_xs().child(error))
                .into_any_element(),
            None => div()
                .w_full()
                .h_16()
                .flex()
                .items_center()
                .justify_center()
                .child(Spinner::new())
                .into_any_element(),
        }
    }
}

pub(super) struct RichContent {
    extensions: MarkdownExtensions,
    paragraphs: super::paragraph::ParagraphRenderer,
}

impl RichContent {
    pub(super) fn extensions(&self) -> MarkdownExtensions {
        self.extensions.clone()
    }
    pub(super) fn contains_focus(&self, window: &Window, cx: &App) -> bool {
        self.paragraphs.contains_focus(window, cx)
    }
}

pub(super) fn extensions(
    state: WeakEntity<TextViewState>,
    owner: WeakEntity<MessageTranscript>,
    index: usize,
    base: Option<PathBuf>,
) -> RichContent {
    let ready = Arc::new(Mutex::new(HashMap::new()));
    let paragraphs =
        super::paragraph::ParagraphRenderer::new(owner.clone(), state.clone(), index, base);
    let extensions = MarkdownExtensions::default()
        .plugin(MathPlugin {
            block: false,
            state: state.clone(),
            ready: ready.clone(),
        })
        .plugin(MathPlugin {
            block: true,
            state,
            ready,
        })
        .plugin(DiagramPlugin { owner })
        .plugin(paragraphs.clone());
    RichContent {
        extensions,
        paragraphs,
    }
}

pub(super) fn image_source(uri: &SharedUri, base: Option<&Path>) -> ImageSource {
    use crate::components::markdown::image::{ImageResolvedSource, resolve_image_source};
    match resolve_image_source(uri.as_ref(), base) {
        ImageResolvedSource::Local(path) => path.into(),
        ImageResolvedSource::Remote(uri) => uri.into(),
    }
}
