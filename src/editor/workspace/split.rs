use super::*;
use std::cell::Cell;
use std::rc::Rc;

mod render;
mod session;
mod shared;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SplitDirection {
    Left,
    Right,
    Up,
    Down,
}

const MIN_PANE_WIDTH: f32 = 320.0;
const MIN_PANE_HEIGHT: f32 = 180.0;
const DIVIDER_SIZE: f32 = 4.0;

impl SplitDirection {
    fn fits(self, size: Size<Pixels>) -> bool {
        let (available, minimum) = if self.vertical() {
            (f32::from(size.height), MIN_PANE_HEIGHT)
        } else {
            (f32::from(size.width), MIN_PANE_WIDTH)
        };
        available <= 0.0 || available >= minimum * 2.0 + DIVIDER_SIZE
    }

    pub(crate) fn label(self, strings: &crate::i18n::I18nStrings) -> String {
        match self {
            Self::Left => strings.split_editor_left.clone(),
            Self::Right => strings.split_editor_right.clone(),
            Self::Up => strings.split_editor_up.clone(),
            Self::Down => strings.split_editor_down.clone(),
        }
    }
    fn vertical(self) -> bool {
        matches!(self, Self::Up | Self::Down)
    }
    fn before(self) -> bool {
        matches!(self, Self::Left | Self::Up)
    }
}

#[derive(Clone)]
enum GroupNode {
    Editor(Entity<Editor>),
    Split {
        id: uuid::Uuid,
        vertical: bool,
        ratio: f32,
        bounds: Rc<Cell<Bounds<Pixels>>>,
        first: Box<GroupNode>,
        second: Box<GroupNode>,
    },
}

impl GroupNode {
    fn size_for(&self, target: EntityId, available: Size<Pixels>) -> Option<Size<Pixels>> {
        match self {
            Self::Editor(editor) => (editor.entity_id() == target).then_some(available),
            Self::Split {
                vertical,
                ratio,
                first,
                second,
                bounds,
                ..
            } => {
                let measured = bounds.get().size;
                let available = if measured.width > px(0.0) && measured.height > px(0.0) {
                    measured
                } else {
                    available
                };
                let mut first_size = available;
                let mut second_size = available;
                if *vertical {
                    let height = (available.height - px(DIVIDER_SIZE)).max(px(0.0));
                    first_size.height = height * *ratio;
                    second_size.height = height * (1.0 - *ratio);
                } else {
                    let width = (available.width - px(DIVIDER_SIZE)).max(px(0.0));
                    first_size.width = width * *ratio;
                    second_size.width = width * (1.0 - *ratio);
                }
                first
                    .size_for(target, first_size)
                    .or_else(|| second.size_for(target, second_size))
            }
        }
    }

    fn minimum_size(&self) -> (f32, f32) {
        match self {
            Self::Editor(_) => (MIN_PANE_WIDTH, MIN_PANE_HEIGHT),
            Self::Split {
                vertical,
                first,
                second,
                ..
            } => {
                let (first_width, first_height) = first.minimum_size();
                let (second_width, second_height) = second.minimum_size();
                if *vertical {
                    (
                        first_width.max(second_width),
                        first_height + second_height + DIVIDER_SIZE,
                    )
                } else {
                    (
                        first_width + second_width + DIVIDER_SIZE,
                        first_height.max(second_height),
                    )
                }
            }
        }
    }

    fn without(&self, target: EntityId) -> Option<Self> {
        match self {
            Self::Editor(editor) => (editor.entity_id() != target).then(|| self.clone()),
            Self::Split {
                id,
                vertical,
                ratio,
                bounds,
                first,
                second,
            } => match (first.without(target), second.without(target)) {
                (Some(first), Some(second)) => Some(Self::Split {
                    id: *id,
                    vertical: *vertical,
                    ratio: *ratio,
                    bounds: bounds.clone(),
                    first: Box::new(first),
                    second: Box::new(second),
                }),
                (remaining, None) | (None, remaining) => remaining,
            },
        }
    }
    fn resize(&mut self, target: uuid::Uuid, value: f32) {
        if let Self::Split {
            id,
            ratio,
            first,
            second,
            bounds,
            vertical,
        } = self
        {
            if *id == target {
                let size = bounds.get().size;
                let total = if *vertical {
                    f32::from(size.height)
                } else {
                    f32::from(size.width)
                };
                let (first_width, first_height) = first.minimum_size();
                let (second_width, second_height) = second.minimum_size();
                let first_minimum = if *vertical { first_height } else { first_width };
                let second_minimum = if *vertical {
                    second_height
                } else {
                    second_width
                };
                let available = total - DIVIDER_SIZE;
                if available >= first_minimum + second_minimum {
                    *ratio =
                        value.clamp(first_minimum / available, 1.0 - second_minimum / available);
                }
            } else {
                first.resize(target, value);
                second.resize(target, value);
            }
        }
    }
    fn editors(&self, output: &mut Vec<Entity<Editor>>) {
        match self {
            Self::Editor(editor) => output.push(editor.clone()),
            Self::Split { first, second, .. } => {
                first.editors(output);
                second.editors(output);
            }
        }
    }

    fn split(
        &mut self,
        target: EntityId,
        editor: Entity<Editor>,
        direction: SplitDirection,
    ) -> bool {
        match self {
            Self::Editor(existing) if existing.entity_id() == target => {
                let previous = self.clone();
                let next = Self::Editor(editor);
                let (first, second) = if direction.before() {
                    (next, previous)
                } else {
                    (previous, next)
                };
                *self = Self::Split {
                    id: uuid::Uuid::new_v4(),
                    vertical: direction.vertical(),
                    ratio: 0.5,
                    bounds: Rc::new(Cell::new(Bounds::default())),
                    first: Box::new(first),
                    second: Box::new(second),
                };
                true
            }
            Self::Editor(_) => false,
            Self::Split { first, second, .. } => {
                first.split(target, editor.clone(), direction)
                    || second.split(target, editor, direction)
            }
        }
    }
}

struct SharedDocument {
    buffer: crate::editor::buffer::TextBuffer,
    owner: WeakEntity<Editor>,
    dirty: bool,
    file_version: Option<u64>,
    recovery_id: uuid::Uuid,
    synced: (u64, u64, Option<crate::editor::buffer::FileShape>),
}

impl SharedDocument {
    fn new(buffer: crate::editor::buffer::TextBuffer, owner: &Entity<Editor>, cx: &App) -> Self {
        let synced = (buffer.identity(), buffer.revision(), buffer.file_shape());
        let editor = owner.read(cx);
        Self {
            buffer,
            owner: owner.downgrade(),
            dirty: editor.document_dirty,
            file_version: editor.file_version,
            recovery_id: editor.recovery_id,
            synced,
        }
    }
}

pub(crate) struct SplitWorkspace {
    layout: GroupNode,
    active: EntityId,
    subscriptions: Vec<Subscription>,
    documents: HashMap<PathBuf, SharedDocument>,
    resizing: bool,
    revealed_tab: Option<(EntityId, PathBuf, usize, Pixels)>,
}

impl Editor {
    pub(crate) fn split_active_document(
        &mut self,
        direction: SplitDirection,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let source = self.active_split_editor().unwrap_or_else(|| cx.entity());
        let path = if source.entity_id() == cx.entity_id() {
            self.active_document_path().cloned()
        } else {
            source.read(cx).active_document_path().cloned()
        };
        if let Some(path) = path {
            self.request_split_document(source.downgrade(), path, direction, true, window, cx);
        }
    }

    pub(crate) fn active_split_editor(&self) -> Option<Entity<Editor>> {
        let workspace = self.split_workspace.as_ref()?;
        let groups = self.split_groups();
        groups
            .iter()
            .find(|group| group.entity_id() == workspace.active)
            .cloned()
            .or_else(|| groups.first().cloned())
    }

    pub(crate) fn split_groups(&self) -> Vec<Entity<Editor>> {
        let mut groups = Vec::new();
        if let Some(workspace) = &self.split_workspace {
            workspace.layout.editors(&mut groups);
        }
        groups
    }

    pub(crate) fn activate_split_group(&mut self, group: EntityId, cx: &mut Context<Self>) {
        if let Some(previous) = self.active_split_editor() {
            previous.update(cx, |editor, cx| {
                editor.refresh_selection_snapshot_if_changed(cx)
            });
        }
        self.transfer_shared_history(group, cx);
        let sidebar = self.active_split_editor().map(|editor| {
            let editor = editor.read(cx);
            (
                editor.workspace.is_open,
                editor.workspace.panel_width,
                editor.workspace.active_tab,
                editor.workspace.sidebar_memory_tab,
            )
        });
        for editor in self.split_groups() {
            editor.update(cx, |editor, cx| {
                if let Some((open, width, tab, memory)) = sidebar {
                    editor.workspace.is_open = open;
                    editor.workspace.panel_width = width;
                    editor.workspace.active_tab = tab;
                    editor.workspace.sidebar_memory_tab = memory;
                }
                let previously_active = editor.split_active;
                editor.split_active = cx.entity_id() == group;
                if previously_active != editor.split_active {
                    cx.notify();
                }
                if !editor.split_active {
                    editor.autosave_task = None;
                } else if editor.document_dirty {
                    editor.schedule_autosave(cx);
                }
            });
        }
        if let Some(workspace) = self.split_workspace.as_mut() {
            workspace.active = group;
        }
        cx.notify();
    }

    pub(crate) fn request_split_document(
        &mut self,
        source: WeakEntity<Editor>,
        path: PathBuf,
        direction: SplitDirection,
        copy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(parent) = self.split_parent.clone() {
            let target = cx.entity_id();
            let handle = window.window_handle();
            cx.defer(move |cx| {
                if let Err(error) = handle.update(cx, |_, window, cx| {
                    if let Err(error) = parent.update(cx, |parent, cx| {
                        parent.create_split(
                            Some(source),
                            target,
                            path,
                            direction,
                            copy,
                            window,
                            cx,
                        );
                    }) {
                        eprintln!("创建分屏失败：{error}");
                    }
                }) {
                    eprintln!("访问分屏窗口失败：{error}");
                }
            });
        } else {
            let target = self
                .active_split_editor()
                .map(|group| group.entity_id())
                .unwrap_or_else(|| cx.entity_id());
            self.create_split(Some(source), target, path, direction, copy, window, cx);
        }
    }

    pub(crate) fn transfer_split_tab(
        &mut self,
        source: WeakEntity<Editor>,
        target: EntityId,
        path: PathBuf,
        direction: Option<SplitDirection>,
        copy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(direction) = direction {
            self.create_split(Some(source), target, path, direction, copy, window, cx);
        } else {
            let groups = self.split_groups();
            let Some(source) = groups
                .iter()
                .find(|group| group.entity_id() == source.entity_id())
                .cloned()
            else {
                return;
            };
            let Some(target) = groups
                .iter()
                .find(|group| group.entity_id() == target)
                .cloned()
            else {
                return;
            };
            if source == target {
                return;
            }
            let tab = source.update(cx, |editor, cx| {
                editor.finalize_pending_undo_capture(cx);
                editor.snapshot_current_document(cx);
                editor
                    .workspace
                    .open_documents
                    .iter()
                    .find(|tab| tab.path == path)
                    .cloned()
            });
            let Some(mut tab) = tab else {
                return;
            };
            if let Some(document) = self
                .split_workspace
                .as_ref()
                .and_then(|workspace| workspace.documents.get(&path))
            {
                tab.markdown = document.buffer.text();
                tab.dirty = document.dirty;
                tab.file_shape = document.buffer.file_shape();
            }
            tab.preview = false;
            target.update(cx, |editor, cx| {
                editor.split_initializing = true;
                if !editor
                    .workspace
                    .open_documents
                    .iter()
                    .any(|existing| existing.path == path)
                {
                    editor.workspace.open_documents.push(tab);
                }
                editor.open_workspace_file_in_mode(
                    path.clone(),
                    WorkspaceOpenMode::Activate,
                    window,
                    cx,
                );
                editor.split_initializing = false;
            });
            self.sync_shared_groups(target.entity_id(), cx);
            self.activate_split_group(target.entity_id(), cx);
            if !copy {
                source.update(cx, |editor, cx| editor.detach_split_tab(&path, window, cx));
            }
        }
        self.remove_empty_split_groups(cx);
        self.persist_session(cx);
        cx.notify();
    }

    pub(in crate::editor) fn receive_editor_file_drop(
        &mut self,
        drag: &EditorFileDrag,
        position: Option<(PathBuf, bool)>,
        direction: Option<SplitDirection>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if direction.is_some_and(|direction| !direction.fits(self.scroll_handle.bounds().size)) {
            return;
        }
        if drag.source.is_none() && direction.is_none() {
            self.open_workspace_file(drag.from_path.clone(), window, cx);
            if let Some((target, after)) = position {
                self.move_tab_to_position(&drag.from_path, &target, after, window, cx);
            }
            if self.workspace.active_document.as_ref() == Some(&drag.from_path) {
                self.pending_focus = self
                    .active_entity_id
                    .or_else(|| self.first_focusable_entity_id(cx));
                if let Some(parent) = self.split_parent.clone() {
                    let target = cx.entity_id();
                    cx.defer(move |cx| {
                        if let Err(error) = parent.update(cx, |editor, cx| {
                            editor.activate_split_group(target, cx);
                            editor.persist_session(cx);
                        }) {
                            eprintln!("激活文件拖入的分组失败：{error}");
                        }
                    });
                }
            }
            return;
        }
        if drag
            .source
            .as_ref()
            .is_some_and(|source| source.entity_id() == cx.entity_id())
            && direction.is_none()
        {
            if let Some((target, after)) = position {
                self.move_tab_to_position(&drag.from_path, &target, after, window, cx);
            }
            return;
        }
        let copy = if cfg!(target_os = "macos") {
            window.modifiers().alt
        } else {
            window.modifiers().control
        };
        let source = drag.source.clone();
        let path = drag.from_path.clone();
        let target = cx.entity_id();
        if let Some(parent) = self.split_parent.clone() {
            let handle = window.window_handle();
            cx.defer(move |cx| {
                if let Err(error) = handle.update(cx, |_, window, cx| {
                    if let Err(error) = parent.update(cx, |editor, cx| {
                        if let Some(source) = source {
                            editor.transfer_split_tab(
                                source,
                                target,
                                path.clone(),
                                direction,
                                copy,
                                window,
                                cx,
                            );
                        } else if let Some(direction) = direction {
                            editor.create_split(
                                None,
                                target,
                                path.clone(),
                                direction,
                                true,
                                window,
                                cx,
                            );
                        }
                        if let Some((destination, after)) = position {
                            if let Some(group) = editor
                                .split_groups()
                                .into_iter()
                                .find(|group| group.entity_id() == target)
                            {
                                group.update(cx, |editor, cx| {
                                    editor.move_tab_to_position(
                                        &path,
                                        &destination,
                                        after,
                                        window,
                                        cx,
                                    )
                                });
                            }
                        }
                    }) {
                        eprintln!("移动分屏标签失败：{error}");
                    }
                }) {
                    eprintln!("访问分屏窗口失败：{error}");
                }
            });
        } else {
            if let Some(source) = source {
                self.transfer_split_tab(source, target, path, direction, copy, window, cx);
            } else if let Some(direction) = direction {
                self.create_split(None, target, path, direction, true, window, cx);
            }
        }
    }

    pub(super) fn remove_empty_split_groups(&mut self, cx: &mut Context<Self>) {
        let groups = self.split_groups();
        for group in &groups {
            if group.read(cx).workspace.open_documents.is_empty()
                && !group.read(cx).document_dirty
                && self.split_groups().len() > 1
            {
                if let Some(workspace) = self.split_workspace.as_mut() {
                    if let Some(layout) = workspace.layout.without(group.entity_id()) {
                        workspace.layout = layout;
                    }
                }
            }
        }
        let remaining = self.split_groups();
        if !remaining.iter().any(|group| {
            self.split_workspace
                .as_ref()
                .is_some_and(|workspace| workspace.active == group.entity_id())
        }) {
            if let Some(group) = remaining.first() {
                self.activate_split_group(group.entity_id(), cx);
                group.update(cx, |editor, cx| {
                    editor.pending_focus = editor
                        .active_entity_id
                        .or_else(|| editor.first_focusable_entity_id(cx));
                    cx.notify();
                });
            }
        }
    }

    pub(crate) fn shared_document_for_open(
        &self,
        path: &Path,
        cx: &App,
    ) -> Option<(
        crate::editor::buffer::TextBuffer,
        bool,
        Option<u64>,
        uuid::Uuid,
    )> {
        if self.split_initializing {
            return None;
        }
        self.split_parent
            .as_ref()?
            .read_with(cx, |editor, _| {
                let document = editor.split_workspace.as_ref()?.documents.get(path)?;
                Some((
                    document.buffer.shared_handle()?,
                    document.dirty,
                    document.file_version,
                    document.recovery_id,
                ))
            })
            .unwrap_or_else(|error| {
                eprintln!("读取共享文档失败：{error}");
                None
            })
    }

    pub(crate) fn refresh_shared_tab_snapshots(&mut self, cx: &App) {
        let paths: Vec<_> = self
            .workspace
            .open_documents
            .iter()
            .map(|tab| tab.path.clone())
            .collect();
        for path in paths {
            if let Some((buffer, dirty, version, recovery_id)) =
                self.shared_document_for_open(&path, cx)
            {
                if let Some(tab) = self
                    .workspace
                    .open_documents
                    .iter_mut()
                    .find(|tab| tab.path == path)
                {
                    tab.markdown = buffer.text();
                    tab.file_shape = buffer.file_shape();
                    tab.dirty = dirty;
                    tab.recovery_id = recovery_id;
                    if let Some(version) = version {
                        tab.file_version = version;
                    }
                }
            }
        }
    }

    pub(crate) fn preserve_shared_history_before_close(
        &mut self,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        let Some(parent) = self.split_parent.clone() else {
            return;
        };
        if self
            .active_document_path()
            .is_some_and(|path| paths.contains(path))
        {
            self.finalize_pending_undo_capture(cx);
        }
        let current = cx.entity_id();
        for path in paths {
            let history = if self.active_document_path() == Some(path) {
                Some((
                    self.undo_history.clone(),
                    self.redo_history.clone(),
                    self.pending_undo_capture.clone(),
                ))
            } else {
                self.workspace
                    .open_documents
                    .iter()
                    .find(|tab| tab.path == *path)
                    .and_then(|tab| tab.view.as_ref())
                    .and_then(|view| view.editing_state.as_ref())
                    .map(|state| {
                        (
                            state.undo_history.clone(),
                            state.redo_history.clone(),
                            state.pending_undo_capture.clone(),
                        )
                    })
            };
            let Some(history) = history else {
                continue;
            };
            let path = path.clone();
            let parent = parent.clone();
            cx.defer(move |cx| {
                if let Err(error) = parent.update(cx, |editor, cx| {
                    if editor
                        .split_workspace
                        .as_ref()
                        .and_then(|workspace| workspace.documents.get(&path))
                        .is_none_or(|document| document.owner.entity_id() != current)
                    {
                        return;
                    }
                    let target = editor.split_groups().into_iter().find(|group| {
                        group.entity_id() != current
                            && group
                                .read(cx)
                                .workspace
                                .open_documents
                                .iter()
                                .any(|tab| tab.path == path)
                    });
                    if let Some(target) = target {
                        target.update(cx, |editor, _| {
                            if editor.active_document_path() == Some(&path) {
                                editor.undo_history = history.0;
                                editor.redo_history = history.1;
                                editor.pending_undo_capture = history.2;
                            } else if let Some(state) = editor
                                .workspace
                                .open_documents
                                .iter_mut()
                                .find(|tab| tab.path == path)
                                .and_then(|tab| tab.view.as_mut())
                                .and_then(|view| view.editing_state.as_mut())
                            {
                                let state = std::sync::Arc::make_mut(state);
                                state.undo_history = history.0;
                                state.redo_history = history.1;
                                state.pending_undo_capture = history.2;
                            }
                        });
                        if let Some(document) = editor
                            .split_workspace
                            .as_mut()
                            .and_then(|workspace| workspace.documents.get_mut(&path))
                        {
                            document.owner = target.downgrade();
                        }
                    }
                }) {
                    eprintln!("保留共享撤销记录失败：{error}");
                }
            });
        }
    }

    pub(crate) fn collect_split_tabs(&mut self, cx: &mut Context<Self>) {
        if self.split_workspace.is_none() {
            return;
        }
        let groups = self.split_groups();
        for group in &groups {
            self.sync_shared_groups(group.entity_id(), cx);
        }
        let mut tabs = Vec::new();
        for group in groups {
            group.update(cx, |editor, cx| {
                editor.snapshot_current_document(cx);
                for tab in &editor.workspace.open_documents {
                    if !tabs
                        .iter()
                        .any(|existing: &WorkspaceDocumentTab| existing.path == tab.path)
                    {
                        tabs.push(tab.clone());
                    }
                }
            });
        }
        if let Some(workspace) = &self.split_workspace {
            for tab in &mut tabs {
                if let Some(document) = workspace.documents.get(&tab.path) {
                    tab.markdown = document.buffer.text();
                    tab.dirty = document.dirty;
                    if let Ok(view) = document.owner.read_with(cx, |editor, _| {
                        editor
                            .workspace
                            .open_documents
                            .iter()
                            .find(|source| source.path == tab.path)
                            .and_then(|source| source.view.clone())
                    }) {
                        tab.view = view;
                    }
                    tab.recovery_id = document.recovery_id;
                    tab.file_version = document.file_version.unwrap_or(tab.file_version);
                    tab.file_shape = document.buffer.file_shape();
                }
            }
        }
        self.workspace.open_documents = tabs;
    }

    pub(crate) fn shared_tab_has_other_view(&self, path: &Path, cx: &Context<Self>) -> bool {
        if self.split_initializing {
            return false;
        }
        let current = cx.entity_id();
        self.split_parent.as_ref().is_some_and(|parent| {
            parent
                .read_with(cx, |editor, cx| {
                    editor.split_groups().iter().any(|group| {
                        group.entity_id() != current
                            && group
                                .read(cx)
                                .workspace
                                .open_documents
                                .iter()
                                .any(|tab| tab.path == path)
                    })
                })
                .unwrap_or_else(|error| {
                    eprintln!("查询分屏标签失败：{error}");
                    false
                })
        })
    }

    fn new_editor_group(
        &self,
        tabs: Vec<WorkspaceDocumentTab>,
        active: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<Editor> {
        let preserve_cached = active.as_ref().is_some_and(|path| {
            self.active_document_path() == Some(path)
                || self
                    .split_workspace
                    .as_ref()
                    .is_some_and(|workspace| workspace.documents.contains_key(path))
        });
        let parent = cx.entity().downgrade();
        let root = self.workspace.root.clone();
        let sidebar_open = self.workspace.is_open;
        let reading_mode = self.reading_mode.clone();
        let ai_surface_visible = self.ai_surface_visible.clone();
        let group = cx.new(move |cx| {
            let mut editor = Editor::from_markdown(cx, String::new(), None);
            editor.reading_mode = reading_mode;
            editor.ai_surface_visible = ai_surface_visible;
            editor.rebuild_image_runtimes(cx);
            editor.split_parent = Some(parent);
            editor.split_active = false;
            editor.split_initializing = true;
            editor.workspace.root = root;
            editor.workspace.is_open = sidebar_open;
            editor.workspace.open_documents = tabs;
            editor
        });
        group.update(cx, |editor, cx| {
            if let Some(path) = active {
                let dirty = editor
                    .workspace
                    .open_documents
                    .iter()
                    .find(|tab| tab.path == path)
                    .map(|tab| tab.dirty);
                // 分屏复制的是已打开的内存文档；即使尚未改脏，也不能在这里另读一份磁盘内容。
                if preserve_cached {
                    if let Some(tab) = editor
                        .workspace
                        .open_documents
                        .iter_mut()
                        .find(|tab| tab.path == path)
                    {
                        tab.dirty = true;
                    }
                }
                editor.open_workspace_file_in_mode(
                    path.clone(),
                    WorkspaceOpenMode::Activate,
                    window,
                    cx,
                );
                editor.flush_pending_materialization(cx);
                if preserve_cached {
                    if let Some(dirty) = dirty {
                        editor.document_dirty = dirty;
                        if let Some(tab) = editor
                            .workspace
                            .open_documents
                            .iter_mut()
                            .find(|tab| tab.path == path)
                        {
                            tab.dirty = dirty;
                        }
                    }
                }
            }
            editor.split_projection_revision = editor.buffer.revision();
            editor.split_initializing = false;
        });
        group
    }

    fn observe_editor_group(&mut self, group: &Entity<Editor>, cx: &mut Context<Self>) {
        let subscription = cx.observe(group, |editor, changed, cx| {
            editor.sync_shared_groups(changed.entity_id(), cx);
            editor.schedule_autosave(cx);
            cx.notify();
        });
        if let Some(workspace) = self.split_workspace.as_mut() {
            workspace.subscriptions.push(subscription);
        }
    }

    fn ensure_split_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.split_workspace.is_some() {
            return;
        }
        self.split_bounds = self.scroll_handle.bounds();
        self.finalize_pending_undo_capture(cx);
        if self.file_path.is_none()
            && self.workspace.active_document.is_none()
            && !self.show_welcome
            && self.unsupported_preview_path.is_none()
        {
            self.register_untitled_document(cx);
        }
        self.snapshot_current_document(cx);
        let active = self.active_document_path().cloned();
        let shared_buffer = self.buffer.shared_view();
        let tabs = std::mem::take(&mut self.workspace.open_documents);
        let group = self.new_editor_group(tabs, active.clone(), window, cx);
        if active.is_none() {
            group.update(cx, |editor, _| editor.show_welcome = self.show_welcome);
        }
        group.update(cx, |editor, _| editor.split_active = true);
        let mut documents = HashMap::new();
        if let Some(path) = active {
            group.update(cx, |editor, _| {
                editor.buffer = shared_buffer;
                editor.split_projection_revision = editor.buffer.revision();
            });
            let buffer = group.update(cx, |editor, _| editor.buffer.shared_view());
            documents.insert(path, SharedDocument::new(buffer, &group, cx));
        }
        self.split_workspace = Some(SplitWorkspace {
            layout: GroupNode::Editor(group.clone()),
            active: group.entity_id(),
            subscriptions: Vec::new(),
            documents,
            resizing: false,
            revealed_tab: None,
        });
        self.autosave_task = None;
        self.file_path = None;
        self.workspace.active_document = None;
        self.document_dirty = false;
        self.recovery_id = uuid::Uuid::new_v4();
        self.replace_document_from_markdown(String::new(), None, cx);
        self.observe_editor_group(&group, cx);
    }

    fn create_split(
        &mut self,
        source: Option<WeakEntity<Editor>>,
        target: EntityId,
        path: PathBuf,
        direction: SplitDirection,
        copy: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let original = cx.entity_id();
        if source.as_ref().is_some_and(|source| {
            source.entity_id() != original
                && !self
                    .split_groups()
                    .iter()
                    .any(|group| group.entity_id() == source.entity_id())
        }) {
            return;
        }
        let target_size = if target == original {
            self.scroll_handle.bounds().size
        } else {
            self.split_workspace
                .as_ref()
                .and_then(|workspace| workspace.layout.size_for(target, self.split_bounds.size))
                .unwrap_or_default()
        };
        if !direction.fits(target_size) {
            let strings = cx.global::<I18nManager>().strings();
            self.show_message_modal(
                direction.label(strings),
                strings.split_insufficient_space.clone(),
                cx,
            );
            return;
        }
        self.ensure_split_workspace(window, cx);
        let groups = self.split_groups();
        let fallback = groups.first().cloned();
        let from_tree = source.is_none();
        let source = source.and_then(|source| {
            if source.entity_id() == original {
                fallback.clone()
            } else {
                source.upgrade()
            }
        });
        let target = if target == original {
            fallback.map(|group| group.entity_id())
        } else {
            Some(target)
        };
        let Some(target) = target else {
            return;
        };
        if !from_tree && source.is_none() {
            return;
        }
        if !groups.iter().any(|group| group.entity_id() == target) {
            return;
        }
        let tab = if let Some(source) = &source {
            source.update(cx, |editor, cx| {
                editor.finalize_pending_undo_capture(cx);
                editor.snapshot_current_document(cx);
                editor
                    .workspace
                    .open_documents
                    .iter()
                    .find(|tab| tab.path == path)
                    .cloned()
            })
        } else {
            self.collect_split_tabs(cx);
            self.workspace
                .open_documents
                .iter()
                .find(|tab| tab.path == path)
                .cloned()
        };
        let mut tabs = Vec::new();
        if let Some(mut tab) = tab {
            tab.preview = false;
            if from_tree {
                tab.view = None;
            }
            if let Some(document) = self
                .split_workspace
                .as_ref()
                .and_then(|workspace| workspace.documents.get(&path))
            {
                tab.markdown = document.buffer.text();
                tab.dirty = document.dirty;
                tab.file_shape = document.buffer.file_shape();
            }
            tabs.push(tab);
        } else if !from_tree {
            return;
        }
        let group = self.new_editor_group(tabs, Some(path.clone()), window, cx);
        if from_tree {
            if let Some(error) = group.read(cx).workspace.file_error.clone() {
                let title = cx
                    .global::<I18nManager>()
                    .strings()
                    .open_failed_title
                    .clone();
                self.show_message_modal(title, error, cx);
                return;
            }
            group.update(cx, |editor, _| {
                if let Some(tab) = editor
                    .workspace
                    .open_documents
                    .iter_mut()
                    .find(|tab| tab.path == path)
                {
                    tab.preview = false;
                }
            });
        }
        if let Some(workspace) = self.split_workspace.as_mut() {
            let document = workspace.documents.entry(path.clone()).or_insert_with(|| {
                let owner = source
                    .as_ref()
                    .filter(|source| source.read(cx).active_document_path() == Some(&path))
                    .cloned()
                    .unwrap_or_else(|| group.clone());
                let buffer = owner.update(cx, |editor, _| editor.buffer.shared_view());
                SharedDocument::new(buffer, &owner, cx)
            });
            group.update(cx, |editor, _| {
                editor.buffer = document.buffer.shared_view();
                editor.split_projection_revision = editor.buffer.revision();
            });
            workspace.layout.split(target, group.clone(), direction);
        }
        self.observe_editor_group(&group, cx);
        if !copy {
            if let Some(source) = source {
                source.update(cx, |editor, cx| editor.detach_split_tab(&path, window, cx));
            }
        }
        self.activate_split_group(group.entity_id(), cx);
        self.remove_empty_split_groups(cx);
        self.persist_session(cx);
        cx.notify();
    }

    fn detach_split_tab(&mut self, path: &Path, window: &mut Window, cx: &mut Context<Self>) {
        self.split_initializing = true;
        self.snapshot_current_document(cx);
        self.workspace.open_documents.retain(|tab| tab.path != path);
        if self.active_document_path().map(PathBuf::as_path) == Some(path) {
            self.file_path = None;
            self.document_dirty = false;
            self.workspace.active_document = None;
            let next = self
                .workspace
                .open_documents
                .first()
                .map(|tab| tab.path.clone());
            if let Some(next) = next {
                self.open_workspace_file(next, window, cx);
            } else {
                self.replace_document_from_markdown(String::new(), None, cx);
                self.show_welcome = true;
            }
        }
        self.split_initializing = false;
        cx.notify();
    }
}

#[cfg(test)]
mod tests;
