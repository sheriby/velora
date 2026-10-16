//! 文件历史只读预览与差异对比；恢复需确认，并保留当前内容和撤销链。

use super::{Editor, HistoryEntry, modal::ModalSpec};
use crate::{
    config::{
        self, EditorSettings,
        file_history::{HistorySource, HistoryVersion},
    },
    i18n::{I18nManager, I18nStrings},
    theme::Theme,
};
use gpui::prelude::FluentBuilder;
use gpui::*;
use std::{path::PathBuf, sync::Arc, time::SystemTime};

mod diff;
#[cfg(test)]
mod tests;

pub(crate) struct FileHistoryOverlay {
    versions: Vec<HistoryVersion>,
    pub(crate) selected: usize,
    document: PathBuf,
    buffer_id: u64,
    compare: bool,
    preview: String,
    lines: Arc<Vec<diff::DiffLine>>,
    compared_revision: Option<u64>,
    scroll: UniformListScrollHandle,
}

#[cfg(test)]
pub(crate) fn file_history_version_label(stem: &str) -> Option<String> {
    let seconds = stem.parse::<u64>().ok()? / 1_000_000_000;
    Some(super::workspace::chrono_like_date_string_public(seconds))
}

impl Editor {
    pub(crate) fn file_history_is_open(&self) -> bool {
        self.file_history_overlay.is_some()
    }

    pub(crate) fn open_file_history(&mut self, cx: &mut Context<Self>) {
        self.dismiss_contextual_overlays(cx);
        let Some(path) = self.file_path.clone() else {
            return;
        };
        let versions = match config::file_history::list_versions(
            &path,
            EditorSettings::file_history(cx),
            SystemTime::now(),
        ) {
            Ok(versions) => versions,
            Err(error) => {
                self.history_error(format!("无法读取文件历史：{error}"), cx);
                return;
            }
        };
        self.file_history_overlay = Some(FileHistoryOverlay {
            versions,
            selected: 0,
            document: path,
            buffer_id: self.buffer.identity(),
            compare: false,
            preview: String::new(),
            lines: Arc::new(Vec::new()),
            compared_revision: None,
            scroll: UniformListScrollHandle::new(),
        });
        self.select_file_history_version(0, cx);
        cx.notify();
    }

    fn history_error(&mut self, detail: String, cx: &mut Context<Self>) {
        eprintln!("{detail}");
        let title = cx
            .global::<I18nManager>()
            .strings()
            .menu_file_history
            .clone();
        self.show_message_modal(title, detail, cx);
    }

    fn select_file_history_version(&mut self, selected: usize, cx: &mut Context<Self>) {
        let Some(overlay) = self.file_history_overlay.as_mut() else {
            return;
        };
        let Some(version) = overlay.versions.get(selected) else {
            return;
        };
        match super::encoding::load_document(&version.path) {
            Ok(document) => {
                overlay.selected = selected;
                overlay.preview = super::encoding::normalize_document_text(&document.text);
                overlay.compared_revision = None;
                overlay.scroll = UniformListScrollHandle::new();
                cx.notify();
            }
            Err(error) => self.history_error(format!("无法读取历史版本：{error}"), cx),
        }
    }

    pub(crate) fn close_file_history(&mut self, cx: &mut Context<Self>) {
        if self.file_history_overlay.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn file_history_key_down(
        &mut self,
        keystroke: &Keystroke,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(overlay) = self.file_history_overlay.as_ref() else {
            return false;
        };
        let selected = overlay.selected;
        let count = overlay.versions.len();
        match keystroke.key.as_str() {
            "up" | "down" => {
                if count > 0 {
                    let next = if keystroke.key == "up" {
                        (selected + count - 1) % count
                    } else {
                        (selected + 1) % count
                    };
                    self.select_file_history_version(next, cx);
                }
            }
            "enter" => self.restore_file_history_version(selected, cx),
            "escape" => self.close_file_history(cx),
            _ => return false,
        }
        cx.stop_propagation();
        true
    }

    pub(crate) fn restore_file_history_version(&mut self, selected: usize, cx: &mut Context<Self>) {
        let Some(overlay) = self.file_history_overlay.as_ref() else {
            return;
        };
        let Some(version) = overlay.versions.get(selected) else {
            return;
        };
        let version = version.path.clone();
        let document = overlay.document.clone();
        let buffer_id = overlay.buffer_id;
        let strings = cx.global::<I18nManager>().strings();
        self.show_modal(
            ModalSpec {
                title: strings.file_history_confirm_title.clone().into(),
                detail: Some(strings.file_history_confirm_detail.clone().into()),
                buttons: vec![
                    strings.file_history_confirm.clone().into(),
                    strings.preferences_cancel.clone().into(),
                ],
                default_index: 1,
                cancel_index: 1,
            },
            move |choice, editor, _window, cx| {
                if choice != 0 {
                    return;
                }
                if editor.file_path.as_ref() != Some(&document)
                    || editor.buffer.identity() != buffer_id
                {
                    editor.history_error("当前文档已改变，请重新打开文件历史。".into(), cx);
                    return;
                }
                if editor.has_marked_document_text(cx) {
                    editor.history_error("请先完成输入法输入，再恢复历史版本。".into(), cx);
                    return;
                }
                // 上限可配置为一条，先读目标再备份，避免备份清理掉待恢复版本。
                let content = match super::encoding::load_document(&version) {
                    Ok(document) => super::encoding::normalize_document_text(&document.text),
                    Err(error) => {
                        editor.history_error(format!("无法读取历史版本：{error}"), cx);
                        return;
                    }
                };
                let current = editor.buffer.file_bytes();
                if !editor.record_saved_history(
                    &document,
                    &current,
                    HistorySource::BeforeRestore,
                    cx,
                ) {
                    return;
                }
                editor.finalize_pending_undo_capture(cx);
                let selection = editor.capture_source_selection_snapshot(cx);
                let edit = editor.buffer.edit(0..editor.buffer.byte_len(), &content);
                editor.undo_history.push(HistoryEntry {
                    edits: vec![edit],
                    selection,
                    timestamp: std::time::Instant::now(),
                    kind: crate::components::UndoCaptureKind::NonCoalescible,
                });
                editor.redo_history.clear();
                editor.clear_cross_block_selection(cx);
                editor.rebuild_document_from_buffer(cx);
                editor.close_file_history(cx);
                editor.mark_dirty_written_back(cx);
                editor.pending_focus = editor.first_focusable_entity_id(cx);
                cx.notify();
            },
            cx,
        );
    }

    pub(crate) fn render_file_history_overlay(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let overlay = self.file_history_overlay.as_mut()?;
        if overlay.compared_revision != Some(self.buffer.revision()) {
            overlay.lines = Arc::new(if overlay.compare {
                diff::diff_lines(&overlay.preview, &self.buffer.text())
            } else {
                diff::diff_lines(&overlay.preview, &overlay.preview)
            });
            overlay.compared_revision = Some(self.buffer.revision());
        }
        let selected = overlay.selected;
        let compare = overlay.compare;
        let empty = overlay.versions.is_empty();
        let lines = overlay.lines.clone();
        let colors = theme.colors.clone();
        let rows = overlay
            .versions
            .iter()
            .enumerate()
            .map(|(index, version)| {
                let seconds = version
                    .timestamp
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let date = format!(
                    "{}:{:02} UTC",
                    super::workspace::chrono_like_date_string_public(seconds),
                    seconds % 60
                );
                let source = match version.source {
                    HistorySource::Automatic => &strings.file_history_automatic,
                    HistorySource::Manual => &strings.file_history_manual,
                    HistorySource::Exit => &strings.file_history_exit,
                    HistorySource::BeforeRestore => &strings.file_history_before_restore,
                    HistorySource::Baseline => &strings.file_history_baseline,
                    HistorySource::Legacy => &strings.file_history_legacy,
                };
                div()
                    .id(ElementId::Name(
                        format!("file-history-entry-{index}").into(),
                    ))
                    .debug_selector(move || format!("file-history-entry-{index}"))
                    .flex()
                    .flex_col()
                    .px_2()
                    .py_2()
                    .rounded_sm()
                    .cursor_pointer()
                    .bg(if index == selected {
                        colors.selection
                    } else {
                        transparent_black()
                    })
                    .hover(|this| this.bg(colors.dialog_secondary_button_hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |editor, _, _, cx| {
                            editor.select_file_history_version(index, cx)
                        }),
                    )
                    .child(div().child(date))
                    .child(
                        div()
                            .text_color(colors.dialog_muted)
                            .text_size(px(12.0))
                            .child(source.clone()),
                    )
            })
            .collect::<Vec<_>>();
        let code_family: SharedString = EditorSettings::fonts(cx).code_family.into();
        let code_size = EditorSettings::scaled_font_sizes(cx).1;
        let content = uniform_list(
            "file-history-lines",
            lines.len().max(1),
            move |range, _, _| {
                range
                    .map(|index| {
                        let line = lines.get(index);
                        let (background, sign) = match line.map(|line| line.kind) {
                            Some(diff::DiffKind::Added) => (hsla(0.33, 0.6, 0.45, 0.16), "+"),
                            Some(diff::DiffKind::Removed) => (hsla(0.0, 0.7, 0.5, 0.16), "−"),
                            _ => (transparent_black(), " "),
                        };
                        let old = line
                            .and_then(|line| line.old_line)
                            .map(|value| value.to_string())
                            .unwrap_or_default();
                        let new = line
                            .and_then(|line| line.new_line)
                            .map(|value| value.to_string())
                            .unwrap_or_default();
                        div()
                            .debug_selector(move || format!("file-history-code-line-{index}"))
                            .flex()
                            .h(px(code_size * 1.5))
                            .items_center()
                            .bg(background)
                            .font_family(code_family.clone())
                            .text_size(px(code_size))
                            .child(
                                div()
                                    .w(px(52.0))
                                    .flex_shrink_0()
                                    .text_color(colors.dialog_muted)
                                    .child(old),
                            )
                            .when(compare, |this| {
                                this.child(
                                    div()
                                        .w(px(52.0))
                                        .flex_shrink_0()
                                        .text_color(colors.dialog_muted)
                                        .child(new),
                                )
                            })
                            .child(div().w(px(20.0)).flex_shrink_0().child(sign))
                            .child(
                                div().whitespace_nowrap().child(
                                    line.map(|line| line.text.trim_end_matches('\n').to_string())
                                        .unwrap_or_default(),
                                ),
                            )
                    })
                    .collect()
            },
        )
        .track_scroll(overlay.scroll.clone())
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0));
        let colors = &theme.colors;
        Some(div().id("file-history-overlay").debug_selector(|| "file-history-overlay".into())
            .absolute().inset_0().occlude().flex().items_center().justify_center().bg(colors.dialog_backdrop)
            .on_mouse_down(MouseButton::Left, cx.listener(|editor, _, _, cx| editor.close_file_history(cx)))
            .child(div().w(relative(0.94)).h(relative(0.86)).max_w(px(1200.0)).min_w(px(0.0)).flex().flex_col().gap_3().p_4()
                .rounded(px(theme.dimensions.dialog_radius)).bg(colors.dialog_surface).border_1().border_color(colors.dialog_border).shadow_lg()
                .text_color(colors.text_default).text_size(px(theme.typography.ui_text_size(14.0)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(div().flex().justify_between().items_center()
                    .child(div().text_size(px(theme.typography.dialog_title_size)).child(strings.menu_file_history.clone()))
                    .child(div().id("file-history-close").cursor_pointer().px_2().py_1().child(strings.file_history_close.clone())
                        .on_mouse_down(MouseButton::Left, cx.listener(|editor, _, _, cx| editor.close_file_history(cx)))))
                .when(empty, |this| this.child(div().flex_1().child(strings.file_history_empty.clone())))
                .when(!empty, |this| this.child(div().flex().flex_1().min_h(px(0.0)).gap_3()
                    .child(div().id("file-history-versions").w(px(205.0)).flex_shrink_0().overflow_y_scroll().children(rows))
                    .child(div().flex().flex_col().flex_1().min_w(px(0.0)).min_h(px(0.0)).gap_2()
                        .child(div().flex().gap_2().children([
                            (false, "file-history-preview", strings.file_history_preview.clone()),
                            (true, "file-history-compare", strings.file_history_compare.clone()),
                        ].into_iter().map(|(mode, id, label)| div().id(id).debug_selector(move || id.into())
                            .px_3().py_1().rounded_sm().cursor_pointer().bg(if compare == mode { colors.selection } else { colors.dialog_surface })
                            .child(label).on_mouse_down(MouseButton::Left, cx.listener(move |editor, _, _, cx| {
                                if let Some(overlay) = editor.file_history_overlay.as_mut() {
                                    overlay.compare = mode; overlay.compared_revision = None; overlay.scroll = UniformListScrollHandle::new();
                                    cx.notify();
                                }
                            })))))
                        .child(content))))
                .when(!empty, |this| this.child(div().flex().justify_end().child(div().id("file-history-restore").debug_selector(|| "file-history-restore".into())
                    .px_3().py_2().rounded_sm().cursor_pointer().bg(colors.dialog_primary_button_bg).text_color(colors.dialog_primary_button_text)
                    .child(strings.file_history_restore.clone())
                    .on_mouse_down(MouseButton::Left, cx.listener(move |editor, _, _, cx| editor.restore_file_history_version(selected, cx)))))))
            .into_any_element())
    }

    pub(crate) fn on_file_history_action(
        &mut self,
        _: &crate::components::FileHistory,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_file_history(cx);
    }
}
