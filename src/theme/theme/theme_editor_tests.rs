//! 主题编辑器的 token 清单生成器。
//!
//! 网页工具不自带一份 token 表：这里从 `Theme` 结构体和 `docs/theme-tokens.md`
//! 生成 `tools/theme-editor/tokens.js`，测试守住生成物与当前代码一致。这样主题
//! 新增字段时，网页不会悄悄少一项。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::theme::Theme;

use super::presets::{SERIF_FONT_FAMILY_DEFAULT, SERIF_FONT_FAMILY_MACOS};

const TOKENS_FILE: &str = "tools/theme-editor/tokens.js";
const REGENERATE_HINT: &str = "主题编辑器的 tokens.js 已过期。重新生成：\n\
    VELORA_UPDATE_THEME_EDITOR_TOKENS=1 cargo test --bin velora theme_editor_tokens -- --nocapture";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeEditorData {
    schema_version: u32,
    generated_note: String,
    groups: Vec<ThemeEditorGroup>,
    tokens: Vec<ThemeEditorToken>,
    bases: Vec<ThemeEditorBase>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeEditorGroup {
    id: String,
    section: String,
    label: Localized,
}

#[derive(Debug, Serialize)]
struct Localized {
    zh: String,
    en: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThemeEditorToken {
    path: String,
    section: String,
    key: String,
    kind: String,
    unit: String,
    step: f64,
    min: Option<f64>,
    max: Option<f64>,
    group: String,
    description: Localized,
    css_var: String,
}

#[derive(Debug, Serialize)]
struct ThemeEditorBase {
    id: String,
    name: String,
    theme: Value,
}

struct DocumentedToken {
    section: &'static str,
    key: String,
    doc_type: String,
    meaning: String,
}

struct GroupDef {
    id: &'static str,
    section: &'static str,
    zh: &'static str,
    en: &'static str,
}

const SECTIONS: [&str; 4] = ["colors", "dimensions", "typography", "placeholders"];

const GROUPS: &[GroupDef] = &[
    GroupDef { id: "colors.editor", section: "colors", zh: "编辑区与高亮", en: "Editor & highlights" },
    GroupDef { id: "colors.text", section: "colors", zh: "正文与标题", en: "Text & headings" },
    GroupDef { id: "colors.quote_callout", section: "colors", zh: "引用与提示块", en: "Quotes & callouts" },
    GroupDef { id: "colors.code", section: "colors", zh: "代码", en: "Code" },
    GroupDef { id: "colors.markdown", section: "colors", zh: "Markdown 源码高亮", en: "Markdown source" },
    GroupDef { id: "colors.table", section: "colors", zh: "表格", en: "Tables" },
    GroupDef { id: "colors.footnote", section: "colors", zh: "脚注", en: "Footnotes" },
    GroupDef { id: "colors.task", section: "colors", zh: "任务列表", en: "Task list" },
    GroupDef { id: "colors.image", section: "colors", zh: "图片", en: "Images" },
    GroupDef { id: "colors.workspace", section: "colors", zh: "工作区", en: "Workspace" },
    GroupDef { id: "colors.dialog", section: "colors", zh: "对话框", en: "Dialogs" },
    GroupDef { id: "colors.status_bar", section: "colors", zh: "状态栏", en: "Status bar" },
    GroupDef { id: "dimensions.layout", section: "dimensions", zh: "布局与节奏", en: "Layout & rhythm" },
    GroupDef { id: "dimensions.headings", section: "dimensions", zh: "标题", en: "Headings" },
    GroupDef { id: "dimensions.quote", section: "dimensions", zh: "引用", en: "Quotes" },
    GroupDef { id: "dimensions.callout", section: "dimensions", zh: "提示块", en: "Callouts" },
    GroupDef { id: "dimensions.footnote", section: "dimensions", zh: "脚注", en: "Footnotes" },
    GroupDef { id: "dimensions.separator", section: "dimensions", zh: "分隔线", en: "Separator" },
    GroupDef { id: "dimensions.code", section: "dimensions", zh: "代码", en: "Code" },
    GroupDef { id: "dimensions.table", section: "dimensions", zh: "表格", en: "Tables" },
    GroupDef { id: "dimensions.image", section: "dimensions", zh: "图片", en: "Images" },
    GroupDef { id: "dimensions.scrollbar", section: "dimensions", zh: "滚动条与版心", en: "Scrollbar & column" },
    GroupDef { id: "dimensions.dialog", section: "dimensions", zh: "对话框", en: "Dialogs" },
    GroupDef { id: "dimensions.menu", section: "dimensions", zh: "菜单", en: "Menus" },
    GroupDef { id: "dimensions.view_mode", section: "dimensions", zh: "视图切换", en: "View mode toggle" },
    GroupDef { id: "dimensions.status_bar", section: "dimensions", zh: "状态栏", en: "Status bar" },
    GroupDef { id: "dimensions.task", section: "dimensions", zh: "任务列表", en: "Task list" },
    GroupDef { id: "typography.fonts", section: "typography", zh: "字体族", en: "Font families" },
    GroupDef { id: "typography.body_text", section: "typography", zh: "正文", en: "Body text" },
    GroupDef { id: "typography.heading_text", section: "typography", zh: "标题", en: "Headings" },
    GroupDef { id: "typography.ui_text", section: "typography", zh: "代码与界面", en: "Code & UI" },
    GroupDef { id: "placeholders.placeholders", section: "placeholders", zh: "占位文案", en: "Placeholders" },
];

pub(super) fn generated_tokens_js() -> String {
    let data = build_data();
    let json = serde_json::to_string_pretty(&data).expect("主题编辑器数据应当可序列化");
    let mut lines = json.lines();
    let first = lines.next().unwrap_or("{}");
    let rest = lines
        .map(|line| format!("    {line}"))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        "// 此文件由 `src/theme/theme/theme_editor_tests.rs` 生成，请勿手工编辑。\n\
         // 重新生成：VELORA_UPDATE_THEME_EDITOR_TOKENS=1 cargo test --bin velora theme_editor_tokens -- --nocapture\n\
         (function (global) {{\n\
         \x20 const data = {first}\n\
         {rest};\n\
         \x20 global.VELORA_THEME_EDITOR_DATA = data;\n\
         \x20 if (typeof module !== 'undefined' && module.exports) {{\n\
         \x20   module.exports = data;\n\
         \x20 }}\n\
         }})(typeof globalThis !== 'undefined' ? globalThis : this);\n"
    )
}

fn build_data() -> ThemeEditorData {
    let documented = documented_tokens();
    let mut tokens = Vec::new();
    let mut used_groups = BTreeSet::new();

    for section in SECTIONS {
        for entry in documented.iter().filter(|entry| entry.section == section) {
            let group = group_for(section, &entry.key);
            used_groups.insert(group);
            let (kind, unit, step, min, max) = token_shape(section, &entry.key);
            tokens.push(ThemeEditorToken {
                path: format!("{section}.{}", entry.key),
                section: section.to_string(),
                key: entry.key.clone(),
                kind: kind.to_string(),
                unit: unit.to_string(),
                step,
                min,
                max,
                group: group.to_string(),
                description: Localized {
                    zh: entry.meaning.clone(),
                    en: humanize(&entry.key),
                },
                css_var: format!(
                    "--tok-{}-{}",
                    section,
                    entry.key.replace('.', "-")
                ),
            });
        }
    }

    let groups = GROUPS
        .iter()
        .map(|group| ThemeEditorGroup {
            id: group.id.to_string(),
            section: group.section.to_string(),
            label: Localized {
                zh: group.zh.to_string(),
                en: group.en.to_string(),
            },
        })
        .collect();

    ThemeEditorData {
        schema_version: 1,
        generated_note: "由 Velora 的 Theme 结构与 docs/theme-tokens.md 生成".into(),
        groups,
        tokens,
        bases: builtin_bases(),
    }
}

fn builtin_bases() -> Vec<ThemeEditorBase> {
    [
        ("velora-dark", "Velora Dark", Theme::default_theme()),
        ("velora-light", "Velora Light", Theme::light_theme()),
        ("paper", "Paper", Theme::paper_theme()),
        ("forest", "Forest", Theme::forest_theme()),
        ("midnight", "Midnight", Theme::midnight_theme()),
        ("ink", "Ink", Theme::ink_theme()),
    ]
    .into_iter()
    .map(|(id, name, theme)| {
        let mut value = serde_json::to_value(theme).expect("内置主题应当可序列化");
        canonicalize_platform_fonts(&mut value);
        ThemeEditorBase {
            id: id.to_string(),
            name: name.to_string(),
            theme: round_numbers(value),
        }
    })
    .collect()
}

/// Paper / Ink 的衬线字体按平台不同（macOS 宋体、其他平台 Georgia）。
/// `tokens.js` 是提交到仓库的单一快照，统一成非 macOS 值，才能让
/// macOS 与 Windows 上的生成结果一致。
fn canonicalize_platform_fonts(theme: &mut Value) {
    let Some(typography) = theme.get_mut("typography").and_then(Value::as_object_mut) else {
        return;
    };
    for key in ["body_font_family", "heading_font_family"] {
        let Some(Value::String(family)) = typography.get_mut(key) else {
            continue;
        };
        if family == SERIF_FONT_FAMILY_MACOS || family == SERIF_FONT_FAMILY_DEFAULT {
            *family = SERIF_FONT_FAMILY_DEFAULT.to_string();
        }
    }
}

/// f32 序列化会带出 `1.600000023841858` 这种尾巴；网页只做展示与导出，保留
/// 四位小数足够，也让 tokens.js 可读。
fn round_numbers(value: Value) -> Value {
    match value {
        Value::Number(number) => {
            let Some(value) = number.as_f64() else {
                return Value::Number(number);
            };
            let rounded = (value * 10_000.0).round() / 10_000.0;
            serde_json::Number::from_f64(rounded)
                .map(Value::Number)
                .unwrap_or(Value::Number(number))
        }
        Value::Array(items) => Value::Array(items.into_iter().map(round_numbers).collect()),
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (key, round_numbers(value)))
                .collect(),
        ),
        other => other,
    }
}

fn documented_tokens() -> Vec<DocumentedToken> {
    let mut section: Option<&'static str> = None;
    let mut entries = Vec::new();
    for line in include_str!("../../../docs/theme-tokens.md").lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            section = SECTIONS
                .iter()
                .copied()
                .find(|candidate| heading.starts_with(candidate));
            continue;
        }
        let Some(section) = section else { continue };
        if !trimmed.starts_with("| `") {
            continue;
        }
        let cells = trimmed.split('|').map(str::trim).collect::<Vec<_>>();
        if cells.len() < 5 {
            continue;
        }
        let key = cells[1].trim_matches('`').trim();
        if key.is_empty() {
            continue;
        }
        let doc_type = cells[2].to_string();
        let meaning = cells[3..cells.len() - 1].join(" | ");
        entries.push(DocumentedToken {
            section,
            key: key.to_string(),
            doc_type,
            meaning,
        });
    }
    entries
}

fn token_shape(section: &str, key: &str) -> (&'static str, &'static str, f64, Option<f64>, Option<f64>) {
    match section {
        "colors" => ("color", "", 1.0, None, None),
        "dimensions" => {
            if key == "centered_min_ratio" {
                ("number", "", 0.01, Some(0.0), Some(1.0))
            } else {
                ("number", "px", 1.0, Some(0.0), None)
            }
        }
        "typography" => {
            if key.ends_with("_font_family") {
                ("font", "", 1.0, None, None)
            } else if key.ends_with("_weight") {
                ("weight", "", 1.0, None, None)
            } else if key == "text_line_height" {
                ("number", "", 0.05, Some(0.5), Some(4.0))
            } else if key == "text_letter_spacing" {
                ("number", "em", 0.01, Some(-0.2), Some(0.5))
            } else {
                ("number", "px", 1.0, Some(1.0), None)
            }
        }
        "placeholders" => ("string", "", 1.0, None, None),
        _ => ("string", "", 1.0, None, None),
    }
}

/// 返回 `colors.editor` 这类带 section 的分组 id；新增 token 若没命中任何规则，
/// 会落到 `other`，由测试报错提醒补分组。
fn group_for(section: &str, key: &str) -> &'static str {
    match section {
        "colors" => {
            if key.starts_with("callout_") {
                "colors.quote_callout"
            } else if key.starts_with("text_") || key.starts_with("border_h") {
                "colors.text"
            } else if key.starts_with("border_quote") || key == "text_quote" {
                "colors.quote_callout"
            } else if key.starts_with("code_") {
                "colors.code"
            } else if key.starts_with("md_syntax_") {
                "colors.markdown"
            } else if key.starts_with("table_") {
                "colors.table"
            } else if key.starts_with("footnote_") {
                "colors.footnote"
            } else if key.starts_with("task_checkbox_") {
                "colors.task"
            } else if key.starts_with("image_") {
                "colors.image"
            } else if key == "workspace_folder_icon" {
                "colors.workspace"
            } else if key.starts_with("dialog_") {
                "colors.dialog"
            } else if key.starts_with("status_bar_") {
                "colors.status_bar"
            } else {
                "colors.editor"
            }
        }
        "dimensions" => {
            if key.starts_with("task_checkbox_") {
                "dimensions.task"
            } else if key.starts_with("h1_") || key.starts_with("h2_") {
                "dimensions.headings"
            } else if key.starts_with("quote_") {
                "dimensions.quote"
            } else if key.starts_with("callout_") {
                "dimensions.callout"
            } else if key.starts_with("footnote_") {
                "dimensions.footnote"
            } else if key.starts_with("separator_") {
                "dimensions.separator"
            } else if key.starts_with("code_") {
                "dimensions.code"
            } else if key.starts_with("table_") {
                "dimensions.table"
            } else if key.starts_with("image_") {
                "dimensions.image"
            } else if key.starts_with("scrollbar_") || key.starts_with("centered_") {
                "dimensions.scrollbar"
            } else if key.starts_with("dialog_") {
                "dimensions.dialog"
            } else if key.starts_with("menu_") || key.starts_with("context_menu_") {
                "dimensions.menu"
            } else if key.starts_with("view_mode_toggle_") {
                "dimensions.view_mode"
            } else if key.starts_with("status_bar_") {
                "dimensions.status_bar"
            } else {
                "dimensions.layout"
            }
        }
        "typography" => {
            if key.ends_with("_font_family") {
                "typography.fonts"
            } else if key.starts_with("text_") {
                "typography.body_text"
            } else if key.starts_with("h1_")
                || key.starts_with("h2_")
                || key.starts_with("h3_")
                || key.starts_with("h4_")
                || key.starts_with("h5_")
                || key.starts_with("h6_")
            {
                "typography.heading_text"
            } else {
                "typography.ui_text"
            }
        }
        "placeholders" => "placeholders.placeholders",
        _ => "other",
    }
}

fn humanize(key: &str) -> String {
    let words = key
        .split('_')
        .map(|word| match word {
            "md" => "Markdown".to_string(),
            "h1" => "H1".to_string(),
            "h2" => "H2".to_string(),
            "h3" => "H3".to_string(),
            "h4" => "H4".to_string(),
            "h5" => "H5".to_string(),
            "h6" => "H6".to_string(),
            "bg" => "background".to_string(),
            "ui" => "UI".to_string(),
            "id" => "ID".to_string(),
            "heading1" => "heading 1".to_string(),
            "heading2" => "heading 2".to_string(),
            "heading3" => "heading 3".to_string(),
            "heading4" => "heading 4".to_string(),
            "heading5" => "heading 5".to_string(),
            "heading6" => "heading 6".to_string(),
            other => other.to_string(),
        })
        .collect::<Vec<_>>();
    let mut text = words.join(" ");
    if let Some(first) = text.get_mut(0..1) {
        first.make_ascii_uppercase();
    }
    text
}

fn tokens_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(TOKENS_FILE)
}

#[test]
fn theme_editor_tokens_file_is_up_to_date() {
    let generated = generated_tokens_js();
    let path = tokens_path();
    if std::env::var_os("VELORA_UPDATE_THEME_EDITOR_TOKENS").is_some() {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("创建主题编辑器目录");
        }
        std::fs::write(&path, generated).expect("写入 tokens.js");
        return;
    }
    let committed = std::fs::read_to_string(&path).unwrap_or_else(|error| {
        panic!("读取 {} 失败：{error}\n{REGENERATE_HINT}", path.display())
    });
    // Windows 检出可能把工作区换成 CRLF；生成物本身固定 LF，比较时统一。
    assert_eq!(
        committed.replace("\r\n", "\n"),
        generated,
        "{REGENERATE_HINT}"
    );
}

#[test]
fn theme_editor_snapshot_uses_portable_fonts() {
    for base in build_data().bases {
        let typography = base
            .theme
            .get("typography")
            .and_then(Value::as_object)
            .expect("快照应当包含 typography");
        for key in ["body_font_family", "heading_font_family"] {
            let family = typography
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("{} 缺少 {key}", base.id));
            assert_ne!(
                family, SERIF_FONT_FAMILY_MACOS,
                "{} 的快照里不能出现 macOS 专有字体，否则 tokens.js 无法跨平台生成",
                base.id
            );
        }
    }
}

#[test]
fn theme_editor_tokens_cover_every_theme_field() {
    let data = build_data();
    let theme = serde_json::to_value(Theme::default_theme()).expect("主题 JSON");
    let mut expected = BTreeSet::new();
    for section in SECTIONS {
        for key in theme[section]
            .as_object()
            .expect("主题 section 应当是对象")
            .keys()
        {
            expected.insert(format!("{section}.{key}"));
        }
    }
    let actual = data
        .tokens
        .iter()
        .map(|token| token.path.clone())
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "token 清单必须与 Theme 字段一一对应");
}

#[test]
fn theme_editor_documented_types_match_the_theme_sections() {
    for entry in documented_tokens() {
        match entry.section {
            "colors" => assert_eq!(entry.doc_type, "颜色", "{} 的类型不对", entry.key),
            "dimensions" => assert!(
                entry.doc_type.starts_with("数值"),
                "{} 应当记录为数值",
                entry.key
            ),
            "typography" if entry.key.ends_with("_font_family") => {
                assert_eq!(entry.doc_type, "字符串", "{} 的类型不对", entry.key)
            }
            "typography" if entry.key.ends_with("_weight") => {
                assert_eq!(entry.doc_type, "FontWeightDef", "{} 的类型不对", entry.key)
            }
            "typography" => assert!(
                entry.doc_type.starts_with("数值"),
                "{} 应当记录为数值",
                entry.key
            ),
            "placeholders" => assert_eq!(entry.doc_type, "字符串", "{} 的类型不对", entry.key),
            other => panic!("未知 section {other}"),
        }
    }
}

#[test]
fn theme_editor_tokens_have_documentation() {
    let data = build_data();
    for token in &data.tokens {
        assert!(
            !token.description.zh.trim().is_empty(),
            "{} 缺少中文说明",
            token.path
        );
        assert!(
            !token.description.en.trim().is_empty(),
            "{} 缺少英文说明",
            token.path
        );
    }
}

#[test]
fn theme_editor_tokens_all_have_real_groups() {
    let data = build_data();
    let group_ids = data
        .groups
        .iter()
        .map(|group| group.id.as_str())
        .collect::<BTreeSet<_>>();
    for token in &data.tokens {
        assert_ne!(token.group, "other", "{} 没有分组规则", token.path);
        assert!(
            group_ids.contains(token.group.as_str()),
            "{} 指向不存在的分组 {}",
            token.path,
            token.group
        );
    }
    for group in &data.groups {
        assert!(
            data.tokens.iter().any(|token| token.group == group.id),
            "分组 {} 没有任何 token",
            group.id
        );
    }
}

#[test]
fn theme_editor_groups_appear_once_in_section_order() {
    let data = build_data();
    let mut seen = BTreeSet::new();
    for group in &data.groups {
        assert!(seen.insert(group.id.clone()), "分组 {} 重复", group.id);
    }
    let mut last_section = 0;
    for group in &data.groups {
        let index = SECTIONS
            .iter()
            .position(|section| *section == group.section)
            .expect("分组 section 必须存在");
        assert!(
            index >= last_section,
            "分组 {} 的 section 顺序不对",
            group.id
        );
        last_section = index;
    }
}

#[test]
fn theme_editor_preview_css_uses_known_tokens() {
    let data = build_data();
    let known = data
        .tokens
        .iter()
        .map(|token| token.css_var.as_str())
        .collect::<BTreeSet<_>>();
    let css = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tools/theme-editor/theme-editor.css"),
    )
    .expect("读取主题编辑器样式");
    let mut references = BTreeSet::new();
    let mut rest = css.as_str();
    while let Some(index) = rest.find("--tok-") {
        rest = &rest[index..];
        let end = rest
            .find(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-')))
            .unwrap_or(rest.len());
        references.insert(rest[..end].to_string());
        rest = &rest[end..];
    }
    assert!(
        references.len() >= 20,
        "预览样式引用的 token 太少，测试可能没覆盖到预览：{references:?}"
    );
    let unknown = references
        .iter()
        .filter(|reference| !known.contains(reference.as_str()))
        .collect::<Vec<_>>();
    assert!(
        unknown.is_empty(),
        "预览样式引用了不存在的 token：{unknown:?}"
    );
}

#[test]
fn theme_editor_full_snapshot_imports_into_app() {
    let theme = Theme::forest_theme();
    let mut theme_value = serde_json::to_value(&theme).expect("主题 JSON");
    theme_value["name"] = Value::String("Forest Snapshot".into());
    let pack = serde_json::json!({
        "name": "Forest Snapshot",
        "creator": "Velora",
        "base_theme_id": "forest",
        "theme": theme_value,
    });

    let (entry, normalized) =
        crate::theme::custom_theme_from_value(pack).expect("完整快照应当可导入");
    assert_eq!(entry.name, "Forest Snapshot");
    assert_eq!(entry.creator, "Velora");
    assert_eq!(entry.base_theme_id, "forest");
    assert_eq!(
        entry.theme.colors.editor_background,
        theme.colors.editor_background
    );
    assert_eq!(
        entry.theme.typography.body_font_family,
        theme.typography.body_font_family
    );
    assert_eq!(
        entry.theme.dimensions.writing_max_width,
        theme.dimensions.writing_max_width
    );
    assert_eq!(
        normalized
            .get("theme")
            .and_then(|theme| theme.get("colors"))
            .and_then(|colors| colors.get("editor_background")),
        Some(&serde_json::to_value(theme.colors.editor_background).expect("颜色 JSON"))
    );
}
