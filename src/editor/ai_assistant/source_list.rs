use super::{AiSession, Editor, PanelMode};
use crate::{ai::workspace::SourceRecord, i18n::I18nManager};
use gpui::*;
use gpui_kit::{
    base::ElementExt as _,
    component::{
        ActiveTheme as _, IndexPath,
        list::{List, ListDelegate, ListItem, ListState},
    },
};

const ROW_HEIGHT_REMS: f32 = 3.0;
const PREVIEW_CHARACTERS: usize = 160;

pub(super) struct SourceList {
    owner: WeakEntity<Editor>,
    session_id: String,
    records: Vec<SourceRecord>,
    selected: Option<IndexPath>,
}

impl SourceList {
    pub(super) fn new(owner: WeakEntity<Editor>) -> Self {
        Self {
            owner,
            session_id: String::new(),
            records: Vec::new(),
            selected: None,
        }
    }

    fn matches(&self, session: &AiSession) -> bool {
        self.session_id == session.saved.id
            && self.records == session.saved.workspace.source_records
    }

    fn selected_record(&self) -> Option<&SourceRecord> {
        self.records.get(self.selected?.row)
    }

    fn replace(&mut self, session: &AiSession) -> Option<IndexPath> {
        let previous = (self.session_id == session.saved.id)
            .then(|| self.selected_record().cloned())
            .flatten();
        self.session_id.clone_from(&session.saved.id);
        self.records
            .clone_from(&session.saved.workspace.source_records);
        previous
            .and_then(|record| self.records.iter().position(|entry| entry == &record))
            .or_else(|| (!self.records.is_empty()).then_some(0))
            .map(IndexPath::new)
    }
}

impl ListDelegate for SourceList {
    type Item = ListItem;

    fn items_count(&self, _: usize, _: &App) -> usize {
        self.records.len()
    }

    fn set_selected_index(
        &mut self,
        index: Option<IndexPath>,
        _: &mut Window,
        _: &mut Context<ListState<Self>>,
    ) {
        self.selected = index;
    }

    fn render_item(
        &mut self,
        index: IndexPath,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> Option<ListItem> {
        let record = self.records.get(index.row)?;
        let path = if record.path.starts_with("untitled:") {
            "Untitled"
        } else {
            record
                .path
                .strip_prefix("external:")
                .unwrap_or(&record.path)
        };
        let line = cx
            .global::<I18nManager>()
            .strings()
            .ai_source_line
            .replace("{line}", &record.line.to_string());
        let title = format!("{path} · {line}");
        let preview = record
            .excerpt
            .chars()
            .take(PREVIEW_CHARACTERS)
            .map(|character| {
                if character.is_whitespace() {
                    ' '
                } else {
                    character
                }
            })
            .collect::<String>();
        Some(
            ListItem::new(SharedString::from(format!(
                "ai-source:{}:{}:{}:{}",
                record.path, record.version, record.range.start, record.range.end
            )))
            .debug_selector(move || format!("ai-source-row-{}", index.row))
            .accessibility_label(title)
            .rounded(cx.theme().radius)
            .h(rems(ROW_HEIGHT_REMS))
            .px_2()
            .py_1()
            .child(
                div()
                    .w_full()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .flex()
                            .items_baseline()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_sm()
                                    .truncate()
                                    .text_ellipsis_middle()
                                    .child(path.to_owned()),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(line),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .truncate()
                            .child(preview),
                    ),
            ),
        )
    }

    fn render_empty(
        &mut self,
        _: &mut Window,
        cx: &mut Context<ListState<Self>>,
    ) -> impl IntoElement {
        div()
            .debug_selector(|| "ai-sources-empty".into())
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .text_sm()
            .text_color(cx.theme().muted_foreground)
            .child(
                cx.global::<I18nManager>()
                    .strings()
                    .ai_sources_empty
                    .clone(),
            )
    }

    fn confirm(&mut self, _: bool, window: &mut Window, cx: &mut Context<ListState<Self>>) {
        let Some(record) = self.selected_record().cloned() else {
            return;
        };
        let owner = self.owner.clone();
        let session_id = self.session_id.clone();
        let list_id = cx.entity().entity_id();
        // 定位会更新编辑器与焦点，必须先释放列表的实体更新租约。
        window.defer(cx, move |window, cx| {
            if let Err(error) = owner.update(cx, |editor, cx| {
                if !editor.modal_is_open()
                    && editor.info_dialog.is_none()
                    && gpui_kit::base::active_focus_trap(window, cx).is_none()
                    && editor.ai_session.as_ref().is_some_and(|session| {
                        session.visible
                            && session.mode == PanelMode::Sources
                            && session.saved.id == session_id
                            && session.source_list.entity_id() == list_id
                    })
                {
                    editor.open_ai_source(record, window, cx);
                }
            }) {
                eprintln!("定位 AI 来源失败：{error}");
            }
        });
    }
}

pub(super) fn render_sources(
    editor: &Editor,
    height: f32,
    window: &mut Window,
    cx: &mut Context<Editor>,
) -> AnyElement {
    let Some(session) = &editor.ai_session else {
        return div().into_any_element();
    };
    let list = session.source_list.clone();
    let changed = !list.read(cx).delegate().matches(session);
    let initialize_selection = session.source_focus_pending
        && !session.saved.workspace.source_records.is_empty()
        && list.read(cx).selected_index().is_none();
    if changed || initialize_selection {
        list.update(cx, |state, cx| {
            let previous = state.selected_index();
            let session_changed = state.delegate().session_id != session.saved.id;
            let previous_record = state.delegate().selected_record().cloned();
            let selected = state.delegate_mut().replace(session);
            state.set_selected_index(selected, window, cx);
            let selection_changed = previous_record.as_ref() != state.delegate().selected_record();
            if session_changed || selection_changed || previous != selected || previous.is_none() {
                if let Some(index) = selected {
                    state.scroll_to_item(index, ScrollStrategy::Top, window, cx);
                }
            }
            cx.notify();
        });
    }
    let available = (px(height) - window.rem_size() * 4.0).max(px(0.0));
    let list_height = (window.rem_size()
        * ROW_HEIGHT_REMS
        * session.saved.workspace.source_records.len().max(1) as f32)
        .min(available);
    let owner = cx.entity().downgrade();
    let list_id = list.entity_id();
    div()
        .id("ai-sources-list")
        .debug_selector(|| "ai-sources-list".into())
        .w_full()
        .min_h_0()
        .h(list_height)
        .child(List::new(&list).max_h(list_height))
        .on_prepaint(move |_, window, cx| {
            if let Err(error) = owner.update(cx, |editor, cx| {
                if let Some(session) = editor.ai_session.as_mut().filter(|session| {
                    session.visible
                        && session.mode == PanelMode::Sources
                        && session.source_focus_pending
                        && session.source_list.entity_id() == list_id
                }) {
                    session.source_focus_pending = false;
                    session
                        .source_list
                        .update(cx, |state, cx| state.focus(window, cx));
                    cx.notify();
                }
            }) {
                eprintln!("聚焦 AI 来源列表失败：{error}");
            }
        })
        .into_any_element()
}
