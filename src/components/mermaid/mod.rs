//! Mermaid fenced-block parsing and SVG rendering helpers.
//! 缓存写入 velora 独立目录。

use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use anyhow::{Context as _, anyhow};
use directories::ProjectDirs;
use merman::svg::SvgPipeline;
use merman::{
    Engine, MermaidConfig, OperationControl, RenderOutput, RenderRequest, Renderer, SvgRequest,
};

use crate::components::block::BlockKind;

const MERMAID_FONT_SIZE: f32 = 16.0;

/// 进程内缓存最多记几条显示结果。草稿每编辑一次就产生一个新键，不设上限就会一直
/// 攒整份 SVG（与 latex 那份同一档：256 条）。
const MERMAID_MEMORY_CACHE_LIMIT: usize = 256;

/// 进程内的渲染结果缓存。
///
/// 渲染是**每次重绘**都跑的：正文里每张可见且失焦的图都会调到这里，而磁盘缓存命中
/// 也要付 `create_dir_all`（两次）加 `fs::read_to_string`（两次：基础 SVG 与显示 SVG）。
/// 形状照 `latex/mod.rs` 的 `LatexSvgCache`：`OnceLock<Mutex<..>>` + 条数上限 + 有序淘汰。
///
/// 为什么不只缓存渲染结果：算显示缩放要用渲染器的**固有宽度**，它平时只能靠读基础
/// SVG 得到。把固有宽度也按正文键记一份，命中那条路就一次磁盘都不碰。
#[derive(Default)]
struct MermaidSvgMemoryCache {
    intrinsic: HashMap<String, f32>,
    renders: HashMap<String, MermaidSvgRender>,
    order: VecDeque<String>,
}

static MEMORY_CACHE: OnceLock<Mutex<MermaidSvgMemoryCache>> = OnceLock::new();

fn memory_cache() -> &'static Mutex<MermaidSvgMemoryCache> {
    MEMORY_CACHE.get_or_init(|| Mutex::new(MermaidSvgMemoryCache::default()))
}

/// 锁中毒按「未命中」处理：缓存未命中永远是对的，只是多付一次磁盘。
fn lock_memory_cache() -> Option<std::sync::MutexGuard<'static, MermaidSvgMemoryCache>> {
    memory_cache().lock().ok()
}

/// 内存里问一遍：命中就整条路不碰磁盘（不建目录、不读文件、不解析 SVG）。
fn memory_cached_render(
    base_key: &str,
    available_width: f32,
) -> Option<MermaidSvgRender> {
    let width = {
        let cache = lock_memory_cache()?;
        *cache.intrinsic.get(base_key)?
    };
    let scale = mermaid_display_scale(width, available_width);
    let display_key = display_cache_key(base_key, scale);
    let cache = lock_memory_cache()?;
    let rendered = cache.renders.get(&display_key)?.clone();
    // 磁盘上那份文件是 `img` 元素要读的东西：缓存目录被清理掉之后，内存里还留着
    // 结果但文件已经没了，直接返回就会让图永远加载不出来。文件不在就按未命中走，
    // 那条路会把它重新写出来。一次 `is_file` 仍比两次读盘加两次建目录便宜。
    rendered.path.is_file().then_some(rendered)
}

fn remember_intrinsic(base_key: &str, width: f32) {
    let Some(mut cache) = lock_memory_cache() else {
        return;
    };
    if cache.intrinsic.len() >= MERMAID_MEMORY_CACHE_LIMIT {
        // 这张表每条只记宽度，超上限整张清掉：最坏多读一次基础 SVG。
        cache.intrinsic.clear();
    }
    cache.intrinsic.insert(base_key.to_string(), width);
}

fn remember_render(display_key: &str, rendered: &MermaidSvgRender) {
    let Some(mut cache) = lock_memory_cache() else {
        return;
    };
    if cache.renders.len() >= MERMAID_MEMORY_CACHE_LIMIT
        && let Some(oldest) = cache.order.pop_front()
    {
        cache.renders.remove(&oldest);
    }
    cache.order.push_back(display_key.to_string());
    cache.renders.insert(display_key.to_string(), rendered.clone());
}

/// Opening fence metadata for a Mermaid fenced code block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct MermaidFence {
    /// Fence marker, either backtick or tilde.
    pub(crate) marker: char,
    /// Opening fence run length.
    pub(crate) len: usize,
}

/// Parsed Mermaid source preserved from Markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MermaidSource {
    /// Full Markdown source, including the opening and closing fences.
    pub(crate) raw: String,
    /// Mermaid diagram source between the fences.
    pub(crate) body: String,
    /// The full info string after the opening fence.
    pub(crate) info: String,
}

/// Result of rendering a Mermaid diagram into an SVG cache file.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MermaidSvgRender {
    /// Path to the SVG file consumed by GPUI's image element.
    pub(crate) path: PathBuf,
    /// SVG document content, used by export paths.
    pub(crate) svg: String,
    /// Concrete display width encoded into the cached SVG.
    pub(crate) display_width: f32,
    /// Concrete display height encoded into the cached SVG.
    pub(crate) display_height: f32,
    /// Scale applied to the renderer's intrinsic SVG size for editor display.
    pub(crate) display_scale: f32,
}

/// Concrete dimensions encoded into a display SVG.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MermaidSvgSize {
    pub(crate) width: f32,
    pub(crate) height: f32,
}

/// Returns true when a fenced code info string declares Mermaid content.
pub(crate) fn is_mermaid_info_string(info: Option<&str>) -> bool {
    info.and_then(|info| info.split_whitespace().next())
        .is_some_and(|first| {
            first.eq_ignore_ascii_case("mermaid") || first.eq_ignore_ascii_case("mmd")
        })
}

/// Parse a line as a Mermaid opening fence.
pub(crate) fn parse_mermaid_fence_start(line: &str) -> Option<MermaidFence> {
    let trimmed = BlockKind::strip_code_fence_indent(line)?.trim_end();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }

    let len = trimmed.chars().take_while(|ch| *ch == marker).count();
    if len < 3 {
        return None;
    }

    let info = trimmed[marker.len_utf8() * len..].trim();
    if marker == '`' && info.contains('`') {
        return None;
    }

    is_mermaid_info_string((!info.is_empty()).then_some(info))
        .then_some(MermaidFence { marker, len })
}

/// Returns true when `line` closes the given Mermaid fence.
pub(crate) fn is_mermaid_closing_fence(line: &str, fence: MermaidFence) -> bool {
    BlockKind::closes_code_fence(line, fence.marker, fence.len)
}

/// Parse raw fenced Markdown into the Mermaid diagram source it contains.
pub(crate) fn parse_mermaid_fence_source(raw: &str) -> Option<MermaidSource> {
    let raw = raw.trim_matches('\n').to_string();
    let lines = raw.split('\n').collect::<Vec<_>>();
    if lines.len() < 2 {
        return None;
    }

    let opening = BlockKind::strip_code_fence_indent(lines[0])?.trim_end();
    let fence = parse_mermaid_fence_start(opening)?;
    let info = opening[fence.marker.len_utf8() * fence.len..]
        .trim()
        .to_string();
    if !is_mermaid_closing_fence(lines.last()?, fence) {
        return None;
    }

    let body = lines[1..lines.len() - 1].join("\n");
    Some(MermaidSource { raw, body, info })
}

/// Render Mermaid source into a cached SVG sized for editor display.
pub(crate) fn render_mermaid_svg_for_display(
    source: &MermaidSource,
    available_width: f32,
) -> anyhow::Result<MermaidSvgRender> {
    render_mermaid_svg_for_display_with(source, available_width, render_mermaid_raw)
}

pub(crate) fn render_mermaid_svg_for_theme(source: &MermaidSource, available_width: f32, dark: bool) -> anyhow::Result<MermaidSvgRender> {
    if !dark { return render_mermaid_svg_for_display(source, available_width); }
    let key = format!("{}-dark", mermaid_cache_key(&source.body));
    render_mermaid_display_cached(source, available_width, render_mermaid_dark_raw, &key)
}

fn render_mermaid_svg_for_display_with(
    source: &MermaidSource,
    available_width: f32,
    renderer: MermaidRenderer,
) -> anyhow::Result<MermaidSvgRender> {
    let base_key = mermaid_cache_key(&source.body);
    render_mermaid_display_cached(source, available_width, renderer, &base_key)
}

fn render_mermaid_display_cached(source: &MermaidSource, available_width: f32, renderer: MermaidRenderer, base_key: &str) -> anyhow::Result<MermaidSvgRender> {
    // 内存里先问一遍：命中就一次磁盘都不碰（这正是每帧渲染要付的那一笔）。
    if let Some(rendered) = memory_cached_render(base_key, available_width) {
        return Ok(rendered);
    }

    let base_path = mermaid_base_cache_path(base_key)?;
    let base_svg = render_mermaid_to_svg_cached_with(&source.body, &base_path, renderer)?;
    let intrinsic = svg_intrinsic_size(&base_svg)?;
    remember_intrinsic(base_key, intrinsic.width);
    let scale = mermaid_display_scale(intrinsic.width, available_width);

    let display_key = display_cache_key(base_key, scale);
    let display_path = mermaid_display_cache_path(&display_key)?;
    if let Some(svg) = read_svg_cache(&display_path) {
        let (start, end) = svg_root_tag_range(&svg)?;
        let size = svg_root_explicit_size(&svg[start..end])?;
        let rendered = MermaidSvgRender {
            path: display_path,
            svg,
            display_width: size.width,
            display_height: size.height,
            display_scale: scale,
        };
        remember_render(&display_key, &rendered);
        return Ok(rendered);
    }

    let (svg, size) = scale_mermaid_svg_for_display(&base_svg, scale)?;
    write_svg_cache(&display_path, &svg)?;
    let rendered = MermaidSvgRender {
        path: display_path,
        svg,
        display_width: size.width,
        display_height: size.height,
        display_scale: scale,
    };
    remember_render(&display_key, &rendered);
    Ok(rendered)
}

/// Render a Mermaid diagram body into cached SVG text.
pub(crate) fn render_mermaid_to_svg(source: &str) -> anyhow::Result<String> {
    let key = mermaid_cache_key(source);
    let path = mermaid_base_cache_path(&key)?;
    render_mermaid_to_svg_cached_with(source, &path, render_mermaid_raw)
}

pub(crate) fn render_mermaid_to_svg_for_theme(source: &str, dark: bool) -> anyhow::Result<String> {
    if !dark { return render_mermaid_to_svg(source); }
    let path = mermaid_base_cache_path(&format!("{}-dark", mermaid_cache_key(source)))?;
    render_mermaid_to_svg_cached_with(source, &path, render_mermaid_dark_raw)
}

type MermaidRenderer = fn(&str) -> anyhow::Result<String>;

fn render_mermaid_to_svg_cached_with(
    source: &str,
    path: &Path,
    renderer: MermaidRenderer,
) -> anyhow::Result<String> {
    if let Some(svg) = read_svg_cache(path) {
        return Ok(svg);
    }

    let svg = renderer(source)?;
    write_svg_cache(path, &svg)?;
    Ok(svg)
}

fn render_mermaid_raw(source: &str) -> anyhow::Result<String> {
    render_mermaid_with_theme(source, false)
}

fn render_mermaid_dark_raw(source: &str) -> anyhow::Result<String> {
    render_mermaid_with_theme(source, true)
}

fn render_mermaid_with_theme(source: &str, dark: bool) -> anyhow::Result<String> {
    if !looks_like_supported_mermaid_source(source) {
        return Err(anyhow::anyhow!("unsupported Mermaid diagram"));
    }
    let config = MermaidConfig::from_value(serde_json::json!({
        "layout": "dagre",
        "theme": if dark { "dark" } else { "default" },
        "look": "classic",
        "htmlLabels": false,
        // resvg 按首个可用字体选主字体，Arial 在前会让中文落入任意缺字回退。
        "fontFamily": "PingFang SC, Microsoft YaHei, Arial, sans-serif",
        "themeVariables": { "fontSize": format!("{MERMAID_FONT_SIZE}px") },
        "flowchart": { "useMaxWidth": false, "wrappingWidth": 240, "minNodeWidth": 0 }
    }));
    let renderer = Renderer::new().with_engine(Engine::new().with_site_config(config));
    // GPUI 的 resvg 不绘制 foreignObject；用户配置启用 HTML 标签时也要保留 SVG 文本。
    let request = SvgRequest {
        pipeline: Some(SvgPipeline::resvg_safe()),
        ..Default::default()
    };
    let output = renderer.render(RenderRequest::svg(source, OperationControl::new(), request))?;
    let RenderOutput::Svg(Some(svg)) = output else {
        return Err(anyhow!("Mermaid renderer produced no diagram"));
    };
    Ok(svg.into_parts().0)
}

/// SVG 缓存键的格式版本串：渲染器/键口径升级后旧档自然失效。
const MERMAID_SVG_CACHE_VERSION: &str = "mermaid-svg-v3";

/// Stable cache key for Mermaid content.
pub(crate) fn mermaid_cache_key(source: &str) -> String {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    format!("{MERMAID_SVG_CACHE_VERSION}-{:016x}", hasher.finish())
}

/// Stable cache key for editor display SVG content and scale.
///
/// scale 是随窗口宽度连续变化的量：按 0.001 分档后进键，拖宽度不再一个
/// 像素一个新键（缓存目录按「每张图 × 每种宽度」膨胀）。
#[cfg(test)]
fn mermaid_display_cache_key(source: &str, scale: f32) -> String {
    display_cache_key(&mermaid_cache_key(source), scale)
}

fn display_cache_key(base_key: &str, scale: f32) -> String {
    let bucketed = ((scale * 1000.0).round() as i64).to_string();
    let mut hasher = DefaultHasher::new();
    base_key.hash(&mut hasher);
    bucketed.hash(&mut hasher);
    format!("{MERMAID_SVG_CACHE_VERSION}-{:016x}", hasher.finish())
}

#[cfg(test)]
thread_local! {
    // 并行的其他测试也会读取 SVG，计数不能跨线程汇总。
    static TEST_CACHE_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn note_cache_read() {
    TEST_CACHE_READS.with(|reads| reads.set(reads.get() + 1));
}

/// 读 SVG 缓存：缺失或读不出按未命中；半截文件（无闭合 `</svg>`，断电/
/// 并发写的产物）同样算未命中，下一次重渲染自愈。base 与 display 共用。
fn read_svg_cache(path: &Path) -> Option<String> {
    #[cfg(test)]
    note_cache_read();
    let svg = fs::read_to_string(path).ok()?;
    svg.contains("</svg>").then_some(svg)
}

/// 写 SVG 缓存：同目录 `<name>.<pid>.tmp` + rename，半截文件不会顶掉
/// 好档。base 与 display 共用。
fn write_svg_cache(path: &Path, svg: &str) -> anyhow::Result<()> {
    let file_name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let temp = path.with_file_name(format!("{file_name}.{}.{}.tmp", std::process::id(), uuid::Uuid::new_v4()));
    fs::write(&temp, svg).with_context(|| {
        format!("failed to write Mermaid SVG cache temp '{}'", temp.display())
    })?;
    fs::rename(&temp, path)
        .with_context(|| format!("failed to install Mermaid SVG cache '{}'", path.display()))
}

/// Display scale used by the editor for rendered Mermaid diagrams.
pub(crate) fn mermaid_display_scale(intrinsic_width: f32, available_width: f32) -> f32 {
    (available_width.max(1.0) / intrinsic_width.max(1.0)).min(1.0)
}

/// Rewrites the root SVG element so GPUI sees the intended intrinsic size.
pub(crate) fn scale_mermaid_svg_for_display(
    svg: &str,
    scale: f32,
) -> anyhow::Result<(String, MermaidSvgSize)> {
    let scale = scale.max(f32::MIN_POSITIVE);
    let (start, end) = svg_root_tag_range(svg)?;
    let root_tag = &svg[start..end];
    let base_size = svg_root_size(root_tag)?;
    let size = MermaidSvgSize {
        width: (base_size.width * scale).max(1.0),
        height: (base_size.height * scale).max(1.0),
    };
    let rewritten_root = rewrite_svg_root_tag(root_tag, size)?;
    let mut rewritten = String::with_capacity(svg.len() + 48);
    rewritten.push_str(&svg[..start]);
    rewritten.push_str(&rewritten_root);
    rewritten.push_str(&svg[end..]);
    Ok((rewritten, size))
}

pub(crate) fn svg_intrinsic_size(svg: &str) -> anyhow::Result<MermaidSvgSize> {
    let (start, end) = svg_root_tag_range(svg)?;
    svg_root_size(&svg[start..end])
}

fn svg_root_tag_range(svg: &str) -> anyhow::Result<(usize, usize)> {
    let start = svg
        .find("<svg")
        .ok_or_else(|| anyhow!("Mermaid renderer output did not contain an SVG root"))?;
    let bytes = svg.as_bytes();
    let mut quote = None;
    let mut index = start;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(active_quote) = quote {
            if byte == active_quote {
                quote = None;
            }
        } else if byte == b'"' || byte == b'\'' {
            quote = Some(byte);
        } else if byte == b'>' {
            return Ok((start, index + 1));
        }
        index += 1;
    }
    Err(anyhow!(
        "Mermaid renderer output had an unterminated SVG root tag"
    ))
}

fn svg_root_size(root_tag: &str) -> anyhow::Result<MermaidSvgSize> {
    if let Some(view_box) = svg_root_attr(root_tag, "viewBox")
        && let Some(size) = parse_view_box_size(&view_box)
    {
        return Ok(size);
    }

    svg_root_explicit_size(root_tag)
}

fn svg_root_explicit_size(root_tag: &str) -> anyhow::Result<MermaidSvgSize> {
    let width = svg_root_attr(root_tag, "width")
        .and_then(|value| parse_svg_length(&value))
        .ok_or_else(|| anyhow!("Mermaid SVG root did not expose a usable width"))?;
    let height = svg_root_attr(root_tag, "height")
        .and_then(|value| parse_svg_length(&value))
        .ok_or_else(|| anyhow!("Mermaid SVG root did not expose a usable height"))?;
    Ok(MermaidSvgSize { width, height })
}

fn parse_view_box_size(view_box: &str) -> Option<MermaidSvgSize> {
    let values = view_box
        .split(|ch: char| ch.is_ascii_whitespace() || ch == ',')
        .filter(|part| !part.is_empty())
        .map(str::parse::<f32>)
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    (values.len() == 4 && values[2].is_finite() && values[3].is_finite()).then_some(
        MermaidSvgSize {
            width: values[2].max(1.0),
            height: values[3].max(1.0),
        },
    )
}

fn parse_svg_length(value: &str) -> Option<f32> {
    let value = value.trim();
    let end = value
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E'))
        .map(|(index, ch)| index + ch.len_utf8())
        .last()?;
    let parsed = value[..end].parse::<f32>().ok()?;
    (parsed.is_finite() && parsed > 0.0).then_some(parsed)
}

fn svg_root_attr(root_tag: &str, attr_name: &str) -> Option<String> {
    svg_root_attrs(root_tag)
        .into_iter()
        .find(|attr| attr.name.eq_ignore_ascii_case(attr_name))
        .and_then(|attr| attr.value)
}

fn rewrite_svg_root_tag(root_tag: &str, size: MermaidSvgSize) -> anyhow::Result<String> {
    let attrs = svg_root_attrs(root_tag)
        .into_iter()
        .filter(|attr| {
            !["width", "height", "style"]
                .iter()
                .any(|name| attr.name.eq_ignore_ascii_case(name))
        })
        .map(|attr| attr.raw)
        .collect::<Vec<_>>();

    let mut rewritten = String::from("<svg");
    for attr in attrs {
        rewritten.push(' ');
        rewritten.push_str(attr.trim());
    }
    rewritten.push_str(&format!(
        " width=\"{:.3}\" height=\"{:.3}\">",
        size.width, size.height
    ));
    Ok(rewritten)
}

#[derive(Debug)]
struct SvgRootAttr {
    name: String,
    value: Option<String>,
    raw: String,
}

fn svg_root_attrs(root_tag: &str) -> Vec<SvgRootAttr> {
    let Some(mut index) = root_tag.find("<svg").map(|index| index + "<svg".len()) else {
        return Vec::new();
    };
    let end = root_tag.rfind('>').unwrap_or(root_tag.len());
    let bytes = root_tag.as_bytes();
    let mut attrs = Vec::new();

    while index < end {
        while index < end && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if index >= end || bytes[index] == b'/' {
            break;
        }

        let attr_start = index;
        while index < end
            && !bytes[index].is_ascii_whitespace()
            && bytes[index] != b'='
            && bytes[index] != b'/'
        {
            index += 1;
        }
        let name = root_tag[attr_start..index].to_string();
        if name.is_empty() {
            break;
        }

        while index < end && bytes[index].is_ascii_whitespace() {
            index += 1;
        }

        let mut value = None;
        if index < end && bytes[index] == b'=' {
            index += 1;
            while index < end && bytes[index].is_ascii_whitespace() {
                index += 1;
            }

            if index < end && (bytes[index] == b'"' || bytes[index] == b'\'') {
                let quote = bytes[index];
                index += 1;
                let value_start = index;
                while index < end && bytes[index] != quote {
                    index += 1;
                }
                value = Some(root_tag[value_start..index].to_string());
                if index < end {
                    index += 1;
                }
            } else {
                let value_start = index;
                while index < end && !bytes[index].is_ascii_whitespace() && bytes[index] != b'/' {
                    index += 1;
                }
                value = Some(root_tag[value_start..index].to_string());
            }
        }

        let raw = root_tag[attr_start..index].trim().to_string();
        attrs.push(SvgRootAttr { name, value, raw });
    }

    attrs
}

fn mermaid_cache_dir() -> anyhow::Result<PathBuf> {
    let root = ProjectDirs::from("app", "velora", "velora")
        .map(|dirs| dirs.cache_dir().to_path_buf())
        .unwrap_or_else(|| std::env::temp_dir().join("velora"));
    let dir = root.join("mermaid-svg");
    fs::create_dir_all(&dir)
        .with_context(|| format!("failed to create Mermaid SVG cache '{}'", dir.display()))?;
    Ok(dir)
}

fn mermaid_base_cache_path(key: &str) -> anyhow::Result<PathBuf> {
    mermaid_cache_file_path("base", key)
}

fn mermaid_display_cache_path(key: &str) -> anyhow::Result<PathBuf> {
    mermaid_cache_file_path("display", key)
}

fn mermaid_cache_file_path(kind: &str, key: &str) -> anyhow::Result<PathBuf> {
    let dir = mermaid_cache_dir()?.join(kind);
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create Mermaid {kind} SVG cache '{}'",
            dir.display()
        )
    })?;
    Ok(dir.join(format!("{key}.svg")))
}

fn looks_like_supported_mermaid_source(source: &str) -> bool {
    let mut in_frontmatter = false;
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "---" {
            in_frontmatter = !in_frontmatter;
            continue;
        }
        if in_frontmatter || trimmed.starts_with("%%") {
            continue;
        }

        let lower = trimmed.to_ascii_lowercase();
        return [
            "sequencediagram",
            "classdiagram",
            "statediagram",
            "erdiagram",
            "pie",
            "mindmap",
            "journey",
            "timeline",
            "gantt",
            "requirementdiagram",
            "gitgraph",
            "c4",
            "sankey",
            "quadrantchart",
            "zenuml",
            "block",
            "packet",
            "kanban",
            "architecture",
            "radar",
            "treemap",
            "xychart",
            "flowchart",
            "graph",
        ]
        .iter()
        .any(|prefix| lower.starts_with(prefix));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};

    static TEST_RENDERER_CALLS: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();

    #[test]
    fn themed_mermaid_uses_distinct_svg_and_cache_entries() {
        let raw = format!("```mermaid\nflowchart LR\n A[Theme {}] --> B[Result]\n```", uuid::Uuid::new_v4());
        let source = parse_mermaid_fence_source(&raw).unwrap();
        let light = render_mermaid_svg_for_theme(&source, 600.0, false).unwrap();
        let dark = render_mermaid_svg_for_theme(&source, 600.0, true).unwrap();
        assert_ne!(light.path, dark.path, "主题必须进入 SVG 缓存键");
        assert_ne!(light.svg, dark.svg, "深色图表必须由相应的渲染主题生成");
    }

    #[test]
    fn authored_mermaid_theme_remains_authoritative() {
        let raw = format!("```mermaid\n%%{{init: {{\"theme\": \"dark\"}}}}%%\nflowchart LR\n A[Authored {}] --> B[Result]\n```", uuid::Uuid::new_v4());
        let source = parse_mermaid_fence_source(&raw).unwrap();
        let light = render_mermaid_svg_for_theme(&source, 600.0, false).unwrap();
        let dark = render_mermaid_svg_for_theme(&source, 600.0, true).unwrap();
        assert_eq!(light.svg, dark.svg, "应用外观只提供默认值，不能覆盖图表自身的主题");
    }

    fn test_renderer(source: &str) -> anyhow::Result<String> {
        let calls = TEST_RENDERER_CALLS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut calls = calls.lock().expect("renderer calls mutex poisoned");
        *calls.entry(source.to_string()).or_default() += 1;
        drop(calls);
        render_mermaid_raw(source)
    }

    fn reset_renderer_calls(source: &str) {
        let calls = TEST_RENDERER_CALLS.get_or_init(|| Mutex::new(HashMap::new()));
        calls
            .lock()
            .expect("renderer calls mutex poisoned")
            .remove(source);
    }

    fn renderer_calls(source: &str) -> usize {
        let calls = TEST_RENDERER_CALLS.get_or_init(|| Mutex::new(HashMap::new()));
        calls
            .lock()
            .expect("renderer calls mutex poisoned")
            .get(source)
            .copied()
            .unwrap_or(0)
    }

    fn unique_mermaid_source(label: &str) -> MermaidSource {
        MermaidSource {
            raw: format!("```mermaid\nflowchart LR\nA[{}] --> B\n```", label),
            body: format!("flowchart LR\nA[{}] --> B", label),
            info: "mermaid".to_string(),
        }
    }

    fn remove_cache_file(path: &Path) {
        if path.exists() {
            fs::remove_file(path).expect("remove cache file");
        }
    }

    #[test]
    fn detects_mermaid_info_string() {
        assert!(is_mermaid_info_string(Some("mermaid")));
        assert!(is_mermaid_info_string(Some("MMD title")));
        assert!(!is_mermaid_info_string(Some("rust")));
        assert!(!is_mermaid_info_string(None));
    }

    #[test]
    fn parses_backtick_mermaid_fence() {
        let parsed = parse_mermaid_fence_source("```mermaid\nflowchart LR\nA --> B\n```")
            .expect("mermaid fence");
        assert_eq!(parsed.info, "mermaid");
        assert_eq!(parsed.body, "flowchart LR\nA --> B");
    }

    #[test]
    fn parses_tilde_mmd_fence() {
        let parsed = parse_mermaid_fence_source("~~~MMD\nflowchart LR\nA --> B\n~~~")
            .expect("mermaid fence");
        assert_eq!(parsed.info, "MMD");
        assert_eq!(parsed.body, "flowchart LR\nA --> B");
    }

    #[test]
    fn rejects_unclosed_mermaid_fence() {
        assert!(parse_mermaid_fence_source("```mermaid\nflowchart LR").is_none());
    }

    #[test]
    fn cache_key_changes_with_source() {
        assert_ne!(
            mermaid_cache_key("flowchart LR\nA --> B"),
            mermaid_cache_key("flowchart LR\nA --> C")
        );
    }

    #[test]
    fn display_scale_keeps_small_diagrams_at_natural_size() {
        assert_eq!(mermaid_display_scale(240.0, 720.0), 1.0);
        assert_eq!(mermaid_display_scale(260.0, 720.0), 1.0);
    }

    #[test]
    fn display_scale_fits_wide_diagrams_to_the_text_column() {
        for width in [1400.0, 14000.0] {
            let scale = mermaid_display_scale(width, 720.0);
            assert!(width * scale <= 720.001, "图表应适配正文列宽：{scale}");
        }
    }

    #[test]
    fn display_cache_key_changes_with_scale() {
        let source = "flowchart LR\nA --> B";
        assert_ne!(
            mermaid_display_cache_key(source, 1.0),
            mermaid_display_cache_key(source, 2.0)
        );
    }

    #[test]
    fn display_svg_scaling_rewrites_root_dimensions() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="50" viewBox="0 0 100 50"><rect width="100" height="50"/></svg>"#;
        let (scaled, size) = scale_mermaid_svg_for_display(svg, 2.0).expect("scaled svg");

        assert_eq!(
            size,
            MermaidSvgSize {
                width: 200.0,
                height: 100.0
            }
        );
        assert!(scaled.contains(r#"width="200.000""#));
        assert!(scaled.contains(r#"height="100.000""#));
        assert!(scaled.contains(r#"viewBox="0 0 100 50""#));
    }

    #[test]
    fn display_svg_scaling_removes_responsive_root_attrs() {
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="100%" style="max-width: 240px; aspect-ratio: 2;" viewBox="0 0 120 60"><text>x</text></svg>"#;
        let (scaled, size) = scale_mermaid_svg_for_display(svg, 1.5).expect("scaled svg");

        assert_eq!(
            size,
            MermaidSvgSize {
                width: 180.0,
                height: 90.0
            }
        );
        let root = &scaled[..scaled.find('>').unwrap()];
        assert!(root.contains(r#"width="180.000""#));
        assert!(root.contains(r#"height="90.000""#));
        assert!(!root.contains("100%"));
        assert!(!root.contains("max-width"));
        assert!(!root.contains("style="));
    }

    #[test]
    fn renders_basic_flowchart_svg() {
        let svg = render_mermaid_to_svg("flowchart LR\nA --> B").expect("svg");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
    }

    #[test]
    fn renders_reported_flowchart_with_endpoint_reentry_without_panicking() {
        let source = reported_complex_flowchart();
        // 绕过缓存，防止已有 SVG 掩盖调试构建中回环连线触发的布局断言。
        let rendered = std::panic::catch_unwind(|| render_mermaid_raw(&source.body));
        assert!(rendered.is_ok(), "打开报修图表时渲染器不应 panic");
        let svg = rendered
            .expect("渲染器未 panic")
            .expect("报修图表应生成 SVG");
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        let size = svg_intrinsic_size(&svg).expect("有效 SVG 尺寸");
        assert!(size.width > 0.0 && size.height > 0.0);
    }

    fn reported_complex_flowchart() -> MermaidSource {
        let raw = r#"```mermaid
flowchart TD
    U["用户/评测 harness"] --> S["dsh 会话"]
    S --> MAIN["主模型: 强 agent 模型<br/>(deepseek-v4.1 等, agentic 强)"]
    MAIN -->|"文本/表格/代码任务"| MAIN
    MAIN -->|"工作区里有图"| RI["read_image<br/>(主模型声明 image input 时)"]
    MAIN -->|"工作区里有视频"| AV["analyze_video (MCP)<br/>→ vllm Qwen3-VL"]
    RI --> ATT["attachment 管线<br/>(image_url 自动拼装)"]
    AV --> TXT["带时间戳的文本结论"]
    ATT --> MAIN
    TXT --> MAIN
    MAIN --> ANS["回答: 先视频取口径 → 表格计算 → 答案"]
```"#;
        parse_mermaid_fence_source(raw).expect("回环流程图的 Mermaid 围栏")
    }

    #[test]
    fn reported_flowchart_keeps_a_compact_portrait_layout() {
        let source = reported_complex_flowchart();
        let svg = render_mermaid_raw(&source.body).expect("报修图表应生成 SVG");
        let size = svg_intrinsic_size(&svg).expect("有效 SVG 尺寸");
        assert!(
            size.height <= size.width,
            "报修图表应紧凑展示主干与回线，不能拉成长条：{size:?}"
        );
    }

    #[test]
    fn reported_flowchart_edge_labels_hide_mermaid_quote_delimiters() {
        let source = reported_complex_flowchart();
        let svg = render_mermaid_raw(&source.body).expect("报修图表应生成 SVG");
        let document = roxmltree::Document::parse(&svg).expect("有效 SVG");
        let labels: Vec<_> = document
            .descendants()
            .filter(|node| node.is_text())
            .filter_map(|node| node.text())
            .collect();
        for label in ["文本/表格/代码任务", "工作区里有图", "工作区里有视频"] {
            assert!(
                labels.contains(&label),
                "连线标签应去掉语法引号：{label}，实际：{labels:?}"
            );
        }
    }

    #[test]
    fn reported_flowchart_native_svg_preserves_hierarchy_and_keeps_text_inside_canvas() {
        fn collect_text(group: &usvg::Group, labels: &mut Vec<(String, usvg::Rect)>) {
            for node in group.children() {
                match node {
                    usvg::Node::Group(group) => collect_text(group, labels),
                    usvg::Node::Text(text) => labels.push((
                        text.chunks().iter().map(|chunk| chunk.text()).collect(),
                        text.abs_bounding_box(),
                    )),
                    _ => {}
                }
            }
        }

        let source = reported_complex_flowchart();
        let svg = render_mermaid_raw(&source.body).expect("报修图表应生成 SVG");
        assert!(
            !svg.contains("<foreignObject"),
            "原生显示不能依赖 HTML 标签"
        );
        let mut options = usvg::Options::default();
        options.fontdb_mut().load_system_fonts();
        let tree =
            usvg::Tree::from_str(&svg, &options).expect("GPUI 使用的 SVG 解析器应能读取原图");
        let mut labels = Vec::new();
        collect_text(tree.root(), &mut labels);
        for (label, bounds) in &labels {
            assert!(
                bounds.x() >= -1.0
                    && bounds.y() >= -1.0
                    && bounds.right() <= tree.size().width() + 1.0
                    && bounds.bottom() <= tree.size().height() + 1.0,
                "文字不能超出画布被裁切：{label}，{bounds:?}"
            );
        }
        let top = |label: &str| {
            labels
                .iter()
                .find(|(text, _)| text.starts_with(label))
                .unwrap_or_else(|| panic!("图中缺少文字：{label}"))
                .1
                .y()
        };
        for (parent, child) in [
            ("用户/评测", "dsh 会话"),
            ("dsh 会话", "主模型:"),
            ("主模型:", "回答:"),
            ("主模型:", "read_image"),
            ("read_image", "attachment"),
            ("主模型:", "analyze_video"),
            ("analyze_video", "带时间戳"),
        ] {
            assert!(
                top(parent) < top(child),
                "主干与分支应按 TD 方向排布：{parent} → {child}"
            );
        }
    }

    #[test]
    fn display_render_uses_scaled_intrinsic_size() {
        let source =
            parse_mermaid_fence_source("```mermaid\nflowchart LR\nA --> B\n```").expect("source");
        let rendered = render_mermaid_svg_for_display(&source, 720.0).expect("display svg");

        assert!(rendered.display_width > 1.0);
        assert!(rendered.display_height > 1.0);
        assert!(rendered.display_scale > 0.0 && rendered.display_scale <= 1.0);
        assert!(
            rendered
                .svg
                .contains(&format!("width=\"{:.3}\"", rendered.display_width))
        );
        assert!(
            rendered
                .svg
                .contains(&format!("height=\"{:.3}\"", rendered.display_height))
        );
        assert!(rendered.path.exists());
    }

    #[test]
    fn invalid_mermaid_returns_error() {
        assert!(render_mermaid_to_svg("not a real mermaid diagram ::::").is_err());
    }

    #[test]
    fn concurrent_mermaid_svg_cache_writes_keep_complete_files() {
        let root = std::env::temp_dir().join(format!("velora-mermaid-concurrent-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).expect("建立缓存测试目录");
        let path = root.join("diagram.svg");
        let svg = format!("<svg xmlns=\"http://www.w3.org/2000/svg\"><desc>{}</desc></svg>", "a".repeat(65536));
        let barrier = std::sync::Barrier::new(8);
        let results = std::thread::scope(|scope| {
            let tasks: Vec<_> = (0..8).map(|_| scope.spawn(|| {
                barrier.wait();
                write_svg_cache(&path, &svg)
            })).collect();
            tasks.into_iter().map(|task| task.join().expect("缓存线程完成")).collect::<Vec<_>>()
        });
        assert!(results.iter().all(Result::is_ok), "相同图表的并发渲染不能抢用同一个临时文件：{results:?}");
        assert_eq!(read_svg_cache(&path).as_deref(), Some(svg.as_str()));
        fs::remove_dir_all(root).expect("清理缓存测试目录");
    }

    #[test]
    fn mermaid_chinese_labels_use_the_first_available_font_in_the_configured_stack() {
        let svg = render_mermaid_raw("flowchart LR\nA[创建订单] --> B[校验库存]").expect("生成中文图表");
        // runner 不保证安装某种系统字体；合成字库按逆优先级注册，验证顺序来自 SVG 字体栈。
        for (families, expected) in [
            (&["Arial", "Microsoft YaHei", "PingFang SC"][..], "PingFang SC"),
            (&["Arial", "Microsoft YaHei"][..], "Microsoft YaHei"),
            (&["Arial"][..], "Arial"),
        ] {
            let mut options = usvg::Options::default();
            let database = options.fontdb_mut();
            database.load_font_data(include_bytes!("../../../fixtures/fonts/mermaid-cjk-test.ttf").to_vec());
            let fixture_face = database.faces().next().cloned().expect("合成字体可解析");
            database.remove_face(fixture_face.id);
            for family in families {
                let mut face = fixture_face.clone();
                face.id = usvg::fontdb::ID::dummy();
                for (name, _) in &mut face.families {
                    *name = (*family).to_string();
                }
                database.push_face_info(face);
            }
            let selections = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
            let select_font = usvg::FontResolver::default_font_selector();
            options.font_resolver.select_font = Box::new({
                let selections = selections.clone();
                move |font, database| {
                    let selected = select_font(font, database);
                    let face = selected.and_then(|id| database.face(id)).expect("配置字体可解析");
                    assert!(face.families.iter().any(|(family, _)| family == expected),
                        "应优先使用可用的配置字体 {expected}，实际：{:?}", face.families);
                    selections.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    selected
                }
            });
            options.font_resolver.select_fallback = Box::new(|character, _, _| {
                panic!("合成字库应覆盖测试文本，不应触发缺字回退：{character}");
            });
            usvg::Tree::from_str(&svg, &options).expect("原生文字解析");
            assert!(selections.load(std::sync::atomic::Ordering::Relaxed) > 0, "必须实际解析图中文字的字体");
        }
    }

    #[test]
    fn mermaid_renderer_preserves_labels_across_diagram_families() {
        for (source, label) in [
            ("sequenceDiagram\nAlice->>Bob: hello", "hello"),
            ("classDiagram\nAnimal <|-- Dog", "Dog"),
            ("stateDiagram-v2\n[*] --> Idle\nIdle --> Done", "Idle"),
            ("pie\n\"Cats\" : 2\n\"Dogs\" : 3", "Cats"),
            ("mindmap\nroot((Root))\n  Branch", "Branch"),
            ("flowchart LR\nsubgraph Outer\nA[输入] --> B[输出]\nend", "输入"),
        ] {
            let svg = render_mermaid_raw(source).expect("既有图表类型应继续渲染");
            let document = roxmltree::Document::parse(&svg).expect("有效 SVG");
            assert!(document.descendants().any(|node|
                node.is_text() && node.text().is_some_and(|text| text.contains(label))),
                "图表中缺少标签：{source}");
        }
    }

    fn cache_reads() -> usize {
        TEST_CACHE_READS.with(std::cell::Cell::get)
    }

    fn reset_cache_reads() {
        TEST_CACHE_READS.with(|reads| reads.set(0));
    }

    #[test]
    fn cache_read_probe_is_isolated_between_parallel_render_threads() {
        reset_cache_reads();
        let other_reads = std::thread::spawn(|| {
            super::note_cache_read();
            cache_reads()
        })
        .join()
        .expect("渲染线程正常结束");
        assert_eq!(other_reads, 1, "计数器必须记录所在渲染线程的读盘");
        assert_eq!(cache_reads(), 0, "其他渲染线程不能污染本线程的读盘计数");
    }

    /// 第二次渲染同一条公式/图必须走内存：磁盘缓存命中也要付 `create_dir_all` 加两次
    /// `fs::read_to_string`，而渲染是**每次重绘**都跑的。这条钉住内存缓存别被绕开。
    #[test]
    fn a_second_display_render_does_not_read_the_disk_again() {
        let source = unique_mermaid_source("memory-cache-hit-skips-disk");
        let base_key = mermaid_cache_key(&source.body);
        let base_path = mermaid_base_cache_path(&base_key).expect("base path");
        remove_cache_file(&base_path);
        reset_renderer_calls(&source.body);
        reset_cache_reads();

        let first = render_mermaid_svg_for_display_with(&source, 720.0, test_renderer)
            .expect("first render");
        assert_eq!(renderer_calls(&source.body), 1);
        assert!(
            cache_reads() > 0,
            "第一次渲染一次盘都没读，这条闸门量不到东西"
        );

        reset_cache_reads();
        let second = render_mermaid_svg_for_display_with(&source, 720.0, test_renderer)
            .expect("cached render");
        assert_eq!(second, first, "内存缓存给回来的结果与第一次不一致");
        assert_eq!(
            cache_reads(),
            0,
            "第二次渲染又读了盘：内存缓存没生效"
        );
        assert_eq!(renderer_calls(&source.body), 1);

        remove_cache_file(&first.path);
        remove_cache_file(&base_path);
    }

    #[test]
    fn display_cache_hit_does_not_call_renderer_again() {
        let source = unique_mermaid_source("display-cache-hit-does-not-call-renderer-again");
        let base_key = mermaid_cache_key(&source.body);
        let base_path = mermaid_base_cache_path(&base_key).expect("base path");
        remove_cache_file(&base_path);

        reset_renderer_calls(&source.body);
        let first = render_mermaid_svg_for_display_with(&source, 720.0, test_renderer)
            .expect("first render");
        assert_eq!(renderer_calls(&source.body), 1);
        let display_path = first.path.clone();

        let second = render_mermaid_svg_for_display_with(&source, 720.0, test_renderer)
            .expect("cached render");
        assert_eq!(renderer_calls(&source.body), 1);
        assert_eq!(second.path, display_path);
        assert_eq!(second.display_width, first.display_width);
        assert_eq!(second.display_height, first.display_height);

        remove_cache_file(&display_path);
        remove_cache_file(&base_path);
    }

    #[test]
    fn display_cache_miss_reuses_base_cache() {
        let source = unique_mermaid_source("display-cache-miss-reuses-base-cache");
        let base_key = mermaid_cache_key(&source.body);
        let base_path = mermaid_base_cache_path(&base_key).expect("base path");
        remove_cache_file(&base_path);

        reset_renderer_calls(&source.body);
        let first = render_mermaid_svg_for_display_with(&source, 720.0, test_renderer)
            .expect("first render");
        assert_eq!(renderer_calls(&source.body), 1);
        remove_cache_file(&first.path);

        let second = render_mermaid_svg_for_display_with(&source, 720.0, test_renderer)
            .expect("display rebuild");
        assert_eq!(renderer_calls(&source.body), 1);
        assert!(second.path.exists());
        assert_eq!(second.display_width, first.display_width);
        assert_eq!(second.display_height, first.display_height);

        remove_cache_file(&second.path);
        remove_cache_file(&base_path);
    }

    #[test]
    fn display_scale_change_reuses_base_cache_with_new_display_file() {
        let source = unique_mermaid_source("display-scale-change-reuses-base-cache");
        let base_key = mermaid_cache_key(&source.body);
        let base_path = mermaid_base_cache_path(&base_key).expect("base path");
        remove_cache_file(&base_path);

        reset_renderer_calls(&source.body);
        let narrow = render_mermaid_svg_for_display_with(&source, 64.0, test_renderer)
            .expect("narrow render");
        assert_eq!(renderer_calls(&source.body), 1);

        let wide = render_mermaid_svg_for_display_with(&source, 900.0, test_renderer)
            .expect("wide render");
        assert_eq!(renderer_calls(&source.body), 1);
        assert!(wide.path.exists());
        assert!(narrow.display_width <= 64.0);
        assert!(wide.display_width > narrow.display_width);
        assert_ne!(narrow.path, wide.path);

        remove_cache_file(&narrow.path);
        remove_cache_file(&wide.path);
        remove_cache_file(&base_path);
    }

    #[test]
    fn display_disk_cache_preserves_fitted_dimensions_after_memory_miss() {
        let source = unique_mermaid_source("fitted-dimensions-survive-memory-miss");
        let first = render_mermaid_svg_for_display_with(&source, 64.0, test_renderer)
            .expect("缩小到正文列");
        let display_key = mermaid_display_cache_key(&source.body, first.display_scale);
        {
            let mut cache = lock_memory_cache().expect("内存缓存锁");
            cache.renders.remove(&display_key);
            cache.order.retain(|key| key != &display_key);
        }
        let second = render_mermaid_svg_for_display_with(&source, 64.0, test_renderer)
            .expect("从磁盘恢复缩放图表");
        assert_eq!(
            second.display_width, first.display_width,
            "缓存命中不能还原为未缩放宽度"
        );
        assert!((second.display_height - first.display_height).abs() < 0.001);
        remove_cache_file(&first.path);
        remove_cache_file(
            &mermaid_base_cache_path(&mermaid_cache_key(&source.body)).expect("基础路径"),
        );
    }
}

    /// display 键的 scale 是连续量（正文列宽随窗口宽度变化）：
    /// 不分档的话拖一下宽度一个新键、一次写盘，缓存目录按「每张图 × 每种
    /// 宽度」无限膨胀（评审报告维度二建议 16）。0.001 分档后进键。
    #[test]
    fn display_cache_key_buckets_subpixel_scale_changes() {
        let source = "flowchart LR\nA --> B";
        assert_eq!(
            mermaid_display_cache_key(source, 1.0),
            mermaid_display_cache_key(source, 1.0005),
            "亚像素 scale 差必须落在同一档"
        );
        assert_ne!(
            mermaid_display_cache_key(source, 1.0),
            mermaid_display_cache_key(source, 1.01),
            "跨档的 scale 仍要换键"
        );
    }

    /// 键带格式版本串：渲染器升级后旧档自然失效，而不是永远命中旧 SVG。
    #[test]
    fn cache_keys_carry_a_format_version() {
        assert!(
            mermaid_cache_key("flowchart LR").starts_with("mermaid-svg-v3-"),
            "base 键缺版本前缀"
        );
        assert!(
            mermaid_display_cache_key("flowchart LR", 1.0).starts_with("mermaid-svg-v3-"),
            "display 键缺版本前缀"
        );
    }

    /// 半截缓存（无闭合 `</svg>`，断电/并发写的产物）按未命中重渲染；
    /// 写盘走同目录临时文件 + rename，成功路径不留 .tmp。
    #[test]
    fn half_written_cache_reads_as_a_miss_and_writes_are_atomic() {
        let root = std::env::temp_dir().join(format!("velora-mermaid-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("create root");
        let path = root.join("base.svg");
        std::fs::write(&path, "<svg xmlns=\"http://www.w3.org/2000/svg\"><g>").expect("seed half file");

        fn probe_renderer(_: &str) -> anyhow::Result<String> {
            Ok("<svg>…renderer output…</svg>".into())
        }
        let svg = render_mermaid_to_svg_cached_with("flowchart LR", &path, probe_renderer)
            .expect("re-render on half cache");
        assert_eq!(svg, "<svg>…renderer output…</svg>", "半截缓存必须重渲染");
        assert!(std::fs::read_to_string(&path).expect("read").contains("</svg>"));

        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .expect("read dir")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "成功路径不得留临时文件：{leftovers:?}");
        let _ = std::fs::remove_dir_all(root);
    }
