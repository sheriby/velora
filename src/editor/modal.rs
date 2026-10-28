//! 应用内确认与提示。
//!
//! 编辑器持有按钮顺序、替换语义和一次性回调；Kit Dialog 管理焦点范围、键盘与遮罩。

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_kit::base::{Dialog, ElementExt as _};
use gpui_kit::component::{
    ActiveTheme as _,
    button::{Button, ButtonVariants as _},
    dialog::{DialogContent, DialogDescription, DialogFooter, DialogTitle},
    scroll::ScrollableElement as _,
};

use crate::i18n::I18nManager;
use crate::theme::Theme;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Editor, InfoDialogKind};

// 分屏按固定组顺序绘制，弹窗必须另外按出现顺序决定遮挡层级。
static NEXT_MODAL_LAYER: AtomicUsize = AtomicUsize::new(0);

/// 弹层的层号发生器：模态与表格插入对话框共用一个计数，后开的盖在先开的上面。
pub(crate) fn next_modal_layer() -> usize {
    NEXT_MODAL_LAYER.fetch_add(1, Ordering::Relaxed)
}

/// 模态的显示内容 + 按钮语义。
pub(crate) struct ModalSpec {
    pub(crate) title: SharedString,
    pub(crate) detail: Option<SharedString>,
    pub(crate) buttons: Vec<SharedString>,
    /// 默认按钮：高亮，并在对话框壳聚焦时由回车触发。
    pub(crate) default_index: usize,
    /// Esc / 点遮罩对应的按钮。
    pub(crate) cancel_index: usize,
}

#[derive(Clone)]
enum ModalOrigin {
    General,
    Information(InfoDialogKind),
    WindowClose,
    DroppedFile(std::path::PathBuf),
    Folder(std::path::PathBuf),
}

impl ModalOrigin {
    fn selectors(&self) -> (&'static str, &'static str) {
        match self {
            Self::General => ("editor-modal", "editor-modal-overlay"),
            Self::Information(_) => ("info-dialog", "info-dialog-overlay"),
            Self::WindowClose => ("unsaved-changes-dialog", "unsaved-changes-overlay"),
            Self::DroppedFile(_) => ("drop-replace-dialog", "drop-replace-overlay"),
            Self::Folder(_) => ("folder-choice-dialog", "folder-choice-overlay"),
        }
    }

    fn button_ids(&self) -> &'static [&'static str] {
        match self {
            Self::General => &[],
            Self::Information(_) => &["dismiss-info-dialog"],
            Self::WindowClose => &[
                "cancel-close-dialog",
                "discard-and-close-dialog",
                "save-and-close-dialog",
            ],
            Self::DroppedFile(_) => &[
                "cancel-drop-replace-dialog",
                "discard-and-replace-drop-dialog",
                "save-and-replace-drop-dialog",
            ],
            Self::Folder(_) => &[
                "folder-choice-cancel",
                "folder-choice-new-window",
                "folder-choice-replace",
            ],
        }
    }

    fn is_destructive(&self, choice: usize) -> bool {
        choice == 1 && matches!(self, Self::WindowClose | Self::DroppedFile(_))
    }
}

/// 编辑器持有的模态内容、焦点快照和完成回调。
pub(crate) struct EditorModal {
    spec: ModalSpec,
    origin: ModalOrigin,
    identity: uuid::Uuid,
    layer: usize,
    focus: FocusHandle,
    mounted: bool,
    // 未挂载与挂载前没有焦点不同；替换模态时也要保留这个区别。
    restore_focus: Option<Option<WeakFocusHandle>>,
    /// 破坏性按钮（删除确认这类）：不做默认、渲染 danger 样式、排在非默认一侧。
    /// General 模态破坏性标记走这里；窗口关闭/拖放那两种由
    /// [`ModalOrigin::is_destructive`] 按下标写死。
    destructive_index: Option<usize>,
    scroll: ScrollHandle,
    on_choice: Option<Box<dyn FnOnce(usize, &mut Editor, &mut Window, &mut Context<Editor>)>>,
}

impl Editor {
    pub(crate) fn show_modal(
        &mut self,
        spec: ModalSpec,
        on_choice: impl FnOnce(usize, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.open_modal(spec, None, on_choice, cx);
    }

    /// 与 [`Self::show_modal`] 相同，但指明哪个按钮是破坏性操作：它不做默认
    /// （默认按钮让给取消位）、渲染 danger 样式、排序时留在非默认一侧。
    /// 删除确认这类「手滑回车就删文件」的模态必须走这一条。
    pub(crate) fn show_modal_with_destructive_button(
        &mut self,
        spec: ModalSpec,
        destructive_index: usize,
        on_choice: impl FnOnce(usize, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        self.open_modal(spec, Some(destructive_index), on_choice, cx);
    }

    fn open_modal(
        &mut self,
        spec: ModalSpec,
        destructive_index: Option<usize>,
        on_choice: impl FnOnce(usize, &mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let button_count = spec.buttons.len().max(1);
        let destructive_index =
            destructive_index.map(|index| index.min(button_count - 1));
        let mut default_index = spec.default_index.min(button_count - 1);
        // 破坏性按钮不做默认：默认让给取消位；取消位也是它时，让给第一个
        // 非破坏性按钮。回车落在一个删不掉就恢复不了的动作上，代价不对称。
        if destructive_index == Some(default_index) {
            default_index = if spec.cancel_index.min(button_count - 1) != default_index {
                spec.cancel_index.min(button_count - 1)
            } else {
                (0..button_count).find(|index| Some(*index) != destructive_index).unwrap_or(default_index)
            };
        }
        let spec = ModalSpec {
            default_index,
            cancel_index: spec.cancel_index.min(button_count - 1),
            ..spec
        };
        let restore_focus = self.modal.take().and_then(|modal| modal.restore_focus);
        self.modal = Some(EditorModal {
            spec,
            origin: ModalOrigin::General,
            identity: uuid::Uuid::new_v4(),
            layer: NEXT_MODAL_LAYER.fetch_add(1, Ordering::Relaxed),
            focus: cx.focus_handle(),
            mounted: false,
            restore_focus,
            destructive_index,
            scroll: ScrollHandle::new(),
            on_choice: Some(Box::new(on_choice)),
        });
        cx.notify();
    }

    /// 只有一个「好」按钮的提示框（取代 `PromptLevel::Info`/`Warning` 原生框）。
    pub(crate) fn show_message_modal(
        &mut self,
        title: impl Into<SharedString>,
        detail: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let ok: SharedString = cx
            .global::<I18nManager>()
            .strings()
            .info_dialog_ok
            .clone()
            .into();
        self.show_modal(
            ModalSpec {
                title: title.into(),
                detail: Some(detail.into()),
                buttons: vec![ok],
                default_index: 0,
                cancel_index: 0,
            },
            move |_choice, _editor, _window, _cx| {},
            cx,
        );
    }

    #[cfg(test)]
    pub(crate) fn modal_spec(&self) -> Option<&ModalSpec> {
        self.modal.as_ref().map(|modal| &modal.spec)
    }

    /// 生效中的默认按钮与破坏性按钮：破坏性标记由 `open_modal` 归一化，
    /// 「默认 = 破坏性」在打开时就已排除。
    #[cfg(test)]
    pub(crate) fn modal_default_and_destructive(&self) -> Option<(usize, Option<usize>)> {
        self.modal
            .as_ref()
            .map(|modal| (modal.spec.default_index, modal.destructive_index))
    }

    pub(crate) fn modal_is_open(&self) -> bool {
        self.modal.is_some()
    }

    /// 按钮序号（含 Esc/遮罩映射的取消位）被触发：先关模态再跑回调，
    /// 回调里可以再开下一个模态（例如「保存后再关」链式流程）。
    pub(crate) fn dismiss_modal(
        &mut self,
        choice: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current = self
            .modal
            .as_ref()
            .is_some_and(|modal| self.modal_origin_is_current(&modal.origin));
        let Some(mut modal) = self.take_modal(window, cx) else {
            return;
        };
        let on_choice = modal.on_choice.take().filter(|_| current);
        drop(modal);
        if let Some(on_choice) = on_choice {
            on_choice(choice, self, window, cx);
        }
        cx.notify();
    }

    fn take_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<EditorModal> {
        let mut modal = self.modal.take()?;
        if modal.focus.is_focused(window) || modal.focus.contains_focused(window, cx) {
            if let Some(focus) = modal
                .restore_focus
                .take()
                .flatten()
                .and_then(|focus| focus.upgrade())
            {
                focus.focus(window, cx);
            } else {
                window.blur(cx);
            }
        }
        Some(modal)
    }

    fn modal_origin_is_current(&self, origin: &ModalOrigin) -> bool {
        match origin {
            ModalOrigin::General => true,
            ModalOrigin::Information(kind) => self.info_dialog == Some(*kind),
            ModalOrigin::WindowClose => {
                self.show_unsaved_changes_dialog && !self.show_drop_replace_dialog
            }
            ModalOrigin::DroppedFile(path) => {
                self.show_drop_replace_dialog
                    && self.pending_drop_replace_path.as_ref() == Some(path)
            }
            ModalOrigin::Folder(path) => {
                !self.show_unsaved_changes_dialog
                    && !self.show_drop_replace_dialog
                    && self.pending_folder_choice.as_ref() == Some(path)
            }
        }
    }

    pub(crate) fn sync_pending_decision_modal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .modal
            .as_ref()
            .is_some_and(|modal| !self.modal_origin_is_current(&modal.origin))
        {
            drop(self.take_modal(window, cx));
            cx.notify();
        }
        if self.modal.is_some() {
            return;
        }
        let origin = if let Some(kind) = self.info_dialog {
            Some(ModalOrigin::Information(kind))
        } else if self.show_drop_replace_dialog {
            self.pending_drop_replace_path
                .clone()
                .map(ModalOrigin::DroppedFile)
        } else if self.show_unsaved_changes_dialog {
            Some(ModalOrigin::WindowClose)
        } else {
            self.pending_folder_choice.clone().map(ModalOrigin::Folder)
        };
        let Some(origin) = origin else {
            return;
        };
        let strings = cx.global::<I18nManager>().strings();
        let (title, detail, buttons) = match &origin {
            ModalOrigin::WindowClose => (
                strings.unsaved_changes_title.clone(),
                strings.unsaved_changes_message.clone(),
                vec![
                    strings.unsaved_changes_cancel.clone().into(),
                    strings.unsaved_changes_discard_and_close.clone().into(),
                    strings.unsaved_changes_save_and_close.clone().into(),
                ],
            ),
            ModalOrigin::DroppedFile(path) => (
                strings.drop_replace_title.clone(),
                format!("{}\n\n{}", strings.drop_replace_message, path.display()),
                vec![
                    strings.drop_replace_cancel.clone().into(),
                    strings.drop_replace_discard_and_replace.clone().into(),
                    strings.drop_replace_save_and_replace.clone().into(),
                ],
            ),
            ModalOrigin::Folder(path) => (
                strings.workspace_folder_choice_title.clone(),
                path.file_name()
                    .map(|name| format!("{}\n\n{}", name.to_string_lossy(), path.display()))
                    .unwrap_or_else(|| path.display().to_string()),
                vec![
                    strings.open_link_cancel.clone().into(),
                    strings.workspace_open_new_window_button.clone().into(),
                    strings.workspace_replace_current_button.clone().into(),
                ],
            ),
            ModalOrigin::Information(kind) => (
                self.info_dialog_title(strings, *kind).to_string(),
                String::new(),
                vec![strings.info_dialog_ok.clone().into()],
            ),
            ModalOrigin::General => return,
        };
        let default_index = buttons.len().saturating_sub(1);
        let decision = origin.clone();
        self.show_modal(
            ModalSpec {
                title: title.into(),
                detail: (!matches!(&origin, ModalOrigin::Information(_))).then_some(detail.into()),
                buttons,
                default_index,
                cancel_index: 0,
            },
            move |choice, editor, window, cx| {
                let event = ClickEvent::default();
                match (decision, choice) {
                    (ModalOrigin::Information(_), _) => editor.hide_info_dialog(cx),
                    (ModalOrigin::WindowClose, 0) => {
                        editor.on_cancel_close_dialog(&event, window, cx)
                    }
                    (ModalOrigin::WindowClose, 1) => {
                        editor.on_discard_and_close(&event, window, cx)
                    }
                    (ModalOrigin::WindowClose, 2) => editor.on_save_and_close(&event, window, cx),
                    (ModalOrigin::DroppedFile(_), 0) => {
                        editor.on_cancel_drop_replace_dialog(&event, window, cx)
                    }
                    (ModalOrigin::DroppedFile(_), 1) => {
                        editor.on_discard_and_replace_drop(&event, window, cx)
                    }
                    (ModalOrigin::DroppedFile(_), 2) => {
                        editor.on_save_and_replace_drop(&event, window, cx)
                    }
                    (ModalOrigin::Folder(_), 0) => {
                        editor.on_folder_choice_cancel(&event, window, cx)
                    }
                    (ModalOrigin::Folder(_), 1) => {
                        editor.on_folder_choice_new_window(&event, window, cx)
                    }
                    (ModalOrigin::Folder(_), 2) => {
                        editor.on_folder_choice_replace(&event, window, cx)
                    }
                    _ => {}
                }
            },
            cx,
        );
        if let Some(modal) = self.modal.as_mut() {
            modal.origin = origin;
            // 这些请求原本会清空窗口焦点，先保存实际控件，再交给模态阻止后续输入。
            modal.restore_focus = Some(window.focused(cx).map(|focus| focus.downgrade()));
            modal.focus.focus(window, cx);
        }
    }

    pub(crate) fn cancel_modal(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(cancel_index) = self.modal.as_ref().map(|modal| modal.spec.cancel_index) else {
            return;
        };
        self.dismiss_modal(cancel_index, window, cx);
    }

    fn dismiss_modal_if_current(
        &mut self,
        identity: uuid::Uuid,
        choice: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .modal
            .as_ref()
            .is_some_and(|modal| modal.identity == identity)
        {
            self.dismiss_modal(choice, window, cx);
        }
    }

    pub(crate) fn render_modal_overlay(
        &mut self,
        theme: &Theme,
        cx: &mut Context<Self>,
    ) -> Option<impl IntoElement> {
        let modal = self.modal.as_ref()?;
        let identity = modal.identity;
        let default_index = modal.spec.default_index;
        let cancel_index = modal.spec.cancel_index;
        let destructive_index = modal.destructive_index;
        let focus = modal.focus.clone();
        let (panel_selector, overlay_selector) = modal.origin.selectors();
        let mut footer = DialogFooter::new().flex_shrink_0().flex_wrap();
        let mut buttons: Vec<_> = modal.spec.buttons.iter().enumerate().collect();
        // 屏显顺序规则：默认按钮稳定挪到队尾（右下主按钮），破坏性按钮因为
        // 不做默认，自然留在非默认一侧（左）。
        buttons.sort_by_key(|(index, _)| *index == default_index);
        for (index, label) in buttons {
            let named_id = modal.origin.button_ids().get(index).copied();
            let button_id: ElementId = named_id
                .map(Into::into)
                .unwrap_or_else(|| ("editor-modal-button", index).into());
            let is_destructive =
                modal.origin.is_destructive(index) || destructive_index == Some(index);
            footer = footer.child(
                Button::new(button_id)
                    .debug_selector(move || {
                        named_id
                            .map(str::to_owned)
                            .unwrap_or_else(|| format!("editor-modal-button-{index}"))
                    })
                    .label(label.clone())
                    .when(index == default_index, |button| button.primary())
                    .when(index != default_index, |button| button.outline())
                    .when(is_destructive, |button| button.danger().outline())
                    .on_click(cx.listener(move |editor, _, window, cx| {
                        editor.dismiss_modal_if_current(identity, index, window, cx);
                    }))
                    .on_action(cx.listener(
                        move |editor, _: &gpui_kit::base::actions::Confirm, window, cx| {
                            editor.dismiss_modal_if_current(identity, index, window, cx);
                        },
                    )),
            );
        }
        let body = match &modal.origin {
            ModalOrigin::Information(kind) => {
                Some(self.render_info_dialog_body(cx.global::<I18nManager>().strings(), *kind, cx))
            }
            _ => modal
                .spec
                .detail
                .clone()
                .map(|detail| DialogDescription::new().child(detail).into_any_element()),
        };
        let owner = cx.entity().downgrade();
        let panel = div()
            .id(panel_selector)
            .debug_selector(move || panel_selector.into())
            .occlude()
            .w(rems(28.0))
            .max_w_full()
            .max_h(relative(1.0))
            .flex()
            .flex_col()
            .gap_4()
            .p_4()
            .bg(cx.theme().popover)
            .text_color(cx.theme().popover_foreground)
            .border_1()
            .border_color(cx.theme().border)
            .rounded(cx.theme().radius_lg)
            .shadow_lg()
            .child(
                DialogTitle::new()
                    .flex_shrink_0()
                    .child(modal.spec.title.clone()),
            )
            .when_some(body, |panel, body| {
                panel.child(
                    div()
                        .id("editor-modal-body")
                        .min_h_0()
                        .relative()
                        .overflow_y_scroll()
                        .track_scroll(&modal.scroll)
                        .child(DialogContent::new().flex_none().child(body))
                        .vertical_scrollbar(&modal.scroll),
                )
            })
            .child(footer)
            .on_prepaint(move |_, window, cx| {
                if let Err(error) = owner.update(cx, |editor, cx| {
                    let Some(modal) = editor
                        .modal
                        .as_mut()
                        .filter(|modal| modal.identity == identity && !modal.mounted)
                    else {
                        return;
                    };
                    // 先让正文完成挂载，避免同帧的待办焦点覆盖新模态。
                    if modal.restore_focus.is_none() {
                        modal.restore_focus =
                            Some(window.focused(cx).map(|focus| focus.downgrade()));
                    }
                    modal.mounted = true;
                    modal.focus.focus(window, cx);
                }) {
                    eprintln!("挂载模态焦点失败：{error}");
                }
            });
        let confirm_owner = cx.entity().downgrade();
        let cancel_owner = confirm_owner.clone();
        // 保留多按钮序号及分屏归属，用受控 Dialog 组合标准部件，不借窗口的全局弹窗堆栈。
        Some(
            div()
                .id(identity)
                .debug_selector(move || overlay_selector.into())
                .absolute()
                .inset_0()
                .child(
                    Dialog::new(cx)
                        .focus_handle(focus)
                        .layer(modal.layer, true)
                        .p_4()
                        .backdrop(div().size_full().bg(theme.colors.dialog_backdrop))
                        .popup(panel)
                        .on_ok(move |_, window, cx| {
                            if let Err(error) = confirm_owner.update(cx, |editor, cx| {
                                editor.dismiss_modal_if_current(
                                    identity,
                                    default_index,
                                    window,
                                    cx,
                                );
                            }) {
                                eprintln!("确认模态失败：{error}");
                            }
                            false
                        })
                        .on_cancel(move |_, window, cx| {
                            if let Err(error) = cancel_owner.update(cx, |editor, cx| {
                                editor.dismiss_modal_if_current(identity, cancel_index, window, cx);
                            }) {
                                eprintln!("取消模态失败：{error}");
                            }
                            false
                        }),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{Editor, ModalSpec};
    use gpui::TestAppContext;
    use std::{cell::Cell, rc::Rc};

    /// 删除确认的「确认删除」是破坏性按钮：不做默认（回车落在取消上）、
    /// 带破坏性标记。以前 default_index=0 就是确认位，一次
    /// 手滑回车就把文件删了。
    #[gpui::test]
    fn delete_confirmation_never_defaults_to_the_destructive_button(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let target = std::env::temp_dir().join(format!(
            "velora-delete-confirm-{}.md",
            uuid::Uuid::new_v4()
        ));
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        editor.update(cx, |editor, _cx| {
            editor.workspace.selected =
                Some(crate::editor::workspace::WorkspaceSelection::File(target));
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.prompt_delete_selected(window, cx);
        });
        let (default_index, destructive) = editor.read_with(cx, |editor, _| {
            editor.modal_default_and_destructive().expect("删除确认应打开模态")
        });
        assert_eq!(destructive, Some(0), "「确认删除」应带破坏性标记");
        assert_ne!(
            default_index, 0,
            "破坏性按钮不能是默认按钮：回车不该直接删文件"
        );
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.modal_spec().unwrap().cancel_index),
            1,
            "Esc 应对应「取消」"
        );
    }

    #[gpui::test]
    fn pending_decision_filters_an_expired_drop_target(cx: &mut TestAppContext) {
        let root =
            std::env::temp_dir().join(format!("velora-decision-target-{}", uuid::Uuid::new_v4()));
        let _config = crate::config::override_test_config_root(root.join("profile"));
        std::fs::create_dir_all(&root).unwrap();
        let first = root.join("first.md");
        let second = root.join("second.md");
        std::fs::write(&first, "旧请求").unwrap();
        std::fs::write(&second, "新请求").unwrap();
        cx.on_quit({
            let root = root.clone();
            move || std::fs::remove_dir_all(root).unwrap()
        });
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "原文".into(), None));
        editor.update_in(cx, |editor, window, cx| {
            editor.mark_dirty(cx);
            editor.request_dropped_markdown_replace(first, window, cx);
            let identity = editor.modal.as_ref().unwrap().identity;
            editor.pending_drop_replace_path = Some(second.clone());
            editor.dismiss_modal_if_current(identity, 1, window, cx);
            assert_eq!(editor.buffer.text(), "原文", "旧决定不能替换为新的拖入目标");
            assert!(editor.document_dirty);
            assert_eq!(editor.pending_drop_replace_path.as_ref(), Some(&second));
            editor.sync_pending_decision_modal(window, cx);
            assert!(
                editor
                    .modal_spec()
                    .unwrap()
                    .detail
                    .as_ref()
                    .unwrap()
                    .contains("second.md")
            );
            editor.cancel_modal(window, cx);
            assert!(editor.pending_drop_replace_path.is_none());
        });
        cx.update(|window, _| window.remove_window());
    }

    #[gpui::test]
    fn modal_completion_only_applies_to_the_current_instance(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        editor.update(cx, |editor, cx| editor.show_message_modal("旧提示", "", cx));
        let old_identity =
            editor.read_with(cx, |editor, _| editor.modal.as_ref().unwrap().identity);
        let choice = Rc::new(Cell::new(None));
        editor.update(cx, |editor, cx| {
            let choice = choice.clone();
            editor.show_modal(
                ModalSpec {
                    title: "新的确认".into(),
                    detail: None,
                    buttons: vec!["应用".into(), "取消".into()],
                    default_index: 0,
                    cancel_index: 1,
                },
                move |index, _, _, _| choice.set(Some(index)),
                cx,
            );
        });
        editor.update_in(cx, |editor, window, cx| {
            editor.dismiss_modal_if_current(old_identity, 0, window, cx)
        });
        assert_eq!(choice.get(), None, "旧实例的完成回调不能确认替换后的弹窗");
        let current = editor.read_with(cx, |editor, _| editor.modal.as_ref().unwrap().identity);
        editor.update_in(cx, |editor, window, cx| {
            editor.dismiss_modal_if_current(current, 1, window, cx)
        });
        assert_eq!(choice.get(), Some(1));
        assert!(editor.read_with(cx, |editor, _| !editor.modal_is_open()));
    }
}
