//! Code-block syntax highlighting support.

use std::collections::HashMap;
use std::ops::Range;
#[cfg(feature = "code-highlight-core")]
use std::sync::LazyLock;

use gpui::Hsla;
#[cfg(feature = "code-highlight-core")]
use tree_sitter_highlight::{Highlight, HighlightConfiguration, HighlightEvent, Highlighter};

use crate::theme::ThemeColors;

/// Canonical language key used by the syntax-highlighting registry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum CodeLanguageKey {
    /// Rust source code.
    Rust,
    /// JavaScript without JSX.
    JavaScript,
    /// JavaScript with JSX syntax.
    JavaScriptJsx,
    /// TypeScript without TSX.
    TypeScript,
    /// TypeScript with TSX syntax.
    TypeScriptTsx,
    /// JSON data.
    Json,
    /// Markdown source.
    Markdown,
    /// POSIX-like shell scripts.
    Bash,
    /// C source code.
    C,
    /// C++ source code.
    Cpp,
    /// C# source code.
    CSharp,
    /// CSS stylesheets.
    Css,
    /// Go source code.
    Go,
    /// HTML markup.
    Html,
    /// Java source code.
    Java,
    /// PHP source code.
    Php,
    /// Python source code.
    Python,
    /// Ruby source code.
    Ruby,
    /// YAML configuration.
    Yaml,
    /// TOML configuration.
    Toml,
    /// Mermaid diagram source.
    Mermaid,
    /// Plain text or unknown language fallback.
    PlainText,
}

/// Semantic highlight classes mapped onto theme colors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CodeHighlightClass {
    /// Comment text.
    Comment,
    /// Language keyword or reserved word.
    Keyword,
    /// String literal.
    String,
    /// Numeric literal.
    Number,
    /// Type name.
    Type,
    /// Function or callable identifier.
    Function,
    /// Constant identifier.
    Constant,
    /// Variable identifier.
    Variable,
    /// Object or record property.
    Property,
    /// Operator token.
    Operator,
    /// Punctuation token.
    Punctuation,
    /// Markdown 源码：标题文字与标题记号（带级别；源码视图统一标题色）。
    MarkdownHeading(u8),
    /// Markdown 源码：结构记号（`#`、`>`、`-`、围栏、`$$` 等）。
    MarkdownMarker,
    /// Markdown 源码：强调类定界符（`**`、`*`、`~~`），与结构记号分色。
    MarkdownEmphasisMarker,
    /// Markdown 源码：加粗内容（粗体字重，不另用颜色）。
    MarkdownStrong,
    /// Markdown 源码：斜体内容（斜体字形）。
    MarkdownEmphasis,
    /// Markdown 源码：删除线内容。
    MarkdownStrikethrough,
    /// Markdown 源码：行内代码与公式正文。
    MarkdownCode,
    /// Markdown 源码：反斜杠转义的字面字符。
    MarkdownEscape,
    /// Markdown 源码：链接文字与图片替代文本。
    MarkdownLinkText,
    /// Markdown 源码：链接地址。
    MarkdownLinkUrl,
    /// Markdown 源码：标注标签（`[!NOTE]`）、标签（`#tag`）、脚注引用（`[^1]`）。
    MarkdownLabel,
}

/// Highlighted byte range inside a code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodeHighlightSpan {
    pub(crate) range: Range<usize>,
    pub(crate) class: CodeHighlightClass,
}

/// Highlight result cached on a code block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CodeHighlightResult {
    pub(crate) language: CodeLanguageKey,
    pub(crate) spans: Vec<CodeHighlightSpan>,
}

/// Language aliases accepted from fenced-code info strings.
#[derive(Clone, Copy)]
struct LanguageDescriptor {
    key: CodeLanguageKey,
    aliases: &'static [&'static str],
}

const LANGUAGE_DESCRIPTORS: &[LanguageDescriptor] = &[
    LanguageDescriptor {
        key: CodeLanguageKey::Rust,
        aliases: &["rust", "rs"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::JavaScript,
        aliases: &["javascript", "js"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::JavaScriptJsx,
        aliases: &["jsx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::TypeScript,
        aliases: &["typescript", "ts"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::TypeScriptTsx,
        aliases: &["tsx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Json,
        aliases: &["json"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Markdown,
        aliases: &["markdown", "md"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Bash,
        aliases: &["bash", "sh", "shell", "zsh"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::C,
        aliases: &["c", "h"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Cpp,
        aliases: &["cpp", "cxx", "cc", "hpp", "hxx"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::CSharp,
        aliases: &["csharp", "cs", "c#"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Css,
        aliases: &["css"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Go,
        aliases: &["go", "golang"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Html,
        aliases: &["html"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Java,
        aliases: &["java"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Php,
        aliases: &["php"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Python,
        aliases: &["python", "py"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Ruby,
        aliases: &["ruby", "rb"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Yaml,
        aliases: &["yaml", "yml"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Toml,
        aliases: &["toml"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::PlainText,
        aliases: &["text", "txt", "plain"],
    },
    LanguageDescriptor {
        key: CodeLanguageKey::Mermaid,
        aliases: &["mermaid"],
    },
];

const HIGHLIGHT_NAMES: &[&str] = &[
    "attribute",
    "comment",
    "constant",
    "constant.builtin",
    "constructor",
    "embedded",
    "function",
    "function.builtin",
    "keyword",
    "module",
    "number",
    "operator",
    "property",
    "property.builtin",
    "punctuation",
    "punctuation.bracket",
    "punctuation.delimiter",
    "punctuation.special",
    "string",
    "string.special",
    "tag",
    "type",
    "type.builtin",
    "variable",
    "variable.builtin",
    "variable.parameter",
];

/// Lazily built tree-sitter highlighter registry.
#[cfg(feature = "code-highlight-core")]
struct CodeHighlightRegistry {
    configs: HashMap<CodeLanguageKey, HighlightConfiguration>,
}

#[cfg(feature = "code-highlight-core")]
static CODE_HIGHLIGHT_REGISTRY: LazyLock<CodeHighlightRegistry> =
    LazyLock::new(CodeHighlightRegistry::new);

#[cfg(feature = "code-highlight-core")]
impl CodeHighlightRegistry {
    fn new() -> Self {
        let mut configs = HashMap::new();
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Rust, build_rust_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::JavaScript,
            build_javascript_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::JavaScriptJsx,
            build_jsx_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::TypeScript,
            build_typescript_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::TypeScriptTsx,
            build_tsx_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Json, build_json_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(
            &mut configs,
            CodeLanguageKey::Markdown,
            build_markdown_config(),
        );
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Bash, build_bash_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::C, build_c_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Cpp, build_cpp_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::CSharp, build_csharp_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Css, build_css_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Go, build_go_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Html, build_html_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Java, build_java_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Php, build_php_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Python, build_python_config());
        #[cfg(feature = "code-highlight-official")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Ruby, build_ruby_config());
        #[cfg(feature = "code-highlight-config")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Yaml, build_yaml_config());
        #[cfg(feature = "code-highlight-config")]
        maybe_insert_config(&mut configs, CodeLanguageKey::Toml, build_toml_config());
        Self { configs }
    }

    fn config_for(&self, key: CodeLanguageKey) -> Option<&HighlightConfiguration> {
        self.configs.get(&key)
    }
}

#[cfg(feature = "code-highlight-core")]
fn maybe_insert_config(
    configs: &mut HashMap<CodeLanguageKey, HighlightConfiguration>,
    key: CodeLanguageKey,
    config: Option<HighlightConfiguration>,
) {
    if let Some(config) = config {
        configs.insert(key, config);
    }
}

#[cfg(feature = "code-highlight-core")]
fn configure_highlights(
    language: tree_sitter::Language,
    name: &'static str,
    highlights_query: &str,
    injections_query: &str,
    locals_query: &str,
) -> Option<HighlightConfiguration> {
    let mut config = HighlightConfiguration::new(
        language,
        name,
        highlights_query,
        injections_query,
        locals_query,
    )
    .ok()?;
    config.configure(HIGHLIGHT_NAMES);
    Some(config)
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
fn build_rust_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_rust::LANGUAGE.into(),
        "rust",
        tree_sitter_rust::HIGHLIGHTS_QUERY,
        tree_sitter_rust::INJECTIONS_QUERY,
        "",
    )
}

fn build_javascript_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_javascript::LANGUAGE.into(),
        "javascript",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::INJECTIONS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    )
}

fn build_jsx_config() -> Option<HighlightConfiguration> {
    let query = format!(
        "{}\n{}",
        tree_sitter_javascript::HIGHLIGHT_QUERY,
        tree_sitter_javascript::JSX_HIGHLIGHT_QUERY
    );
    configure_highlights(
        tree_sitter_javascript::LANGUAGE.into(),
        "javascript",
        &query,
        tree_sitter_javascript::INJECTIONS_QUERY,
        tree_sitter_javascript::LOCALS_QUERY,
    )
}

fn build_typescript_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "typescript",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_typescript::LOCALS_QUERY,
    )
}

fn build_tsx_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_typescript::LANGUAGE_TSX.into(),
        "tsx",
        tree_sitter_typescript::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_typescript::LOCALS_QUERY,
    )
}

fn build_json_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_json::LANGUAGE.into(),
        "json",
        tree_sitter_json::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_markdown_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_md::LANGUAGE.into(),
        "markdown",
        tree_sitter_md::HIGHLIGHT_QUERY_BLOCK,
        tree_sitter_md::INJECTION_QUERY_BLOCK,
        "",
    )
}

fn build_bash_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_bash::LANGUAGE.into(),
        "bash",
        tree_sitter_bash::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_c_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_c::LANGUAGE.into(),
        "c",
        tree_sitter_c::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_cpp_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_cpp::LANGUAGE.into(),
        "cpp",
        tree_sitter_cpp::HIGHLIGHT_QUERY,
        "",
        "",
    )
}

fn build_csharp_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_c_sharp::LANGUAGE.into(),
        "c_sharp",
        tree_sitter_c_sharp::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_css_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_css::LANGUAGE.into(),
        "css",
        tree_sitter_css::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_go_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_go::LANGUAGE.into(),
        "go",
        tree_sitter_go::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_html_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_html::LANGUAGE.into(),
        "html",
        tree_sitter_html::HIGHLIGHTS_QUERY,
        tree_sitter_html::INJECTIONS_QUERY,
        "",
    )
}

fn build_java_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_java::LANGUAGE.into(),
        "java",
        tree_sitter_java::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_php_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_php::LANGUAGE_PHP.into(),
        "php",
        tree_sitter_php::HIGHLIGHTS_QUERY,
        tree_sitter_php::INJECTIONS_QUERY,
        "",
    )
}

fn build_python_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_python::LANGUAGE.into(),
        "python",
        tree_sitter_python::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn build_ruby_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_ruby::LANGUAGE.into(),
        "ruby",
        tree_sitter_ruby::HIGHLIGHTS_QUERY,
        "",
        tree_sitter_ruby::LOCALS_QUERY,
    )
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-config"))]
fn build_yaml_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_yaml::LANGUAGE.into(),
        "yaml",
        tree_sitter_yaml::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

#[cfg(all(feature = "code-highlight-core", feature = "code-highlight-config"))]
fn build_toml_config() -> Option<HighlightConfiguration> {
    configure_highlights(
        tree_sitter_toml::LANGUAGE.into(),
        "toml",
        tree_sitter_toml::HIGHLIGHTS_QUERY,
        "",
        "",
    )
}

fn descriptor_for_language(language: &str) -> Option<&'static LanguageDescriptor> {
    LANGUAGE_DESCRIPTORS.iter().find(|descriptor| {
        descriptor
            .aliases
            .iter()
            .any(|alias| alias.eq_ignore_ascii_case(language))
    })
}

pub(crate) fn resolve_code_language_key(language: Option<&str>) -> Option<CodeLanguageKey> {
    let normalized = language?
        .split_whitespace()
        .next()
        .map(str::trim)
        .filter(|value| !value.is_empty())?;

    descriptor_for_language(normalized).map(|descriptor| descriptor.key)
}

pub(crate) fn highlight_code_block(
    language: Option<&str>,
    source: &str,
) -> Option<CodeHighlightResult> {
    let key = resolve_code_language_key(language)?;

    #[cfg(feature = "code-highlight-core")]
    if let Some(config) = CODE_HIGHLIGHT_REGISTRY.config_for(key) {
        let mut highlighter = Highlighter::new();
        let events = match highlighter.highlight(config, source.as_bytes(), None, |_| None) {
            Ok(events) => events,
            Err(_) => {
                return Some(CodeHighlightResult {
                    language: key,
                    spans: Vec::new(),
                });
            }
        };

        let mut spans = Vec::new();
        let mut active = Vec::new();
        for event in events {
            let Ok(event) = event else {
                return Some(CodeHighlightResult {
                    language: key,
                    spans: Vec::new(),
                });
            };

            match event {
                HighlightEvent::Source { start, end } => {
                    if let Some(class) = active.last().copied() {
                        push_highlight_span(&mut spans, start..end, class);
                    }
                }
                HighlightEvent::HighlightStart(highlight) => {
                    if let Some(class) = class_for_highlight(highlight) {
                        active.push(class);
                    }
                }
                HighlightEvent::HighlightEnd => {
                    active.pop();
                }
            }
        }

        return Some(CodeHighlightResult {
            language: key,
            spans,
        });
    }

    Some(CodeHighlightResult {
        language: key,
        spans: Vec::new(),
    })
}

fn push_highlight_span(
    spans: &mut Vec<CodeHighlightSpan>,
    range: Range<usize>,
    class: CodeHighlightClass,
) {
    if range.start >= range.end {
        return;
    }

    if let Some(last) = spans.last_mut()
        && last.class == class
        && last.range.end == range.start
    {
        last.range.end = range.end;
        return;
    }

    spans.push(CodeHighlightSpan { range, class });
}

#[cfg(feature = "code-highlight-core")]
fn class_for_highlight(highlight: Highlight) -> Option<CodeHighlightClass> {
    let name = HIGHLIGHT_NAMES.get(highlight.0)?;
    Some(match *name {
        "comment" => CodeHighlightClass::Comment,
        "keyword" | "tag" => CodeHighlightClass::Keyword,
        "string" | "string.special" | "embedded" => CodeHighlightClass::String,
        "number" => CodeHighlightClass::Number,
        "type" | "type.builtin" | "module" => CodeHighlightClass::Type,
        "function" | "function.builtin" | "constructor" => CodeHighlightClass::Function,
        "constant" | "constant.builtin" => CodeHighlightClass::Constant,
        "variable" | "variable.builtin" | "variable.parameter" => CodeHighlightClass::Variable,
        "property" | "property.builtin" | "attribute" => CodeHighlightClass::Property,
        "operator" => CodeHighlightClass::Operator,
        "punctuation" | "punctuation.bracket" | "punctuation.delimiter" | "punctuation.special" => {
            CodeHighlightClass::Punctuation
        }
        _ => return None,
    })
}

pub(crate) fn code_highlight_color(colors: &ThemeColors, class: CodeHighlightClass) -> Hsla {
    match class {
        CodeHighlightClass::Comment => colors.code_syntax_comment,
        CodeHighlightClass::Keyword => colors.code_syntax_keyword,
        CodeHighlightClass::String => colors.code_syntax_string,
        CodeHighlightClass::Number => colors.code_syntax_number,
        CodeHighlightClass::Type => colors.code_syntax_type,
        CodeHighlightClass::Function => colors.code_syntax_function,
        CodeHighlightClass::Constant => colors.code_syntax_constant,
        CodeHighlightClass::Variable => colors.code_syntax_variable,
        CodeHighlightClass::Property => colors.code_syntax_property,
        CodeHighlightClass::Operator => colors.code_syntax_operator,
        CodeHighlightClass::Punctuation => colors.code_syntax_punctuation,
        // markdown 源码的语义色直接取主题的 md_syntax_*（用户要求：标题
        // 六级各有颜色、加粗内容有颜色、`*` 定界符保持正文色）。
        CodeHighlightClass::MarkdownHeading(level) => match level {
            1 => colors.md_syntax_heading1,
            2 => colors.md_syntax_heading2,
            3 => colors.md_syntax_heading3,
            4 => colors.md_syntax_heading4,
            5 => colors.md_syntax_heading5,
            _ => colors.md_syntax_heading6,
        },
        CodeHighlightClass::MarkdownMarker => colors.md_syntax_marker,
        // 强调定界符（`*`、`**`、`~~`）不加色：层次交给内容的字重/字形。
        CodeHighlightClass::MarkdownEmphasisMarker => colors.text_default,
        CodeHighlightClass::MarkdownStrong => colors.md_syntax_strong,
        CodeHighlightClass::MarkdownEmphasis | CodeHighlightClass::MarkdownStrikethrough => {
            colors.text_default
        }
        CodeHighlightClass::MarkdownCode => colors.md_syntax_code,
        CodeHighlightClass::MarkdownEscape => colors.md_syntax_code,
        CodeHighlightClass::MarkdownLinkText => colors.md_syntax_link_text,
        CodeHighlightClass::MarkdownLinkUrl => colors.md_syntax_link_url,
        CodeHighlightClass::MarkdownLabel => colors.md_syntax_label,
    }
}

/// class 附带的字形修饰：加粗 / 斜体 / 删除线 / 下划线。链接文字与地址按
/// 下划线（与渲染视图的链接一致），其余类不动字形。
pub(crate) fn code_highlight_font_decoration(
    class: CodeHighlightClass,
) -> (bool, bool, bool, bool) {
    match class {
        CodeHighlightClass::MarkdownStrong => (true, false, false, false),
        CodeHighlightClass::MarkdownEmphasis => (false, true, false, false),
        CodeHighlightClass::MarkdownStrikethrough => (false, false, true, false),
        CodeHighlightClass::MarkdownLinkText | CodeHighlightClass::MarkdownLinkUrl => {
            (false, false, false, true)
        }
        _ => (false, false, false, false),
    }
}

#[cfg(test)]
mod tests {
    use super::{CodeHighlightClass, CodeLanguageKey, code_highlight_color, highlight_code_block, resolve_code_language_key};
    use crate::theme::Theme;

    #[test]
    fn builtin_themes_give_markdown_syntax_distinct_colours() {
        // 用户报修两轮：满屏一个颜色不行，标题六级之间也要有区分度。
        // 谁把字段映射到同一个来源，这两条断言就红。
        let key = |color: &gpui::Hsla| {
            let rgba = gpui::Rgba::from(*color);
            ((rgba.r * 255.0).round() as u32) << 16
                | ((rgba.g * 255.0).round() as u32) << 8
                | ((rgba.b * 255.0).round() as u32)
        };
        for theme in [
            Theme::default_theme(),
            Theme::light_theme(),
            Theme::paper_theme(),
            Theme::forest_theme(),
            Theme::midnight_theme(),
            Theme::ink_theme(),
        ] {
            let headings = [
                CodeHighlightClass::MarkdownHeading(1),
                CodeHighlightClass::MarkdownHeading(2),
                CodeHighlightClass::MarkdownHeading(3),
                CodeHighlightClass::MarkdownHeading(4),
                CodeHighlightClass::MarkdownHeading(5),
                CodeHighlightClass::MarkdownHeading(6),
            ];
            let heading_keys: std::collections::BTreeSet<u32> = headings
                .iter()
                .map(|class| key(&code_highlight_color(&theme.colors, *class)))
                .collect();
            assert_eq!(
                heading_keys.len(),
                6,
                "主题 {} 的六级标题色必须互不相同: {heading_keys:?}",
                theme.name
            );

            let others = [
                CodeHighlightClass::MarkdownMarker,
                CodeHighlightClass::MarkdownStrong,
                CodeHighlightClass::MarkdownCode,
                CodeHighlightClass::MarkdownLinkText,
                CodeHighlightClass::MarkdownLinkUrl,
                CodeHighlightClass::MarkdownLabel,
            ];
            let other_keys: std::collections::BTreeSet<u32> = others
                .iter()
                .map(|class| key(&code_highlight_color(&theme.colors, *class)))
                .collect();
            assert!(
                other_keys.len() >= 5,
                "主题 {} 的加粗/代码/链接/标签色坍缩成 {} 种: {other_keys:?}",
                theme.name,
                other_keys.len()
            );
        }
    }

    #[test]
    fn emphasis_markers_render_in_default_text_colour() {
        // 用户报修：`*`/`**` 定界符不该有自己的颜色。
        for theme in [Theme::default_theme(), Theme::forest_theme()] {
            assert_eq!(
                code_highlight_color(&theme.colors, CodeHighlightClass::MarkdownEmphasisMarker),
                theme.colors.text_default
            );
        }
    }

    #[test]
    fn balanced_bundle_aliases_resolve_to_expected_keys() {
        assert_eq!(
            resolve_code_language_key(Some("rust")),
            Some(CodeLanguageKey::Rust)
        );
        assert_eq!(
            resolve_code_language_key(Some("js")),
            Some(CodeLanguageKey::JavaScript)
        );
        assert_eq!(
            resolve_code_language_key(Some("jsx")),
            Some(CodeLanguageKey::JavaScriptJsx)
        );
        assert_eq!(
            resolve_code_language_key(Some("ts")),
            Some(CodeLanguageKey::TypeScript)
        );
        assert_eq!(
            resolve_code_language_key(Some("tsx")),
            Some(CodeLanguageKey::TypeScriptTsx)
        );
        assert_eq!(
            resolve_code_language_key(Some("sh")),
            Some(CodeLanguageKey::Bash)
        );
        assert_eq!(
            resolve_code_language_key(Some("hpp")),
            Some(CodeLanguageKey::Cpp)
        );
        assert_eq!(
            resolve_code_language_key(Some("c#")),
            Some(CodeLanguageKey::CSharp)
        );
        assert_eq!(
            resolve_code_language_key(Some("golang")),
            Some(CodeLanguageKey::Go)
        );
        assert_eq!(
            resolve_code_language_key(Some("py")),
            Some(CodeLanguageKey::Python)
        );
        assert_eq!(
            resolve_code_language_key(Some("rb")),
            Some(CodeLanguageKey::Ruby)
        );
        assert_eq!(
            resolve_code_language_key(Some("yml")),
            Some(CodeLanguageKey::Yaml)
        );
        assert_eq!(
            resolve_code_language_key(Some("plain")),
            Some(CodeLanguageKey::PlainText)
        );
        assert_eq!(
            resolve_code_language_key(Some("mermaid")),
            Some(CodeLanguageKey::Mermaid)
        );
        assert_eq!(resolve_code_language_key(Some("unknown")), None);
    }

    #[test]
    fn plain_fallback_languages_produce_empty_spans() {
        let mermaid = highlight_code_block(Some("mermaid"), "graph TD;\nA-->B")
            .expect("known plain fallback should still produce a result");
        assert_eq!(mermaid.language, CodeLanguageKey::Mermaid);
        assert!(mermaid.spans.is_empty());

        let text = highlight_code_block(Some("text"), "just text")
            .expect("plain text should still produce a result");
        assert_eq!(text.language, CodeLanguageKey::PlainText);
        assert!(text.spans.is_empty());
    }

    #[test]
    fn code_extensions_without_grammars_remain_available_as_plain_text() {
        for language in ["sql", "swift", "kt", "xml"] {
            assert_eq!(resolve_code_language_key(Some(language)), None);
            assert_eq!(highlight_code_block(Some(language), "source text"), None);
        }
    }

    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-official"))]
    #[test]
    fn default_official_highlight_bundle_produces_spans() {
        let samples = [
            ("rust", "fn main() {\n    let value: i32 = 42;\n}\n"),
            ("js", "function greet(name) { return `hi ${name}`; }\n"),
            ("jsx", "const App = () => <div className=\"x\">Hi</div>;\n"),
            (
                "ts",
                "type User = { id: number };\nconst user: User = { id: 1 };\n",
            ),
            (
                "tsx",
                "const App = (): JSX.Element => <button>OK</button>;\n",
            ),
            ("json", "{\n  \"answer\": 42\n}\n"),
            ("md", "# Heading\n\n`code`\n"),
            ("bash", "echo \"hello\"\nif [ -f file ]; then echo ok; fi\n"),
            ("c", "int main(void) { return 0; }\n"),
            ("cpp", "class Box { public: int value = 1; };\n"),
            (
                "csharp",
                "class App { static void Main() { var x = 1; } }\n",
            ),
            ("css", "body { color: #fff; display: grid; }\n"),
            ("go", "package main\nfunc main() { println(\"hi\") }\n"),
            ("html", "<div class=\"card\"><span>Hi</span></div>\n"),
            (
                "java",
                "class App { int add(int a, int b) { return a + b; } }\n",
            ),
            ("php", "<?php echo \"hi\"; $x = 1; ?>\n"),
            ("python", "def double(x):\n    return x * 2\n"),
            ("ruby", "def hello(name)\n  puts \"Hi #{name}\"\nend\n"),
        ];

        for (language, sample) in samples {
            let result = highlight_code_block(Some(language), sample)
                .expect("known language should produce a result");
            assert!(
                !result.spans.is_empty(),
                "expected non-empty spans for {language}"
            );
        }
    }

    #[cfg(all(feature = "code-highlight-core", feature = "code-highlight-config"))]
    #[test]
    fn config_language_bundle_produces_spans() {
        let yaml = highlight_code_block(Some("yaml"), "key:\n  - value\n")
            .expect("yaml should produce a result");
        assert!(!yaml.spans.is_empty());

        let toml = highlight_code_block(
            Some("toml"),
            "[package]\nname = \"velora\"\nversion = \"0.1.0\"\n",
        )
        .expect("toml should produce a result");
        assert!(!toml.spans.is_empty());
    }
}
