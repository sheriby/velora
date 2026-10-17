use super::common::*;
use crate::editor::update::{UpdateBackend, set_test_update_backend};
use crate::net::update::{
    self, InstallPlan, UpdateCheckError, UpdateCheckResult, UpdatePackage, UpdatePlatform,
};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

struct MemoryUpdateBackend {
    root: PathBuf,
    payload: Vec<u8>,
    release: String,
    corrupt_download: AtomicBool,
    fail_install: AtomicBool,
    events: Mutex<Vec<&'static str>>,
    plans: Mutex<Vec<InstallPlan>>,
}

impl MemoryUpdateBackend {
    fn new(root: PathBuf) -> Self {
        let platform = UpdatePlatform::current().unwrap();
        let payload = match platform {
            UpdatePlatform::WindowsX64 => b"MZfixture".to_vec(),
            _ => b"xar!fixture".to_vec(),
        };
        let name = platform.asset_name(&semver::Version::parse("999.0.0").unwrap());
        let release = serde_json::json!([{
            "tag_name": "v999.0.0", "html_url": update::RELEASES_URL,
            "draft": false, "prerelease": false, "published_at": "2026-10-17T00:00:00Z",
            "assets": [{"name": name, "state": "uploaded", "size": payload.len(),
                "browser_download_url": format!("{}/download/v999.0.0/{name}", update::RELEASES_URL),
                "digest": format!("sha256:{:x}", Sha256::digest(&payload))}]
        }]).to_string();
        Self {
            root,
            payload,
            release,
            corrupt_download: AtomicBool::new(false),
            fail_install: AtomicBool::new(false),
            events: Mutex::new(Vec::new()),
            plans: Mutex::new(Vec::new()),
        }
    }

    fn events(&self) -> Vec<&'static str> {
        self.events.lock().unwrap().clone()
    }
}

impl UpdateBackend for MemoryUpdateBackend {
    fn check(
        &self,
        current: &str,
        prereleases: bool,
    ) -> Result<UpdateCheckResult, UpdateCheckError> {
        self.events.lock().unwrap().push("check");
        update::select_release(
            &self.release,
            current,
            prereleases,
            UpdatePlatform::current().unwrap(),
        )
    }

    fn ensure_single_instance(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn download(
        &self,
        package: &UpdatePackage,
        cancelled: &AtomicBool,
        progress: &mut dyn FnMut(u64),
    ) -> anyhow::Result<PathBuf> {
        self.events.lock().unwrap().push("download");
        let mut payload = self.payload.clone();
        if self.corrupt_download.load(Ordering::Relaxed) {
            if let Some(last) = payload.last_mut() {
                *last ^= 1;
            }
        }
        let mut validated = Vec::new();
        update::receive_package(
            std::io::Cursor::new(payload),
            package,
            &mut validated,
            cancelled,
            &mut |bytes| progress(bytes),
        )?;
        let path = self.root.join(&package.name);
        fs::write(&path, validated)?;
        Ok(path)
    }

    fn prepare_install(
        &self,
        package: &UpdatePackage,
        path: &Path,
        arguments: Vec<String>,
    ) -> anyhow::Result<InstallPlan> {
        self.events.lock().unwrap().push("prepare");
        update::prepare_install_plan(package, path, arguments)
    }

    fn launch_install(&self, plan: &InstallPlan) -> anyhow::Result<()> {
        self.events.lock().unwrap().push("install");
        if self.fail_install.load(Ordering::Relaxed) {
            anyhow::bail!("演练安装器启动失败");
        }
        self.plans.lock().unwrap().push(plan.clone());
        Ok(())
    }
}

struct UpdateFlow<'a> {
    editor: gpui::Entity<Editor>,
    cx: &'a mut VisualTestContext,
    backend: Arc<MemoryUpdateBackend>,
    document: PathBuf,
    _config: crate::config::TestConfigRootGuard,
}

impl<'a> UpdateFlow<'a> {
    fn new(cx: &'a mut TestAppContext) -> Self {
        let root = temp_fixture_dir().join(format!("update-flow-{}", temp_fixture_token()));
        fs::create_dir_all(&root).unwrap();
        let config = crate::config::override_test_config_root(root.join("config"));
        cx.on_quit({
            let root = root.clone();
            move || fs::remove_dir_all(root).unwrap()
        });
        let document = root.join("document.md");
        fs::write(&document, "原文").unwrap();
        let backend = Arc::new(MemoryUpdateBackend::new(root));
        init_editor_test_app(cx);
        cx.update(|cx| {
            crate::app_menu::init(cx);
            let mut preferences = crate::config::preferences::UpdatePreferences::default();
            preferences.check_on_startup = false;
            crate::config::EditorSettings::set_updates_in_memory(cx, preferences);
            crate::config::EditorSettings::set_autosave_in_memory(false, cx);
            set_test_update_backend(cx, backend.clone());
        });
        let (editor, cx) = cx.add_window_view(|_, cx| {
            Editor::from_markdown(cx, "原文".into(), Some(document.clone()))
        });
        redraw(cx);
        cx.update(|window, _| window.activate_window());
        Self {
            editor,
            cx,
            backend,
            document,
            _config: config,
        }
    }

    fn check(&mut self) {
        self.cx.update(|_, cx| {
            crate::app_menu::dispatch_menu_action(&crate::components::CheckForUpdates, cx)
        });
        self.cx.run_until_parked();
        assert!(
            self.editor
                .read_with(self.cx, |editor, _| editor.update_notification.is_some()),
            "菜单检查后应显示可更新版本"
        );
    }

    fn click(&mut self, selector: &'static str) {
        redraw(self.cx);
        let bounds = self
            .cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("应能点击 {selector}"));
        self.cx.simulate_click(bounds.center(), Modifiers::none());
        self.cx.run_until_parked();
    }

    fn edit(&mut self) -> String {
        self.cx.simulate_input("测试修改");
        self.cx.run_until_parked();
        let text = self
            .editor
            .read_with(self.cx, |editor, _| editor.buffer.text());
        assert!(text.contains("测试修改"));
        text
    }
}

#[gpui::test]
async fn update_flow_clean_document_reaches_installer_once(cx: &mut TestAppContext) {
    let mut flow = UpdateFlow::new(cx);
    flow.check();
    flow.click("update-confirm");
    assert_eq!(
        flow.backend.events(),
        ["check", "download", "prepare", "install"]
    );
    let plans = flow.backend.plans.lock().unwrap();
    assert_eq!(plans.len(), 1);
    assert_eq!(
        fs::read(&plans[0].package_path).unwrap(),
        flow.backend.payload
    );
    assert!(
        flow.editor
            .read_with(flow.cx, |editor, _| !editor.show_unsaved_changes_dialog)
    );
}

#[gpui::test]
async fn update_flow_saves_dirty_document_before_installing(cx: &mut TestAppContext) {
    let mut flow = UpdateFlow::new(cx);
    let expected = flow.edit();
    flow.check();
    flow.click("update-confirm");
    assert_eq!(flow.backend.events(), ["check", "download", "prepare"]);
    assert!(
        flow.editor
            .read_with(flow.cx, |editor, _| editor.show_unsaved_changes_dialog)
    );
    flow.click("save-and-close-dialog");
    assert_eq!(fs::read_to_string(&flow.document).unwrap(), expected);
    assert!(
        flow.cx.cx.windows().is_empty(),
        "保存后应关闭文档窗口，当前状态：{:?}",
        flow.editor.read_with(flow.cx, |editor, _| (
            editor.document_dirty,
            editor.pending_close_after_save,
            editor.pending_save,
            editor.show_unsaved_changes_dialog
        ))
    );
    assert_eq!(
        flow.backend.events(),
        ["check", "download", "prepare", "install"]
    );
}

#[gpui::test]
async fn update_flow_cancel_preserves_document_without_installing(cx: &mut TestAppContext) {
    let mut flow = UpdateFlow::new(cx);
    let expected = flow.edit();
    flow.check();
    flow.click("update-confirm");
    flow.click("cancel-close-dialog");
    assert_eq!(flow.backend.events(), ["check", "download", "prepare"]);
    assert!(flow.backend.plans.lock().unwrap().is_empty());
    assert_eq!(
        flow.editor
            .read_with(flow.cx, |editor, _| editor.buffer.text()),
        expected
    );
    assert_eq!(fs::read_to_string(&flow.document).unwrap(), "原文");
}

#[gpui::test]
async fn update_flow_checksum_failure_can_retry(cx: &mut TestAppContext) {
    let mut flow = UpdateFlow::new(cx);
    flow.backend.corrupt_download.store(true, Ordering::Relaxed);
    flow.check();
    flow.click("update-confirm");
    assert_eq!(flow.backend.events(), ["check", "download"]);
    assert!(flow.editor.read_with(flow.cx, |editor, _| {
        editor
            .modal_spec()
            .and_then(|modal| modal.detail.as_ref())
            .is_some_and(|detail| detail.contains("校验"))
    }));
    flow.backend
        .corrupt_download
        .store(false, Ordering::Relaxed);
    flow.cx.simulate_keystrokes("escape");
    flow.click("update-confirm");
    assert_eq!(
        flow.backend.events(),
        ["check", "download", "download", "prepare", "install"]
    );
}

#[gpui::test]
async fn update_flow_installer_failure_restores_retry(cx: &mut TestAppContext) {
    let mut flow = UpdateFlow::new(cx);
    flow.backend.fail_install.store(true, Ordering::Relaxed);
    flow.check();
    flow.click("update-confirm");
    assert_eq!(
        flow.backend.events(),
        ["check", "download", "prepare", "install"]
    );
    assert!(flow.backend.plans.lock().unwrap().is_empty());
    assert!(
        flow.editor
            .read_with(flow.cx, |editor, _| editor.modal_is_open()
                && editor.update_download_progress.is_none())
    );
    flow.backend.fail_install.store(false, Ordering::Relaxed);
    flow.cx.simulate_keystrokes("escape");
    flow.click("update-confirm");
    assert_eq!(flow.backend.plans.lock().unwrap().len(), 1);
}
