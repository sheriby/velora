// 此文件由 `src/theme/theme/theme_editor_tests.rs` 生成，请勿手工编辑。
// 重新生成：VELORA_UPDATE_THEME_EDITOR_TOKENS=1 cargo test --bin velora theme_editor_tokens -- --nocapture
(function (global) {
  const data = {
      "schemaVersion": 1,
      "generatedNote": "由 Velora 的 Theme 结构与 docs/theme-tokens.md 生成",
      "groups": [
        {
          "id": "colors.editor",
          "section": "colors",
          "label": {
            "zh": "编辑区与高亮",
            "en": "Editor & highlights"
          }
        },
        {
          "id": "colors.text",
          "section": "colors",
          "label": {
            "zh": "正文与标题",
            "en": "Text & headings"
          }
        },
        {
          "id": "colors.quote_callout",
          "section": "colors",
          "label": {
            "zh": "引用与提示块",
            "en": "Quotes & callouts"
          }
        },
        {
          "id": "colors.code",
          "section": "colors",
          "label": {
            "zh": "代码",
            "en": "Code"
          }
        },
        {
          "id": "colors.markdown",
          "section": "colors",
          "label": {
            "zh": "Markdown 源码高亮",
            "en": "Markdown source"
          }
        },
        {
          "id": "colors.table",
          "section": "colors",
          "label": {
            "zh": "表格",
            "en": "Tables"
          }
        },
        {
          "id": "colors.footnote",
          "section": "colors",
          "label": {
            "zh": "脚注",
            "en": "Footnotes"
          }
        },
        {
          "id": "colors.task",
          "section": "colors",
          "label": {
            "zh": "任务列表",
            "en": "Task list"
          }
        },
        {
          "id": "colors.image",
          "section": "colors",
          "label": {
            "zh": "图片",
            "en": "Images"
          }
        },
        {
          "id": "colors.workspace",
          "section": "colors",
          "label": {
            "zh": "工作区",
            "en": "Workspace"
          }
        },
        {
          "id": "colors.dialog",
          "section": "colors",
          "label": {
            "zh": "对话框",
            "en": "Dialogs"
          }
        },
        {
          "id": "colors.status_bar",
          "section": "colors",
          "label": {
            "zh": "状态栏",
            "en": "Status bar"
          }
        },
        {
          "id": "dimensions.layout",
          "section": "dimensions",
          "label": {
            "zh": "布局与节奏",
            "en": "Layout & rhythm"
          }
        },
        {
          "id": "dimensions.headings",
          "section": "dimensions",
          "label": {
            "zh": "标题",
            "en": "Headings"
          }
        },
        {
          "id": "dimensions.quote",
          "section": "dimensions",
          "label": {
            "zh": "引用",
            "en": "Quotes"
          }
        },
        {
          "id": "dimensions.callout",
          "section": "dimensions",
          "label": {
            "zh": "提示块",
            "en": "Callouts"
          }
        },
        {
          "id": "dimensions.footnote",
          "section": "dimensions",
          "label": {
            "zh": "脚注",
            "en": "Footnotes"
          }
        },
        {
          "id": "dimensions.separator",
          "section": "dimensions",
          "label": {
            "zh": "分隔线",
            "en": "Separator"
          }
        },
        {
          "id": "dimensions.code",
          "section": "dimensions",
          "label": {
            "zh": "代码",
            "en": "Code"
          }
        },
        {
          "id": "dimensions.table",
          "section": "dimensions",
          "label": {
            "zh": "表格",
            "en": "Tables"
          }
        },
        {
          "id": "dimensions.image",
          "section": "dimensions",
          "label": {
            "zh": "图片",
            "en": "Images"
          }
        },
        {
          "id": "dimensions.scrollbar",
          "section": "dimensions",
          "label": {
            "zh": "滚动条与版心",
            "en": "Scrollbar & column"
          }
        },
        {
          "id": "dimensions.dialog",
          "section": "dimensions",
          "label": {
            "zh": "对话框",
            "en": "Dialogs"
          }
        },
        {
          "id": "dimensions.menu",
          "section": "dimensions",
          "label": {
            "zh": "菜单",
            "en": "Menus"
          }
        },
        {
          "id": "dimensions.view_mode",
          "section": "dimensions",
          "label": {
            "zh": "视图切换",
            "en": "View mode toggle"
          }
        },
        {
          "id": "dimensions.status_bar",
          "section": "dimensions",
          "label": {
            "zh": "状态栏",
            "en": "Status bar"
          }
        },
        {
          "id": "dimensions.task",
          "section": "dimensions",
          "label": {
            "zh": "任务列表",
            "en": "Task list"
          }
        },
        {
          "id": "typography.fonts",
          "section": "typography",
          "label": {
            "zh": "字体族",
            "en": "Font families"
          }
        },
        {
          "id": "typography.body_text",
          "section": "typography",
          "label": {
            "zh": "正文",
            "en": "Body text"
          }
        },
        {
          "id": "typography.heading_text",
          "section": "typography",
          "label": {
            "zh": "标题",
            "en": "Headings"
          }
        },
        {
          "id": "typography.ui_text",
          "section": "typography",
          "label": {
            "zh": "代码与界面",
            "en": "Code & UI"
          }
        },
        {
          "id": "placeholders.placeholders",
          "section": "placeholders",
          "label": {
            "zh": "占位文案",
            "en": "Placeholders"
          }
        }
      ],
      "tokens": [
        {
          "path": "colors.editor_background",
          "section": "colors",
          "key": "editor_background",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "编辑器背景",
            "en": "Editor background"
          },
          "cssVar": "--tok-colors-editor_background"
        },
        {
          "path": "colors.source_mode_block_bg",
          "section": "colors",
          "key": "source_mode_block_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "源码模式块背景",
            "en": "Source mode block background"
          },
          "cssVar": "--tok-colors-source_mode_block_bg"
        },
        {
          "path": "colors.comment_bg",
          "section": "colors",
          "key": "comment_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "注释块背景",
            "en": "Comment background"
          },
          "cssVar": "--tok-colors-comment_bg"
        },
        {
          "path": "colors.text_default",
          "section": "colors",
          "key": "text_default",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "正文文字",
            "en": "Text default"
          },
          "cssVar": "--tok-colors-text_default"
        },
        {
          "path": "colors.text_link",
          "section": "colors",
          "key": "text_link",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "链接与正文粗体的强调色（标题保留自身文字色）",
            "en": "Text link"
          },
          "cssVar": "--tok-colors-text_link"
        },
        {
          "path": "colors.text_placeholder",
          "section": "colors",
          "key": "text_placeholder",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "占位文字",
            "en": "Text placeholder"
          },
          "cssVar": "--tok-colors-text_placeholder"
        },
        {
          "path": "colors.text_h1",
          "section": "colors",
          "key": "text_h1",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "一级标题文字",
            "en": "Text H1"
          },
          "cssVar": "--tok-colors-text_h1"
        },
        {
          "path": "colors.text_h2",
          "section": "colors",
          "key": "text_h2",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "二级标题文字",
            "en": "Text H2"
          },
          "cssVar": "--tok-colors-text_h2"
        },
        {
          "path": "colors.text_h3",
          "section": "colors",
          "key": "text_h3",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "三级标题文字",
            "en": "Text H3"
          },
          "cssVar": "--tok-colors-text_h3"
        },
        {
          "path": "colors.text_h4",
          "section": "colors",
          "key": "text_h4",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "四级标题文字",
            "en": "Text H4"
          },
          "cssVar": "--tok-colors-text_h4"
        },
        {
          "path": "colors.text_h5",
          "section": "colors",
          "key": "text_h5",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "五级标题文字",
            "en": "Text H5"
          },
          "cssVar": "--tok-colors-text_h5"
        },
        {
          "path": "colors.text_h6",
          "section": "colors",
          "key": "text_h6",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "六级标题文字",
            "en": "Text H6"
          },
          "cssVar": "--tok-colors-text_h6"
        },
        {
          "path": "colors.border_h1",
          "section": "colors",
          "key": "border_h1",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "一级标题下边框",
            "en": "Border H1"
          },
          "cssVar": "--tok-colors-border_h1"
        },
        {
          "path": "colors.border_h2",
          "section": "colors",
          "key": "border_h2",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "二级标题下边框",
            "en": "Border H2"
          },
          "cssVar": "--tok-colors-border_h2"
        },
        {
          "path": "colors.text_quote",
          "section": "colors",
          "key": "text_quote",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.text",
          "description": {
            "zh": "引用文字",
            "en": "Text quote"
          },
          "cssVar": "--tok-colors-text_quote"
        },
        {
          "path": "colors.border_quote",
          "section": "colors",
          "key": "border_quote",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "引用左边框",
            "en": "Border quote"
          },
          "cssVar": "--tok-colors-border_quote"
        },
        {
          "path": "colors.callout_note_bg",
          "section": "colors",
          "key": "callout_note_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Note：背景",
            "en": "Callout note background"
          },
          "cssVar": "--tok-colors-callout_note_bg"
        },
        {
          "path": "colors.callout_note_border",
          "section": "colors",
          "key": "callout_note_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Note：边框",
            "en": "Callout note border"
          },
          "cssVar": "--tok-colors-callout_note_border"
        },
        {
          "path": "colors.callout_tip_bg",
          "section": "colors",
          "key": "callout_tip_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Tip：背景",
            "en": "Callout tip background"
          },
          "cssVar": "--tok-colors-callout_tip_bg"
        },
        {
          "path": "colors.callout_tip_border",
          "section": "colors",
          "key": "callout_tip_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Tip：边框",
            "en": "Callout tip border"
          },
          "cssVar": "--tok-colors-callout_tip_border"
        },
        {
          "path": "colors.callout_important_bg",
          "section": "colors",
          "key": "callout_important_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Important：背景",
            "en": "Callout important background"
          },
          "cssVar": "--tok-colors-callout_important_bg"
        },
        {
          "path": "colors.callout_important_border",
          "section": "colors",
          "key": "callout_important_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Important：边框",
            "en": "Callout important border"
          },
          "cssVar": "--tok-colors-callout_important_border"
        },
        {
          "path": "colors.callout_warning_bg",
          "section": "colors",
          "key": "callout_warning_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Warning：背景",
            "en": "Callout warning background"
          },
          "cssVar": "--tok-colors-callout_warning_bg"
        },
        {
          "path": "colors.search_highlight_bg",
          "section": "colors",
          "key": "search_highlight_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "搜索命中高亮",
            "en": "Search highlight background"
          },
          "cssVar": "--tok-colors-search_highlight_bg"
        },
        {
          "path": "colors.search_active_highlight_bg",
          "section": "colors",
          "key": "search_active_highlight_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "搜索当前活动命中高亮（比普通命中更深）",
            "en": "Search active highlight background"
          },
          "cssVar": "--tok-colors-search_active_highlight_bg"
        },
        {
          "path": "colors.callout_warning_border",
          "section": "colors",
          "key": "callout_warning_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Warning：边框",
            "en": "Callout warning border"
          },
          "cssVar": "--tok-colors-callout_warning_border"
        },
        {
          "path": "colors.callout_caution_bg",
          "section": "colors",
          "key": "callout_caution_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Caution：背景",
            "en": "Callout caution background"
          },
          "cssVar": "--tok-colors-callout_caution_bg"
        },
        {
          "path": "colors.callout_caution_border",
          "section": "colors",
          "key": "callout_caution_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.quote_callout",
          "description": {
            "zh": "提示块 Caution：边框",
            "en": "Callout caution border"
          },
          "cssVar": "--tok-colors-callout_caution_border"
        },
        {
          "path": "colors.footnote_bg",
          "section": "colors",
          "key": "footnote_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.footnote",
          "description": {
            "zh": "脚注背景",
            "en": "Footnote background"
          },
          "cssVar": "--tok-colors-footnote_bg"
        },
        {
          "path": "colors.footnote_border",
          "section": "colors",
          "key": "footnote_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.footnote",
          "description": {
            "zh": "脚注边框",
            "en": "Footnote border"
          },
          "cssVar": "--tok-colors-footnote_border"
        },
        {
          "path": "colors.footnote_badge_bg",
          "section": "colors",
          "key": "footnote_badge_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.footnote",
          "description": {
            "zh": "脚注角标背景",
            "en": "Footnote badge background"
          },
          "cssVar": "--tok-colors-footnote_badge_bg"
        },
        {
          "path": "colors.footnote_badge_text",
          "section": "colors",
          "key": "footnote_badge_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.footnote",
          "description": {
            "zh": "脚注角标文字",
            "en": "Footnote badge text"
          },
          "cssVar": "--tok-colors-footnote_badge_text"
        },
        {
          "path": "colors.footnote_backref",
          "section": "colors",
          "key": "footnote_backref",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.footnote",
          "description": {
            "zh": "脚注返回箭头",
            "en": "Footnote backref"
          },
          "cssVar": "--tok-colors-footnote_backref"
        },
        {
          "path": "colors.task_checkbox_border",
          "section": "colors",
          "key": "task_checkbox_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.task",
          "description": {
            "zh": "任务复选框：边框",
            "en": "Task checkbox border"
          },
          "cssVar": "--tok-colors-task_checkbox_border"
        },
        {
          "path": "colors.task_checkbox_bg",
          "section": "colors",
          "key": "task_checkbox_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.task",
          "description": {
            "zh": "任务复选框：背景",
            "en": "Task checkbox background"
          },
          "cssVar": "--tok-colors-task_checkbox_bg"
        },
        {
          "path": "colors.task_checkbox_checked_bg",
          "section": "colors",
          "key": "task_checkbox_checked_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.task",
          "description": {
            "zh": "任务复选框：选中背景",
            "en": "Task checkbox checked background"
          },
          "cssVar": "--tok-colors-task_checkbox_checked_bg"
        },
        {
          "path": "colors.task_checkbox_check",
          "section": "colors",
          "key": "task_checkbox_check",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.task",
          "description": {
            "zh": "任务复选框：勾选标记",
            "en": "Task checkbox check"
          },
          "cssVar": "--tok-colors-task_checkbox_check"
        },
        {
          "path": "colors.separator_color",
          "section": "colors",
          "key": "separator_color",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "分隔线",
            "en": "Separator color"
          },
          "cssVar": "--tok-colors-separator_color"
        },
        {
          "path": "colors.code_bg",
          "section": "colors",
          "key": "code_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码块背景",
            "en": "Code background"
          },
          "cssVar": "--tok-colors-code_bg"
        },
        {
          "path": "colors.code_text",
          "section": "colors",
          "key": "code_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码块正文",
            "en": "Code text"
          },
          "cssVar": "--tok-colors-code_text"
        },
        {
          "path": "colors.code_language_input_bg",
          "section": "colors",
          "key": "code_language_input_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "语言输入框：背景",
            "en": "Code language input background"
          },
          "cssVar": "--tok-colors-code_language_input_bg"
        },
        {
          "path": "colors.code_language_input_border",
          "section": "colors",
          "key": "code_language_input_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "语言输入框：边框",
            "en": "Code language input border"
          },
          "cssVar": "--tok-colors-code_language_input_border"
        },
        {
          "path": "colors.code_language_input_text",
          "section": "colors",
          "key": "code_language_input_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "语言输入框：文字",
            "en": "Code language input text"
          },
          "cssVar": "--tok-colors-code_language_input_text"
        },
        {
          "path": "colors.code_language_input_placeholder",
          "section": "colors",
          "key": "code_language_input_placeholder",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "语言输入框：占位文字",
            "en": "Code language input placeholder"
          },
          "cssVar": "--tok-colors-code_language_input_placeholder"
        },
        {
          "path": "colors.code_syntax_comment",
          "section": "colors",
          "key": "code_syntax_comment",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：注释",
            "en": "Code syntax comment"
          },
          "cssVar": "--tok-colors-code_syntax_comment"
        },
        {
          "path": "colors.code_syntax_keyword",
          "section": "colors",
          "key": "code_syntax_keyword",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：关键字",
            "en": "Code syntax keyword"
          },
          "cssVar": "--tok-colors-code_syntax_keyword"
        },
        {
          "path": "colors.code_syntax_string",
          "section": "colors",
          "key": "code_syntax_string",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：字符串",
            "en": "Code syntax string"
          },
          "cssVar": "--tok-colors-code_syntax_string"
        },
        {
          "path": "colors.code_syntax_number",
          "section": "colors",
          "key": "code_syntax_number",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：数字",
            "en": "Code syntax number"
          },
          "cssVar": "--tok-colors-code_syntax_number"
        },
        {
          "path": "colors.code_syntax_type",
          "section": "colors",
          "key": "code_syntax_type",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：类型",
            "en": "Code syntax type"
          },
          "cssVar": "--tok-colors-code_syntax_type"
        },
        {
          "path": "colors.code_syntax_function",
          "section": "colors",
          "key": "code_syntax_function",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：函数名",
            "en": "Code syntax function"
          },
          "cssVar": "--tok-colors-code_syntax_function"
        },
        {
          "path": "colors.code_syntax_constant",
          "section": "colors",
          "key": "code_syntax_constant",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：常量",
            "en": "Code syntax constant"
          },
          "cssVar": "--tok-colors-code_syntax_constant"
        },
        {
          "path": "colors.code_syntax_variable",
          "section": "colors",
          "key": "code_syntax_variable",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：变量",
            "en": "Code syntax variable"
          },
          "cssVar": "--tok-colors-code_syntax_variable"
        },
        {
          "path": "colors.code_syntax_property",
          "section": "colors",
          "key": "code_syntax_property",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：属性",
            "en": "Code syntax property"
          },
          "cssVar": "--tok-colors-code_syntax_property"
        },
        {
          "path": "colors.code_syntax_operator",
          "section": "colors",
          "key": "code_syntax_operator",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：运算符",
            "en": "Code syntax operator"
          },
          "cssVar": "--tok-colors-code_syntax_operator"
        },
        {
          "path": "colors.code_syntax_punctuation",
          "section": "colors",
          "key": "code_syntax_punctuation",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.code",
          "description": {
            "zh": "代码高亮：标点",
            "en": "Code syntax punctuation"
          },
          "cssVar": "--tok-colors-code_syntax_punctuation"
        },
        {
          "path": "colors.md_syntax_heading1",
          "section": "colors",
          "key": "md_syntax_heading1",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：一级标题",
            "en": "Markdown syntax heading 1"
          },
          "cssVar": "--tok-colors-md_syntax_heading1"
        },
        {
          "path": "colors.md_syntax_heading2",
          "section": "colors",
          "key": "md_syntax_heading2",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：二级标题",
            "en": "Markdown syntax heading 2"
          },
          "cssVar": "--tok-colors-md_syntax_heading2"
        },
        {
          "path": "colors.md_syntax_heading3",
          "section": "colors",
          "key": "md_syntax_heading3",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：三级标题",
            "en": "Markdown syntax heading 3"
          },
          "cssVar": "--tok-colors-md_syntax_heading3"
        },
        {
          "path": "colors.md_syntax_heading4",
          "section": "colors",
          "key": "md_syntax_heading4",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：四级标题",
            "en": "Markdown syntax heading 4"
          },
          "cssVar": "--tok-colors-md_syntax_heading4"
        },
        {
          "path": "colors.md_syntax_heading5",
          "section": "colors",
          "key": "md_syntax_heading5",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：五级标题",
            "en": "Markdown syntax heading 5"
          },
          "cssVar": "--tok-colors-md_syntax_heading5"
        },
        {
          "path": "colors.md_syntax_heading6",
          "section": "colors",
          "key": "md_syntax_heading6",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：六级标题",
            "en": "Markdown syntax heading 6"
          },
          "cssVar": "--tok-colors-md_syntax_heading6"
        },
        {
          "path": "colors.md_syntax_marker",
          "section": "colors",
          "key": "md_syntax_marker",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：结构记号（列表、引用、围栏、分隔线、`$$`）",
            "en": "Markdown syntax marker"
          },
          "cssVar": "--tok-colors-md_syntax_marker"
        },
        {
          "path": "colors.md_syntax_strong",
          "section": "colors",
          "key": "md_syntax_strong",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：加粗内容（配粗体字重）",
            "en": "Markdown syntax strong"
          },
          "cssVar": "--tok-colors-md_syntax_strong"
        },
        {
          "path": "colors.md_syntax_code",
          "section": "colors",
          "key": "md_syntax_code",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：行内代码、公式正文与转义",
            "en": "Markdown syntax code"
          },
          "cssVar": "--tok-colors-md_syntax_code"
        },
        {
          "path": "colors.md_syntax_link_text",
          "section": "colors",
          "key": "md_syntax_link_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：链接文字",
            "en": "Markdown syntax link text"
          },
          "cssVar": "--tok-colors-md_syntax_link_text"
        },
        {
          "path": "colors.md_syntax_link_url",
          "section": "colors",
          "key": "md_syntax_link_url",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：链接地址",
            "en": "Markdown syntax link url"
          },
          "cssVar": "--tok-colors-md_syntax_link_url"
        },
        {
          "path": "colors.md_syntax_label",
          "section": "colors",
          "key": "md_syntax_label",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.markdown",
          "description": {
            "zh": "Markdown 源码高亮：标签、标注标签与脚注引用",
            "en": "Markdown syntax label"
          },
          "cssVar": "--tok-colors-md_syntax_label"
        },
        {
          "path": "colors.table_border",
          "section": "colors",
          "key": "table_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "表格边框",
            "en": "Table border"
          },
          "cssVar": "--tok-colors-table_border"
        },
        {
          "path": "colors.table_header_bg",
          "section": "colors",
          "key": "table_header_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "表头背景",
            "en": "Table header background"
          },
          "cssVar": "--tok-colors-table_header_bg"
        },
        {
          "path": "colors.current_line_bg",
          "section": "colors",
          "key": "current_line_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "当前行高亮背景",
            "en": "Current line background"
          },
          "cssVar": "--tok-colors-current_line_bg"
        },
        {
          "path": "colors.matching_bracket_bg",
          "section": "colors",
          "key": "matching_bracket_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "括号匹配高亮背景",
            "en": "Matching bracket background"
          },
          "cssVar": "--tok-colors-matching_bracket_bg"
        },
        {
          "path": "colors.table_cell_bg",
          "section": "colors",
          "key": "table_cell_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "单元格背景",
            "en": "Table cell background"
          },
          "cssVar": "--tok-colors-table_cell_bg"
        },
        {
          "path": "colors.table_cell_active_outline",
          "section": "colors",
          "key": "table_cell_active_outline",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "表格：活动单元格描边",
            "en": "Table cell active outline"
          },
          "cssVar": "--tok-colors-table_cell_active_outline"
        },
        {
          "path": "colors.table_axis_preview_bg",
          "section": "colors",
          "key": "table_axis_preview_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "表格：行列预览背景",
            "en": "Table axis preview background"
          },
          "cssVar": "--tok-colors-table_axis_preview_bg"
        },
        {
          "path": "colors.table_axis_selected_bg",
          "section": "colors",
          "key": "table_axis_selected_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "表格：行列选中背景",
            "en": "Table axis selected background"
          },
          "cssVar": "--tok-colors-table_axis_selected_bg"
        },
        {
          "path": "colors.table_append_button_bg",
          "section": "colors",
          "key": "table_append_button_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "表格：追加按钮背景",
            "en": "Table append button background"
          },
          "cssVar": "--tok-colors-table_append_button_bg"
        },
        {
          "path": "colors.table_append_button_hover",
          "section": "colors",
          "key": "table_append_button_hover",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "表格：追加按钮悬停",
            "en": "Table append button hover"
          },
          "cssVar": "--tok-colors-table_append_button_hover"
        },
        {
          "path": "colors.table_append_button_text",
          "section": "colors",
          "key": "table_append_button_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.table",
          "description": {
            "zh": "表格：追加按钮文字",
            "en": "Table append button text"
          },
          "cssVar": "--tok-colors-table_append_button_text"
        },
        {
          "path": "colors.image_placeholder_bg",
          "section": "colors",
          "key": "image_placeholder_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.image",
          "description": {
            "zh": "图片占位背景",
            "en": "Image placeholder background"
          },
          "cssVar": "--tok-colors-image_placeholder_bg"
        },
        {
          "path": "colors.image_placeholder_border",
          "section": "colors",
          "key": "image_placeholder_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.image",
          "description": {
            "zh": "图片占位边框",
            "en": "Image placeholder border"
          },
          "cssVar": "--tok-colors-image_placeholder_border"
        },
        {
          "path": "colors.image_placeholder_text",
          "section": "colors",
          "key": "image_placeholder_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.image",
          "description": {
            "zh": "图片占位文字",
            "en": "Image placeholder text"
          },
          "cssVar": "--tok-colors-image_placeholder_text"
        },
        {
          "path": "colors.image_caption_text",
          "section": "colors",
          "key": "image_caption_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.image",
          "description": {
            "zh": "图注文字",
            "en": "Image caption text"
          },
          "cssVar": "--tok-colors-image_caption_text"
        },
        {
          "path": "colors.scrollbar_thumb",
          "section": "colors",
          "key": "scrollbar_thumb",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "滚动条滑块",
            "en": "Scrollbar thumb"
          },
          "cssVar": "--tok-colors-scrollbar_thumb"
        },
        {
          "path": "colors.cursor",
          "section": "colors",
          "key": "cursor",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "光标",
            "en": "Cursor"
          },
          "cssVar": "--tok-colors-cursor"
        },
        {
          "path": "colors.workspace_folder_icon",
          "section": "colors",
          "key": "workspace_folder_icon",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.workspace",
          "description": {
            "zh": "蓝色样式的文件夹图标，默认沿用原有蓝色",
            "en": "Workspace folder icon"
          },
          "cssVar": "--tok-colors-workspace_folder_icon"
        },
        {
          "path": "colors.selection",
          "section": "colors",
          "key": "selection",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.editor",
          "description": {
            "zh": "选区",
            "en": "Selection"
          },
          "cssVar": "--tok-colors-selection"
        },
        {
          "path": "colors.dialog_backdrop",
          "section": "colors",
          "key": "dialog_backdrop",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框遮罩",
            "en": "Dialog backdrop"
          },
          "cssVar": "--tok-colors-dialog_backdrop"
        },
        {
          "path": "colors.dialog_surface",
          "section": "colors",
          "key": "dialog_surface",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框面板",
            "en": "Dialog surface"
          },
          "cssVar": "--tok-colors-dialog_surface"
        },
        {
          "path": "colors.dialog_border",
          "section": "colors",
          "key": "dialog_border",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框边框",
            "en": "Dialog border"
          },
          "cssVar": "--tok-colors-dialog_border"
        },
        {
          "path": "colors.dialog_title",
          "section": "colors",
          "key": "dialog_title",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框标题",
            "en": "Dialog title"
          },
          "cssVar": "--tok-colors-dialog_title"
        },
        {
          "path": "colors.dialog_body",
          "section": "colors",
          "key": "dialog_body",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "界面主文字与对话框正文；Kit 控件、导航和标签共用，区别于文档 `text_default`",
            "en": "Dialog body"
          },
          "cssVar": "--tok-colors-dialog_body"
        },
        {
          "path": "colors.dialog_muted",
          "section": "colors",
          "key": "dialog_muted",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框次要文字",
            "en": "Dialog muted"
          },
          "cssVar": "--tok-colors-dialog_muted"
        },
        {
          "path": "colors.dialog_primary_button_bg",
          "section": "colors",
          "key": "dialog_primary_button_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：primary_button_bg",
            "en": "Dialog primary button background"
          },
          "cssVar": "--tok-colors-dialog_primary_button_bg"
        },
        {
          "path": "colors.dialog_primary_button_hover",
          "section": "colors",
          "key": "dialog_primary_button_hover",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：primary_button_hover",
            "en": "Dialog primary button hover"
          },
          "cssVar": "--tok-colors-dialog_primary_button_hover"
        },
        {
          "path": "colors.dialog_primary_button_text",
          "section": "colors",
          "key": "dialog_primary_button_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：primary_button_text",
            "en": "Dialog primary button text"
          },
          "cssVar": "--tok-colors-dialog_primary_button_text"
        },
        {
          "path": "colors.dialog_secondary_button_bg",
          "section": "colors",
          "key": "dialog_secondary_button_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：secondary_button_bg",
            "en": "Dialog secondary button background"
          },
          "cssVar": "--tok-colors-dialog_secondary_button_bg"
        },
        {
          "path": "colors.dialog_secondary_button_hover",
          "section": "colors",
          "key": "dialog_secondary_button_hover",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：secondary_button_hover",
            "en": "Dialog secondary button hover"
          },
          "cssVar": "--tok-colors-dialog_secondary_button_hover"
        },
        {
          "path": "colors.dialog_secondary_button_text",
          "section": "colors",
          "key": "dialog_secondary_button_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：secondary_button_text",
            "en": "Dialog secondary button text"
          },
          "cssVar": "--tok-colors-dialog_secondary_button_text"
        },
        {
          "path": "colors.dialog_danger_button_bg",
          "section": "colors",
          "key": "dialog_danger_button_bg",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：danger_button_bg",
            "en": "Dialog danger button background"
          },
          "cssVar": "--tok-colors-dialog_danger_button_bg"
        },
        {
          "path": "colors.dialog_danger_button_hover",
          "section": "colors",
          "key": "dialog_danger_button_hover",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：danger_button_hover",
            "en": "Dialog danger button hover"
          },
          "cssVar": "--tok-colors-dialog_danger_button_hover"
        },
        {
          "path": "colors.dialog_danger_button_text",
          "section": "colors",
          "key": "dialog_danger_button_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.dialog",
          "description": {
            "zh": "对话框：danger_button_text",
            "en": "Dialog danger button text"
          },
          "cssVar": "--tok-colors-dialog_danger_button_text"
        },
        {
          "path": "colors.status_bar_background",
          "section": "colors",
          "key": "status_bar_background",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.status_bar",
          "description": {
            "zh": "状态栏背景",
            "en": "Status bar background"
          },
          "cssVar": "--tok-colors-status_bar_background"
        },
        {
          "path": "colors.status_bar_text",
          "section": "colors",
          "key": "status_bar_text",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.status_bar",
          "description": {
            "zh": "状态栏文字",
            "en": "Status bar text"
          },
          "cssVar": "--tok-colors-status_bar_text"
        },
        {
          "path": "colors.status_bar_text_dim",
          "section": "colors",
          "key": "status_bar_text_dim",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.status_bar",
          "description": {
            "zh": "状态栏次要文字",
            "en": "Status bar text dim"
          },
          "cssVar": "--tok-colors-status_bar_text_dim"
        },
        {
          "path": "colors.status_bar_button_hover",
          "section": "colors",
          "key": "status_bar_button_hover",
          "kind": "color",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "colors.status_bar",
          "description": {
            "zh": "状态栏按钮悬停",
            "en": "Status bar button hover"
          },
          "cssVar": "--tok-colors-status_bar_button_hover"
        },
        {
          "path": "dimensions.editor_padding",
          "section": "dimensions",
          "key": "editor_padding",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "编辑器四周内边距",
            "en": "Editor padding"
          },
          "cssVar": "--tok-dimensions-editor_padding"
        },
        {
          "path": "dimensions.writing_max_width",
          "section": "dimensions",
          "key": "writing_max_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "正文最大宽度（仅写作列宽选「跟随主题」时生效）",
            "en": "Writing max width"
          },
          "cssVar": "--tok-dimensions-writing_max_width"
        },
        {
          "path": "dimensions.block_gap",
          "section": "dimensions",
          "key": "block_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "块间距",
            "en": "Block gap"
          },
          "cssVar": "--tok-dimensions-block_gap"
        },
        {
          "path": "dimensions.block_min_height",
          "section": "dimensions",
          "key": "block_min_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "块最小高度",
            "en": "Block min height"
          },
          "cssVar": "--tok-dimensions-block_min_height"
        },
        {
          "path": "dimensions.block_padding_y",
          "section": "dimensions",
          "key": "block_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "块垂直内边距",
            "en": "Block padding y"
          },
          "cssVar": "--tok-dimensions-block_padding_y"
        },
        {
          "path": "dimensions.block_padding_x",
          "section": "dimensions",
          "key": "block_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "块水平内边距",
            "en": "Block padding x"
          },
          "cssVar": "--tok-dimensions-block_padding_x"
        },
        {
          "path": "dimensions.nested_block_indent",
          "section": "dimensions",
          "key": "nested_block_indent",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "嵌套缩进",
            "en": "Nested block indent"
          },
          "cssVar": "--tok-dimensions-nested_block_indent"
        },
        {
          "path": "dimensions.list_marker_gap",
          "section": "dimensions",
          "key": "list_marker_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "列表标记：标记与内容间距",
            "en": "List marker gap"
          },
          "cssVar": "--tok-dimensions-list_marker_gap"
        },
        {
          "path": "dimensions.list_marker_width",
          "section": "dimensions",
          "key": "list_marker_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "列表标记：标记宽度",
            "en": "List marker width"
          },
          "cssVar": "--tok-dimensions-list_marker_width"
        },
        {
          "path": "dimensions.ordered_list_marker_width",
          "section": "dimensions",
          "key": "ordered_list_marker_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "有序列表标记：宽度",
            "en": "Ordered list marker width"
          },
          "cssVar": "--tok-dimensions-ordered_list_marker_width"
        },
        {
          "path": "dimensions.task_checkbox_size",
          "section": "dimensions",
          "key": "task_checkbox_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.task",
          "description": {
            "zh": "任务复选框：尺寸",
            "en": "Task checkbox size"
          },
          "cssVar": "--tok-dimensions-task_checkbox_size"
        },
        {
          "path": "dimensions.task_checkbox_radius",
          "section": "dimensions",
          "key": "task_checkbox_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.task",
          "description": {
            "zh": "任务复选框：圆角",
            "en": "Task checkbox radius"
          },
          "cssVar": "--tok-dimensions-task_checkbox_radius"
        },
        {
          "path": "dimensions.task_checkbox_border_width",
          "section": "dimensions",
          "key": "task_checkbox_border_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.task",
          "description": {
            "zh": "任务复选框：边框宽度",
            "en": "Task checkbox border width"
          },
          "cssVar": "--tok-dimensions-task_checkbox_border_width"
        },
        {
          "path": "dimensions.task_checkbox_check_size",
          "section": "dimensions",
          "key": "task_checkbox_check_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.task",
          "description": {
            "zh": "任务复选框：勾选标记尺寸",
            "en": "Task checkbox check size"
          },
          "cssVar": "--tok-dimensions-task_checkbox_check_size"
        },
        {
          "path": "dimensions.h1_padding_bottom",
          "section": "dimensions",
          "key": "h1_padding_bottom",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.headings",
          "description": {
            "zh": "一级标题：下内边距",
            "en": "H1 padding bottom"
          },
          "cssVar": "--tok-dimensions-h1_padding_bottom"
        },
        {
          "path": "dimensions.h1_margin_bottom",
          "section": "dimensions",
          "key": "h1_margin_bottom",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.headings",
          "description": {
            "zh": "一级标题：下外边距",
            "en": "H1 margin bottom"
          },
          "cssVar": "--tok-dimensions-h1_margin_bottom"
        },
        {
          "path": "dimensions.cursor_width",
          "section": "dimensions",
          "key": "cursor_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "光标宽度",
            "en": "Cursor width"
          },
          "cssVar": "--tok-dimensions-cursor_width"
        },
        {
          "path": "dimensions.underline_thickness",
          "section": "dimensions",
          "key": "underline_thickness",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.layout",
          "description": {
            "zh": "下划线粗细",
            "en": "Underline thickness"
          },
          "cssVar": "--tok-dimensions-underline_thickness"
        },
        {
          "path": "dimensions.h1_border_width",
          "section": "dimensions",
          "key": "h1_border_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.headings",
          "description": {
            "zh": "一级标题：下边框宽度",
            "en": "H1 border width"
          },
          "cssVar": "--tok-dimensions-h1_border_width"
        },
        {
          "path": "dimensions.quote_border_width",
          "section": "dimensions",
          "key": "quote_border_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.quote",
          "description": {
            "zh": "引用：左边框宽度",
            "en": "Quote border width"
          },
          "cssVar": "--tok-dimensions-quote_border_width"
        },
        {
          "path": "dimensions.quote_padding_left",
          "section": "dimensions",
          "key": "quote_padding_left",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.quote",
          "description": {
            "zh": "引用：左内边距",
            "en": "Quote padding left"
          },
          "cssVar": "--tok-dimensions-quote_padding_left"
        },
        {
          "path": "dimensions.callout_padding_x",
          "section": "dimensions",
          "key": "callout_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.callout",
          "description": {
            "zh": "提示块：水平内边距",
            "en": "Callout padding x"
          },
          "cssVar": "--tok-dimensions-callout_padding_x"
        },
        {
          "path": "dimensions.callout_padding_y",
          "section": "dimensions",
          "key": "callout_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.callout",
          "description": {
            "zh": "提示块：垂直内边距",
            "en": "Callout padding y"
          },
          "cssVar": "--tok-dimensions-callout_padding_y"
        },
        {
          "path": "dimensions.callout_body_gap",
          "section": "dimensions",
          "key": "callout_body_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.callout",
          "description": {
            "zh": "提示块：正文间距",
            "en": "Callout body gap"
          },
          "cssVar": "--tok-dimensions-callout_body_gap"
        },
        {
          "path": "dimensions.callout_radius",
          "section": "dimensions",
          "key": "callout_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.callout",
          "description": {
            "zh": "提示块：圆角",
            "en": "Callout radius"
          },
          "cssVar": "--tok-dimensions-callout_radius"
        },
        {
          "path": "dimensions.callout_border_width",
          "section": "dimensions",
          "key": "callout_border_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.callout",
          "description": {
            "zh": "提示块：边框宽度",
            "en": "Callout border width"
          },
          "cssVar": "--tok-dimensions-callout_border_width"
        },
        {
          "path": "dimensions.callout_header_gap",
          "section": "dimensions",
          "key": "callout_header_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.callout",
          "description": {
            "zh": "提示块：标题间距",
            "en": "Callout header gap"
          },
          "cssVar": "--tok-dimensions-callout_header_gap"
        },
        {
          "path": "dimensions.callout_header_margin_bottom",
          "section": "dimensions",
          "key": "callout_header_margin_bottom",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.callout",
          "description": {
            "zh": "提示块：标题下外边距",
            "en": "Callout header margin bottom"
          },
          "cssVar": "--tok-dimensions-callout_header_margin_bottom"
        },
        {
          "path": "dimensions.footnote_padding_x",
          "section": "dimensions",
          "key": "footnote_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.footnote",
          "description": {
            "zh": "脚注：水平内边距",
            "en": "Footnote padding x"
          },
          "cssVar": "--tok-dimensions-footnote_padding_x"
        },
        {
          "path": "dimensions.footnote_padding_y",
          "section": "dimensions",
          "key": "footnote_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.footnote",
          "description": {
            "zh": "脚注：垂直内边距",
            "en": "Footnote padding y"
          },
          "cssVar": "--tok-dimensions-footnote_padding_y"
        },
        {
          "path": "dimensions.footnote_radius",
          "section": "dimensions",
          "key": "footnote_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.footnote",
          "description": {
            "zh": "脚注：圆角",
            "en": "Footnote radius"
          },
          "cssVar": "--tok-dimensions-footnote_radius"
        },
        {
          "path": "dimensions.footnote_badge_padding_x",
          "section": "dimensions",
          "key": "footnote_badge_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.footnote",
          "description": {
            "zh": "脚注：角标水平内边距",
            "en": "Footnote badge padding x"
          },
          "cssVar": "--tok-dimensions-footnote_badge_padding_x"
        },
        {
          "path": "dimensions.footnote_badge_padding_y",
          "section": "dimensions",
          "key": "footnote_badge_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.footnote",
          "description": {
            "zh": "脚注：角标垂直内边距",
            "en": "Footnote badge padding y"
          },
          "cssVar": "--tok-dimensions-footnote_badge_padding_y"
        },
        {
          "path": "dimensions.separator_thickness",
          "section": "dimensions",
          "key": "separator_thickness",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.separator",
          "description": {
            "zh": "分隔线：线宽",
            "en": "Separator thickness"
          },
          "cssVar": "--tok-dimensions-separator_thickness"
        },
        {
          "path": "dimensions.separator_inset_x",
          "section": "dimensions",
          "key": "separator_inset_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.separator",
          "description": {
            "zh": "分隔线：水平内缩",
            "en": "Separator inset x"
          },
          "cssVar": "--tok-dimensions-separator_inset_x"
        },
        {
          "path": "dimensions.separator_margin_y",
          "section": "dimensions",
          "key": "separator_margin_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.separator",
          "description": {
            "zh": "分隔线：垂直外边距",
            "en": "Separator margin y"
          },
          "cssVar": "--tok-dimensions-separator_margin_y"
        },
        {
          "path": "dimensions.code_block_padding_y",
          "section": "dimensions",
          "key": "code_block_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "代码块：垂直内边距",
            "en": "Code block padding y"
          },
          "cssVar": "--tok-dimensions-code_block_padding_y"
        },
        {
          "path": "dimensions.code_block_padding_x",
          "section": "dimensions",
          "key": "code_block_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "代码块：水平内边距",
            "en": "Code block padding x"
          },
          "cssVar": "--tok-dimensions-code_block_padding_x"
        },
        {
          "path": "dimensions.code_bg_pad_x",
          "section": "dimensions",
          "key": "code_bg_pad_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "代码：背景水平内边距",
            "en": "Code background pad x"
          },
          "cssVar": "--tok-dimensions-code_bg_pad_x"
        },
        {
          "path": "dimensions.code_bg_pad_y",
          "section": "dimensions",
          "key": "code_bg_pad_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "代码：背景垂直内边距",
            "en": "Code background pad y"
          },
          "cssVar": "--tok-dimensions-code_bg_pad_y"
        },
        {
          "path": "dimensions.code_bg_radius",
          "section": "dimensions",
          "key": "code_bg_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "代码：背景圆角",
            "en": "Code background radius"
          },
          "cssVar": "--tok-dimensions-code_bg_radius"
        },
        {
          "path": "dimensions.code_language_input_width",
          "section": "dimensions",
          "key": "code_language_input_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "语言输入框：宽度",
            "en": "Code language input width"
          },
          "cssVar": "--tok-dimensions-code_language_input_width"
        },
        {
          "path": "dimensions.code_language_input_height",
          "section": "dimensions",
          "key": "code_language_input_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "语言输入框：高度",
            "en": "Code language input height"
          },
          "cssVar": "--tok-dimensions-code_language_input_height"
        },
        {
          "path": "dimensions.code_language_input_padding_x",
          "section": "dimensions",
          "key": "code_language_input_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "语言输入框：水平内边距",
            "en": "Code language input padding x"
          },
          "cssVar": "--tok-dimensions-code_language_input_padding_x"
        },
        {
          "path": "dimensions.code_language_input_padding_y",
          "section": "dimensions",
          "key": "code_language_input_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "语言输入框：垂直内边距",
            "en": "Code language input padding y"
          },
          "cssVar": "--tok-dimensions-code_language_input_padding_y"
        },
        {
          "path": "dimensions.code_language_input_radius",
          "section": "dimensions",
          "key": "code_language_input_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "语言输入框：圆角",
            "en": "Code language input radius"
          },
          "cssVar": "--tok-dimensions-code_language_input_radius"
        },
        {
          "path": "dimensions.code_language_input_border_width",
          "section": "dimensions",
          "key": "code_language_input_border_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "语言输入框：边框宽度",
            "en": "Code language input border width"
          },
          "cssVar": "--tok-dimensions-code_language_input_border_width"
        },
        {
          "path": "dimensions.code_language_input_gap",
          "section": "dimensions",
          "key": "code_language_input_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.code",
          "description": {
            "zh": "语言输入框：与代码间距",
            "en": "Code language input gap"
          },
          "cssVar": "--tok-dimensions-code_language_input_gap"
        },
        {
          "path": "dimensions.table_cell_padding_x",
          "section": "dimensions",
          "key": "table_cell_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：单元格水平内边距",
            "en": "Table cell padding x"
          },
          "cssVar": "--tok-dimensions-table_cell_padding_x"
        },
        {
          "path": "dimensions.table_cell_padding_y",
          "section": "dimensions",
          "key": "table_cell_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：单元格垂直内边距",
            "en": "Table cell padding y"
          },
          "cssVar": "--tok-dimensions-table_cell_padding_y"
        },
        {
          "path": "dimensions.table_cell_min_height",
          "section": "dimensions",
          "key": "table_cell_min_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：单元格最小高度",
            "en": "Table cell min height"
          },
          "cssVar": "--tok-dimensions-table_cell_min_height"
        },
        {
          "path": "dimensions.table_append_button_extent",
          "section": "dimensions",
          "key": "table_append_button_extent",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：追加按钮尺寸",
            "en": "Table append button extent"
          },
          "cssVar": "--tok-dimensions-table_append_button_extent"
        },
        {
          "path": "dimensions.table_append_button_inset",
          "section": "dimensions",
          "key": "table_append_button_inset",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：追加按钮偏移",
            "en": "Table append button inset"
          },
          "cssVar": "--tok-dimensions-table_append_button_inset"
        },
        {
          "path": "dimensions.table_append_activation_band",
          "section": "dimensions",
          "key": "table_append_activation_band",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：追加激活带宽度",
            "en": "Table append activation band"
          },
          "cssVar": "--tok-dimensions-table_append_activation_band"
        },
        {
          "path": "dimensions.image_radius",
          "section": "dimensions",
          "key": "image_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.image",
          "description": {
            "zh": "图片：圆角",
            "en": "Image radius"
          },
          "cssVar": "--tok-dimensions-image_radius"
        },
        {
          "path": "dimensions.image_root_max_height",
          "section": "dimensions",
          "key": "image_root_max_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.image",
          "description": {
            "zh": "图片：最大高度，默认 0，按原图比例排版、不设固定上限；数学公式在值为 0 时仍限高 420px",
            "en": "Image root max height"
          },
          "cssVar": "--tok-dimensions-image_root_max_height"
        },
        {
          "path": "dimensions.image_root_max_width",
          "section": "dimensions",
          "key": "image_root_max_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.image",
          "description": {
            "zh": "图片：根级最大宽度，默认 0，以正文容器宽度的 95% 作为缩放 100% 的基准；始终受容器宽度约束",
            "en": "Image root max width"
          },
          "cssVar": "--tok-dimensions-image_root_max_width"
        },
        {
          "path": "dimensions.image_cell_max_height",
          "section": "dimensions",
          "key": "image_cell_max_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.image",
          "description": {
            "zh": "图片：单元格内最大高度",
            "en": "Image cell max height"
          },
          "cssVar": "--tok-dimensions-image_cell_max_height"
        },
        {
          "path": "dimensions.image_root_placeholder_height",
          "section": "dimensions",
          "key": "image_root_placeholder_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.image",
          "description": {
            "zh": "图片：根级占位高度",
            "en": "Image root placeholder height"
          },
          "cssVar": "--tok-dimensions-image_root_placeholder_height"
        },
        {
          "path": "dimensions.image_cell_placeholder_height",
          "section": "dimensions",
          "key": "image_cell_placeholder_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.image",
          "description": {
            "zh": "图片：单元格占位高度",
            "en": "Image cell placeholder height"
          },
          "cssVar": "--tok-dimensions-image_cell_placeholder_height"
        },
        {
          "path": "dimensions.image_caption_gap",
          "section": "dimensions",
          "key": "image_caption_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.image",
          "description": {
            "zh": "图片：图注间距",
            "en": "Image caption gap"
          },
          "cssVar": "--tok-dimensions-image_caption_gap"
        },
        {
          "path": "dimensions.scrollbar_width",
          "section": "dimensions",
          "key": "scrollbar_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.scrollbar",
          "description": {
            "zh": "滚动条：宽度",
            "en": "Scrollbar width"
          },
          "cssVar": "--tok-dimensions-scrollbar_width"
        },
        {
          "path": "dimensions.scrollbar_right",
          "section": "dimensions",
          "key": "scrollbar_right",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.scrollbar",
          "description": {
            "zh": "滚动条：距右偏移",
            "en": "Scrollbar right"
          },
          "cssVar": "--tok-dimensions-scrollbar_right"
        },
        {
          "path": "dimensions.centered_shrink_start",
          "section": "dimensions",
          "key": "centered_shrink_start",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.scrollbar",
          "description": {
            "zh": "居中版心：收缩起点",
            "en": "Centered shrink start"
          },
          "cssVar": "--tok-dimensions-centered_shrink_start"
        },
        {
          "path": "dimensions.centered_shrink_end",
          "section": "dimensions",
          "key": "centered_shrink_end",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.scrollbar",
          "description": {
            "zh": "居中版心：收缩终点",
            "en": "Centered shrink end"
          },
          "cssVar": "--tok-dimensions-centered_shrink_end"
        },
        {
          "path": "dimensions.centered_min_ratio",
          "section": "dimensions",
          "key": "centered_min_ratio",
          "kind": "number",
          "unit": "",
          "step": 0.01,
          "min": 0.0,
          "max": 1.0,
          "group": "dimensions.scrollbar",
          "description": {
            "zh": "居中版心：最小占比",
            "en": "Centered min ratio"
          },
          "cssVar": "--tok-dimensions-centered_min_ratio"
        },
        {
          "path": "dimensions.dialog_width",
          "section": "dimensions",
          "key": "dialog_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.dialog",
          "description": {
            "zh": "对话框：宽度",
            "en": "Dialog width"
          },
          "cssVar": "--tok-dimensions-dialog_width"
        },
        {
          "path": "dimensions.dialog_padding",
          "section": "dimensions",
          "key": "dialog_padding",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.dialog",
          "description": {
            "zh": "对话框：内边距",
            "en": "Dialog padding"
          },
          "cssVar": "--tok-dimensions-dialog_padding"
        },
        {
          "path": "dimensions.dialog_gap",
          "section": "dimensions",
          "key": "dialog_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.dialog",
          "description": {
            "zh": "对话框：间距",
            "en": "Dialog gap"
          },
          "cssVar": "--tok-dimensions-dialog_gap"
        },
        {
          "path": "dimensions.dialog_radius",
          "section": "dimensions",
          "key": "dialog_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.dialog",
          "description": {
            "zh": "对话框：圆角",
            "en": "Dialog radius"
          },
          "cssVar": "--tok-dimensions-dialog_radius"
        },
        {
          "path": "dimensions.dialog_border_width",
          "section": "dimensions",
          "key": "dialog_border_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.dialog",
          "description": {
            "zh": "对话框：边框宽度",
            "en": "Dialog border width"
          },
          "cssVar": "--tok-dimensions-dialog_border_width"
        },
        {
          "path": "dimensions.dialog_button_height",
          "section": "dimensions",
          "key": "dialog_button_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.dialog",
          "description": {
            "zh": "对话框：按钮高度",
            "en": "Dialog button height"
          },
          "cssVar": "--tok-dimensions-dialog_button_height"
        },
        {
          "path": "dimensions.dialog_button_gap",
          "section": "dimensions",
          "key": "dialog_button_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.dialog",
          "description": {
            "zh": "对话框：按钮间距",
            "en": "Dialog button gap"
          },
          "cssVar": "--tok-dimensions-dialog_button_gap"
        },
        {
          "path": "dimensions.dialog_button_padding_x",
          "section": "dimensions",
          "key": "dialog_button_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.dialog",
          "description": {
            "zh": "对话框：按钮水平内边距",
            "en": "Dialog button padding x"
          },
          "cssVar": "--tok-dimensions-dialog_button_padding_x"
        },
        {
          "path": "dimensions.menu_bar_height",
          "section": "dimensions",
          "key": "menu_bar_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单栏高度",
            "en": "Menu bar height"
          },
          "cssVar": "--tok-dimensions-menu_bar_height"
        },
        {
          "path": "dimensions.menu_bar_padding_x",
          "section": "dimensions",
          "key": "menu_bar_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单栏水平内边距",
            "en": "Menu bar padding x"
          },
          "cssVar": "--tok-dimensions-menu_bar_padding_x"
        },
        {
          "path": "dimensions.menu_bar_padding_y",
          "section": "dimensions",
          "key": "menu_bar_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单栏垂直内边距",
            "en": "Menu bar padding y"
          },
          "cssVar": "--tok-dimensions-menu_bar_padding_y"
        },
        {
          "path": "dimensions.menu_bar_gap",
          "section": "dimensions",
          "key": "menu_bar_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单栏项间距",
            "en": "Menu bar gap"
          },
          "cssVar": "--tok-dimensions-menu_bar_gap"
        },
        {
          "path": "dimensions.menu_bar_button_width",
          "section": "dimensions",
          "key": "menu_bar_button_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单栏按钮宽度",
            "en": "Menu bar button width"
          },
          "cssVar": "--tok-dimensions-menu_bar_button_width"
        },
        {
          "path": "dimensions.menu_bar_button_height",
          "section": "dimensions",
          "key": "menu_bar_button_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单栏按钮高度",
            "en": "Menu bar button height"
          },
          "cssVar": "--tok-dimensions-menu_bar_button_height"
        },
        {
          "path": "dimensions.menu_bar_button_padding_x",
          "section": "dimensions",
          "key": "menu_bar_button_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单栏按钮水平内边距",
            "en": "Menu bar button padding x"
          },
          "cssVar": "--tok-dimensions-menu_bar_button_padding_x"
        },
        {
          "path": "dimensions.menu_bar_button_radius",
          "section": "dimensions",
          "key": "menu_bar_button_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单栏按钮圆角",
            "en": "Menu bar button radius"
          },
          "cssVar": "--tok-dimensions-menu_bar_button_radius"
        },
        {
          "path": "dimensions.menu_text_size",
          "section": "dimensions",
          "key": "menu_text_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单字号",
            "en": "Menu text size"
          },
          "cssVar": "--tok-dimensions-menu_text_size"
        },
        {
          "path": "dimensions.menu_panel_top",
          "section": "dimensions",
          "key": "menu_panel_top",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：面板顶部偏移",
            "en": "Menu panel top"
          },
          "cssVar": "--tok-dimensions-menu_panel_top"
        },
        {
          "path": "dimensions.menu_panel_width",
          "section": "dimensions",
          "key": "menu_panel_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：面板宽度",
            "en": "Menu panel width"
          },
          "cssVar": "--tok-dimensions-menu_panel_width"
        },
        {
          "path": "dimensions.menu_panel_padding",
          "section": "dimensions",
          "key": "menu_panel_padding",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：面板内边距",
            "en": "Menu panel padding"
          },
          "cssVar": "--tok-dimensions-menu_panel_padding"
        },
        {
          "path": "dimensions.menu_panel_gap",
          "section": "dimensions",
          "key": "menu_panel_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：面板项间距",
            "en": "Menu panel gap"
          },
          "cssVar": "--tok-dimensions-menu_panel_gap"
        },
        {
          "path": "dimensions.menu_panel_radius",
          "section": "dimensions",
          "key": "menu_panel_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：面板圆角",
            "en": "Menu panel radius"
          },
          "cssVar": "--tok-dimensions-menu_panel_radius"
        },
        {
          "path": "dimensions.menu_item_height",
          "section": "dimensions",
          "key": "menu_item_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单项高度",
            "en": "Menu item height"
          },
          "cssVar": "--tok-dimensions-menu_item_height"
        },
        {
          "path": "dimensions.menu_item_padding_x",
          "section": "dimensions",
          "key": "menu_item_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单项水平内边距",
            "en": "Menu item padding x"
          },
          "cssVar": "--tok-dimensions-menu_item_padding_x"
        },
        {
          "path": "dimensions.menu_item_radius",
          "section": "dimensions",
          "key": "menu_item_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：菜单项圆角",
            "en": "Menu item radius"
          },
          "cssVar": "--tok-dimensions-menu_item_radius"
        },
        {
          "path": "dimensions.menu_separator_margin_x",
          "section": "dimensions",
          "key": "menu_separator_margin_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：分隔线水平外边距",
            "en": "Menu separator margin x"
          },
          "cssVar": "--tok-dimensions-menu_separator_margin_x"
        },
        {
          "path": "dimensions.menu_separator_margin_y",
          "section": "dimensions",
          "key": "menu_separator_margin_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：分隔线垂直外边距",
            "en": "Menu separator margin y"
          },
          "cssVar": "--tok-dimensions-menu_separator_margin_y"
        },
        {
          "path": "dimensions.menu_separator_height",
          "section": "dimensions",
          "key": "menu_separator_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "菜单：分隔线高度",
            "en": "Menu separator height"
          },
          "cssVar": "--tok-dimensions-menu_separator_height"
        },
        {
          "path": "dimensions.context_menu_panel_width",
          "section": "dimensions",
          "key": "context_menu_panel_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "右键菜单：面板宽度",
            "en": "Context menu panel width"
          },
          "cssVar": "--tok-dimensions-context_menu_panel_width"
        },
        {
          "path": "dimensions.context_menu_submenu_width",
          "section": "dimensions",
          "key": "context_menu_submenu_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "右键菜单：子菜单宽度",
            "en": "Context menu submenu width"
          },
          "cssVar": "--tok-dimensions-context_menu_submenu_width"
        },
        {
          "path": "dimensions.context_menu_submenu_gap",
          "section": "dimensions",
          "key": "context_menu_submenu_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "右键菜单：子菜单偏移",
            "en": "Context menu submenu gap"
          },
          "cssVar": "--tok-dimensions-context_menu_submenu_gap"
        },
        {
          "path": "dimensions.context_menu_axis_panel_width",
          "section": "dimensions",
          "key": "context_menu_axis_panel_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.menu",
          "description": {
            "zh": "右键菜单：行列面板宽度",
            "en": "Context menu axis panel width"
          },
          "cssVar": "--tok-dimensions-context_menu_axis_panel_width"
        },
        {
          "path": "dimensions.table_insert_dialog_width",
          "section": "dimensions",
          "key": "table_insert_dialog_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：插入对话框宽度",
            "en": "Table insert dialog width"
          },
          "cssVar": "--tok-dimensions-table_insert_dialog_width"
        },
        {
          "path": "dimensions.table_insert_stepper_gap",
          "section": "dimensions",
          "key": "table_insert_stepper_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：insert_stepper_gap",
            "en": "Table insert stepper gap"
          },
          "cssVar": "--tok-dimensions-table_insert_stepper_gap"
        },
        {
          "path": "dimensions.table_insert_stepper_button_size",
          "section": "dimensions",
          "key": "table_insert_stepper_button_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：insert_stepper_button_size",
            "en": "Table insert stepper button size"
          },
          "cssVar": "--tok-dimensions-table_insert_stepper_button_size"
        },
        {
          "path": "dimensions.table_insert_stepper_value_min_width",
          "section": "dimensions",
          "key": "table_insert_stepper_value_min_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：insert_stepper_value_min_width",
            "en": "Table insert stepper value min width"
          },
          "cssVar": "--tok-dimensions-table_insert_stepper_value_min_width"
        },
        {
          "path": "dimensions.table_insert_stepper_value_padding_x",
          "section": "dimensions",
          "key": "table_insert_stepper_value_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：insert_stepper_value_padding_x",
            "en": "Table insert stepper value padding x"
          },
          "cssVar": "--tok-dimensions-table_insert_stepper_value_padding_x"
        },
        {
          "path": "dimensions.table_insert_stepper_radius",
          "section": "dimensions",
          "key": "table_insert_stepper_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.table",
          "description": {
            "zh": "表格：insert_stepper_radius",
            "en": "Table insert stepper radius"
          },
          "cssVar": "--tok-dimensions-table_insert_stepper_radius"
        },
        {
          "path": "dimensions.view_mode_toggle_left",
          "section": "dimensions",
          "key": "view_mode_toggle_left",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.view_mode",
          "description": {
            "zh": "视图切换按钮：左偏移",
            "en": "View mode toggle left"
          },
          "cssVar": "--tok-dimensions-view_mode_toggle_left"
        },
        {
          "path": "dimensions.view_mode_toggle_bottom",
          "section": "dimensions",
          "key": "view_mode_toggle_bottom",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.view_mode",
          "description": {
            "zh": "视图切换按钮：底偏移",
            "en": "View mode toggle bottom"
          },
          "cssVar": "--tok-dimensions-view_mode_toggle_bottom"
        },
        {
          "path": "dimensions.view_mode_toggle_padding_x",
          "section": "dimensions",
          "key": "view_mode_toggle_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.view_mode",
          "description": {
            "zh": "视图切换按钮：水平内边距",
            "en": "View mode toggle padding x"
          },
          "cssVar": "--tok-dimensions-view_mode_toggle_padding_x"
        },
        {
          "path": "dimensions.view_mode_toggle_padding_y",
          "section": "dimensions",
          "key": "view_mode_toggle_padding_y",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.view_mode",
          "description": {
            "zh": "视图切换按钮：垂直内边距",
            "en": "View mode toggle padding y"
          },
          "cssVar": "--tok-dimensions-view_mode_toggle_padding_y"
        },
        {
          "path": "dimensions.view_mode_toggle_min_width",
          "section": "dimensions",
          "key": "view_mode_toggle_min_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.view_mode",
          "description": {
            "zh": "视图切换按钮：最小宽度",
            "en": "View mode toggle min width"
          },
          "cssVar": "--tok-dimensions-view_mode_toggle_min_width"
        },
        {
          "path": "dimensions.view_mode_toggle_radius",
          "section": "dimensions",
          "key": "view_mode_toggle_radius",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.view_mode",
          "description": {
            "zh": "视图切换按钮：圆角",
            "en": "View mode toggle radius"
          },
          "cssVar": "--tok-dimensions-view_mode_toggle_radius"
        },
        {
          "path": "dimensions.view_mode_toggle_border_width",
          "section": "dimensions",
          "key": "view_mode_toggle_border_width",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.view_mode",
          "description": {
            "zh": "视图切换按钮：边框宽度",
            "en": "View mode toggle border width"
          },
          "cssVar": "--tok-dimensions-view_mode_toggle_border_width"
        },
        {
          "path": "dimensions.view_mode_toggle_text_size",
          "section": "dimensions",
          "key": "view_mode_toggle_text_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.view_mode",
          "description": {
            "zh": "视图切换按钮：字号",
            "en": "View mode toggle text size"
          },
          "cssVar": "--tok-dimensions-view_mode_toggle_text_size"
        },
        {
          "path": "dimensions.status_bar_height",
          "section": "dimensions",
          "key": "status_bar_height",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.status_bar",
          "description": {
            "zh": "状态栏：高度",
            "en": "Status bar height"
          },
          "cssVar": "--tok-dimensions-status_bar_height"
        },
        {
          "path": "dimensions.status_bar_padding_x",
          "section": "dimensions",
          "key": "status_bar_padding_x",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.status_bar",
          "description": {
            "zh": "状态栏：水平内边距",
            "en": "Status bar padding x"
          },
          "cssVar": "--tok-dimensions-status_bar_padding_x"
        },
        {
          "path": "dimensions.status_bar_item_gap",
          "section": "dimensions",
          "key": "status_bar_item_gap",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.status_bar",
          "description": {
            "zh": "状态栏：项间距",
            "en": "Status bar item gap"
          },
          "cssVar": "--tok-dimensions-status_bar_item_gap"
        },
        {
          "path": "dimensions.status_bar_text_size",
          "section": "dimensions",
          "key": "status_bar_text_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 0.0,
          "max": null,
          "group": "dimensions.status_bar",
          "description": {
            "zh": "状态栏：字号",
            "en": "Status bar text size"
          },
          "cssVar": "--tok-dimensions-status_bar_text_size"
        },
        {
          "path": "typography.body_font_family",
          "section": "typography",
          "key": "body_font_family",
          "kind": "font",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.fonts",
          "description": {
            "zh": "正文字体族",
            "en": "Body font family"
          },
          "cssVar": "--tok-typography-body_font_family"
        },
        {
          "path": "typography.heading_font_family",
          "section": "typography",
          "key": "heading_font_family",
          "kind": "font",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.fonts",
          "description": {
            "zh": "标题字体族",
            "en": "Heading font family"
          },
          "cssVar": "--tok-typography-heading_font_family"
        },
        {
          "path": "typography.text_size",
          "section": "typography",
          "key": "text_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.body_text",
          "description": {
            "zh": "正文字号",
            "en": "Text size"
          },
          "cssVar": "--tok-typography-text_size"
        },
        {
          "path": "typography.text_line_height",
          "section": "typography",
          "key": "text_line_height",
          "kind": "number",
          "unit": "",
          "step": 0.05,
          "min": 0.5,
          "max": 4.0,
          "group": "typography.body_text",
          "description": {
            "zh": "正文行高（倍数）",
            "en": "Text line height"
          },
          "cssVar": "--tok-typography-text_line_height"
        },
        {
          "path": "typography.text_letter_spacing",
          "section": "typography",
          "key": "text_letter_spacing",
          "kind": "number",
          "unit": "em",
          "step": 0.01,
          "min": -0.2,
          "max": 0.5,
          "group": "typography.body_text",
          "description": {
            "zh": "正文字距；不拉伸代码内部字符",
            "en": "Text letter spacing"
          },
          "cssVar": "--tok-typography-text_letter_spacing"
        },
        {
          "path": "typography.h1_size",
          "section": "typography",
          "key": "h1_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "一级标题字号",
            "en": "H1 size"
          },
          "cssVar": "--tok-typography-h1_size"
        },
        {
          "path": "typography.h1_weight",
          "section": "typography",
          "key": "h1_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "一级标题字重",
            "en": "H1 weight"
          },
          "cssVar": "--tok-typography-h1_weight"
        },
        {
          "path": "typography.h2_size",
          "section": "typography",
          "key": "h2_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "二级标题字号",
            "en": "H2 size"
          },
          "cssVar": "--tok-typography-h2_size"
        },
        {
          "path": "typography.h2_weight",
          "section": "typography",
          "key": "h2_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "二级标题字重",
            "en": "H2 weight"
          },
          "cssVar": "--tok-typography-h2_weight"
        },
        {
          "path": "typography.h3_size",
          "section": "typography",
          "key": "h3_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "三级标题字号",
            "en": "H3 size"
          },
          "cssVar": "--tok-typography-h3_size"
        },
        {
          "path": "typography.h3_weight",
          "section": "typography",
          "key": "h3_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "三级标题字重",
            "en": "H3 weight"
          },
          "cssVar": "--tok-typography-h3_weight"
        },
        {
          "path": "typography.h4_size",
          "section": "typography",
          "key": "h4_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "四级标题字号",
            "en": "H4 size"
          },
          "cssVar": "--tok-typography-h4_size"
        },
        {
          "path": "typography.h4_weight",
          "section": "typography",
          "key": "h4_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "四级标题字重",
            "en": "H4 weight"
          },
          "cssVar": "--tok-typography-h4_weight"
        },
        {
          "path": "typography.h5_size",
          "section": "typography",
          "key": "h5_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "五级标题字号",
            "en": "H5 size"
          },
          "cssVar": "--tok-typography-h5_size"
        },
        {
          "path": "typography.h5_weight",
          "section": "typography",
          "key": "h5_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "五级标题字重",
            "en": "H5 weight"
          },
          "cssVar": "--tok-typography-h5_weight"
        },
        {
          "path": "typography.h6_size",
          "section": "typography",
          "key": "h6_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "六级标题字号",
            "en": "H6 size"
          },
          "cssVar": "--tok-typography-h6_size"
        },
        {
          "path": "typography.h6_weight",
          "section": "typography",
          "key": "h6_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.heading_text",
          "description": {
            "zh": "六级标题字重",
            "en": "H6 weight"
          },
          "cssVar": "--tok-typography-h6_weight"
        },
        {
          "path": "typography.code_size",
          "section": "typography",
          "key": "code_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.ui_text",
          "description": {
            "zh": "代码字号",
            "en": "Code size"
          },
          "cssVar": "--tok-typography-code_size"
        },
        {
          "path": "typography.dialog_title_size",
          "section": "typography",
          "key": "dialog_title_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.ui_text",
          "description": {
            "zh": "对话框标题字号",
            "en": "Dialog title size"
          },
          "cssVar": "--tok-typography-dialog_title_size"
        },
        {
          "path": "typography.dialog_title_weight",
          "section": "typography",
          "key": "dialog_title_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.ui_text",
          "description": {
            "zh": "对话框标题字重",
            "en": "Dialog title weight"
          },
          "cssVar": "--tok-typography-dialog_title_weight"
        },
        {
          "path": "typography.dialog_body_size",
          "section": "typography",
          "key": "dialog_body_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.ui_text",
          "description": {
            "zh": "对话框正文字号",
            "en": "Dialog body size"
          },
          "cssVar": "--tok-typography-dialog_body_size"
        },
        {
          "path": "typography.dialog_body_weight",
          "section": "typography",
          "key": "dialog_body_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.ui_text",
          "description": {
            "zh": "对话框正文字重",
            "en": "Dialog body weight"
          },
          "cssVar": "--tok-typography-dialog_body_weight"
        },
        {
          "path": "typography.dialog_button_size",
          "section": "typography",
          "key": "dialog_button_size",
          "kind": "number",
          "unit": "px",
          "step": 1.0,
          "min": 1.0,
          "max": null,
          "group": "typography.ui_text",
          "description": {
            "zh": "对话框按钮字号",
            "en": "Dialog button size"
          },
          "cssVar": "--tok-typography-dialog_button_size"
        },
        {
          "path": "typography.dialog_button_weight",
          "section": "typography",
          "key": "dialog_button_weight",
          "kind": "weight",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "typography.ui_text",
          "description": {
            "zh": "对话框按钮字重",
            "en": "Dialog button weight"
          },
          "cssVar": "--tok-typography-dialog_button_weight"
        },
        {
          "path": "placeholders.empty_editing",
          "section": "placeholders",
          "key": "empty_editing",
          "kind": "string",
          "unit": "",
          "step": 1.0,
          "min": null,
          "max": null,
          "group": "placeholders.placeholders",
          "description": {
            "zh": "空文档占位提示文案",
            "en": "Empty editing"
          },
          "cssVar": "--tok-placeholders-empty_editing"
        }
      ],
      "bases": [
        {
          "id": "velora-dark",
          "name": "Velora Dark",
          "theme": {
            "name": "Velora Dark",
            "colors": {
              "editor_background": "#1b1d24ff",
              "source_mode_block_bg": "#292929ff",
              "comment_bg": "#fce10026",
              "text_default": "#e8e9eeff",
              "text_link": "#75beffff",
              "text_placeholder": "#9c9c9cff",
              "text_h1": "#f5f5f5ff",
              "text_h2": "#f5f5f5ff",
              "text_h3": "#f5f5f5ff",
              "text_h4": "#f5f5f5ff",
              "text_h5": "#f5f5f5ff",
              "text_h6": "#f5f5f5ff",
              "border_h1": "#484644ff",
              "border_h2": "#3b3a39ff",
              "text_quote": "#d6d6d6ff",
              "border_quote": "#aaa0ffff",
              "callout_note_bg": "#94a3b81f",
              "callout_note_border": "#94a3b4ff",
              "callout_tip_bg": "#4cc2ff1f",
              "callout_tip_border": "#4cc2ffff",
              "callout_important_bg": "#a78bfa1f",
              "callout_important_border": "#a78bfaff",
              "callout_warning_bg": "#fce1001f",
              "search_highlight_bg": "#ffe06638",
              "search_active_highlight_bg": "#ff9500b4",
              "callout_warning_border": "#fce100ff",
              "callout_caution_bg": "#d134381f",
              "callout_caution_border": "#d13438ff",
              "footnote_bg": "#292929ff",
              "footnote_border": "#484644ff",
              "footnote_badge_bg": "#3b3a39ff",
              "footnote_badge_text": "#d6d6d6ff",
              "footnote_backref": "#75beffff",
              "task_checkbox_border": "#8a8886ff",
              "task_checkbox_bg": "#00000000",
              "task_checkbox_checked_bg": "#4cc2ffff",
              "task_checkbox_check": "#1f1f1fff",
              "separator_color": "#5a5a5aff",
              "code_bg": "#252832ff",
              "code_text": "#d6d6d6ff",
              "code_language_input_bg": "#333333ff",
              "code_language_input_border": "#484644ff",
              "code_language_input_text": "#f5f5f5ff",
              "code_language_input_placeholder": "#9c9c9cff",
              "code_syntax_comment": "#858585ff",
              "code_syntax_keyword": "#c586c0ff",
              "code_syntax_string": "#ce9178ff",
              "code_syntax_number": "#b5cea8ff",
              "code_syntax_type": "#4ec9b0ff",
              "code_syntax_function": "#dcdcaaff",
              "code_syntax_constant": "#4fc1ffff",
              "code_syntax_variable": "#9cdcfeff",
              "code_syntax_property": "#9cdcfeff",
              "code_syntax_operator": "#d4d4d4ff",
              "code_syntax_punctuation": "#d4d4d4ff",
              "md_syntax_heading1": "#569cd6ff",
              "md_syntax_heading2": "#4ec9b0ff",
              "md_syntax_heading3": "#c586c0ff",
              "md_syntax_heading4": "#dcdcaaff",
              "md_syntax_heading5": "#b5cea8ff",
              "md_syntax_heading6": "#9cdcfeff",
              "md_syntax_marker": "#8a8a8aff",
              "md_syntax_strong": "#e5c07bff",
              "md_syntax_code": "#ce9178ff",
              "md_syntax_link_text": "#3794ffff",
              "md_syntax_link_url": "#ce9178ff",
              "md_syntax_label": "#c586c0ff",
              "table_border": "#484644ff",
              "table_header_bg": "#333333ff",
              "current_line_bg": "#ffffff0d",
              "matching_bracket_bg": "#ffffff1a",
              "table_cell_bg": "#292929ff",
              "table_cell_active_outline": "#4cc2ffff",
              "table_axis_preview_bg": "#f5f5f51a",
              "table_axis_selected_bg": "#f5f5f533",
              "table_append_button_bg": "#333333ff",
              "table_append_button_hover": "#484644ff",
              "table_append_button_text": "#f5f5f5ff",
              "image_placeholder_bg": "#292929ff",
              "image_placeholder_border": "#484644ff",
              "image_placeholder_text": "#d6d6d6ff",
              "image_caption_text": "#a19f9dff",
              "scrollbar_thumb": "#a19f9dcc",
              "cursor": "#f5f5f5ff",
              "selection": "#403a61ff",
              "workspace_folder_icon": "#4a93d8ff",
              "dialog_backdrop": "#00000088",
              "dialog_surface": "#1b1d24ff",
              "dialog_border": "#333640ff",
              "dialog_title": "#f5f5f5ff",
              "dialog_body": "#d6d6d6ff",
              "dialog_muted": "#a0a4afff",
              "dialog_primary_button_bg": "#aaa0ffff",
              "dialog_primary_button_hover": "#b9afffff",
              "dialog_primary_button_text": "#1f1f1fff",
              "dialog_secondary_button_bg": "#20232bff",
              "dialog_secondary_button_hover": "#292c35ff",
              "dialog_secondary_button_text": "#c5c9d1ff",
              "dialog_danger_button_bg": "#d13438ff",
              "dialog_danger_button_hover": "#a4262cff",
              "dialog_danger_button_text": "#ffffffff",
              "status_bar_background": "#20232bff",
              "status_bar_text": "#d6d6d6ff",
              "status_bar_text_dim": "#a19f9dff",
              "status_bar_button_hover": "#484644ff"
            },
            "dimensions": {
              "editor_padding": 24.0,
              "writing_max_width": 760.0,
              "block_gap": 6.0,
              "block_min_height": 28.0,
              "block_padding_y": 4.0,
              "block_padding_x": 12.0,
              "nested_block_indent": 20.0,
              "list_marker_gap": 8.0,
              "list_marker_width": 24.0,
              "ordered_list_marker_width": 24.0,
              "task_checkbox_size": 14.0,
              "task_checkbox_radius": 2.0,
              "task_checkbox_border_width": 1.0,
              "task_checkbox_check_size": 10.0,
              "h1_padding_bottom": 0.0,
              "h1_margin_bottom": 4.0,
              "cursor_width": 2.0,
              "underline_thickness": 1.0,
              "h1_border_width": 0.0,
              "quote_border_width": 3.0,
              "quote_padding_left": 12.0,
              "callout_padding_x": 14.0,
              "callout_padding_y": 10.0,
              "callout_body_gap": 8.0,
              "callout_radius": 10.0,
              "callout_border_width": 4.0,
              "callout_header_gap": 6.0,
              "callout_header_margin_bottom": 6.0,
              "footnote_padding_x": 10.0,
              "footnote_padding_y": 6.0,
              "footnote_radius": 6.0,
              "footnote_badge_padding_x": 4.0,
              "footnote_badge_padding_y": 1.0,
              "separator_thickness": 1.0,
              "separator_inset_x": 40.0,
              "separator_margin_y": 10.0,
              "code_block_padding_y": 8.0,
              "code_block_padding_x": 12.0,
              "code_bg_pad_x": 4.0,
              "code_bg_pad_y": 2.0,
              "code_bg_radius": 4.0,
              "code_language_input_width": 156.0,
              "code_language_input_height": 18.0,
              "code_language_input_padding_x": 8.0,
              "code_language_input_padding_y": 3.0,
              "code_language_input_radius": 6.0,
              "code_language_input_border_width": 1.0,
              "code_language_input_gap": 8.0,
              "table_cell_padding_x": 10.0,
              "table_cell_padding_y": 8.0,
              "table_cell_min_height": 42.0,
              "table_append_button_extent": 16.0,
              "table_append_button_inset": 8.0,
              "table_append_activation_band": 18.0,
              "image_radius": 12.0,
              "image_root_max_height": 0.0,
              "image_root_max_width": 0.0,
              "image_cell_max_height": 180.0,
              "image_root_placeholder_height": 260.0,
              "image_cell_placeholder_height": 120.0,
              "image_caption_gap": 8.0,
              "scrollbar_width": 6.0,
              "scrollbar_right": 6.0,
              "centered_shrink_start": 1100.0,
              "centered_shrink_end": 2200.0,
              "centered_min_ratio": 0.58,
              "dialog_width": 460.0,
              "dialog_padding": 20.0,
              "dialog_gap": 14.0,
              "dialog_radius": 14.0,
              "dialog_border_width": 1.0,
              "dialog_button_height": 36.0,
              "dialog_button_gap": 10.0,
              "dialog_button_padding_x": 14.0,
              "menu_bar_height": 32.0,
              "menu_bar_padding_x": 10.0,
              "menu_bar_padding_y": 4.0,
              "menu_bar_gap": 2.0,
              "menu_bar_button_width": 48.0,
              "menu_bar_button_height": 24.0,
              "menu_bar_button_padding_x": 8.0,
              "menu_bar_button_radius": 5.0,
              "menu_text_size": 12.0,
              "menu_panel_top": 30.0,
              "menu_panel_width": 180.0,
              "menu_panel_padding": 4.0,
              "menu_panel_gap": 1.0,
              "menu_panel_radius": 8.0,
              "menu_item_height": 28.0,
              "menu_item_padding_x": 8.0,
              "menu_item_radius": 5.0,
              "menu_separator_margin_x": 6.0,
              "menu_separator_margin_y": 3.0,
              "menu_separator_height": 1.0,
              "context_menu_panel_width": 132.0,
              "context_menu_submenu_width": 148.0,
              "context_menu_submenu_gap": 2.0,
              "context_menu_axis_panel_width": 164.0,
              "table_insert_dialog_width": 380.0,
              "table_insert_stepper_gap": 8.0,
              "table_insert_stepper_button_size": 32.0,
              "table_insert_stepper_value_min_width": 56.0,
              "table_insert_stepper_value_padding_x": 10.0,
              "table_insert_stepper_radius": 8.0,
              "view_mode_toggle_left": 12.0,
              "view_mode_toggle_bottom": 12.0,
              "view_mode_toggle_padding_x": 8.0,
              "view_mode_toggle_padding_y": 4.0,
              "view_mode_toggle_min_width": 88.0,
              "view_mode_toggle_radius": 999.0,
              "view_mode_toggle_border_width": 1.0,
              "view_mode_toggle_text_size": 11.0,
              "status_bar_height": 28.0,
              "status_bar_padding_x": 12.0,
              "status_bar_item_gap": 12.0,
              "status_bar_text_size": 11.0
            },
            "typography": {
              "body_font_family": ".SystemUIFont",
              "heading_font_family": ".SystemUIFont",
              "text_size": 17.0,
              "text_line_height": 1.6,
              "text_letter_spacing": 0.0125,
              "h1_size": 36.0,
              "h1_weight": "bold",
              "h2_size": 30.0,
              "h2_weight": "bold",
              "h3_size": 24.0,
              "h3_weight": "bold",
              "h4_size": 18.0,
              "h4_weight": "semibold",
              "h5_size": 16.0,
              "h5_weight": "semibold",
              "h6_size": 14.0,
              "h6_weight": "semibold",
              "code_size": 15.0,
              "dialog_title_size": 20.0,
              "dialog_title_weight": "semibold",
              "dialog_body_size": 14.0,
              "dialog_body_weight": "normal",
              "dialog_button_size": 14.0,
              "dialog_button_weight": "medium"
            },
            "placeholders": {
              "empty_editing": ""
            }
          }
        },
        {
          "id": "velora-light",
          "name": "Velora Light",
          "theme": {
            "name": "Velora Light",
            "colors": {
              "editor_background": "#ffffffff",
              "source_mode_block_bg": "#f3f2f1ff",
              "comment_bg": "#fff4ce99",
              "text_default": "#252832ff",
              "text_link": "#6558d3ff",
              "text_placeholder": "#8a8886cc",
              "text_h1": "#201f1eff",
              "text_h2": "#201f1eff",
              "text_h3": "#201f1eff",
              "text_h4": "#201f1eff",
              "text_h5": "#201f1eff",
              "text_h6": "#201f1eff",
              "border_h1": "#d1d1d1ff",
              "border_h2": "#edebe9ff",
              "text_quote": "#484644ff",
              "border_quote": "#6558d3ff",
              "callout_note_bg": "#0078d414",
              "callout_note_border": "#0078d4ff",
              "callout_tip_bg": "#107c1014",
              "callout_tip_border": "#107c10ff",
              "callout_important_bg": "#8764b814",
              "callout_important_border": "#8764b8ff",
              "callout_warning_bg": "#ca501014",
              "search_highlight_bg": "#ffd60a4d",
              "search_active_highlight_bg": "#ff9500b4",
              "callout_warning_border": "#ca5010ff",
              "callout_caution_bg": "#d1343814",
              "callout_caution_border": "#d13438ff",
              "footnote_bg": "#ffffffff",
              "footnote_border": "#d1d1d1ff",
              "footnote_badge_bg": "#f3f2f1ff",
              "footnote_badge_text": "#484644ff",
              "footnote_backref": "#0078d4ff",
              "task_checkbox_border": "#8a8886ff",
              "task_checkbox_bg": "#ffffffff",
              "task_checkbox_checked_bg": "#6558d3ff",
              "task_checkbox_check": "#ffffffff",
              "separator_color": "#d1d1d1ff",
              "code_bg": "#f2f3f6ff",
              "code_text": "#242424ff",
              "code_language_input_bg": "#ffffffff",
              "code_language_input_border": "#d1d1d1ff",
              "code_language_input_text": "#242424ff",
              "code_language_input_placeholder": "#8a8886cc",
              "code_syntax_comment": "#6a6a6aff",
              "code_syntax_keyword": "#af00dbff",
              "code_syntax_string": "#008000ff",
              "code_syntax_number": "#098658ff",
              "code_syntax_type": "#267f99ff",
              "code_syntax_function": "#795e26ff",
              "code_syntax_constant": "#0070c1ff",
              "code_syntax_variable": "#001080ff",
              "code_syntax_property": "#001080ff",
              "code_syntax_operator": "#393a34ff",
              "code_syntax_punctuation": "#393a34ff",
              "md_syntax_heading1": "#0451a5ff",
              "md_syntax_heading2": "#098658ff",
              "md_syntax_heading3": "#af00dbff",
              "md_syntax_heading4": "#e8590cff",
              "md_syntax_heading5": "#986801ff",
              "md_syntax_heading6": "#0b7285ff",
              "md_syntax_marker": "#717984ff",
              "md_syntax_strong": "#c2255cff",
              "md_syntax_code": "#a31515ff",
              "md_syntax_link_text": "#0b58caff",
              "md_syntax_link_url": "#986801ff",
              "md_syntax_label": "#af00dbff",
              "table_border": "#d1d1d1ff",
              "table_header_bg": "#f3f2f1ff",
              "current_line_bg": "#0000000a",
              "matching_bracket_bg": "#00000014",
              "table_cell_bg": "#ffffffff",
              "table_cell_active_outline": "#6558d3ff",
              "table_axis_preview_bg": "#0078d414",
              "table_axis_selected_bg": "#0078d429",
              "table_append_button_bg": "#f3f2f1ff",
              "table_append_button_hover": "#edebe9ff",
              "table_append_button_text": "#484644ff",
              "image_placeholder_bg": "#f3f2f1ff",
              "image_placeholder_border": "#d1d1d1ff",
              "image_placeholder_text": "#484644ff",
              "image_caption_text": "#605e5cff",
              "scrollbar_thumb": "#8a8886b8",
              "cursor": "#242424ff",
              "selection": "#e8e5ffff",
              "workspace_folder_icon": "#4a93d8ff",
              "dialog_backdrop": "#00000066",
              "dialog_surface": "#ffffffff",
              "dialog_border": "#e6e8edff",
              "dialog_title": "#201f1eff",
              "dialog_body": "#565d68ff",
              "dialog_muted": "#777d89ff",
              "dialog_primary_button_bg": "#6558d3ff",
              "dialog_primary_button_hover": "#574bc2ff",
              "dialog_primary_button_text": "#ffffffff",
              "dialog_secondary_button_bg": "#f8f9fbff",
              "dialog_secondary_button_hover": "#f1f3f6ff",
              "dialog_secondary_button_text": "#565d68ff",
              "dialog_danger_button_bg": "#d13438ff",
              "dialog_danger_button_hover": "#a4262cff",
              "dialog_danger_button_text": "#ffffffff",
              "status_bar_background": "#f8f9fbff",
              "status_bar_text": "#323130ff",
              "status_bar_text_dim": "#605e5cff",
              "status_bar_button_hover": "#edebe9ff"
            },
            "dimensions": {
              "editor_padding": 24.0,
              "writing_max_width": 760.0,
              "block_gap": 6.0,
              "block_min_height": 28.0,
              "block_padding_y": 4.0,
              "block_padding_x": 12.0,
              "nested_block_indent": 20.0,
              "list_marker_gap": 8.0,
              "list_marker_width": 24.0,
              "ordered_list_marker_width": 24.0,
              "task_checkbox_size": 14.0,
              "task_checkbox_radius": 2.0,
              "task_checkbox_border_width": 1.0,
              "task_checkbox_check_size": 10.0,
              "h1_padding_bottom": 0.0,
              "h1_margin_bottom": 4.0,
              "cursor_width": 2.0,
              "underline_thickness": 1.0,
              "h1_border_width": 0.0,
              "quote_border_width": 3.0,
              "quote_padding_left": 12.0,
              "callout_padding_x": 14.0,
              "callout_padding_y": 10.0,
              "callout_body_gap": 8.0,
              "callout_radius": 10.0,
              "callout_border_width": 4.0,
              "callout_header_gap": 6.0,
              "callout_header_margin_bottom": 6.0,
              "footnote_padding_x": 10.0,
              "footnote_padding_y": 6.0,
              "footnote_radius": 6.0,
              "footnote_badge_padding_x": 4.0,
              "footnote_badge_padding_y": 1.0,
              "separator_thickness": 1.0,
              "separator_inset_x": 40.0,
              "separator_margin_y": 10.0,
              "code_block_padding_y": 8.0,
              "code_block_padding_x": 12.0,
              "code_bg_pad_x": 4.0,
              "code_bg_pad_y": 2.0,
              "code_bg_radius": 4.0,
              "code_language_input_width": 156.0,
              "code_language_input_height": 18.0,
              "code_language_input_padding_x": 8.0,
              "code_language_input_padding_y": 3.0,
              "code_language_input_radius": 6.0,
              "code_language_input_border_width": 1.0,
              "code_language_input_gap": 8.0,
              "table_cell_padding_x": 10.0,
              "table_cell_padding_y": 8.0,
              "table_cell_min_height": 42.0,
              "table_append_button_extent": 16.0,
              "table_append_button_inset": 8.0,
              "table_append_activation_band": 18.0,
              "image_radius": 12.0,
              "image_root_max_height": 0.0,
              "image_root_max_width": 0.0,
              "image_cell_max_height": 180.0,
              "image_root_placeholder_height": 260.0,
              "image_cell_placeholder_height": 120.0,
              "image_caption_gap": 8.0,
              "scrollbar_width": 6.0,
              "scrollbar_right": 6.0,
              "centered_shrink_start": 1100.0,
              "centered_shrink_end": 2200.0,
              "centered_min_ratio": 0.58,
              "dialog_width": 460.0,
              "dialog_padding": 20.0,
              "dialog_gap": 14.0,
              "dialog_radius": 14.0,
              "dialog_border_width": 1.0,
              "dialog_button_height": 36.0,
              "dialog_button_gap": 10.0,
              "dialog_button_padding_x": 14.0,
              "menu_bar_height": 32.0,
              "menu_bar_padding_x": 10.0,
              "menu_bar_padding_y": 4.0,
              "menu_bar_gap": 2.0,
              "menu_bar_button_width": 48.0,
              "menu_bar_button_height": 24.0,
              "menu_bar_button_padding_x": 8.0,
              "menu_bar_button_radius": 5.0,
              "menu_text_size": 12.0,
              "menu_panel_top": 30.0,
              "menu_panel_width": 180.0,
              "menu_panel_padding": 4.0,
              "menu_panel_gap": 1.0,
              "menu_panel_radius": 8.0,
              "menu_item_height": 28.0,
              "menu_item_padding_x": 8.0,
              "menu_item_radius": 5.0,
              "menu_separator_margin_x": 6.0,
              "menu_separator_margin_y": 3.0,
              "menu_separator_height": 1.0,
              "context_menu_panel_width": 132.0,
              "context_menu_submenu_width": 148.0,
              "context_menu_submenu_gap": 2.0,
              "context_menu_axis_panel_width": 164.0,
              "table_insert_dialog_width": 380.0,
              "table_insert_stepper_gap": 8.0,
              "table_insert_stepper_button_size": 32.0,
              "table_insert_stepper_value_min_width": 56.0,
              "table_insert_stepper_value_padding_x": 10.0,
              "table_insert_stepper_radius": 8.0,
              "view_mode_toggle_left": 12.0,
              "view_mode_toggle_bottom": 12.0,
              "view_mode_toggle_padding_x": 8.0,
              "view_mode_toggle_padding_y": 4.0,
              "view_mode_toggle_min_width": 88.0,
              "view_mode_toggle_radius": 999.0,
              "view_mode_toggle_border_width": 1.0,
              "view_mode_toggle_text_size": 11.0,
              "status_bar_height": 28.0,
              "status_bar_padding_x": 12.0,
              "status_bar_item_gap": 12.0,
              "status_bar_text_size": 11.0
            },
            "typography": {
              "body_font_family": ".SystemUIFont",
              "heading_font_family": ".SystemUIFont",
              "text_size": 17.0,
              "text_line_height": 1.6,
              "text_letter_spacing": 0.0125,
              "h1_size": 36.0,
              "h1_weight": "bold",
              "h2_size": 30.0,
              "h2_weight": "bold",
              "h3_size": 24.0,
              "h3_weight": "bold",
              "h4_size": 18.0,
              "h4_weight": "semibold",
              "h5_size": 16.0,
              "h5_weight": "semibold",
              "h6_size": 14.0,
              "h6_weight": "semibold",
              "code_size": 15.0,
              "dialog_title_size": 20.0,
              "dialog_title_weight": "semibold",
              "dialog_body_size": 14.0,
              "dialog_body_weight": "normal",
              "dialog_button_size": 14.0,
              "dialog_button_weight": "medium"
            },
            "placeholders": {
              "empty_editing": ""
            }
          }
        },
        {
          "id": "paper",
          "name": "Paper",
          "theme": {
            "name": "Paper",
            "colors": {
              "editor_background": "#fffdf8ff",
              "source_mode_block_bg": "#f5efe6ff",
              "comment_bg": "#f5efe6ff",
              "text_default": "#2c2924ff",
              "text_link": "#9b603fff",
              "text_placeholder": "#756e65ff",
              "text_h1": "#2c2924ff",
              "text_h2": "#2c2924ff",
              "text_h3": "#2c2924ff",
              "text_h4": "#2c2924ff",
              "text_h5": "#2c2924ff",
              "text_h6": "#2c2924ff",
              "border_h1": "#e4dacbff",
              "border_h2": "#e4dacbff",
              "text_quote": "#756e65ff",
              "border_quote": "#9b603fff",
              "callout_note_bg": "#f7f2e9ff",
              "callout_note_border": "#9b603fff",
              "callout_tip_bg": "#f7f2e9ff",
              "callout_tip_border": "#9b603fff",
              "callout_important_bg": "#8764b814",
              "callout_important_border": "#8764b8ff",
              "callout_warning_bg": "#ca501014",
              "search_highlight_bg": "#ffd60a4d",
              "search_active_highlight_bg": "#ff9500b4",
              "callout_warning_border": "#ca5010ff",
              "callout_caution_bg": "#d1343814",
              "callout_caution_border": "#d13438ff",
              "footnote_bg": "#f7f2e9ff",
              "footnote_border": "#e4dacbff",
              "footnote_badge_bg": "#f5efe6ff",
              "footnote_badge_text": "#756e65ff",
              "footnote_backref": "#9b603fff",
              "task_checkbox_border": "#756e65ff",
              "task_checkbox_bg": "#fffdf8ff",
              "task_checkbox_checked_bg": "#9b603fff",
              "task_checkbox_check": "#ffffffff",
              "separator_color": "#e4dacbff",
              "code_bg": "#f5efe6ff",
              "code_text": "#2c2924ff",
              "code_language_input_bg": "#f7f2e9ff",
              "code_language_input_border": "#e4dacbff",
              "code_language_input_text": "#2c2924ff",
              "code_language_input_placeholder": "#756e65ff",
              "code_syntax_comment": "#6a6a6aff",
              "code_syntax_keyword": "#af00dbff",
              "code_syntax_string": "#008000ff",
              "code_syntax_number": "#098658ff",
              "code_syntax_type": "#267f99ff",
              "code_syntax_function": "#795e26ff",
              "code_syntax_constant": "#0070c1ff",
              "code_syntax_variable": "#001080ff",
              "code_syntax_property": "#001080ff",
              "code_syntax_operator": "#393a34ff",
              "code_syntax_punctuation": "#393a34ff",
              "md_syntax_heading1": "#2b5f8aff",
              "md_syntax_heading2": "#4a7a5aff",
              "md_syntax_heading3": "#7d4fb2ff",
              "md_syntax_heading4": "#b45a3cff",
              "md_syntax_heading5": "#9c6b1eff",
              "md_syntax_heading6": "#31708fff",
              "md_syntax_marker": "#8a7f70ff",
              "md_syntax_strong": "#8a3a52ff",
              "md_syntax_code": "#9c4222ff",
              "md_syntax_link_text": "#316aa5ff",
              "md_syntax_link_url": "#9c6b1eff",
              "md_syntax_label": "#7d4fb2ff",
              "table_border": "#e4dacbff",
              "table_header_bg": "#f7f2e9ff",
              "current_line_bg": "#f1e1d466",
              "matching_bracket_bg": "#2c29241a",
              "table_cell_bg": "#fffdf8ff",
              "table_cell_active_outline": "#9b603fff",
              "table_axis_preview_bg": "#9b603f0f",
              "table_axis_selected_bg": "#9b603f19",
              "table_append_button_bg": "#f7f2e9ff",
              "table_append_button_hover": "#efe7dbff",
              "table_append_button_text": "#2c2924ff",
              "image_placeholder_bg": "#f5efe6ff",
              "image_placeholder_border": "#e4dacbff",
              "image_placeholder_text": "#756e65ff",
              "image_caption_text": "#756e65ff",
              "scrollbar_thumb": "#756e65b8",
              "cursor": "#2c2924ff",
              "selection": "#f1e1d4ff",
              "workspace_folder_icon": "#4a93d8ff",
              "dialog_backdrop": "#00000066",
              "dialog_surface": "#fffdf8ff",
              "dialog_border": "#e4dacbff",
              "dialog_title": "#2c2924ff",
              "dialog_body": "#625d55ff",
              "dialog_muted": "#756e65ff",
              "dialog_primary_button_bg": "#9b603fff",
              "dialog_primary_button_hover": "#9b603fff",
              "dialog_primary_button_text": "#ffffffff",
              "dialog_secondary_button_bg": "#f7f2e9ff",
              "dialog_secondary_button_hover": "#efe7dbff",
              "dialog_secondary_button_text": "#625d55ff",
              "dialog_danger_button_bg": "#d13438ff",
              "dialog_danger_button_hover": "#a4262cff",
              "dialog_danger_button_text": "#ffffffff",
              "status_bar_background": "#f7f2e9ff",
              "status_bar_text": "#2c2924ff",
              "status_bar_text_dim": "#756e65ff",
              "status_bar_button_hover": "#efe7dbff"
            },
            "dimensions": {
              "editor_padding": 24.0,
              "writing_max_width": 700.0,
              "block_gap": 8.0,
              "block_min_height": 28.0,
              "block_padding_y": 4.0,
              "block_padding_x": 12.0,
              "nested_block_indent": 20.0,
              "list_marker_gap": 8.0,
              "list_marker_width": 24.0,
              "ordered_list_marker_width": 24.0,
              "task_checkbox_size": 14.0,
              "task_checkbox_radius": 2.0,
              "task_checkbox_border_width": 1.0,
              "task_checkbox_check_size": 10.0,
              "h1_padding_bottom": 0.0,
              "h1_margin_bottom": 10.0,
              "cursor_width": 2.0,
              "underline_thickness": 1.0,
              "h1_border_width": 0.0,
              "quote_border_width": 3.0,
              "quote_padding_left": 12.0,
              "callout_padding_x": 14.0,
              "callout_padding_y": 10.0,
              "callout_body_gap": 8.0,
              "callout_radius": 10.0,
              "callout_border_width": 4.0,
              "callout_header_gap": 6.0,
              "callout_header_margin_bottom": 6.0,
              "footnote_padding_x": 10.0,
              "footnote_padding_y": 6.0,
              "footnote_radius": 6.0,
              "footnote_badge_padding_x": 4.0,
              "footnote_badge_padding_y": 1.0,
              "separator_thickness": 1.0,
              "separator_inset_x": 40.0,
              "separator_margin_y": 10.0,
              "code_block_padding_y": 8.0,
              "code_block_padding_x": 12.0,
              "code_bg_pad_x": 4.0,
              "code_bg_pad_y": 2.0,
              "code_bg_radius": 4.0,
              "code_language_input_width": 156.0,
              "code_language_input_height": 18.0,
              "code_language_input_padding_x": 8.0,
              "code_language_input_padding_y": 3.0,
              "code_language_input_radius": 6.0,
              "code_language_input_border_width": 1.0,
              "code_language_input_gap": 8.0,
              "table_cell_padding_x": 10.0,
              "table_cell_padding_y": 9.0,
              "table_cell_min_height": 42.0,
              "table_append_button_extent": 16.0,
              "table_append_button_inset": 8.0,
              "table_append_activation_band": 18.0,
              "image_radius": 12.0,
              "image_root_max_height": 0.0,
              "image_root_max_width": 0.0,
              "image_cell_max_height": 180.0,
              "image_root_placeholder_height": 260.0,
              "image_cell_placeholder_height": 120.0,
              "image_caption_gap": 8.0,
              "scrollbar_width": 6.0,
              "scrollbar_right": 6.0,
              "centered_shrink_start": 1100.0,
              "centered_shrink_end": 2200.0,
              "centered_min_ratio": 0.58,
              "dialog_width": 460.0,
              "dialog_padding": 20.0,
              "dialog_gap": 14.0,
              "dialog_radius": 14.0,
              "dialog_border_width": 1.0,
              "dialog_button_height": 36.0,
              "dialog_button_gap": 10.0,
              "dialog_button_padding_x": 14.0,
              "menu_bar_height": 32.0,
              "menu_bar_padding_x": 10.0,
              "menu_bar_padding_y": 4.0,
              "menu_bar_gap": 2.0,
              "menu_bar_button_width": 48.0,
              "menu_bar_button_height": 24.0,
              "menu_bar_button_padding_x": 8.0,
              "menu_bar_button_radius": 5.0,
              "menu_text_size": 12.0,
              "menu_panel_top": 30.0,
              "menu_panel_width": 180.0,
              "menu_panel_padding": 4.0,
              "menu_panel_gap": 1.0,
              "menu_panel_radius": 8.0,
              "menu_item_height": 28.0,
              "menu_item_padding_x": 8.0,
              "menu_item_radius": 5.0,
              "menu_separator_margin_x": 6.0,
              "menu_separator_margin_y": 3.0,
              "menu_separator_height": 1.0,
              "context_menu_panel_width": 132.0,
              "context_menu_submenu_width": 148.0,
              "context_menu_submenu_gap": 2.0,
              "context_menu_axis_panel_width": 164.0,
              "table_insert_dialog_width": 380.0,
              "table_insert_stepper_gap": 8.0,
              "table_insert_stepper_button_size": 32.0,
              "table_insert_stepper_value_min_width": 56.0,
              "table_insert_stepper_value_padding_x": 10.0,
              "table_insert_stepper_radius": 8.0,
              "view_mode_toggle_left": 12.0,
              "view_mode_toggle_bottom": 12.0,
              "view_mode_toggle_padding_x": 8.0,
              "view_mode_toggle_padding_y": 4.0,
              "view_mode_toggle_min_width": 88.0,
              "view_mode_toggle_radius": 999.0,
              "view_mode_toggle_border_width": 1.0,
              "view_mode_toggle_text_size": 11.0,
              "status_bar_height": 28.0,
              "status_bar_padding_x": 12.0,
              "status_bar_item_gap": 12.0,
              "status_bar_text_size": 11.0
            },
            "typography": {
              "body_font_family": "Songti SC",
              "heading_font_family": "Songti SC",
              "text_size": 17.0,
              "text_line_height": 1.72,
              "text_letter_spacing": 0.0125,
              "h1_size": 36.0,
              "h1_weight": "semibold",
              "h2_size": 30.0,
              "h2_weight": "semibold",
              "h3_size": 24.0,
              "h3_weight": "bold",
              "h4_size": 18.0,
              "h4_weight": "semibold",
              "h5_size": 16.0,
              "h5_weight": "semibold",
              "h6_size": 14.0,
              "h6_weight": "semibold",
              "code_size": 15.0,
              "dialog_title_size": 20.0,
              "dialog_title_weight": "semibold",
              "dialog_body_size": 14.0,
              "dialog_body_weight": "normal",
              "dialog_button_size": 14.0,
              "dialog_button_weight": "medium"
            },
            "placeholders": {
              "empty_editing": ""
            }
          }
        },
        {
          "id": "forest",
          "name": "Forest",
          "theme": {
            "name": "Forest",
            "colors": {
              "editor_background": "#fdfffcff",
              "source_mode_block_bg": "#eff5eaff",
              "comment_bg": "#eff5eaff",
              "text_default": "#303831ff",
              "text_link": "#287a3dff",
              "text_placeholder": "#657269ff",
              "text_h1": "#303831ff",
              "text_h2": "#303831ff",
              "text_h3": "#303831ff",
              "text_h4": "#2c7040ff",
              "text_h5": "#2c7040ff",
              "text_h6": "#2c7040ff",
              "border_h1": "#dde7d8ff",
              "border_h2": "#dde7d8ff",
              "text_quote": "#657269ff",
              "border_quote": "#287a3dff",
              "callout_note_bg": "#f5f8f3ff",
              "callout_note_border": "#287a3dff",
              "callout_tip_bg": "#f5f8f3ff",
              "callout_tip_border": "#287a3dff",
              "callout_important_bg": "#8764b814",
              "callout_important_border": "#8764b8ff",
              "callout_warning_bg": "#ca501014",
              "search_highlight_bg": "#ffd60a4d",
              "search_active_highlight_bg": "#ff9500b4",
              "callout_warning_border": "#ca5010ff",
              "callout_caution_bg": "#d1343814",
              "callout_caution_border": "#d13438ff",
              "footnote_bg": "#f5f8f3ff",
              "footnote_border": "#dde7d8ff",
              "footnote_badge_bg": "#eff5eaff",
              "footnote_badge_text": "#657269ff",
              "footnote_backref": "#287a3dff",
              "task_checkbox_border": "#657269ff",
              "task_checkbox_bg": "#fdfffcff",
              "task_checkbox_checked_bg": "#287a3dff",
              "task_checkbox_check": "#ffffffff",
              "separator_color": "#dde7d8ff",
              "code_bg": "#eff5eaff",
              "code_text": "#4a6352ff",
              "code_language_input_bg": "#f5f8f3ff",
              "code_language_input_border": "#dde7d8ff",
              "code_language_input_text": "#303831ff",
              "code_language_input_placeholder": "#657269ff",
              "code_syntax_comment": "#6a6a6aff",
              "code_syntax_keyword": "#af00dbff",
              "code_syntax_string": "#008000ff",
              "code_syntax_number": "#098658ff",
              "code_syntax_type": "#267f99ff",
              "code_syntax_function": "#795e26ff",
              "code_syntax_constant": "#0070c1ff",
              "code_syntax_variable": "#001080ff",
              "code_syntax_property": "#001080ff",
              "code_syntax_operator": "#393a34ff",
              "code_syntax_punctuation": "#393a34ff",
              "md_syntax_heading1": "#1e6fa8ff",
              "md_syntax_heading2": "#287a3dff",
              "md_syntax_heading3": "#8250dfff",
              "md_syntax_heading4": "#9a6700ff",
              "md_syntax_heading5": "#b3492fff",
              "md_syntax_heading6": "#5c6f91ff",
              "md_syntax_marker": "#5e6b60ff",
              "md_syntax_strong": "#a83a5bff",
              "md_syntax_code": "#0f7b6cff",
              "md_syntax_link_text": "#1a67c9ff",
              "md_syntax_link_url": "#986801ff",
              "md_syntax_label": "#8250dfff",
              "table_border": "#dde7d8ff",
              "table_header_bg": "#f5f8f3ff",
              "current_line_bg": "#dcecd666",
              "matching_bracket_bg": "#3038311a",
              "table_cell_bg": "#fdfffcff",
              "table_cell_active_outline": "#287a3dff",
              "table_axis_preview_bg": "#287a3d0f",
              "table_axis_selected_bg": "#287a3d19",
              "table_append_button_bg": "#f5f8f3ff",
              "table_append_button_hover": "#e8f0e3ff",
              "table_append_button_text": "#303831ff",
              "image_placeholder_bg": "#eff5eaff",
              "image_placeholder_border": "#dde7d8ff",
              "image_placeholder_text": "#657269ff",
              "image_caption_text": "#657269ff",
              "scrollbar_thumb": "#657269b8",
              "cursor": "#303831ff",
              "selection": "#dcecd6ff",
              "workspace_folder_icon": "#4a93d8ff",
              "dialog_backdrop": "#00000066",
              "dialog_surface": "#fdfffcff",
              "dialog_border": "#dde7d8ff",
              "dialog_title": "#303831ff",
              "dialog_body": "#56635aff",
              "dialog_muted": "#657269ff",
              "dialog_primary_button_bg": "#287a3dff",
              "dialog_primary_button_hover": "#287a3dff",
              "dialog_primary_button_text": "#ffffffff",
              "dialog_secondary_button_bg": "#f5f8f3ff",
              "dialog_secondary_button_hover": "#e8f0e3ff",
              "dialog_secondary_button_text": "#56635aff",
              "dialog_danger_button_bg": "#d13438ff",
              "dialog_danger_button_hover": "#a4262cff",
              "dialog_danger_button_text": "#ffffffff",
              "status_bar_background": "#f5f8f3ff",
              "status_bar_text": "#303831ff",
              "status_bar_text_dim": "#657269ff",
              "status_bar_button_hover": "#e8f0e3ff"
            },
            "dimensions": {
              "editor_padding": 24.0,
              "writing_max_width": 760.0,
              "block_gap": 10.0,
              "block_min_height": 28.0,
              "block_padding_y": 4.0,
              "block_padding_x": 12.0,
              "nested_block_indent": 20.0,
              "list_marker_gap": 8.0,
              "list_marker_width": 24.0,
              "ordered_list_marker_width": 24.0,
              "task_checkbox_size": 14.0,
              "task_checkbox_radius": 2.0,
              "task_checkbox_border_width": 1.0,
              "task_checkbox_check_size": 10.0,
              "h1_padding_bottom": 0.0,
              "h1_margin_bottom": 4.0,
              "cursor_width": 2.0,
              "underline_thickness": 1.0,
              "h1_border_width": 0.0,
              "quote_border_width": 3.0,
              "quote_padding_left": 12.0,
              "callout_padding_x": 14.0,
              "callout_padding_y": 10.0,
              "callout_body_gap": 8.0,
              "callout_radius": 10.0,
              "callout_border_width": 4.0,
              "callout_header_gap": 6.0,
              "callout_header_margin_bottom": 6.0,
              "footnote_padding_x": 10.0,
              "footnote_padding_y": 6.0,
              "footnote_radius": 6.0,
              "footnote_badge_padding_x": 4.0,
              "footnote_badge_padding_y": 1.0,
              "separator_thickness": 1.0,
              "separator_inset_x": 40.0,
              "separator_margin_y": 10.0,
              "code_block_padding_y": 12.0,
              "code_block_padding_x": 14.0,
              "code_bg_pad_x": 3.0,
              "code_bg_pad_y": 2.0,
              "code_bg_radius": 4.0,
              "code_language_input_width": 156.0,
              "code_language_input_height": 18.0,
              "code_language_input_padding_x": 8.0,
              "code_language_input_padding_y": 3.0,
              "code_language_input_radius": 6.0,
              "code_language_input_border_width": 1.0,
              "code_language_input_gap": 8.0,
              "table_cell_padding_x": 10.0,
              "table_cell_padding_y": 7.0,
              "table_cell_min_height": 42.0,
              "table_append_button_extent": 16.0,
              "table_append_button_inset": 8.0,
              "table_append_activation_band": 18.0,
              "image_radius": 12.0,
              "image_root_max_height": 0.0,
              "image_root_max_width": 0.0,
              "image_cell_max_height": 180.0,
              "image_root_placeholder_height": 260.0,
              "image_cell_placeholder_height": 120.0,
              "image_caption_gap": 8.0,
              "scrollbar_width": 6.0,
              "scrollbar_right": 6.0,
              "centered_shrink_start": 1100.0,
              "centered_shrink_end": 2200.0,
              "centered_min_ratio": 0.76,
              "dialog_width": 460.0,
              "dialog_padding": 20.0,
              "dialog_gap": 14.0,
              "dialog_radius": 14.0,
              "dialog_border_width": 1.0,
              "dialog_button_height": 36.0,
              "dialog_button_gap": 10.0,
              "dialog_button_padding_x": 14.0,
              "menu_bar_height": 32.0,
              "menu_bar_padding_x": 10.0,
              "menu_bar_padding_y": 4.0,
              "menu_bar_gap": 2.0,
              "menu_bar_button_width": 48.0,
              "menu_bar_button_height": 24.0,
              "menu_bar_button_padding_x": 8.0,
              "menu_bar_button_radius": 5.0,
              "menu_text_size": 12.0,
              "menu_panel_top": 30.0,
              "menu_panel_width": 180.0,
              "menu_panel_padding": 4.0,
              "menu_panel_gap": 1.0,
              "menu_panel_radius": 8.0,
              "menu_item_height": 28.0,
              "menu_item_padding_x": 8.0,
              "menu_item_radius": 5.0,
              "menu_separator_margin_x": 6.0,
              "menu_separator_margin_y": 3.0,
              "menu_separator_height": 1.0,
              "context_menu_panel_width": 132.0,
              "context_menu_submenu_width": 148.0,
              "context_menu_submenu_gap": 2.0,
              "context_menu_axis_panel_width": 164.0,
              "table_insert_dialog_width": 380.0,
              "table_insert_stepper_gap": 8.0,
              "table_insert_stepper_button_size": 32.0,
              "table_insert_stepper_value_min_width": 56.0,
              "table_insert_stepper_value_padding_x": 10.0,
              "table_insert_stepper_radius": 8.0,
              "view_mode_toggle_left": 12.0,
              "view_mode_toggle_bottom": 12.0,
              "view_mode_toggle_padding_x": 8.0,
              "view_mode_toggle_padding_y": 4.0,
              "view_mode_toggle_min_width": 88.0,
              "view_mode_toggle_radius": 999.0,
              "view_mode_toggle_border_width": 1.0,
              "view_mode_toggle_text_size": 11.0,
              "status_bar_height": 28.0,
              "status_bar_padding_x": 12.0,
              "status_bar_item_gap": 12.0,
              "status_bar_text_size": 11.0
            },
            "typography": {
              "body_font_family": ".SystemUIFont",
              "heading_font_family": ".SystemUIFont",
              "text_size": 17.0,
              "text_line_height": 1.78,
              "text_letter_spacing": 0.02,
              "h1_size": 36.0,
              "h1_weight": "bold",
              "h2_size": 30.0,
              "h2_weight": "bold",
              "h3_size": 24.0,
              "h3_weight": "bold",
              "h4_size": 18.0,
              "h4_weight": "semibold",
              "h5_size": 16.0,
              "h5_weight": "semibold",
              "h6_size": 14.0,
              "h6_weight": "semibold",
              "code_size": 15.0,
              "dialog_title_size": 20.0,
              "dialog_title_weight": "semibold",
              "dialog_body_size": 14.0,
              "dialog_body_weight": "normal",
              "dialog_button_size": 14.0,
              "dialog_button_weight": "medium"
            },
            "placeholders": {
              "empty_editing": ""
            }
          }
        },
        {
          "id": "midnight",
          "name": "Midnight",
          "theme": {
            "name": "Midnight",
            "colors": {
              "editor_background": "#171d27ff",
              "source_mode_block_bg": "#202b3aff",
              "comment_bg": "#202b3aff",
              "text_default": "#e8edf4ff",
              "text_link": "#8bb6e9ff",
              "text_placeholder": "#9caabeff",
              "text_h1": "#e8edf4ff",
              "text_h2": "#e8edf4ff",
              "text_h3": "#e8edf4ff",
              "text_h4": "#e8edf4ff",
              "text_h5": "#e8edf4ff",
              "text_h6": "#e8edf4ff",
              "border_h1": "#344254ff",
              "border_h2": "#344254ff",
              "text_quote": "#9caabeff",
              "border_quote": "#8bb6e9ff",
              "callout_note_bg": "#1d2633ff",
              "callout_note_border": "#8bb6e9ff",
              "callout_tip_bg": "#1d2633ff",
              "callout_tip_border": "#8bb6e9ff",
              "callout_important_bg": "#a78bfa1f",
              "callout_important_border": "#a78bfaff",
              "callout_warning_bg": "#fce1001f",
              "search_highlight_bg": "#ffe06638",
              "search_active_highlight_bg": "#ff9500b4",
              "callout_warning_border": "#fce100ff",
              "callout_caution_bg": "#d134381f",
              "callout_caution_border": "#d13438ff",
              "footnote_bg": "#1d2633ff",
              "footnote_border": "#344254ff",
              "footnote_badge_bg": "#202b3aff",
              "footnote_badge_text": "#9caabeff",
              "footnote_backref": "#8bb6e9ff",
              "task_checkbox_border": "#9caabeff",
              "task_checkbox_bg": "#171d27ff",
              "task_checkbox_checked_bg": "#8bb6e9ff",
              "task_checkbox_check": "#171b22ff",
              "separator_color": "#344254ff",
              "code_bg": "#202b3aff",
              "code_text": "#e8edf4ff",
              "code_language_input_bg": "#1d2633ff",
              "code_language_input_border": "#344254ff",
              "code_language_input_text": "#e8edf4ff",
              "code_language_input_placeholder": "#9caabeff",
              "code_syntax_comment": "#858585ff",
              "code_syntax_keyword": "#c586c0ff",
              "code_syntax_string": "#ce9178ff",
              "code_syntax_number": "#b5cea8ff",
              "code_syntax_type": "#4ec9b0ff",
              "code_syntax_function": "#dcdcaaff",
              "code_syntax_constant": "#4fc1ffff",
              "code_syntax_variable": "#9cdcfeff",
              "code_syntax_property": "#9cdcfeff",
              "code_syntax_operator": "#d4d4d4ff",
              "code_syntax_punctuation": "#d4d4d4ff",
              "md_syntax_heading1": "#79b8ffff",
              "md_syntax_heading2": "#63d0b8ff",
              "md_syntax_heading3": "#c792c9ff",
              "md_syntax_heading4": "#e8c47cff",
              "md_syntax_heading5": "#9ecf9aff",
              "md_syntax_heading6": "#b0a8f0ff",
              "md_syntax_marker": "#8b9cb5ff",
              "md_syntax_strong": "#f2b570ff",
              "md_syntax_code": "#e8a878ff",
              "md_syntax_link_text": "#58a6ffff",
              "md_syntax_link_url": "#d7b17cff",
              "md_syntax_label": "#c792c9ff",
              "table_border": "#344254ff",
              "table_header_bg": "#1d2633ff",
              "current_line_bg": "#2d486866",
              "matching_bracket_bg": "#e8edf41a",
              "table_cell_bg": "#171d27ff",
              "table_cell_active_outline": "#8bb6e9ff",
              "table_axis_preview_bg": "#8bb6e90f",
              "table_axis_selected_bg": "#8bb6e919",
              "table_append_button_bg": "#1d2633ff",
              "table_append_button_hover": "#263244ff",
              "table_append_button_text": "#e8edf4ff",
              "image_placeholder_bg": "#202b3aff",
              "image_placeholder_border": "#344254ff",
              "image_placeholder_text": "#9caabeff",
              "image_caption_text": "#9caabeff",
              "scrollbar_thumb": "#9caabeb8",
              "cursor": "#e8edf4ff",
              "selection": "#2d4868ff",
              "workspace_folder_icon": "#4a93d8ff",
              "dialog_backdrop": "#00000088",
              "dialog_surface": "#171d27ff",
              "dialog_border": "#344254ff",
              "dialog_title": "#e8edf4ff",
              "dialog_body": "#bfcbd9ff",
              "dialog_muted": "#9caabeff",
              "dialog_primary_button_bg": "#8bb6e9ff",
              "dialog_primary_button_hover": "#8bb6e9ff",
              "dialog_primary_button_text": "#171b22ff",
              "dialog_secondary_button_bg": "#1d2633ff",
              "dialog_secondary_button_hover": "#263244ff",
              "dialog_secondary_button_text": "#bfcbd9ff",
              "dialog_danger_button_bg": "#d13438ff",
              "dialog_danger_button_hover": "#a4262cff",
              "dialog_danger_button_text": "#ffffffff",
              "status_bar_background": "#1d2633ff",
              "status_bar_text": "#e8edf4ff",
              "status_bar_text_dim": "#9caabeff",
              "status_bar_button_hover": "#263244ff"
            },
            "dimensions": {
              "editor_padding": 24.0,
              "writing_max_width": 760.0,
              "block_gap": 7.0,
              "block_min_height": 28.0,
              "block_padding_y": 4.0,
              "block_padding_x": 12.0,
              "nested_block_indent": 20.0,
              "list_marker_gap": 8.0,
              "list_marker_width": 24.0,
              "ordered_list_marker_width": 24.0,
              "task_checkbox_size": 14.0,
              "task_checkbox_radius": 2.0,
              "task_checkbox_border_width": 1.0,
              "task_checkbox_check_size": 10.0,
              "h1_padding_bottom": 0.0,
              "h1_margin_bottom": 4.0,
              "cursor_width": 2.0,
              "underline_thickness": 1.0,
              "h1_border_width": 0.0,
              "quote_border_width": 3.0,
              "quote_padding_left": 12.0,
              "callout_padding_x": 14.0,
              "callout_padding_y": 10.0,
              "callout_body_gap": 8.0,
              "callout_radius": 10.0,
              "callout_border_width": 4.0,
              "callout_header_gap": 6.0,
              "callout_header_margin_bottom": 6.0,
              "footnote_padding_x": 10.0,
              "footnote_padding_y": 6.0,
              "footnote_radius": 6.0,
              "footnote_badge_padding_x": 4.0,
              "footnote_badge_padding_y": 1.0,
              "separator_thickness": 1.0,
              "separator_inset_x": 40.0,
              "separator_margin_y": 10.0,
              "code_block_padding_y": 8.0,
              "code_block_padding_x": 12.0,
              "code_bg_pad_x": 4.0,
              "code_bg_pad_y": 2.0,
              "code_bg_radius": 4.0,
              "code_language_input_width": 156.0,
              "code_language_input_height": 18.0,
              "code_language_input_padding_x": 8.0,
              "code_language_input_padding_y": 3.0,
              "code_language_input_radius": 6.0,
              "code_language_input_border_width": 1.0,
              "code_language_input_gap": 8.0,
              "table_cell_padding_x": 10.0,
              "table_cell_padding_y": 8.0,
              "table_cell_min_height": 42.0,
              "table_append_button_extent": 16.0,
              "table_append_button_inset": 8.0,
              "table_append_activation_band": 18.0,
              "image_radius": 12.0,
              "image_root_max_height": 0.0,
              "image_root_max_width": 0.0,
              "image_cell_max_height": 180.0,
              "image_root_placeholder_height": 260.0,
              "image_cell_placeholder_height": 120.0,
              "image_caption_gap": 8.0,
              "scrollbar_width": 6.0,
              "scrollbar_right": 6.0,
              "centered_shrink_start": 1100.0,
              "centered_shrink_end": 2200.0,
              "centered_min_ratio": 0.58,
              "dialog_width": 460.0,
              "dialog_padding": 20.0,
              "dialog_gap": 14.0,
              "dialog_radius": 14.0,
              "dialog_border_width": 1.0,
              "dialog_button_height": 36.0,
              "dialog_button_gap": 10.0,
              "dialog_button_padding_x": 14.0,
              "menu_bar_height": 32.0,
              "menu_bar_padding_x": 10.0,
              "menu_bar_padding_y": 4.0,
              "menu_bar_gap": 2.0,
              "menu_bar_button_width": 48.0,
              "menu_bar_button_height": 24.0,
              "menu_bar_button_padding_x": 8.0,
              "menu_bar_button_radius": 5.0,
              "menu_text_size": 12.0,
              "menu_panel_top": 30.0,
              "menu_panel_width": 180.0,
              "menu_panel_padding": 4.0,
              "menu_panel_gap": 1.0,
              "menu_panel_radius": 8.0,
              "menu_item_height": 28.0,
              "menu_item_padding_x": 8.0,
              "menu_item_radius": 5.0,
              "menu_separator_margin_x": 6.0,
              "menu_separator_margin_y": 3.0,
              "menu_separator_height": 1.0,
              "context_menu_panel_width": 132.0,
              "context_menu_submenu_width": 148.0,
              "context_menu_submenu_gap": 2.0,
              "context_menu_axis_panel_width": 164.0,
              "table_insert_dialog_width": 380.0,
              "table_insert_stepper_gap": 8.0,
              "table_insert_stepper_button_size": 32.0,
              "table_insert_stepper_value_min_width": 56.0,
              "table_insert_stepper_value_padding_x": 10.0,
              "table_insert_stepper_radius": 8.0,
              "view_mode_toggle_left": 12.0,
              "view_mode_toggle_bottom": 12.0,
              "view_mode_toggle_padding_x": 8.0,
              "view_mode_toggle_padding_y": 4.0,
              "view_mode_toggle_min_width": 88.0,
              "view_mode_toggle_radius": 999.0,
              "view_mode_toggle_border_width": 1.0,
              "view_mode_toggle_text_size": 11.0,
              "status_bar_height": 28.0,
              "status_bar_padding_x": 12.0,
              "status_bar_item_gap": 12.0,
              "status_bar_text_size": 11.0
            },
            "typography": {
              "body_font_family": ".SystemUIFont",
              "heading_font_family": ".SystemUIFont",
              "text_size": 17.0,
              "text_line_height": 1.68,
              "text_letter_spacing": 0.0125,
              "h1_size": 36.0,
              "h1_weight": "bold",
              "h2_size": 30.0,
              "h2_weight": "bold",
              "h3_size": 24.0,
              "h3_weight": "bold",
              "h4_size": 18.0,
              "h4_weight": "semibold",
              "h5_size": 16.0,
              "h5_weight": "semibold",
              "h6_size": 14.0,
              "h6_weight": "semibold",
              "code_size": 15.0,
              "dialog_title_size": 20.0,
              "dialog_title_weight": "semibold",
              "dialog_body_size": 14.0,
              "dialog_body_weight": "normal",
              "dialog_button_size": 14.0,
              "dialog_button_weight": "medium"
            },
            "placeholders": {
              "empty_editing": ""
            }
          }
        },
        {
          "id": "ink",
          "name": "Ink",
          "theme": {
            "name": "Ink",
            "colors": {
              "editor_background": "#171616ff",
              "source_mode_block_bg": "#282422ff",
              "comment_bg": "#282422ff",
              "text_default": "#eee8ddff",
              "text_link": "#d6a079ff",
              "text_placeholder": "#aea398ff",
              "text_h1": "#eee8ddff",
              "text_h2": "#eee8ddff",
              "text_h3": "#eee8ddff",
              "text_h4": "#eee8ddff",
              "text_h5": "#eee8ddff",
              "text_h6": "#eee8ddff",
              "border_h1": "#3b3430ff",
              "border_h2": "#3b3430ff",
              "text_quote": "#aea398ff",
              "border_quote": "#d6a079ff",
              "callout_note_bg": "#201d1dff",
              "callout_note_border": "#d6a079ff",
              "callout_tip_bg": "#201d1dff",
              "callout_tip_border": "#d6a079ff",
              "callout_important_bg": "#a78bfa1f",
              "callout_important_border": "#a78bfaff",
              "callout_warning_bg": "#fce1001f",
              "search_highlight_bg": "#ffe06638",
              "search_active_highlight_bg": "#ff9500b4",
              "callout_warning_border": "#fce100ff",
              "callout_caution_bg": "#d134381f",
              "callout_caution_border": "#d13438ff",
              "footnote_bg": "#201d1dff",
              "footnote_border": "#3b3430ff",
              "footnote_badge_bg": "#282422ff",
              "footnote_badge_text": "#aea398ff",
              "footnote_backref": "#d6a079ff",
              "task_checkbox_border": "#aea398ff",
              "task_checkbox_bg": "#171616ff",
              "task_checkbox_checked_bg": "#d6a079ff",
              "task_checkbox_check": "#171b22ff",
              "separator_color": "#3b3430ff",
              "code_bg": "#282422ff",
              "code_text": "#eee8ddff",
              "code_language_input_bg": "#201d1dff",
              "code_language_input_border": "#3b3430ff",
              "code_language_input_text": "#eee8ddff",
              "code_language_input_placeholder": "#aea398ff",
              "code_syntax_comment": "#858585ff",
              "code_syntax_keyword": "#c586c0ff",
              "code_syntax_string": "#ce9178ff",
              "code_syntax_number": "#b5cea8ff",
              "code_syntax_type": "#4ec9b0ff",
              "code_syntax_function": "#dcdcaaff",
              "code_syntax_constant": "#4fc1ffff",
              "code_syntax_variable": "#9cdcfeff",
              "code_syntax_property": "#9cdcfeff",
              "code_syntax_operator": "#d4d4d4ff",
              "code_syntax_punctuation": "#d4d4d4ff",
              "md_syntax_heading1": "#8fb8d8ff",
              "md_syntax_heading2": "#86c0a8ff",
              "md_syntax_heading3": "#d3a0c0ff",
              "md_syntax_heading4": "#d9b87cff",
              "md_syntax_heading5": "#c08a7aff",
              "md_syntax_heading6": "#a89ec4ff",
              "md_syntax_marker": "#9a8f85ff",
              "md_syntax_strong": "#d98a7eff",
              "md_syntax_code": "#d9a074ff",
              "md_syntax_link_text": "#7fa8ccff",
              "md_syntax_link_url": "#c99a6eff",
              "md_syntax_label": "#b98bc0ff",
              "table_border": "#3b3430ff",
              "table_header_bg": "#201d1dff",
              "current_line_bg": "#503d3166",
              "matching_bracket_bg": "#eee8dd1a",
              "table_cell_bg": "#171616ff",
              "table_cell_active_outline": "#d6a079ff",
              "table_axis_preview_bg": "#d6a0790f",
              "table_axis_selected_bg": "#d6a07919",
              "table_append_button_bg": "#201d1dff",
              "table_append_button_hover": "#2c2725ff",
              "table_append_button_text": "#eee8ddff",
              "image_placeholder_bg": "#282422ff",
              "image_placeholder_border": "#3b3430ff",
              "image_placeholder_text": "#aea398ff",
              "image_caption_text": "#aea398ff",
              "scrollbar_thumb": "#aea398b8",
              "cursor": "#eee8ddff",
              "selection": "#503d31ff",
              "workspace_folder_icon": "#4a93d8ff",
              "dialog_backdrop": "#00000088",
              "dialog_surface": "#171616ff",
              "dialog_border": "#3b3430ff",
              "dialog_title": "#eee8ddff",
              "dialog_body": "#c7beb3ff",
              "dialog_muted": "#aea398ff",
              "dialog_primary_button_bg": "#d6a079ff",
              "dialog_primary_button_hover": "#d6a079ff",
              "dialog_primary_button_text": "#171b22ff",
              "dialog_secondary_button_bg": "#201d1dff",
              "dialog_secondary_button_hover": "#2c2725ff",
              "dialog_secondary_button_text": "#c7beb3ff",
              "dialog_danger_button_bg": "#d13438ff",
              "dialog_danger_button_hover": "#a4262cff",
              "dialog_danger_button_text": "#ffffffff",
              "status_bar_background": "#201d1dff",
              "status_bar_text": "#eee8ddff",
              "status_bar_text_dim": "#aea398ff",
              "status_bar_button_hover": "#2c2725ff"
            },
            "dimensions": {
              "editor_padding": 24.0,
              "writing_max_width": 720.0,
              "block_gap": 8.0,
              "block_min_height": 28.0,
              "block_padding_y": 4.0,
              "block_padding_x": 12.0,
              "nested_block_indent": 20.0,
              "list_marker_gap": 8.0,
              "list_marker_width": 24.0,
              "ordered_list_marker_width": 24.0,
              "task_checkbox_size": 14.0,
              "task_checkbox_radius": 2.0,
              "task_checkbox_border_width": 1.0,
              "task_checkbox_check_size": 10.0,
              "h1_padding_bottom": 0.0,
              "h1_margin_bottom": 10.0,
              "cursor_width": 2.0,
              "underline_thickness": 1.0,
              "h1_border_width": 0.0,
              "quote_border_width": 3.0,
              "quote_padding_left": 12.0,
              "callout_padding_x": 14.0,
              "callout_padding_y": 10.0,
              "callout_body_gap": 8.0,
              "callout_radius": 10.0,
              "callout_border_width": 4.0,
              "callout_header_gap": 6.0,
              "callout_header_margin_bottom": 6.0,
              "footnote_padding_x": 10.0,
              "footnote_padding_y": 6.0,
              "footnote_radius": 6.0,
              "footnote_badge_padding_x": 4.0,
              "footnote_badge_padding_y": 1.0,
              "separator_thickness": 1.0,
              "separator_inset_x": 40.0,
              "separator_margin_y": 10.0,
              "code_block_padding_y": 8.0,
              "code_block_padding_x": 12.0,
              "code_bg_pad_x": 4.0,
              "code_bg_pad_y": 2.0,
              "code_bg_radius": 4.0,
              "code_language_input_width": 156.0,
              "code_language_input_height": 18.0,
              "code_language_input_padding_x": 8.0,
              "code_language_input_padding_y": 3.0,
              "code_language_input_radius": 6.0,
              "code_language_input_border_width": 1.0,
              "code_language_input_gap": 8.0,
              "table_cell_padding_x": 10.0,
              "table_cell_padding_y": 8.0,
              "table_cell_min_height": 42.0,
              "table_append_button_extent": 16.0,
              "table_append_button_inset": 8.0,
              "table_append_activation_band": 18.0,
              "image_radius": 12.0,
              "image_root_max_height": 0.0,
              "image_root_max_width": 0.0,
              "image_cell_max_height": 180.0,
              "image_root_placeholder_height": 260.0,
              "image_cell_placeholder_height": 120.0,
              "image_caption_gap": 8.0,
              "scrollbar_width": 6.0,
              "scrollbar_right": 6.0,
              "centered_shrink_start": 1100.0,
              "centered_shrink_end": 2200.0,
              "centered_min_ratio": 0.58,
              "dialog_width": 460.0,
              "dialog_padding": 20.0,
              "dialog_gap": 14.0,
              "dialog_radius": 14.0,
              "dialog_border_width": 1.0,
              "dialog_button_height": 36.0,
              "dialog_button_gap": 10.0,
              "dialog_button_padding_x": 14.0,
              "menu_bar_height": 32.0,
              "menu_bar_padding_x": 10.0,
              "menu_bar_padding_y": 4.0,
              "menu_bar_gap": 2.0,
              "menu_bar_button_width": 48.0,
              "menu_bar_button_height": 24.0,
              "menu_bar_button_padding_x": 8.0,
              "menu_bar_button_radius": 5.0,
              "menu_text_size": 12.0,
              "menu_panel_top": 30.0,
              "menu_panel_width": 180.0,
              "menu_panel_padding": 4.0,
              "menu_panel_gap": 1.0,
              "menu_panel_radius": 8.0,
              "menu_item_height": 28.0,
              "menu_item_padding_x": 8.0,
              "menu_item_radius": 5.0,
              "menu_separator_margin_x": 6.0,
              "menu_separator_margin_y": 3.0,
              "menu_separator_height": 1.0,
              "context_menu_panel_width": 132.0,
              "context_menu_submenu_width": 148.0,
              "context_menu_submenu_gap": 2.0,
              "context_menu_axis_panel_width": 164.0,
              "table_insert_dialog_width": 380.0,
              "table_insert_stepper_gap": 8.0,
              "table_insert_stepper_button_size": 32.0,
              "table_insert_stepper_value_min_width": 56.0,
              "table_insert_stepper_value_padding_x": 10.0,
              "table_insert_stepper_radius": 8.0,
              "view_mode_toggle_left": 12.0,
              "view_mode_toggle_bottom": 12.0,
              "view_mode_toggle_padding_x": 8.0,
              "view_mode_toggle_padding_y": 4.0,
              "view_mode_toggle_min_width": 88.0,
              "view_mode_toggle_radius": 999.0,
              "view_mode_toggle_border_width": 1.0,
              "view_mode_toggle_text_size": 11.0,
              "status_bar_height": 28.0,
              "status_bar_padding_x": 12.0,
              "status_bar_item_gap": 12.0,
              "status_bar_text_size": 11.0
            },
            "typography": {
              "body_font_family": "Songti SC",
              "heading_font_family": "Songti SC",
              "text_size": 17.0,
              "text_line_height": 1.7,
              "text_letter_spacing": 0.0125,
              "h1_size": 36.0,
              "h1_weight": "semibold",
              "h2_size": 30.0,
              "h2_weight": "semibold",
              "h3_size": 24.0,
              "h3_weight": "bold",
              "h4_size": 18.0,
              "h4_weight": "semibold",
              "h5_size": 16.0,
              "h5_weight": "semibold",
              "h6_size": 14.0,
              "h6_weight": "semibold",
              "code_size": 15.0,
              "dialog_title_size": 20.0,
              "dialog_title_weight": "semibold",
              "dialog_body_size": 14.0,
              "dialog_body_weight": "normal",
              "dialog_button_size": 14.0,
              "dialog_button_weight": "medium"
            },
            "placeholders": {
              "empty_editing": ""
            }
          }
        }
      ]
    };
  global.VELORA_THEME_EDITOR_DATA = data;
  if (typeof module !== 'undefined' && module.exports) {
    module.exports = data;
  }
})(typeof globalThis !== 'undefined' ? globalThis : this);
