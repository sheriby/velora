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
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

mod diff;
#[cfg(test)]
mod tests;

fn file_history_title(document: &Path, strings: &I18nStrings) -> String {
    let name = document
        .file_name()
        .unwrap_or(document.as_os_str())
        .to_string_lossy();
    format!(
        "{} · {name}",
        strings.menu_file_history.trim_end_matches('…')
    )
}

fn file_history_timestamp_label(timestamp: SystemTime) -> String {
    let local: chrono::DateTime<chrono::Local> = timestamp.into();
    local.format("%Y-%m-%d %H:%M").to_string()
}

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
    measured_width: Option<(SharedString, f32, Pixels)>,
    horizontal_drag: bool,
    change_starts: Vec<usize>,
    change_index: usize,
}

#[cfg(test)]
pub(crate) fn file_history_version_label(stem: &str) -> Option<String> {
    let timestamp =
        SystemTime::UNIX_EPOCH.checked_add(std::time::Duration::from_nanos(stem.parse().ok()?))?;
    Some(file_history_timestamp_label(timestamp))
}

impl Editor {
    fn file_history_document_is_supported(&self) -> bool {
        let Some(path) = self.file_path.as_ref() else {
            return false;
        };
        if !config::file_history::supports_document(path, 0) {
            return false;
        }
        // UTF-8 每个标量最多四字节，先排除必然超限的正文，避免为大文件复制全文。
        if self.buffer.byte_len() > config::file_history::MAX_SNAPSHOT_BYTES * 4 {
            return false;
        }
        config::file_history::supports_document(path, self.buffer.file_bytes().len() as u64)
    }

    fn show_file_history_limits(&mut self, cx: &mut Context<Self>) {
        let strings = cx.global::<I18nManager>().strings();
        self.show_message_modal(
            strings.menu_file_history.clone(),
            strings.file_history_supported_files.clone(),
            cx,
        );
    }

    pub(crate) fn file_history_is_open(&self) -> bool {
        self.file_history_overlay.is_some()
    }

    pub(crate) fn open_file_history(&mut self, cx: &mut Context<Self>) {
        self.dismiss_contextual_overlays(cx);
        let Some(path) = self.file_path.clone() else {
            return;
        };
        if !self.file_history_document_is_supported() {
            self.close_file_history(cx);
            self.show_file_history_limits(cx);
            return;
        }
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
            measured_width: None,
            horizontal_drag: false,
            change_starts: Vec::new(),
            change_index: 0,
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
        match super::encoding::load_history_document(&version.path) {
            Ok(document) => {
                overlay.selected = selected;
                overlay.preview = super::encoding::normalize_document_text(&document.text);
                overlay.compared_revision = None;
                overlay.scroll = UniformListScrollHandle::new();
                overlay.horizontal_drag = false;
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
        if self.is_read_only() {
            return;
        }

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
                if choice != 0 || editor.is_read_only() {
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
                if !editor.file_history_document_is_supported() {
                    editor.show_file_history_limits(cx);
                    return;
                }
                // 上限可配置为一条，先读目标再备份，避免备份清理掉待恢复版本。
                let content = match super::encoding::load_history_document(&version) {
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let read_only = self.is_read_only();
        if self.file_history_overlay.as_ref().is_some_and(|overlay| overlay.compared_revision != Some(self.buffer.revision()))
            && !self.file_history_document_is_supported() {
            self.close_file_history(cx);
            return None;
        }
        let overlay = self.file_history_overlay.as_mut()?;
        if overlay.compared_revision != Some(self.buffer.revision()) {
            overlay.lines = Arc::new(if overlay.compare {
                diff::diff_lines(&self.buffer.text(), &overlay.preview)
            } else {
                diff::diff_lines(&overlay.preview, &overlay.preview)
            });
            overlay.compared_revision = Some(self.buffer.revision());
            overlay.measured_width = None;
            overlay.change_starts.clear();
            let mut in_change = false;
            for (index, line) in overlay.lines.iter().enumerate() {
                let changed = line.kind != diff::DiffKind::Context;
                if changed && !in_change {
                    overlay.change_starts.push(index);
                }
                in_change = changed;
            }
            overlay.change_index = 0;
            if let Some(index) = overlay.change_starts.first() {
                overlay.scroll.scroll_to_item_strict(*index, ScrollStrategy::Center);
            }
        }
        let selected = overlay.selected;
        let compare = overlay.compare;
        let empty = overlay.versions.is_empty();
        let can_navigate = overlay.change_starts.len() > 1;
        let change_label = format!("{}/{}", overlay.change_index + 1, overlay.change_starts.len());
        let lines = overlay.lines.clone();
        let colors = theme.colors.clone();
        let rows = overlay
            .versions
            .iter()
            .enumerate()
            .map(|(index, version)| {
                let date = file_history_timestamp_label(version.timestamp);
                let source = match version.source {
                    HistorySource::Automatic => &strings.file_history_automatic,
                    HistorySource::Manual => &strings.file_history_manual,
                    HistorySource::Exit => &strings.file_history_exit,
                    HistorySource::BeforeRestore => &strings.file_history_before_restore,
                    HistorySource::Baseline => &strings.file_history_baseline,
                    HistorySource::Legacy => &strings.file_history_legacy,
                };
                let detail = format!("{date}\n{source}");
                div()
                    .id(ElementId::Name(
                        format!("file-history-entry-{index}").into(),
                    ))
                    .debug_selector(move || format!("file-history-entry-{index}"))
                    .flex()
                    .items_center()
                    .h(px(38.0))
                    .flex_shrink_0()
                    .px_3()
                    .rounded(px(6.0))
                    .cursor_pointer()
                    .bg(if index == selected {
                        colors.selection.opacity(0.55)
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
                    .tooltip(move |_, cx| {
                        cx.new(|_| super::workspace::WorkspaceTooltip { label: detail.clone() }).into()
                    })
                    .child(div().text_size(px(theme.typography.ui_text_size(12.0))).whitespace_nowrap().child(date))
            })
            .collect::<Vec<_>>();
        let code_family: SharedString = EditorSettings::fonts(cx).code_family.into();
        let code_size = EditorSettings::scaled_font_sizes(cx).1;
        // uniform_list 只测量一行，先量全文才能让后续长行滚到末尾。
        let text_width = match &overlay.measured_width {
            Some((family, size, width)) if family == &code_family && *size == code_size => *width,
            _ => {
                let mut width = px(0.0);
                for line in lines.iter() {
                    let text: SharedString = line.text.trim_end_matches('\n').to_string().into();
                    let shaped = window.text_system().shape_line(
                        text.clone(),
                        px(code_size),
                        &[TextRun {
                            len: text.len(),
                            font: font(code_family.clone()),
                            color: colors.text_default,
                            background_color: None,
                            underline: None,
                            strikethrough: None,

                        }],
                        None,
                    );
                    width = width.max(shaped.width);
                }
                overlay.measured_width = Some((code_family.clone(), code_size, width));
                width
            }
        };
        let gutter_width = px(44.0);
        let content_width = text_width + gutter_width * if compare { 2.0 } else { 1.0 } + px(28.0);
        let content = uniform_list(
            "file-history-lines",
            lines.len().max(1),
            move |range, _, _| {
                range
                    .map(|index| {
                        let line = lines.get(index);
                        let (background, sign) = match line.map(|line| line.kind) {
                            Some(diff::DiffKind::Added) => (hsla(0.33, 0.6, 0.45, 0.10), "+"),
                            Some(diff::DiffKind::Removed) => (hsla(0.0, 0.7, 0.5, 0.10), "−"),
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
                            .w_full()
                            .min_w(content_width)
                            .h(px(code_size * 1.5))
                            .items_center()
                            .bg(background)
                            .font_family(code_family.clone())
                            .text_size(px(code_size))
                            .child(
                                div()
                                    .w(gutter_width)
                                    .pr_2()
                                    .text_right()
                                    .flex_shrink_0()
                                    .text_color(colors.dialog_muted)
                                    .child(old),
                            )
                            .when(compare, |this| {
                                this.child(
                                    div()
                                        .w(gutter_width)
                                        .pr_2()
                                        .text_right()
                                        .flex_shrink_0()
                                        .text_color(colors.dialog_muted)
                                        .child(new),
                                )
                            })
                            .child(div().w(px(20.0)).flex_shrink_0().child(sign))
                            .child(
                                div().flex_shrink_0().whitespace_nowrap().child(
                                    line.map(|line| line.text.trim_end_matches('\n').to_string())
                                        .unwrap_or_default(),
                                ),
                            )
                    })
                    .collect()
            },
        )
        .track_scroll(&overlay.scroll)
        .with_horizontal_sizing_behavior(ListHorizontalSizingBehavior::Unconstrained)
        .flex_1()
        .min_h(px(0.0))
        .min_w(px(0.0));
        let scroll = overlay.scroll.clone();
        let scrollbar = div()
            .id("file-history-horizontal-scrollbar")
            .debug_selector(|| "file-history-horizontal-scrollbar".into())
            .h(px(10.0))
            .w_full()
            .flex_shrink_0()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|editor, event: &MouseDownEvent, _, cx| {
                    if let Some(overlay) = editor.file_history_overlay.as_mut() {
                        overlay.horizontal_drag = true;
                    }
                    editor.scroll_file_history_horizontally(event.position.x, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                canvas(
                    move |_, _, _| (),
                    move |bounds, _, window, _| {
                        let state = scroll.0.borrow();
                        let Some(item) = state.last_item_size else {
                            return;
                        };
                        if item.contents.width <= item.item.width {
                            return;
                        }
                        let thumb_width =
                            (bounds.size.width * (item.item.width / item.contents.width)).max(px(24.0));
                        let fraction =
                            -state.base_handle.offset().x / (item.contents.width - item.item.width);
                        let top = bounds.top() + px(4.0);
                        window.paint_quad(fill(
                            Bounds::new(point(bounds.left(), top), size(bounds.size.width, px(3.0))),
                            colors.dialog_border.opacity(0.35),
                        ).corner_radii(px(1.5)));
                        window.paint_quad(fill(
                            Bounds::new(
                                point(
                                    bounds.left() + (bounds.size.width - thumb_width) * fraction,
                                    top,
                                ),
                                size(thumb_width, px(3.0)),
                            ),
                            colors.scrollbar_thumb,
                        ).corner_radii(px(1.5)));
                    },
                )
                .size_full(),
            );
        let colors = &theme.colors;
        let mode_switch = div().flex().gap(px(2.0)).p(px(3.0)).rounded(px(7.0))
            .bg(colors.dialog_secondary_button_bg.opacity(0.6))
            .children([
                (false, "file-history-preview", strings.file_history_preview.clone()),
                (true, "file-history-compare", strings.file_history_compare.clone()),
            ].into_iter().map(|(mode, id, label)| div().id(id).debug_selector(move || id.into())
                .h(px(26.0)).px_3().flex().items_center().rounded(px(5.0)).cursor_pointer()
                .text_size(px(theme.typography.ui_text_size(12.0)))
                .text_color(if compare == mode { colors.text_default } else { colors.dialog_muted })
                .when(compare == mode, |this| this.bg(colors.dialog_surface).shadow_sm())
                .child(label).on_mouse_down(MouseButton::Left, cx.listener(move |editor, _, _, cx| {
                    if let Some(overlay) = editor.file_history_overlay.as_mut() {
                        overlay.compare = mode;
                        overlay.compared_revision = None;
                        overlay.scroll = UniformListScrollHandle::new();
                        overlay.horizontal_drag = false;
                        cx.notify();
                    }
                }))));
        let navigation = div().flex().items_center().gap_1()
            .child(div().px_2().text_size(px(theme.typography.ui_text_size(12.0))).text_color(colors.dialog_muted).child(change_label))
            .children([
                (false, "file-history-previous-change", "file-history-previous-change-icon", strings.file_history_previous_change.clone()),
                (true, "file-history-next-change", "file-history-next-change-icon", strings.file_history_next_change.clone()),
            ].into_iter().map(|(forward, id, icon_id, label)| div().id(id).debug_selector(move || id.into())
                .size(px(26.0)).flex().items_center().justify_center().rounded(px(5.0)).cursor_pointer()
                .hover(|this| this.bg(colors.dialog_secondary_button_hover))
                .tooltip(move |_, cx| cx.new(|_| super::workspace::WorkspaceTooltip { label: label.clone() }).into())
                .child(svg().path("icon/workspace/chevron-down.svg").debug_selector(move || icon_id.into())
                    .with_transformation(Transformation::rotate(radians(if forward { 0.0 } else { std::f32::consts::PI })))
                    .size(px(14.0)).text_color(colors.dialog_muted))
                .on_mouse_down(MouseButton::Left, cx.listener(move |editor, _, _, cx| editor.navigate_file_history_change(forward, cx)))));
        let close_label = strings.file_history_close.clone();
        let close = div().id("file-history-close").debug_selector(|| "file-history-close".into())
            .size(px(28.0)).flex_shrink_0().flex().items_center().justify_center().rounded(px(6.0)).cursor_pointer()
            .hover(|this| this.bg(colors.dialog_secondary_button_hover))
            .tooltip(move |_, cx| cx.new(|_| super::workspace::WorkspaceTooltip { label: close_label.clone() }).into())
            .child(svg().path("icon/workspace/tab-close.svg").debug_selector(|| "file-history-close-icon".into())
                .size(px(14.0)).text_color(colors.dialog_muted))
            .on_mouse_down(MouseButton::Left, cx.listener(|editor, _, _, cx| editor.close_file_history(cx)));
        let header = div().flex().items_center().gap_4().px_5().h(px(56.0)).flex_shrink_0()
            .border_b_1().border_color(colors.dialog_border.opacity(0.55))
            .child(div().flex().items_center().gap_2().flex_1().min_w(px(0.0))
                .child(svg().path("icon/workspace/history.svg").size(px(16.0)).flex_shrink_0().text_color(colors.dialog_muted))
                .child(div().min_w(px(0.0)).truncate().text_size(px(theme.typography.ui_text_size(15.0))).font_weight(FontWeight::MEDIUM)
                    .child(file_history_title(&overlay.document, strings))))
            .when(!empty, |this| this.child(mode_switch))
            .when(compare && can_navigate, |this| this.child(navigation))
            .child(close);
        Some(div().id("file-history-overlay").debug_selector(|| "file-history-overlay".into())
            .absolute().inset_0().occlude().flex().items_center().justify_center().bg(colors.dialog_backdrop)
            .on_mouse_down(MouseButton::Left, cx.listener(|editor, _, _, cx| editor.close_file_history(cx)))
            .on_mouse_move(cx.listener(|editor, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left)
                    && editor.file_history_overlay.as_ref().is_some_and(|overlay| overlay.horizontal_drag) {
                    editor.scroll_file_history_horizontally(event.position.x, cx);
                }
            }))
            .on_mouse_up(MouseButton::Left, cx.listener(|editor, _, _, _| {
                if let Some(overlay) = editor.file_history_overlay.as_mut() { overlay.horizontal_drag = false; }
            }))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|editor, _, _, _| {
                if let Some(overlay) = editor.file_history_overlay.as_mut() { overlay.horizontal_drag = false; }
            }))
            .child(div().w(relative(0.90)).h(relative(0.80)).max_w(px(1120.0)).max_h(px(760.0))
                .min_w(px(0.0)).flex().flex_col().overflow_hidden()
                .rounded(px(theme.dimensions.dialog_radius)).bg(colors.dialog_surface)
                .border_1().border_color(colors.dialog_border).shadow_lg()
                .text_color(colors.text_default).text_size(px(theme.typography.ui_text_size(13.0)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(header)
                .when(empty, |this| this.child(div().flex_1().p_5().text_color(colors.dialog_muted).child(strings.file_history_empty.clone())))
                .when(!empty, |this| this.child(div().flex().flex_1().min_h(px(0.0))
                    .child(div().id("file-history-versions").w(px(184.0)).flex_shrink_0()
                        .flex().flex_col().gap_1().p_2().overflow_y_scroll()
                        .border_r_1().border_color(colors.dialog_border.opacity(0.55))
                        .bg(colors.dialog_secondary_button_bg.opacity(0.15)).children(rows))
                    .child(div().flex().flex_col().flex_1().min_w(px(0.0)).min_h(px(0.0)).px_3().pt_3().pb_1()
                        .child(content).child(scrollbar))))
                .when(!empty && !read_only, |this| this.child(div().flex().justify_end().items_center().h(px(50.0)).px_5().flex_shrink_0()
                    .border_t_1().border_color(colors.dialog_border.opacity(0.55))
                    .child(div().id("file-history-restore").debug_selector(|| "file-history-restore".into())
                        .h(px(30.0)).px_4().flex().items_center().rounded(px(6.0)).cursor_pointer()
                        .text_size(px(theme.typography.ui_text_size(12.0))).font_weight(FontWeight::MEDIUM)
                        .bg(colors.dialog_primary_button_bg).text_color(colors.dialog_primary_button_text)
                        .hover(|this| this.bg(colors.dialog_primary_button_hover))
                        .child(strings.file_history_restore.clone())
                        .on_mouse_down(MouseButton::Left, cx.listener(move |editor, _, _, cx| editor.restore_file_history_version(selected, cx)))))))
            .into_any_element())
    }

    fn navigate_file_history_change(&mut self, forward: bool, cx: &mut Context<Self>) {
        let Some(overlay) = self.file_history_overlay.as_mut() else {
            return;
        };
        let count = overlay.change_starts.len();
        if count == 0 {
            return;
        }
        overlay.change_index = if forward {
            (overlay.change_index + 1) % count
        } else {
            (overlay.change_index + count - 1) % count
        };
        if let Some(index) = overlay.change_starts.get(overlay.change_index) {
            overlay
                .scroll
                .scroll_to_item_strict(*index, ScrollStrategy::Center);
            cx.notify();
        }
    }

    fn scroll_file_history_horizontally(&mut self, position: Pixels, cx: &mut Context<Self>) {
        let Some(overlay) = self.file_history_overlay.as_ref() else {
            return;
        };
        let state = overlay.scroll.0.borrow();
        let Some(item) = state.last_item_size else {
            return;
        };
        let bounds = state.base_handle.bounds();
        let overflow = item.contents.width - item.item.width;
        if overflow <= px(0.0) || bounds.size.width <= px(24.0) {
            return;
        }
        let thumb_width =
            (bounds.size.width * (item.item.width / item.contents.width)).max(px(24.0));
        let fraction = ((position - bounds.left() - thumb_width / 2.0)
            / (bounds.size.width - thumb_width))
            .clamp(0.0, 1.0);
        let offset = state.base_handle.offset();
        state
            .base_handle
            .set_offset(point(-overflow * fraction, offset.y));
        cx.notify();
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
