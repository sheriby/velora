//! 窗口内容与 GPUI Kit 的 Root、主题和资源之间的应用边界。

use crate::{config::EditorSettings, theme::ThemeManager};
use anyhow::{Context as _, Result};
use gpui::{
    AnyWeakEntity, AnyWindowHandle, App, AppContext, BorrowAppContext as _, Context, Entity,
    Global, Render, Subscription, Window, WindowId, WindowOptions, px,
};
use gpui_kit::component::{Theme as ComponentTheme, ThemeMode};
use std::{collections::HashMap, marker::PhantomData};

#[derive(Default)]
struct WindowContents {
    views: HashMap<WindowId, AnyWeakEntity>,
    _subscriptions: Vec<Subscription>,
}
impl Global for WindowContents {}

pub(crate) fn init(cx: &mut App) {
    if cx.try_global::<WindowContents>().is_some() {
        return;
    }
    gpui_kit::init(cx);
    cx.set_global(WindowContents::default());
    let subscriptions = vec![
        cx.on_window_closed(|cx, window| {
            cx.update_global::<WindowContents, _>(|contents, _| {
                contents.views.remove(&window);
            });
        }),
        cx.observe_global::<ThemeManager>(sync_theme),
        cx.observe_global::<EditorSettings>(sync_theme),
    ];
    cx.update_global::<WindowContents, _>(|contents, _| contents._subscriptions = subscriptions);
    sync_theme(cx);
}

fn sync_theme(cx: &mut App) {
    let Some(source) = cx
        .try_global::<ThemeManager>()
        .map(ThemeManager::current_arc)
    else {
        return;
    };
    let colors = &source.colors;
    let fonts = EditorSettings::fonts(cx);
    let font_size = fonts.ui_size as f32;
    let mode = if colors.editor_background.l < 0.5 {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    if ComponentTheme::global(cx).mode != mode {
        ComponentTheme::change(mode, None, cx);
    }
    ComponentTheme::update(cx, |theme| {
        // Kit 常规控件用 text_sm（rem 的 7/8），现有 14px 界面字号对应 16px 基准。
        theme.font_size = px(16.0 * font_size / 14.0);
        theme.font_family = fonts.ui_family.into();
        theme.mono_font_family = fonts.code_family.into();
        theme.mono_font_size = px(fonts.code_size as f32);
        theme.colors.background = colors.editor_background;
        theme.colors.foreground = colors.dialog_body;
        theme.colors.border = colors.dialog_border;
        theme.colors.primary = colors.dialog_primary_button_bg;
        theme.colors.primary_hover = colors.dialog_primary_button_hover;
        theme.colors.primary_active = colors.dialog_primary_button_hover;
        theme.colors.primary_foreground = colors.dialog_primary_button_text;
        theme.colors.secondary = colors.dialog_secondary_button_bg;
        theme.colors.secondary_hover = colors.dialog_secondary_button_hover;
        theme.colors.secondary_active = colors.dialog_secondary_button_hover;
        theme.colors.secondary_foreground = colors.dialog_secondary_button_text;
        theme.colors.muted = colors.dialog_secondary_button_bg;
        theme.colors.muted_foreground = colors.dialog_muted;
        theme.colors.accent = colors.selection;
        theme.colors.accent_foreground = colors.dialog_body;
        theme.colors.selection = colors.selection;
        theme.colors.ring = colors.dialog_primary_button_bg;
        theme.colors.input = colors.dialog_border;
        theme.colors.popover = colors.dialog_surface;
        theme.colors.popover_foreground = colors.dialog_body;
        theme.colors.sidebar = colors.dialog_secondary_button_bg;
        theme.colors.sidebar_foreground = colors.dialog_body;
        theme.colors.sidebar_border = colors.dialog_border;
        theme.colors.sidebar_accent = colors.selection;
        theme.colors.sidebar_accent_foreground = colors.dialog_body;
        theme.colors.sidebar_primary = colors.dialog_primary_button_bg;
        theme.colors.sidebar_primary_foreground = colors.dialog_primary_button_text;
        theme.colors.danger = colors.dialog_danger_button_bg;
        theme.colors.danger_hover = colors.dialog_danger_button_hover;
        theme.colors.danger_foreground = colors.dialog_danger_button_text;
        theme.colors.group_box = colors.dialog_surface;
        theme.colors.group_box_foreground = colors.dialog_body;
        theme.colors.link = colors.text_link;
        theme.colors.link_hover = colors.text_link;
        theme.colors.link_active = colors.text_link;
        theme.colors.list_active = colors.selection;
        theme.colors.list_active_border = colors.dialog_primary_button_bg;
        theme.colors.tab_active = colors.selection;
        theme.colors.tab_active_foreground = colors.dialog_body;
        theme.colors.title_bar = colors.editor_background;
        theme.colors.title_bar_border = colors.dialog_border;
    });
}

// Root 成为真正的窗口根后，应用命令仍需定位其内容实体；弱索引避免窗口事件中重读窗口。
pub(crate) struct ContentWindowHandle<V> {
    window: AnyWindowHandle,
    content: PhantomData<fn() -> V>,
}
impl<V> Copy for ContentWindowHandle<V> {}
impl<V> Clone for ContentWindowHandle<V> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<V> From<ContentWindowHandle<V>> for AnyWindowHandle {
    fn from(handle: ContentWindowHandle<V>) -> Self {
        handle.window
    }
}

impl<V: Render + 'static> ContentWindowHandle<V> {
    pub(crate) fn read<'a>(&self, cx: &'a App) -> Result<&'a V> {
        Ok(self.entity(cx)?.read(cx))
    }
    pub(crate) fn entity<C: AppContext>(&self, cx: &C) -> Result<Entity<V>> {
        if let Some(legacy) = self.window.downcast::<V>() {
            return legacy.entity(cx);
        }
        cx.read_global::<WindowContents, _>(|contents, _| {
            contents
                .views
                .get(&self.window.window_id())
                .and_then(AnyWeakEntity::upgrade)
                .and_then(|entity| entity.downcast::<V>().ok())
                .context("窗口内容已释放")
        })
    }
    pub(crate) fn update<C: AppContext, R>(
        &self,
        cx: &mut C,
        update: impl FnOnce(&mut V, &mut Window, &mut Context<V>) -> R,
    ) -> Result<R> {
        // 派发期间窗口已离开全局表，必须先取得窗口上下文，再读取它的内容。
        self.window.update(cx, |view, window, cx| {
            let entity = view.downcast::<V>().or_else(|view| {
                view.downcast::<gpui_kit::base::Root>()
                    .map(|root| root.read(cx).view().clone())
                    .and_then(|view| view.downcast::<V>())
            }).map_err(|_| anyhow::anyhow!("窗口内容类型不匹配"))?;
            Ok(entity.update(cx, |content, cx| update(content, window, cx)))
        })?
    }
    pub(crate) fn read_with<C: AppContext, R>(
        &self,
        cx: &C,
        read: impl FnOnce(&V, &App) -> R,
    ) -> Result<R> {
        let entity = self.entity(cx)?;
        Ok(cx.read_entity(&entity, read))
    }
    #[cfg(test)]
    pub(crate) fn window_id(&self) -> WindowId {
        self.window.window_id()
    }
}

pub(crate) fn window_content<V: Render + 'static>(
    window: impl std::borrow::Borrow<AnyWindowHandle>,
    cx: &App,
) -> Option<ContentWindowHandle<V>> {
    let window = *window.borrow();
    if window.downcast::<V>().is_some() {
        return Some(ContentWindowHandle {
            window,
            content: PhantomData,
        });
    }
    cx.try_global::<WindowContents>()?
        .views
        .get(&window.window_id())?
        .upgrade()?
        .downcast::<V>()
        .ok()?;
    Some(ContentWindowHandle {
        window,
        content: PhantomData,
    })
}

pub(crate) fn open_window<V: Render + 'static>(
    options: WindowOptions,
    cx: &mut App,
    build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
) -> Result<ContentWindowHandle<V>> {
    init(cx);
    let (window, view) = gpui_kit::open_window(options, cx, build)?;
    cx.update_global::<WindowContents, _>(|contents, _| {
        contents
            .views
            .insert(window.window_id(), view.downgrade().into());
    });
    Ok(ContentWindowHandle {
        window,
        content: PhantomData,
    })
}
