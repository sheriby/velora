//! AI 助手面板(编辑器内唯一的 AI 入口浮层)。
//!
//! 一个面板承载全部动作:菜单(动作列表 + 自定义指令输入)→ 流式预览 →
//! 应用(替换选区 / 插入到下方)或重试。唤起方式(⌘J、右键菜单、命令
//! 面板、菜单栏 AI 菜单)都落到 `open_ai_assistant`,不另设散装入口。
//!
//! 生成的文本不进缓冲区,点「应用」才落盘,所以一次应用就是一步撤销;
//! 应用前校验锚点区间的文本没有漂移,文档被改过就拒绝套用,绝不写错位置。

use std::ops::Range;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use futures::{StreamExt, channel::mpsc};
use gpui::prelude::FluentBuilder;
use gpui::*;

use super::Editor;
use crate::ai::{
    AiAction, AiEndpointConfig, AiPromptContext, AiRequestError, RewriteTone, TranslateTarget,
    build_prompt, default_client, stream_completion,
};
use crate::components::{TextField, UndoCaptureKind};
use crate::config::preferences::AiSettings;
use crate::i18n::I18nManager;
use crate::theme::Theme;

/// 面板宽度与最大高度(逻辑像素)。
const PANEL_WIDTH: f32 = 400.0;
const PANEL_MAX_HEIGHT: f32 = 440.0;
/// 插入点前用于防漂移校验的上下文字节数(字符边界安全截取)。
const APPLY_TAIL_CONTEXT_BYTES: usize = 48;
/// 续写上下文在捕获阶段的截断字节上限(提示词侧还会按字符再截一次)。
const BEFORE_CURSOR_CAPTURE_BYTES: usize = 4096;

pub(in crate::editor) enum AiPhase {
    /// 动作菜单(含自定义指令输入)。
    Menu,
    /// 流式生成中。
    Running,
    /// 生成完成,可应用/重试。
    Finished,
    /// 请求失败或文档漂移,可重试/关闭。
    Failed,
}

/// 触发动作时捕获的缓冲区锚点:应用时校验这段文本仍是原样才动手。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::editor) struct AiAnchor {
    /// 选区(或插入点)的缓冲区字节区间;无选区时为空区间。
    pub(in crate::editor) source_range: Range<usize>,
    /// 区间文本快照(`buffer.slice(source_range)`)。
    pub(in crate::editor) selected_text: String,
    /// 插入点之前的短上下文,应用时校验「前面还是这段话」。
    pub(in crate::editor) tail_context: String,
    /// 光标/选区之前的文档文本(续写提示词用,已截断)。
    pub(in crate::editor) before_cursor: String,
}

pub(in crate::editor) struct AiAssistantState {
    pub(in crate::editor) phase: AiPhase,
    pub(in crate::editor) action: Option<AiAction>,
    pub(in crate::editor) anchor: Option<AiAnchor>,
    /// 自定义指令输入(菜单阶段显示)。
    pub(in crate::editor) prompt: Entity<TextField>,
    pub(in crate::editor) translate_open: bool,
    pub(in crate::editor) rewrite_open: bool,
    /// 面板锚点(视口坐标,打开时按光标位置算好)。
    pub(in crate::editor) origin: Point<Pixels>,
    pub(in crate::editor) result: String,
    pub(in crate::editor) error: Option<String>,
    /// 递增使旧流的增量/完成事件失效。
    pub(in crate::editor) generation: u64,
    pub(in crate::editor) cancel: Arc<AtomicBool>,
    pub(in crate::editor) focus: FocusHandle,
}

/// 工作线程 → UI 的消息。
enum AiStreamMessage {
    Delta(String),
    Finished(Result<String, AiRequestError>),
}

impl Editor {
    pub(in crate::editor) fn ai_assistant_is_open(&self) -> bool {
        self.ai_assistant.is_some()
    }

    /// 打开 AI 面板(所有唤起路径的唯一入口)。重复唤起 = 重新定位。
    #[allow(dead_code)]
    /// ⌘J / 菜单项 / 右键菜单共用的切换入口。
    pub(crate) fn toggle_ai_assistant(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ai_assistant.is_some() {
            self.close_ai_assistant(cx);
        } else {
            self.open_ai_assistant(window, cx);
        }
    }

    /// 动作处理器(gpui on_action 派发)。
    pub(crate) fn on_open_ai_assistant(
        &mut self,
        _: &crate::components::OpenAiAssistant,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_ai_assistant(window, cx);
    }

    pub(in crate::editor) fn open_ai_assistant(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_contextual_overlays(cx);
        // 与其他全屏浮层互斥:面板接管焦点与键盘。
        self.close_quick_open(cx);
        self.close_command_palette(cx);
        // 锚点在打开瞬间捕获:焦点移进面板后选区会被块层收起,运行时再取
        // 就拿不到原文了。锚定「用户打开面板时看到的内容」也更符合直觉。
        let anchor = self.capture_ai_anchor(cx);
        self.overlay_focus_restore_target = self.focused_edit_target_entity_id(window, cx);
        let origin = self.ai_panel_origin(window, cx);
        let placeholder = cx
            .global::<I18nManager>()
            .strings()
            .ai_custom_placeholder
            .clone();
        let editor_handle = cx.entity().downgrade();
        let prompt = cx.new(|cx| {
            TextField::new(placeholder, cx).on_enter(move |field, _window, cx| {
                // 回车即执行;内容保留,便于改两个字再跑。
                let instruction = field.value().trim().to_string();
                if instruction.is_empty() {
                    return;
                }
                let _ = editor_handle.update(cx, |editor, cx| {
                    editor.run_ai_action(AiAction::Custom(instruction), cx);
                });
            })
        });
        let focus = cx.focus_handle();
        window.focus(&focus);
        self.ai_assistant = Some(AiAssistantState {
            phase: AiPhase::Menu,
            action: None,
            anchor: Some(anchor),
            prompt,
            translate_open: false,
            rewrite_open: false,
            origin,
            result: String::new(),
            error: None,
            generation: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            focus,
        });
        cx.notify();
    }

    /// 关闭面板;生成中先取消请求(结果作废)。
    pub(in crate::editor) fn close_ai_assistant(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.ai_assistant.as_ref() else {
            return;
        };
        state.cancel.store(true, Ordering::Relaxed);
        self.ai_assistant = None;
        self.restore_focus_after_overlay(cx);
        cx.notify();
    }

    /// Esc 语义:生成中 = 停止(保留已生成的部分),其余 = 关闭。
    pub(in crate::editor) fn ai_escape(&mut self, cx: &mut Context<Self>) {
        let running = matches!(
            self.ai_assistant.as_ref().map(|state| &state.phase),
            Some(AiPhase::Running)
        );
        if running {
            self.ai_stop(cx);
        } else {
            self.close_ai_assistant(cx);
        }
    }

    /// 停止生成:置取消标志并把已有内容转入 Finished(可应用部分结果)。
    pub(in crate::editor) fn ai_stop(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.ai_assistant.as_mut() else {
            return;
        };
        state.cancel.store(true, Ordering::Relaxed);
        state.generation += 1;
        if state.result.is_empty() {
            state.phase = AiPhase::Menu;
            state.action = None;
        } else {
            state.phase = AiPhase::Finished;
        }
        cx.notify();
    }

    /// 按视口与光标位置计算面板锚点(尽量贴着光标下方,钳回视口内)。
    fn ai_panel_origin(&self, window: &Window, cx: &App) -> Point<Pixels> {
        let viewport = window.viewport_size();
        let caret = self
            .current_edit_target_from_state(cx)
            .and_then(|block| block.read(cx).active_range_or_cursor_bounds());
        let x = match caret {
            Some(bounds) => bounds.right(),
            None => px(48.0),
        }
        .min(viewport.width - px(PANEL_WIDTH + 12.0))
        .max(px(12.0));
        let y = match caret {
            Some(bounds) => bounds.bottom() + px(6.0),
            None => px(96.0),
        }
        .min(viewport.height - px(200.0))
        .max(px(12.0));
        point(x, y)
    }

    /// 捕获锚点:跨块选区/单块选区取其缓冲区区间,无选区取光标插入点。
    pub(in crate::editor) fn capture_ai_anchor(&self, cx: &App) -> AiAnchor {
        let source_range = self.ai_selection_source_range(cx);
        let selected_text = self.buffer.slice(source_range.clone());
        // 防漂移尾巴:插入点前至多 48 字节,按字符边界截。
        let head = source_range.start.min(self.buffer.byte_len());
        let mut tail_start = head.saturating_sub(APPLY_TAIL_CONTEXT_BYTES);
        while tail_start > 0 && !self.buffer.text().is_char_boundary(tail_start) {
            tail_start += 1;
        }
        let tail_context = self.buffer.slice(tail_start..head);
        // 续写上下文:缓冲区开头到插入点,按字符边界截尾。
        let mut context_start = head.saturating_sub(BEFORE_CURSOR_CAPTURE_BYTES);
        while context_start > 0 && !self.buffer.text().is_char_boundary(context_start) {
            context_start -= 1;
        }
        let before_cursor = self.buffer.slice(context_start..head);

        AiAnchor {
            source_range,
            selected_text,
            tail_context,
            before_cursor,
        }
    }

    /// 当前选区(或光标)的缓冲区字节区间。
    fn ai_selection_source_range(&self, cx: &App) -> Range<usize> {
        if let Some(selection) = self.normalized_cross_block_selection(cx) {
            if let Some(range) = self.cross_block_source_range_for_normalized(selection, cx) {
                return range;
            }
        }
        // 单块选区:找到带非空选区的可见块,按 source mapping 换算回缓冲区。
        let mappings = self.build_source_target_mappings(cx);
        let mut selected: Option<(gpui::Entity<crate::components::Block>, Range<usize>)> = None;
        for visible in self.document.visible_blocks() {
            let block = visible.entity.read_untracked(cx);
            if block.selected_range.is_empty() {
                continue;
            }
            selected = Some((
                visible.entity.clone(),
                block.current_range_to_markdown_range(block.selected_range.clone()),
            ));
            break;
        }
        if let Some((entity, markdown_range)) = selected
            && let Some(mapping) = mappings.iter().find(|mapping| mapping.entity == entity)
        {
            let start = mapping
                .full_source_range
                .start
                + markdown_range.start.min(mapping.full_source_range.len());
            let end = mapping
                .full_source_range
                .start
                + markdown_range.end.min(mapping.full_source_range.len());
            return start..end.max(start);
        }
        // 无选区:活动块光标位置换算;再不行落到文档末尾(空文档续写)。
        if let Some(target) = self.current_edit_target_from_state(cx) {
            let cursor_content = target.read(cx).cursor_offset();
            if let Some(mapping) = mappings.iter().find(|mapping| mapping.entity == target) {
                let local = cursor_content.min(mapping.content_to_source.len().saturating_sub(1));
                let source = mapping.full_source_range.start + mapping.content_to_source[local];
                return source..source;
            }
        }
        let end = self.buffer.byte_len();
        end..end
    }

    /// 执行一个动作:未配置 → 带去设置页;已配置 → 起流式请求。
    pub(in crate::editor) fn run_ai_action(
        &mut self,
        action: AiAction,
        cx: &mut Context<Self>,
    ) {
        let ai_settings = crate::config::EditorSettings::ai(cx);
        let endpoint = default_endpoint_from_settings(&ai_settings)
            .filter(|endpoint| endpoint.is_configured());
        let Some(endpoint) = endpoint else {
            // 没有可用端点(全删了,或默认端点没配完):带去设置页补齐。
            self.close_ai_assistant(cx);
            let _ = crate::config::open_preferences_window_at(
                cx,
                crate::config::PreferencesNav::Ai,
            );
            return;
        };
        let (generation, prompt) = {
            let Some(state) = self.ai_assistant.as_mut() else {
                return;
            };
            // 锚点在面板打开时已捕获;没有它(理论不可达)就不发请求。
            let Some(anchor) = state.anchor.clone() else {
                return;
            };
            let prompt = build_ai_prompt(&action, &anchor);
            state.phase = AiPhase::Running;
            state.action = Some(action);
            state.anchor = Some(anchor);
            state.translate_open = false;
            state.rewrite_open = false;
            state.result.clear();
            state.error = None;
            state.generation += 1;
            state.cancel = Arc::new(AtomicBool::new(false));
            (state.generation, prompt)
        };
        self.spawn_ai_request(endpoint, prompt, generation, cx);
        cx.notify();
    }

    /// 工作线程跑阻塞式流式请求,增量经 channel 回 UI(`cx.spawn` 泵)。
    fn spawn_ai_request(
        &mut self,
        endpoint: AiEndpointConfig,
        prompt: crate::ai::AiPrompt,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let (sender, mut receiver) = mpsc::unbounded::<AiStreamMessage>();
        let cancel = self
            .ai_assistant
            .as_ref()
            .map(|state| state.cancel.clone())
            .unwrap_or_default();
        std::thread::Builder::new()
            .name("velora-ai".to_string())
            .spawn(move || {
                let client = default_client();
                let result = stream_completion(
                    &client,
                    &endpoint,
                    &prompt,
                    &mut |delta| {
                        let _ = sender
                            .unbounded_send(AiStreamMessage::Delta(delta.to_string()));
                    },
                    cancel,
                );
                let _ = sender.unbounded_send(AiStreamMessage::Finished(result));
            })
            .expect("spawn velora-ai thread");

        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            while let Some(message) = receiver.next().await {
                match message {
                    AiStreamMessage::Delta(delta) => {
                        let update = this.update(cx, |editor, cx| {
                            editor.ai_push_delta(&delta, generation, cx)
                        });
                        if update.is_err() {
                            break;
                        }
                    }
                    AiStreamMessage::Finished(result) => {
                        let _ = this.update(cx, |editor, cx| {
                            editor.ai_on_finished(result, generation, cx)
                        });
                        break;
                    }
                }
            }
        })
        .detach();
    }

    fn ai_push_delta(&mut self, delta: &str, generation: u64, cx: &mut Context<Self>) {
        let Some(state) = self.ai_assistant.as_mut() else {
            return;
        };
        if state.generation != generation || !matches!(state.phase, AiPhase::Running) {
            return;
        }
        state.result.push_str(delta);
        cx.notify();
    }

    fn ai_on_finished(
        &mut self,
        result: Result<String, AiRequestError>,
        generation: u64,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.ai_assistant.as_mut() else {
            return;
        };
        if state.generation != generation || !matches!(state.phase, AiPhase::Running) {
            return;
        }
        state.generation += 1;
        match result {
            Ok(full) => {
                state.result = full;
                if state.result.is_empty() {
                    state.phase = AiPhase::Failed;
                    state.error = Some(localize_ai_error(&AiRequestError::Protocol(
                        "empty completion".to_string(),
                    ), cx));
                } else {
                    state.phase = AiPhase::Finished;
                }
            }
            Err(AiRequestError::Cancelled) => {
                // 主动停止已在 ai_stop 里转过状态;这里只兜底。
                if state.result.is_empty() {
                    state.phase = AiPhase::Menu;
                    state.action = None;
                } else {
                    state.phase = AiPhase::Finished;
                }
            }
            Err(error) => {
                state.phase = AiPhase::Failed;
                state.error = Some(localize_ai_error(&error, cx));
            }
        }
        cx.notify();
    }

    /// 重试当前动作(沿用 Failed/Finished 时的动作)。
    pub(in crate::editor) fn ai_retry(&mut self, cx: &mut Context<Self>) {
        let Some(action) = self
            .ai_assistant
            .as_ref()
            .and_then(|state| state.action.clone())
        else {
            return;
        };
        self.run_ai_action(action, cx);
    }

    /// 应用结果:替换锚点区间,或在锚点末尾插入(总结/续写)。
    pub(in crate::editor) fn ai_apply(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.ai_assistant.as_ref() else {
            return false;
        };
        let Some(action) = state.action.clone() else {
            return false;
        };
        let Some(anchor) = state.anchor.clone() else {
            return false;
        };
        let result = strip_wrapping_code_fence(state.result.trim());
        if result.is_empty() {
            return false;
        }

        // 防漂移:替换类校验整段原文;插入类校验插入点前的短上下文。
        let insert_only = !action.replaces_selection();
        let drifted = if insert_only {
            let start = anchor.source_range.start;
            !anchor.tail_context.is_empty()
                && (start < anchor.tail_context.len()
                    || self.buffer.slice(start - anchor.tail_context.len()..start)
                        != anchor.tail_context)
        } else {
            self.buffer.slice(anchor.source_range.clone()) != anchor.selected_text
        };
        if drifted {
            let state = self.ai_assistant.as_mut().expect("checked above");
            state.generation += 1;
            state.phase = AiPhase::Failed;
            state.error = Some(
                cx.global::<I18nManager>()
                    .strings()
                    .ai_panel_stale_document
                    .clone(),
            );
            cx.notify();
            return false;
        }

        let new_text = match action {
            AiAction::Summarize => format!("\n\n{result}"),
            AiAction::ContinueWriting => result.to_string(),
            _ => result.to_string(),
        };
        let apply_range = if insert_only {
            let end = anchor.source_range.end;
            end..end
        } else {
            anchor.source_range
        };

        self.prepare_undo_capture(UndoCaptureKind::NonCoalescible, cx);
        self.write_back_cross_block_source_edit(apply_range, &new_text, cx);
        self.mark_dirty_written_back(cx);
        self.finalize_pending_undo_capture(cx);
        self.request_active_block_scroll_into_view(cx);
        self.close_ai_assistant(cx);
        cx.notify();
        true
    }
}

/// 默认端点的传输配置;列表为空返回 `None`(面板引导去设置页)。
fn default_endpoint_from_settings(settings: &AiSettings) -> Option<AiEndpointConfig> {
    settings
        .default_endpoint()
        .map(|endpoint| endpoint.endpoint_config())
}

/// 组装一次请求的提示词(动作里的翻译目标已由面板解析好)。
fn build_ai_prompt(action: &AiAction, anchor: &AiAnchor) -> crate::ai::AiPrompt {
    let context = AiPromptContext {
        document_title: String::new(),
        selected: anchor.selected_text.clone(),
        before_cursor: anchor.before_cursor.clone(),
        after_cursor: String::new(),
    };
    build_prompt(action, &context)
}

/// 把请求错误翻成用户能读的一句话(替换模板里的 `{error}`)。
fn localize_ai_error(error: &AiRequestError, cx: &App) -> String {
    let strings = cx.global::<I18nManager>().strings();
    let (template, detail) = match error {
        AiRequestError::Cancelled => return String::new(),
        AiRequestError::Network(detail) => (&strings.ai_error_network, detail.clone()),
        AiRequestError::Http { message, .. } => (&strings.ai_error_http, message.clone()),
        AiRequestError::Protocol(detail) => (&strings.ai_error_protocol, detail.clone()),
    };
    template.replace("{error}", &detail)
}

/// 模型偶发把整段结果包进一对代码围栏;只有「开头一对围栏且中间再无围栏」
/// 时剥掉,避免把普通改写结果贴成代码块。内容本身带代码块的不动。
pub(in crate::editor) fn strip_wrapping_code_fence(text: &str) -> &str {
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix("```") else {
        return trimmed;
    };
    // 跳过语言标注行。
    let Some(after_first_line) = rest.find('\n') else {
        return trimmed;
    };
    let body = &rest[after_first_line + 1..];
    let Some((inner, _)) = body.rsplit_once("```") else {
        return trimmed;
    };
    if inner.contains("```") {
        return trimmed;
    }
    inner.trim_matches('\n')
}

#[cfg(test)]
mod tests;

/// 动作的菜单/面板标题。
fn ai_action_label(action: &AiAction, strings: &crate::i18n::I18nStrings) -> String {
    match action {
        AiAction::Polish => strings.ai_action_polish.clone(),
        AiAction::FixGrammar => strings.ai_action_fix_grammar.clone(),
        AiAction::Translate(_) => strings.ai_action_translate.clone(),
        AiAction::Summarize => strings.ai_action_summarize.clone(),
        AiAction::ContinueWriting => strings.ai_action_continue.clone(),
        AiAction::Rewrite(_) => strings.ai_action_rewrite.clone(),
        AiAction::Custom(instruction) => {
            let mut label: String = instruction.chars().take(24).collect();
            if instruction.chars().count() > 24 {
                label.push('…');
            }
            label
        }
    }
}

/// 一行动作/结果区里的通用小按钮。
fn ai_button(
    id: &'static str,
    label: String,
    primary: bool,
    theme: &Theme,
    on_click: impl Fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>) + 'static,
    editor_handle: &WeakEntity<Editor>,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    let handle = editor_handle.clone();
    div()
        .id(id)
        .h(px(26.0))
        .px(px(10.0))
        .flex()
        .items_center()
        .rounded(px((d.dialog_radius - 4.0).max(3.0)))
        .border(px(if primary { 0.0 } else { d.dialog_border_width }))
        .border_color(c.dialog_border)
        .bg(if primary {
            c.dialog_primary_button_bg
        } else {
            c.dialog_secondary_button_bg
        })
        .hover(move |this| {
            this.bg(if primary {
                c.dialog_primary_button_hover
            } else {
                c.dialog_secondary_button_hover
            })
        })
        .cursor_pointer()
        .text_size(px(t.dialog_button_size))
        .font_weight(t.dialog_button_weight.to_font_weight())
        .text_color(if primary {
            c.dialog_primary_button_text
        } else {
            c.dialog_secondary_button_text
        })
        .child(SharedString::from(label))
        .on_click(move |event: &ClickEvent, window, cx| {
            let _ = handle.update(cx, |editor, cx| on_click(editor, event, window, cx));
        })
        .into_any_element()
}

/// 一行动作:整行可点,悬停高亮;`expanded` 为真时行首箭头转向。
fn ai_menu_row(
    id: impl Into<ElementId>,
    label: String,
    expanded: Option<bool>,
    theme: &Theme,
    on_click: impl Fn(&mut Editor, &ClickEvent, &mut Window, &mut Context<Editor>) + 'static,
    editor_handle: &WeakEntity<Editor>,
) -> AnyElement {
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    let handle = editor_handle.clone();
    div()
        .id(id.into())
        .h(px(28.0))
        .w_full()
        .px(px(8.0))
        .flex()
        .items_center()
        .justify_between()
        .rounded(px(d.menu_item_radius))
        .cursor_pointer()
        .hover(|this| this.bg(c.dialog_secondary_button_hover))
        .text_size(px(t.dialog_body_size))
        .text_color(c.dialog_body)
        .on_click(move |event: &ClickEvent, window, cx| {
            let _ = handle.update(cx, |editor, cx| on_click(editor, event, window, cx));
        })
        .child(label)
        .when_some(expanded, |this, expanded| {
            this.child(
                div()
                    .text_size(px(t.dialog_body_size))
                    .text_color(c.dialog_muted)
                    .child(if expanded { "\u{2303}" } else { "\u{2304}" }),
            )
        })
        .into_any_element()
}

/// 流式/完成状态的正文:按行渲染(空行占位),保证换行可见。
fn ai_result_lines(text: &str, theme: &Theme, muted: bool) -> AnyElement {
    let c = &theme.colors;
    let t = &theme.typography;
    let mut column = div().w_full().flex().flex_col();
    for line in text.split('\n') {
        if line.is_empty() {
            column = column.child(div().h(px(t.text_size * 0.6)));
            continue;
        }
        column = column.child(
            div()
                .w_full()
                .text_size(px(t.dialog_body_size))
                .line_height(relative(t.text_line_height))
                .text_color(if muted { c.dialog_muted } else { c.text_default })
                .child(line.to_string()),
        );
    }
    column.into_any_element()
}

/// 渲染 AI 面板浮层(菜单 / 生成中 / 完成 / 失败 四种相)。
pub(in crate::editor) fn render_ai_assistant_overlay(
    editor: &Editor,
    theme: &Theme,
    cx: &mut Context<Editor>,
) -> Option<AnyElement> {
    let state = editor.ai_assistant.as_ref()?;
    let c = &theme.colors;
    let d = &theme.dimensions;
    let t = &theme.typography;
    let strings = cx.global::<I18nManager>().strings_arc();
    let editor_handle = cx.entity().downgrade();

    // 有可用端点就不再挡「未配置」:出厂演示端点保证 ⌘J 永远能跑通。
    let configured = {
        let ai_settings = crate::config::EditorSettings::ai(cx);
        default_endpoint_from_settings(&ai_settings)
            .is_some_and(|endpoint| endpoint.is_configured())
    };

    let mut body: Vec<AnyElement> = Vec::new();
    match state.phase {
        AiPhase::Menu => {
            body.push(state.prompt.clone().into_any_element());
            if !configured {
                // 未配置:动作点不了也没意义,直接给一条入口。
                let handle = editor_handle.clone();
                body.push(
                    div()
                        .id("ai-assistant-not-configured")
                        .mt(px(4.0))
                        .px(px(8.0))
                        .py(px(6.0))
                        .rounded(px(d.menu_item_radius))
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .text_size(px(t.dialog_body_size))
                        .text_color(c.dialog_danger_button_bg)
                        .child(strings.ai_not_configured.clone())
                        .on_click(move |_event, _window, cx| {
                            let _ = handle.update(cx, |editor, cx| {
                                editor.close_ai_assistant(cx);
                                let _ = crate::config::open_preferences_window_at(
                                    cx,
                                    crate::config::PreferencesNav::Ai,
                                );
                            });
                        })
                        .into_any_element(),
                );
            } else {
                body.push(
                    ai_menu_row(
                        "ai-action-polish",
                        strings.ai_action_polish.clone(),
                        None,
                        theme,
                        |editor, _event, _window, cx| {
                            editor.run_ai_action(AiAction::Polish, cx);
                        },
                        &editor_handle,
                    ),
                );
                body.push(
                    ai_menu_row(
                        "ai-action-fix-grammar",
                        strings.ai_action_fix_grammar.clone(),
                        None,
                        theme,
                        |editor, _event, _window, cx| {
                            editor.run_ai_action(AiAction::FixGrammar, cx);
                        },
                        &editor_handle,
                    ),
                );
                body.push(
                    ai_menu_row(
                        "ai-action-translate",
                        strings.ai_action_translate.clone(),
                        Some(state.translate_open),
                        theme,
                        |editor, _event, _window, cx| {
                            if let Some(state) = editor.ai_assistant.as_mut() {
                                state.translate_open = !state.translate_open;
                                state.rewrite_open = false;
                                cx.notify();
                            }
                        },
                        &editor_handle,
                    ),
                );
                if state.translate_open {
                    for target in TranslateTarget::ALL {
                        let target = *target;
                        body.push(
                            ai_menu_row(
                                gpui::ElementId::Name(
                                    format!("ai-translate-{}", target.id()).into(),
                                ),
                                target.label().to_string(),
                                None,
                                theme,
                                move |editor, _event, _window, cx| {
                                    editor.run_ai_action(AiAction::Translate(target), cx);
                                },
                                &editor_handle,
                            ),
                        );
                    }
                }
                body.push(
                    ai_menu_row(
                        "ai-action-summarize",
                        strings.ai_action_summarize.clone(),
                        None,
                        theme,
                        |editor, _event, _window, cx| {
                            editor.run_ai_action(AiAction::Summarize, cx);
                        },
                        &editor_handle,
                    ),
                );
                body.push(
                    ai_menu_row(
                        "ai-action-continue",
                        strings.ai_action_continue.clone(),
                        None,
                        theme,
                        |editor, _event, _window, cx| {
                            editor.run_ai_action(AiAction::ContinueWriting, cx);
                        },
                        &editor_handle,
                    ),
                );
                body.push(
                    ai_menu_row(
                        "ai-action-rewrite",
                        strings.ai_action_rewrite.clone(),
                        Some(state.rewrite_open),
                        theme,
                        |editor, _event, _window, cx| {
                            if let Some(state) = editor.ai_assistant.as_mut() {
                                state.rewrite_open = !state.rewrite_open;
                                state.translate_open = false;
                                cx.notify();
                            }
                        },
                        &editor_handle,
                    ),
                );
                if state.rewrite_open {
                    for (tone, label) in [
                        (RewriteTone::Neutral, strings.ai_rewrite_tone_neutral.clone()),
                        (
                            RewriteTone::Professional,
                            strings.ai_rewrite_tone_professional.clone(),
                        ),
                        (RewriteTone::Concise, strings.ai_rewrite_tone_concise.clone()),
                        (RewriteTone::Friendly, strings.ai_rewrite_tone_friendly.clone()),
                    ] {
                        body.push(
                            ai_menu_row(
                                gpui::ElementId::Name(
                                    format!("ai-rewrite-{}", tone.id()).into(),
                                ),
                                label,
                                None,
                                theme,
                                move |editor, _event, _window, cx| {
                                    editor.run_ai_action(AiAction::Rewrite(tone), cx);
                                },
                                &editor_handle,
                            ),
                        );
                    }
                }
            }
        }
        AiPhase::Running | AiPhase::Finished | AiPhase::Failed => {
            let action_label = state
                .action
                .as_ref()
                .map(|action| ai_action_label(action, &strings))
                .unwrap_or_else(|| strings.ai_assistant.clone());
            // 标题行:动作名 + (生成中呼吸点 | 关闭按钮)。
            let mut header = div()
                .w_full()
                .h(px(28.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .text_size(px(t.dialog_body_size))
                        .font_weight(t.dialog_button_weight.to_font_weight())
                        .text_color(c.dialog_title)
                        .child(action_label),
                );
            if matches!(state.phase, AiPhase::Running) {
                header = header.child(
                    div()
                        .id("ai-running-indicator")
                        .flex()
                        .items_center()
                        .gap(px(6.0))
                        .text_size(px(t.dialog_body_size))
                        .text_color(c.dialog_muted)
                        .child(strings.ai_panel_running.clone())
                        .child(
                            div()
                                .w(px(6.0))
                                .h(px(6.0))
                                .rounded(px(3.0))
                                .bg(c.dialog_primary_button_bg)
                                // 呼吸点:透明度循环,提示仍在生成。
                                .with_animation(
                                    "ai-running-pulse",
                                    Animation::new(Duration::from_millis(1100)).repeat(),
                                    |this, delta| this.opacity(0.35 + 0.65 * delta),
                                ),
                        ),
                );
            } else {
                let handle = editor_handle.clone();
                header = header.child(
                    div()
                        .id("ai-panel-close")
                        .w(px(20.0))
                        .h(px(20.0))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(4.0))
                        .cursor_pointer()
                        .hover(|this| this.bg(c.dialog_secondary_button_hover))
                        .text_size(px(t.dialog_body_size))
                        .text_color(c.dialog_muted)
                        .child("✕")
                        .on_click(move |_event, _window, cx| {
                            let _ = handle.update(cx, |editor, cx| editor.close_ai_assistant(cx));
                        }),
                );
            }
            body.push(header.into_any_element());

            // 正文:生成中显示部分结果;失败显示错误。
            let mut result_area = div()
                .id("ai-assistant-result")
                .w_full()
                .max_h(px(250.0))
                .overflow_y_scroll()
                .px(px(8.0))
                .py(px(4.0))
                .flex()
                .flex_col();
            match &state.phase {
                AiPhase::Failed => {
                    result_area = result_area.child(
                        div()
                            .text_size(px(t.dialog_body_size))
                            .line_height(relative(t.text_line_height))
                            .text_color(c.dialog_danger_button_bg)
                            .child(
                                state
                                    .error
                                    .clone()
                                    .unwrap_or_else(|| strings.ai_error_http.clone()),
                            ),
                    );
                }
                _ => {
                    result_area = result_area.child(ai_result_lines(&state.result, theme, false));
                }
            }
            body.push(result_area.into_any_element());

            // 底部按钮:生成中=停止;完成=应用+重试;失败=重试+关闭。
            let mut footer = div()
                .w_full()
                .pt(px(4.0))
                .px(px(8.0))
                .flex()
                .items_center()
                .justify_end()
                .gap(px(6.0));
            match state.phase {
                AiPhase::Running => {
                    footer = footer.child(ai_button(
                        "ai-stop",
                        strings.ai_panel_stop.clone(),
                        false,
                        theme,
                        |editor, _event, _window, cx| editor.ai_stop(cx),
                        &editor_handle,
                    ));
                }
                AiPhase::Finished => {
                    let apply_label = if state
                        .action
                        .as_ref()
                        .is_some_and(|action| action.replaces_selection())
                    {
                        strings.ai_panel_replace.clone()
                    } else {
                        strings.ai_panel_insert_below.clone()
                    };
                    footer = footer
                        .child(ai_button(
                            "ai-retry",
                            strings.ai_panel_retry.clone(),
                            false,
                            theme,
                            |editor, _event, _window, cx| editor.ai_retry(cx),
                            &editor_handle,
                        ))
                        .child(ai_button(
                            "ai-apply",
                            apply_label,
                            true,
                            theme,
                            |editor, _event, _window, cx| {
                                editor.ai_apply(cx);
                            },
                            &editor_handle,
                        ));
                }
                AiPhase::Failed => {
                    footer = footer
                        .child(ai_button(
                            "ai-close",
                            strings.preferences_cancel.clone(),
                            false,
                            theme,
                            |editor, _event, _window, cx| editor.close_ai_assistant(cx),
                            &editor_handle,
                        ))
                        .child(ai_button(
                            "ai-retry",
                            strings.ai_panel_retry.clone(),
                            true,
                            theme,
                            |editor, _event, _window, cx| editor.ai_retry(cx),
                            &editor_handle,
                        ));
                }
                AiPhase::Menu => {}
            }
            body.push(footer.into_any_element());
        }
    }

    Some(
        div()
            .id("ai-assistant-overlay")
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .on_mouse_down(MouseButton::Left, {
                let handle = editor_handle.clone();
                move |_, _, cx| {
                    let _ = handle.update(cx, |editor, cx| editor.close_ai_assistant(cx));
                }
            })
            .child(
                div()
                    .id("ai-assistant-panel")
                    .debug_selector(|| "ai-assistant-panel".to_string())
                    .absolute()
                    .left(state.origin.x)
                    .top(state.origin.y)
                    .w(px(PANEL_WIDTH))
                    .max_h(px(PANEL_MAX_HEIGHT))
                    .overflow_y_scroll()
                    .p(px(6.0))
                    .flex()
                    .flex_col()
                    .gap(px(2.0))
                    .bg(c.dialog_surface)
                    .border(px(d.dialog_border_width))
                    .border_color(c.dialog_border)
                    .rounded(px(d.dialog_radius))
                    .shadow_lg()
                    .track_focus(&state.focus)
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(body),
            )
            .into_any_element(),
    )
}
