//! 已发布版本的通知、下载和保存后安装。

use gpui::*;
use gpui::prelude::FluentBuilder as _;
use gpui_kit::component::{
    ActiveTheme as _, Sizable as _,
    button::{Button, ButtonVariants as _},
    progress::Progress,
    scroll::ScrollableElement as _,
};
use std::path::{Path, PathBuf};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

use super::{Editor, InfoDialogKind};
use crate::config::EditorSettings;
use crate::config::preferences::update_app_preferences;
use crate::i18n::{I18nManager, I18nStrings};
use crate::net::update::{
    self as update_check, InstallPlan, UpdateCheckResult, UpdatePackage, UpdateVersionInfo,
};
use crate::theme::Theme;

// 测试仅替换外部副作用，窗口事件、校验、保存守卫和安装交接沿用同一条流程。
pub(super) trait UpdateBackend: Send + Sync {
    fn check(
        &self,
        current: &str,
        prereleases: bool,
    ) -> Result<UpdateCheckResult, update_check::UpdateCheckError>;
    fn ensure_single_instance(&self) -> anyhow::Result<()>;
    fn download(
        &self,
        package: &UpdatePackage,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(u64),
    ) -> anyhow::Result<PathBuf>;
    fn prepare_install(
        &self,
        package: &UpdatePackage,
        path: &Path,
        arguments: Vec<String>,
    ) -> anyhow::Result<InstallPlan>;
    fn launch_install(&self, plan: &InstallPlan) -> anyhow::Result<()>;
}

struct SystemUpdateBackend;

impl UpdateBackend for SystemUpdateBackend {
    fn check(
        &self,
        current: &str,
        prereleases: bool,
    ) -> Result<UpdateCheckResult, update_check::UpdateCheckError> {
        if prereleases {
            update_check::check_latest_version_with_options(current, true)
        } else {
            update_check::check_latest_version(current)
        }
    }
    fn ensure_single_instance(&self) -> anyhow::Result<()> {
        update_check::ensure_no_other_instances()
    }
    fn download(
        &self,
        package: &UpdatePackage,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(u64),
    ) -> anyhow::Result<PathBuf> {
        update_check::download_package(package, cancelled, progress)
    }
    fn prepare_install(
        &self,
        package: &UpdatePackage,
        path: &Path,
        arguments: Vec<String>,
    ) -> anyhow::Result<InstallPlan> {
        update_check::prepare_install(package, path, arguments)
    }
    fn launch_install(&self, plan: &InstallPlan) -> anyhow::Result<()> {
        update_check::launch_install_helper(plan)
    }
}

struct UpdateSession {
    backend: Arc<dyn UpdateBackend>,
    startup_checked: bool,
    download_active: bool,
    download_cancel: Option<Arc<AtomicBool>>,
    download_window: Option<WindowId>,
    pending_install: Option<InstallPlan>,
    fallback_notification: Option<UpdateVersionInfo>,
    closed_subscription: Option<Subscription>,
}
impl Global for UpdateSession {}

impl Default for UpdateSession {
    fn default() -> Self {
        Self {
            backend: Arc::new(SystemUpdateBackend),
            startup_checked: false,
            download_active: false,
            download_cancel: None,
            download_window: None,
            pending_install: None,
            fallback_notification: None,
            closed_subscription: None,
        }
    }
}

#[cfg(test)]
pub(super) fn set_test_update_backend(cx: &mut App, backend: Arc<dyn UpdateBackend>) {
    init_update_session(cx);
    cx.update_global::<UpdateSession, _>(|session, _| session.backend = backend);
}

fn init_update_session(cx: &mut App) {
    if cx.try_global::<UpdateSession>().is_some() {
        return;
    }
    cx.set_global(UpdateSession::default());
    let subscription = cx.on_window_closed(|cx, _| {
        let closed_download = cx
            .global::<UpdateSession>()
            .download_window
            .is_some_and(|owner| {
                cx.windows()
                    .iter()
                    .all(|window| window.window_id() != owner)
            });
        if closed_download {
            cx.update_global::<UpdateSession, _>(|session, _| {
                if let Some(cancelled) = session.download_cancel.take() {
                    cancelled.store(true, Ordering::Relaxed);
                }
                session.download_active = false;
                session.download_window = None;
            });
        }
        // 最后一个窗口由应用统一退出入口接管，避免重复发起安装和退出。
        if !cx.windows().is_empty()
            && cx
                .try_global::<UpdateSession>()
                .is_some_and(|session| session.pending_install.is_some())
        {
            crate::app_menu::request_quit_application(cx);
        }
    });
    cx.update_global::<UpdateSession, _>(|session, _| {
        session.closed_subscription = Some(subscription)
    });
}

pub(crate) fn cancel_pending_install(cx: &mut App) {
    if cx
        .try_global::<UpdateSession>()
        .is_some_and(|session| session.pending_install.is_some())
    {
        cx.update_global::<UpdateSession, _>(|session, _| {
            session.pending_install = None;
            session.download_active = false;
            session.download_cancel = None;
            session.download_window = None;
        });
        cx.defer(|cx| {
            for handle in cx.windows() {
                let Some(handle) = crate::ui::window_content::<Editor>(handle, cx) else {
                    continue;
                };
                if let Err(error) = handle.update(cx, |editor, _, cx| {
                    editor.clear_update_download_state(cx);
                }) {
                    eprintln!("取消更新状态失败：{error}");
                }
            }
        });
    }
}

pub(crate) fn install_pending_update(cx: &mut App) -> bool {
    let plan = if cx.try_global::<UpdateSession>().is_some() {
        cx.update_global::<UpdateSession, _>(|session, _| session.pending_install.take())
    } else {
        None
    };
    let Some(plan) = plan else {
        return true;
    };
    match cx.global::<UpdateSession>().backend.launch_install(&plan) {
        Ok(()) => true,
        Err(error) => {
            show_failure_on_app(cx, &error.to_string());
            false
        }
    }
}

fn show_failure_on_app(cx: &mut App, detail: &str) {
    cx.update_global::<UpdateSession, _>(|session, _| {
        session.pending_install = None;
        session.download_active = false;
        if let Some(cancelled) = session.download_cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        session.download_window = None;
    });
    // 保存确认可能已关闭最后一个文档，安装失败仍须有窗口承载错误提示。
    if cx
        .windows()
        .iter()
        .all(|window| crate::ui::window_content::<Editor>(window, cx).is_none())
    {
        let (width, height) = EditorSettings::default_window_size(cx);
        let bounds = Bounds::centered(None, size(px(width as f32), px(height as f32)), cx);
        let options = crate::window_chrome::velora_window_options("Velora".into(), bounds);
        if let Err(error) = crate::ui::open_window(options, cx, |_, cx| {
            cx.new(|cx| Editor::from_markdown(cx, String::new(), None))
        }) {
            eprintln!("无法打开更新错误窗口：{error}");
        }
    }
    let active = cx.active_window();
    let mut windows = cx.windows();
    windows.sort_by_key(|window| Some(*window) != active);
    let mut shown = false;
    for handle in windows {
        let Some(handle) = crate::ui::window_content::<Editor>(handle, cx) else {
            continue;
        };
        match handle.update(cx, |editor, window, cx| {
            editor.clear_update_download_state(cx);
            if !shown {
                window.activate_window();
                editor.show_update_failure(detail, cx);
            }
        }) {
            Ok(()) => shown = true,
            Err(error) => eprintln!("显示更新错误失败：{error}"),
        }
    }
    if !shown {
        eprintln!("更新失败：{detail}");
    }
}

impl Editor {
    fn clear_update_download_state(&mut self, cx: &mut Context<Self>) {
        if let Some(cancelled) = self.update_download_cancel.take() {
            cancelled.store(true, Ordering::Relaxed);
        }
        self.update_download_progress = None;
        for group in self.split_groups() {
            group.update(cx, |editor, cx| editor.clear_update_download_state(cx));
        }
        cx.notify();
    }

    pub(crate) fn maybe_check_updates_on_startup(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        init_update_session(cx);
        if let Some(info) =
            cx.update_global::<UpdateSession, _>(|session, _| session.fallback_notification.take())
        {
            if EditorSettings::updates(cx).ignored_version != info.latest_version {
                self.update_notification = Some(info);
            }
        }
        if cx.global::<UpdateSession>().startup_checked {
            return;
        }
        cx.update_global::<UpdateSession, _>(|session, _| session.startup_checked = true);
        // 开机路径一律不打扰用户：上次安装留下的失败原因只进日志。用户自己点的路径另说。
        match update_check::take_install_error() {
            Ok(Some(detail)) => {
                eprintln!("上次更新安装失败：{detail}");
                return;
            }
            Ok(None) => {}
            Err(error) => {
                eprintln!("读取更新安装结果失败：{error}");
                return;
            }
        }
        if EditorSettings::updates(cx).check_on_startup {
            self.start_update_check(false, window, cx);
        }
    }

    pub(crate) fn request_check_updates(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.show_unsaved_changes_dialog {
            return;
        }
        init_update_session(cx);
        if self.update_download_progress.is_some()
            || (cx.global::<UpdateSession>().download_active
                || cx.global::<UpdateSession>().pending_install.is_some())
        {
            let message = cx
                .global::<I18nManager>()
                .strings()
                .update_preparing
                .clone();
            self.show_message_modal(message.clone(), message, cx);
            return;
        }
        if self.update_check_in_progress {
            self.show_info_dialog(InfoDialogKind::CheckForUpdates, cx);
            return;
        }
        self.start_update_check(true, window, cx);
    }

    fn start_update_check(&mut self, manual: bool, _window: &mut Window, cx: &mut Context<Self>) {
        self.update_check_in_progress = true;
        if manual {
            self.show_info_dialog(InfoDialogKind::CheckForUpdates, cx);
        }
        let preferences = EditorSettings::updates(cx);
        let backend = cx.global::<UpdateSession>().backend.clone();
        let weak_editor = cx.entity().downgrade();
        let worker = cx.background_executor().spawn(async move {
            backend.check(env!("CARGO_PKG_VERSION"), preferences.include_prereleases)
        });
        self.update_task = Some(cx.spawn(
            async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
                let result = worker.await;
                let available = match &result {
                    Ok(UpdateCheckResult::UpdateAvailable(info)) => Some(info.clone()),
                    _ => None,
                };
                if let Err(error) = weak_editor.update(cx, |editor, cx| {
                    editor.update_check_in_progress = false;
                    if manual && editor.info_dialog == Some(InfoDialogKind::CheckForUpdates) {
                        editor.hide_info_dialog(cx);
                    }
                    editor.apply_update_result(result, manual, cx);
                }) {
                    eprintln!("更新检查窗口已关闭：{error}");
                    if let Some(info) = available {
                        cx.update_global::<UpdateSession, _>(|session, _| {
                            session.fallback_notification = Some(info)
                        });
                    }
                }
            },
        ));
    }

    fn apply_update_result(
        &mut self,
        result: Result<UpdateCheckResult, update_check::UpdateCheckError>,
        manual: bool,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(UpdateCheckResult::UpdateAvailable(info)) => {
                if manual || EditorSettings::updates(cx).ignored_version != info.latest_version {
                    self.update_notification = Some(info);
                }
            }
            Ok(UpdateCheckResult::UpToDate(info)) if manual => {
                let strings = cx.global::<I18nManager>().strings().clone();
                let detail = format_update_message(
                    &strings.update_up_to_date_message_template,
                    &info.current_version,
                    &info.latest_version,
                );
                self.show_message_modal(strings.update_up_to_date_title, detail, cx);
            }
            Ok(UpdateCheckResult::UpToDate(_)) => {}
            // 自动检查失败静默，只记日志；手动检查是用户自己问的，要回话。
            Err(error) if manual => self.show_update_failure(&error.to_string(), cx),
            Err(error) => eprintln!("检查更新失败（自动检查，不打扰用户）：{error}"),
        }
        cx.notify();
    }

    fn show_update_failure(&mut self, detail: &str, cx: &mut Context<Self>) {
        let strings = cx.global::<I18nManager>().strings().clone();
        self.show_message_modal(
            strings.update_failed_title,
            strings
                .update_failed_message_template
                .replace("{error}", detail),
            cx,
        );
    }

    fn dismiss_update_notice(&mut self, cx: &mut Context<Self>) {
        let cancelled = self.update_download_cancel.take();
        if let Some(cancelled) = cancelled.as_ref() {
            cancelled.store(true, Ordering::Relaxed);
            cancel_pending_install(cx);
        }
        self.update_task = None;
        self.update_download_progress = None;
        self.update_notification = None;
        // 其它窗口也能检查更新并关闭通知，只有下载所属窗口能解除全局互斥。
        if cancelled.is_some() && cx.try_global::<UpdateSession>().is_some() {
            cx.update_global::<UpdateSession, _>(|session, _| {
                session.download_active = false;
                session.download_cancel = None;
                session.download_window = None;
            });
        }
        cx.notify();
    }

    fn skip_update_version(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.update_notification.as_ref() else {
            return;
        };
        let version = info.latest_version.clone();
        match update_app_preferences(|preferences| preferences.updates.ignored_version = version) {
            Ok(preferences) => {
                EditorSettings::set_updates_in_memory(cx, preferences.updates);
                self.dismiss_update_notice(cx);
            }
            Err(error) => self.show_update_failure(&error.to_string(), cx),
        }
    }

    fn download_update(&mut self, cx: &mut Context<Self>) {
        init_update_session(cx);
        if cx.global::<UpdateSession>().download_active
            || cx.global::<UpdateSession>().pending_install.is_some()
        {
            let message = cx
                .global::<I18nManager>()
                .strings()
                .update_preparing
                .clone();
            self.show_message_modal(message.clone(), message, cx);
            return;
        }
        let Some(info) = self.update_notification.clone() else {
            return;
        };
        let Some(package) = info.package.clone() else {
            self.show_update_failure("缺少当前系统的安装包", cx);
            return;
        };
        let backend = cx.global::<UpdateSession>().backend.clone();
        if let Err(error) = backend.ensure_single_instance() {
            self.show_update_failure(&error.to_string(), cx);
            return;
        }
        cx.update_global::<UpdateSession, _>(|session, _| session.download_active = true);
        let progress = Arc::new(AtomicU64::new(0));
        let cancelled = Arc::new(AtomicBool::new(false));
        self.update_download_progress = Some(progress.clone());
        self.update_download_cancel = Some(cancelled.clone());
        cx.update_global::<UpdateSession, _>(|session, _| {
            session.download_cancel = Some(cancelled.clone());
            session.download_window = self.window_handle.map(|window| window.window_id());
        });
        let worker = cx.background_executor().spawn({
            let progress = progress.clone();
            let cancelled = cancelled.clone();
            let package = package.clone();
            let backend = backend.clone();
            async move {
                backend.download(&package, &cancelled, &mut |bytes| {
                    progress.store(bytes, Ordering::Relaxed)
                })
            }
        });
        let weak_editor = cx.entity().downgrade();
        let timer = cx.background_executor().clone();
        self.update_task = Some(cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            use futures::FutureExt;
            let worker = worker.fuse();
            futures::pin_mut!(worker);
            let result = loop {
                let tick = timer.timer(std::time::Duration::from_millis(100)).fuse();
                futures::pin_mut!(tick);
                futures::select! {
                    result = worker => break result,
                    _ = tick => {
                        if let Err(error) = weak_editor.update(cx, |_, cx| cx.notify()) { eprintln!("更新下载窗口已关闭：{error}"); }
                    }
                }
            };
            if cancelled.load(Ordering::Relaxed) { return; }
            if let Err(error) = weak_editor.update(cx, |editor, cx| {
                cx.update_global::<UpdateSession,_>(|session,_| { session.download_active = false; session.download_cancel = None;
            session.download_window = None; });
                match result {
                    Ok(path) => {
                        cx.defer(move |cx| {
                            let mut arguments = Vec::new();
                            for window in cx.windows() {
                                let Some(window) = crate::ui::window_content::<Editor>(window, cx) else { continue; };
                                match window.update(cx, |editor, _, cx| {
                                    editor.persist_session(cx);
                                    editor.workspace.root.clone().or_else(|| editor.file_path.clone())
                                }) {
                                    Ok(Some(path)) => { let argument = path.to_string_lossy().into_owned(); if !arguments.contains(&argument) { arguments.push(argument); } }
                                    Ok(None) => {}
                                    Err(error) => eprintln!("记录更新前的文档失败：{error}"),
                                }
                            }
                            match backend.prepare_install(&package,&path,arguments) {
                                Ok(plan) => {
                                    cx.update_global::<UpdateSession,_>(|session,_| session.pending_install = Some(plan));
                                    crate::app_menu::request_quit_application(cx);
                                }
                                Err(error) => show_failure_on_app(cx,&error.to_string()),
                            }
                        });
                    }
                    Err(error) => {
                        editor.update_download_progress = None;
                        editor.update_download_cancel = None;
                        editor.show_update_failure(&error.to_string(),cx);
                    }
                }
                cx.notify();
            }) { eprintln!("更新下载结束的窗口已关闭：{error}"); }
        }));
        cx.notify();
    }

    pub(crate) fn render_update_notification(
        &mut self,
        theme: &Theme,
        strings: &I18nStrings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let info = self.update_notification.as_ref()?;
        let inset = window.rem_size();
        let available_width = (window.viewport_size().width - inset * 2.0).max(px(0.0));
        let scroll = window
            .use_keyed_state("update-notice-scroll", cx, |_, _| ScrollHandle::new())
            .read(cx)
            .clone();
        let percent = self.update_download_progress.as_ref().map(|progress| {
            let total = info
                .package
                .as_ref()
                .map_or(1, |package| package.size)
                .max(1);
            (u128::from(progress.load(Ordering::Relaxed).min(total)) * 100 / u128::from(total))
                as f32
        });
        let description = match percent {
            Some(percent) if percent >= 100.0 => strings.update_preparing.clone(),
            Some(percent) => strings
                .update_downloading
                .replace("{percent}", &format!("{percent:.0}")),
            None => format_update_message(
                &strings.update_available_message_template,
                &info.current_version,
                &info.latest_version,
            ),
        };
        let mut actions = div()
            .w_full()
            .flex()
            .flex_wrap()
            .gap_2()
            .items_center()
            .justify_end();
        if percent.is_none() {
            actions = actions.child(
                Button::new("update-skip")
                    .debug_selector(|| "update-skip".into())
                    .small()
                    .ghost()
                    .label(strings.update_skip_version.clone())
                    .on_click(cx.listener(|editor, _, _, cx| editor.skip_update_version(cx))),
            );
        }
        actions = actions.child(
            Button::new("update-cancel")
                .debug_selector(|| "update-cancel".into())
                .small()
                .outline()
                .label(if percent.is_some() {
                    strings.preferences_cancel.clone()
                } else {
                    strings.update_later.clone()
                })
                .on_click(cx.listener(|editor, _, _, cx| editor.dismiss_update_notice(cx))),
        );
        if percent.is_none() {
            actions = actions.child(
                Button::new("update-confirm")
                    .debug_selector(|| "update-confirm".into())
                    .small()
                    .primary()
                    .label(strings.update_install.clone())
                    .on_click(cx.listener(|editor, _, _, cx| editor.download_update(cx))),
            );
        }
        Some(
            div()
                .id(SharedString::from(format!(
                    "update-notification:{}",
                    info.latest_version
                )))
                .debug_selector(|| "update-notification".into())
                .absolute()
                .right_4()
                .bottom(px(theme.dimensions.status_bar_height) + cx.theme().font_size * 0.75)
                .w((inset * 22.0).min(available_width))
                .max_h(relative(0.75))
                .p_3()
                .flex()
                .flex_col()
                .gap_3()
                .bg(cx.theme().popover)
                .text_color(cx.theme().popover_foreground)
                .border_1()
                .border_color(cx.theme().border)
                .rounded(cx.theme().radius_lg)
                .shadow_lg()
                .occlude()
                .text_sm()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .flex_shrink_0()
                        .font_weight(FontWeight::BOLD)
                        .child(format!(
                            "{} {}",
                            strings.update_available_title, info.latest_version
                        )),
                )
                .when_some(percent, |card, percent| {
                    card.child(
                        Progress::new("update-progress")
                            .value(percent)
                            .loading(percent >= 100.0)
                            .small()
                            .color(cx.theme().primary)
                            .accessibility_label(description.clone()),
                    )
                })
                .child(
                    div()
                        .id("update-description")
                        .debug_selector(|| "update-description".into())
                        .min_h_0()
                        .relative()
                        .overflow_y_scroll()
                        .track_scroll(&scroll)
                        .flex()
                        .flex_col()
                        .gap_2()
                        .child(description)
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(strings.update_restart_notice.clone()),
                        )
                        .vertical_scrollbar(&scroll),
                )
                .child(actions.flex_shrink_0())
                .into_any_element(),
        )
    }
}

fn format_update_message(template: &str, current: &str, latest: &str) -> String {
    template
        .replace("{current}", current)
        .replace("{latest}", latest)
}

#[cfg(test)]
mod tests {
    use super::{
        Editor, UpdateCheckResult, UpdateSession, UpdateVersionInfo, format_update_message,
    };
    use crate::net::update::UpdateSource;
    use gpui::{AppContext as _, BorrowAppContext, TestAppContext};

    #[gpui::test]
    fn update_notice_exposes_progress_and_keyboard_cancellation(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt as _;
        let root =
            std::env::temp_dir().join(format!("velora-update-controls-{}", uuid::Uuid::new_v4()));
        let _config = crate::config::override_test_config_root(&root);
        std::fs::create_dir_all(&root).unwrap();
        cx.on_quit(move || std::fs::remove_dir_all(root).unwrap());
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let handle = cx
            .update(|cx| {
                crate::ui::open_window(gpui::WindowOptions::default(), cx, |_, cx| {
                    cx.new(|cx| Editor::from_markdown(cx, "正文".into(), None))
                })
            })
            .unwrap();
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        handle
            .update(&mut visual, |editor, window, cx| {
                editor.update_notification = Some(UpdateVersionInfo {
                    current_version: "0.2.4".into(),
                    latest_version: "0.2.5".into(),
                    source: crate::net::update::UpdateSource::GitHub,
                    release_url: crate::net::update::RELEASES_URL.into(),
                    package: Some(crate::net::update::UpdatePackage {
                        name: "fixture.pkg".into(),
                        url: "https://example.invalid/update".into(),
                        size: 100,
                        digest: None,
                        platform: crate::net::update::UpdatePlatform::current().unwrap(),
                    }),
                });
                editor.update_download_progress =
                    Some(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(50)));
                editor.update_download_cancel = Some(cancelled.clone());
                cx.update_global::<UpdateSession, _>(|session, _| {
                    session.download_active = true;
                    session.download_cancel = Some(cancelled.clone());
                    session.download_window = Some(window.window_handle().window_id());
                });
                cx.notify();
            })
            .unwrap();
        visual.update(|window, cx| {
            window.render_frame(cx);
            window.blur(cx);
            window.focus_next(cx);
        });
        visual.run_until_parked();
        assert!(
            visual.update(|window, _| window.try_find("update-progress").is_some()),
            "下载需要可访问的真实进度控件"
        );
        visual.update(|window, _| {
            let progress = window.find("update-progress");
            assert_eq!(progress.role(), Some(gpui::Role::ProgressIndicator));
            assert!(progress.label().is_some_and(|label| label.contains("50")));
            assert_eq!(
                window.find("update-cancel").role(),
                Some(gpui::Role::Button)
            );
        });
        let mut reached_cancel = false;
        for _ in 0..40 {
            visual.simulate_keystrokes("tab");
            if visual.update(|window, _| window.find("update-cancel").focused() == Some(true)) {
                reached_cancel = true;
                break;
            }
        }
        assert!(reached_cancel, "键盘应能到达取消下载");
        visual.update(|window, cx| window.render_frame(cx));
        let stroke = gpui::Keystroke::parse("space").unwrap();
        visual.simulate_event(gpui::KeyDownEvent {
            keystroke: stroke.clone(),
            is_held: false,
            prefer_character_input: false,
        });
        visual.simulate_event(gpui::KeyUpEvent { keystroke: stroke });
        visual.run_until_parked();
        assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
        handle
            .read_with(&visual, |editor, cx| {
                assert!(editor.update_notification.is_none());
                assert!(!cx.global::<UpdateSession>().download_active);
                assert_eq!(editor.buffer.text(), "正文");
            })
            .unwrap();
        visual.update(|window, _| window.remove_window());
    }

    #[gpui::test]
    fn update_notice_keeps_its_description_and_actions_inside_a_narrow_window(cx: &mut TestAppContext) {
        use gpui_kit::test::TestWindowExt as _;
        let root = std::env::temp_dir().join(format!("velora-update-layout-{}", uuid::Uuid::new_v4()));
        let _config = crate::config::override_test_config_root(&root);
        std::fs::create_dir_all(&root).unwrap();
        crate::config::preferences::update_app_preferences(|preferences| {
            preferences.fonts.ui_size = 36;
        })
        .unwrap();
        cx.on_quit(move || std::fs::remove_dir_all(root).unwrap());
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::config::EditorSettings::init(cx, true);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let handle = cx
            .update(|cx| {
                crate::ui::open_window(gpui::WindowOptions::default(), cx, |_, cx| {
                    cx.new(|cx| Editor::from_markdown(cx, "正文".into(), None))
                })
            })
            .unwrap();
        let mut visual = gpui::VisualTestContext::from_window(handle.into(), cx);
        visual.simulate_resize(gpui::size(gpui::px(600.0), gpui::px(620.0)));
        handle
            .update(&mut visual, |editor, _, cx| {
                editor.update_notification = Some(UpdateVersionInfo {
                    current_version: "0.2.4".into(),
                    latest_version: "0.2.5".into(),
                    source: UpdateSource::GitHub,
                    release_url: crate::net::update::RELEASES_URL.into(),
                    package: None,
                });
                cx.notify();
            })
            .unwrap();
        for _ in 0..3 {
            visual.update(|window, cx| window.render_frame(cx));
            visual.run_until_parked();
        }
        let card = visual.debug_bounds("update-notification").unwrap();
        let body = visual.debug_bounds("update-description").unwrap();
        let confirm = visual.debug_bounds("update-confirm").unwrap();
        assert!(
            card.left() >= gpui::px(0.0) && card.right() <= gpui::px(600.0),
            "通知不能超出窗口：{card:?}"
        );
        assert!(
            body.size.height >= gpui::px(36.0),
            "版本与重启说明必须保留可滚动阅读空间：{body:?}"
        );
        assert!(
            confirm.bottom() <= card.bottom() && confirm.right() <= card.right(),
            "操作不能被正文挤出通知：{confirm:?}，{card:?}"
        );
        visual.update(|window, _| window.remove_window());
    }

    fn missing_install_plan() -> crate::net::update::InstallPlan {
        let path = std::env::temp_dir().join(format!("missing-update-{}", uuid::Uuid::new_v4()));
        crate::net::update::InstallPlan {
            package_path: path.clone(),
            platform: crate::net::update::UpdatePlatform::current().expect("支持的系统"),
            process_id: std::process::id(),
            install_dir: path.clone(),
            launch_path: path.clone(),
            previous_executable: path.clone(),
            launch_arguments: String::new(),
            launch_args: Vec::new(),
            error_path: path,
        }
    }

    #[gpui::test]
    async fn last_window_close_attempts_pending_install_before_quit(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
            cx.update_global::<UpdateSession, _>(|session, _| {
                session.pending_install = Some(missing_install_plan());
            });
            crate::app_menu::handle_window_closed(cx);
        });
        cx.run_until_parked();
        cx.update(|cx| {
            assert!(
                cx.global::<UpdateSession>().pending_install.is_none(),
                "最后一个窗口关闭也必须处理待安装更新，不能直接退出"
            )
        });
        cx.update(|cx| {
            assert!(
                cx.windows().iter().any(|window| {
                    crate::ui::window_content::<Editor>(window, cx).is_some_and(|window| {
                        window
                            .read_with(cx, |editor, _| editor.modal_is_open())
                            .unwrap_or(false)
                    })
                }),
                "安装助手启动失败时，关闭全部窗口也必须看到错误"
            )
        });
    }

    #[gpui::test]
    async fn failed_install_releases_the_downloading_window_and_shows_its_error(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let (_other, _) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "其他窗口".into(), None));
        let (owner, visual) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "下载窗口".into(), None));
        visual.update(|window, cx| {
            window.activate_window();
            owner.update(cx, |editor, _| {
                editor.update_download_progress =
                    Some(std::sync::Arc::new(std::sync::atomic::AtomicU64::new(100)));
                editor.update_download_cancel = Some(std::sync::Arc::new(
                    std::sync::atomic::AtomicBool::new(false),
                ));
            });
            cx.update_global::<UpdateSession, _>(|session, _| {
                session.pending_install = Some(missing_install_plan());
            });
            cx.defer(|cx| assert!(!super::install_pending_update(cx), "缺失安装包应阻止退出"));
        });
        cx.run_until_parked();
        owner.read_with(cx, |editor, _| {
            assert!(
                editor.update_download_progress.is_none(),
                "安装失败后不能永久显示正在准备"
            );
            assert!(editor.update_download_cancel.is_none());
            assert!(editor.modal_is_open(), "错误应显示在当前窗口");
        });
        cx.update(|cx| {
            assert!(!cx.global::<UpdateSession>().download_active);
            assert!(cx.global::<UpdateSession>().pending_install.is_none());
        });
    }
    #[gpui::test]
    async fn dismissing_another_windows_notice_keeps_the_download_active(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let (owner, _) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        let (other, visual) = cx.add_window_view(|_, cx| Editor::from_markdown(cx, "另一个窗口".into(), None));
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        owner.update(visual, |editor, cx| {
            editor.update_download_cancel = Some(cancelled.clone());
            cx.update_global::<UpdateSession, _>(|session, _| {
                session.download_active = true;
                session.download_cancel = Some(cancelled.clone());
            });
        });
        other.update(visual, |editor, cx| editor.dismiss_update_notice(cx));
        visual.update(|_, cx| {
            assert!(cx.global::<UpdateSession>().download_active,
                "另一窗口取消通知不能解除进行中的下载保护");
        });
        assert!(!cancelled.load(std::sync::atomic::Ordering::Relaxed));
        owner.update(visual, |editor, cx| editor.dismiss_update_notice(cx));
        assert!(cancelled.load(std::sync::atomic::Ordering::Relaxed));
        visual.update(|_, cx| assert!(!cx.global::<UpdateSession>().download_active));
    }
    #[test]
    fn update_message_templates_replace_versions() {
        assert_eq!(
            format_update_message("{current} → {latest}", "0.2.4", "0.2.5"),
            "0.2.4 → 0.2.5"
        );
    }
    #[gpui::test]
    async fn startup_toggle_and_ignored_versions_do_not_block_manual_checks(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            let mut settings = crate::config::preferences::UpdatePreferences::default();
            settings.check_on_startup = false;
            settings.ignored_version = "0.2.5".into();
            crate::config::EditorSettings::set_updates_in_memory(cx, settings);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        editor.update_in(cx, |editor, window, cx| {
            editor.maybe_check_updates_on_startup(window, cx);
            assert!(!editor.update_check_in_progress);
            assert!(cx.global::<UpdateSession>().startup_checked);
            let info = UpdateVersionInfo {
                current_version: "0.2.4".into(),
                latest_version: "0.2.5".into(),
                source: UpdateSource::GitHub,
                release_url: crate::net::update::RELEASES_URL.into(),
                package: None,
            };
            editor.apply_update_result(
                Ok(UpdateCheckResult::UpdateAvailable(info.clone())),
                false,
                cx,
            );
            assert!(editor.update_notification.is_none());
            editor.apply_update_result(Ok(UpdateCheckResult::UpdateAvailable(info)), true, cx);
            assert!(editor.update_notification.is_some());
        });
        cx.update(|window, cx| window.draw(cx).clear(cx));
        assert!(cx.debug_bounds("update-notification").is_some());
        assert!(cx.debug_bounds("update-confirm").is_some());
        assert!(cx.debug_bounds("update-cancel").is_some());
        assert!(cx.debug_bounds("update-skip").is_some());
    }

    #[gpui::test]
    async fn installation_waits_for_unsaved_documents_and_cancel_aborts_it(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "未保存正文".into(), None));
        editor.update(cx, |editor, _| editor.document_dirty = true);
        cx.update(|_, cx| {
            let path =
                std::env::temp_dir().join(format!("missing-update-{}", uuid::Uuid::new_v4()));
            cx.update_global::<UpdateSession, _>(|session, _| {
                session.pending_install = Some(crate::net::update::InstallPlan {
                    package_path: path.clone(),
                    platform: crate::net::update::UpdatePlatform::MacOsArm64,
                    process_id: std::process::id(),
                    install_dir: path.clone(),
                    launch_path: path.clone(),
                    previous_executable: path.clone(),
                    launch_arguments: String::new(),
                    launch_args: Vec::new(),
                    error_path: path,
                })
            });
            crate::app_menu::request_quit_application(cx);
        });
        cx.run_until_parked();
        editor.read_with(cx, |editor, cx| {
            assert!(editor.show_unsaved_changes_dialog, "必须先处理未保存文档");
            assert!(
                cx.global::<UpdateSession>().pending_install.is_some(),
                "保存守卫期间不能开始安装"
            );
        });
        cx.update(|_, cx| super::cancel_pending_install(cx));
        cx.run_until_parked();
        cx.update(|_, cx| assert!(cx.global::<UpdateSession>().pending_install.is_none()));
        assert_eq!(
            editor.read_with(cx, |editor, _| editor.buffer.text()),
            "未保存正文"
        );
    }

    #[gpui::test]
    async fn closing_the_download_window_cancels_the_worker_and_releases_the_session(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
        });
        let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (editor, visual) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        visual.update(|window, cx| {
            let id = window.window_handle().window_id();
            editor.update(cx, |editor, cx| {
                editor.update_download_cancel = Some(cancelled.clone());
                cx.update_global::<UpdateSession, _>(|session, _| {
                    session.download_active = true;
                    session.download_cancel = Some(cancelled.clone());
                    session.download_window = Some(id);
                });
            });
            window.remove_window();
        });
        cx.run_until_parked();
        assert!(
            cancelled.load(std::sync::atomic::Ordering::Relaxed),
            "关闭窗口时应立即取消下载，不依赖实体释放时机"
        );
        cx.update(|cx| assert!(!cx.global::<UpdateSession>().download_active));
        drop(editor);
    }

    /// 自动检查更新失败必须静默：连不上 GitHub 的机器不该在每次启动时被「更新失败」
    /// 打扰。手动点「检查更新」失败仍然要说一声：那是用户自己问的，静默等于点了没反应。
    #[gpui::test]
    async fn automatic_update_check_failures_are_silent_while_manual_ones_speak(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
            let mut settings = crate::config::preferences::UpdatePreferences::default();
            settings.check_on_startup = false;
            crate::config::EditorSettings::set_updates_in_memory(cx, settings);
        });
        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        let failure = || crate::net::update::UpdateCheckError::Fetch("连不上 GitHub".into());
        editor.update(cx, |editor, cx| {
            editor.apply_update_result(Err(failure()), false, cx);
            assert!(
                !editor.modal_is_open(),
                "开机自动检查失败不该弹窗：连不上 GitHub 的机器每次启动都会被拦一下"
            );
            editor.apply_update_result(Err(failure()), true, cx);
            assert!(
                editor.modal_is_open(),
                "手动点「检查更新」失败要告诉用户，静默等于点了没反应"
            );
        });
    }

    /// 上次安装/自检留在磁盘上的失败原因，也不能在开机路径上变成弹窗。
    #[gpui::test]
    async fn a_leftover_install_failure_does_not_greet_the_user_on_startup(
        cx: &mut TestAppContext,
    ) {
        let root = std::env::temp_dir().join(format!("velora-update-silent-{}", uuid::Uuid::new_v4()));
        let _config_root = crate::config::override_test_config_root(root.clone());
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            crate::components::init(cx);
            super::init_update_session(cx);
            let mut settings = crate::config::preferences::UpdatePreferences::default();
            settings.check_on_startup = false;
            crate::config::EditorSettings::set_updates_in_memory(cx, settings);
        });
        let updates = crate::net::update::updates_dir().expect("更新目录");
        std::fs::create_dir_all(&updates).expect("建更新目录");
        std::fs::write(updates.join("install-error.txt"), "安装包校验失败").expect("写安装失败记录");

        let (editor, cx) =
            cx.add_window_view(|_, cx| Editor::from_markdown(cx, "正文".into(), None));
        editor.update_in(cx, |editor, window, cx| {
            editor.maybe_check_updates_on_startup(window, cx);
            assert!(
                !editor.modal_is_open(),
                "上次安装失败的原因只在日志里说，不该在开机路径上弹窗"
            );
            assert!(
                !editor.update_check_in_progress,
                "前置：有安装失败记录时开机路径不该同时去发网络请求"
            );
        });
        let _ = std::fs::remove_dir_all(root);
    }
}
