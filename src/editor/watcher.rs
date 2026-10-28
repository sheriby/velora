//! Workspace file watching (roadmap D3): externally modified open documents
//! are reloaded when clean; dirty documents keep the existing save-time
//! conflict protection.

use std::path::{Path, PathBuf};
use std::time::Duration;

use futures::channel::mpsc;
use futures::StreamExt;
use notify::{RecursiveMode, Watcher};

use super::Editor;

/// 外部变更事件的合并窗口：外部分片写大文件（重定向、curl、同步盘增量上传）
/// 会连发一串事件，逐条转发就是逐条「主线程全量读盘 + 解码重载」。与文件树
/// 刷新的 250ms 合并同一个量级。
const EVENT_COALESCE_WINDOW: Duration = Duration::from_millis(250);

pub(crate) enum WorkspaceEvent {
    Changed(PathBuf),
    Rescan,
}

/// 一批事件收拢成派发清单：路径去重（同一文件的多条事件只重载一次）；Rescan
/// 与 Changed 并存时都保留——Rescan 只重扫文件树，替代不了文档重载。
fn collapse_event_batch(batch: Vec<WorkspaceEvent>) -> (Vec<PathBuf>, bool) {
    let mut changed = Vec::new();
    let mut rescan = false;
    for event in batch {
        match event {
            WorkspaceEvent::Changed(path) => {
                if !changed.contains(&path) {
                    changed.push(path);
                }
            }
            WorkspaceEvent::Rescan => rescan = true,
        }
    }
    (changed, rescan)
}

/// 监听建立失败要走应用内通道（侧栏红字）：只写 stderr 的话，监听失效时用户
/// 毫无感知——干净文档会一直停在打开时的旧内容上，外部修改检测整体静默失效。
pub(crate) fn note_watcher_unavailable(
    root: &Path,
    error: &notify::Error,
    editor: &mut Editor,
    cx: &mut gpui::Context<Editor>,
) {
    eprintln!("failed to watch workspace {}: {error}", root.display());
    editor.report_workspace_file_error(
        format!(
            "无法监听「{}」的文件变更，外部修改不会自动刷新：{error}",
            root.display()
        ),
        cx,
    );
}

/// Starts a recursive watcher on `root`, forwarding modified file paths into
/// the editor. Any previous watcher is dropped (replacing the workspace root).
pub(crate) fn start_watching(editor: &mut Editor, root: &Path, cx: &mut gpui::Context<Editor>) {
    editor.external_watcher = None;

    let (tx, rx) = mpsc::unbounded::<WorkspaceEvent>();
    let watcher = notify::recommended_watcher(
        move |res: Result<notify::Event, notify::Error>| {
            let Ok(event) = res else { return };
            if event.need_rescan() {
                if let Err(error) = tx.unbounded_send(WorkspaceEvent::Rescan) {
                    eprintln!("发送工作区重新同步事件失败：{error}");
                }
                return;
            }
            let relevant = matches!(
                event.kind,
                notify::EventKind::Modify(_)
                    | notify::EventKind::Create(_)
                    | notify::EventKind::Remove(_)
            );
            if !relevant {
                return;
            }
            for path in event.paths {
                // 自己的保存临时文件不转发：autosave 每轮
                // prepare/rename 都会造出 Create/Modify/Remove 一串事件，转出去
                // 就是给文件树与文档重载各添一轮噪声。写临时文件的进程本来就是
                // 自己，识别判据与生成共用一处（config::is_save_temp_file_name）。
                if path
                    .file_name()
                    .is_some_and(|name| crate::config::is_save_temp_file_name(&name.to_string_lossy()))
                {
                    continue;
                }
                if let Err(error) = tx.unbounded_send(WorkspaceEvent::Changed(path)) {
                    eprintln!("发送工作区文件变动失败：{error}");
                    return;
                }
            }
        },
    );
    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            note_watcher_unavailable(root, &error, editor, cx);
            return;
        }
    };
    if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
        note_watcher_unavailable(root, &error, editor, cx);
        return;
    }
    // Store the watcher (kept alive) and spawn the event pump. The receiver
    // must move into the spawn, so the editor field is filled from a clone
    // stored beforehand: use an Option swap instead.
    editor.external_watcher = Some(watcher);
    let watch_root = root.to_path_buf();
    let mut events = rx;
    cx.spawn(async move |this, cx| {
        while let Some(first) = events.next().await {
            // 首条事件后开一个合并窗口，窗口内到达的事件并成一批再派发：一批
            // 只付一次读盘，而不是每条事件付一次。事件合并不改变
            // 单条事件的语义，只把同一窗口内的重复读盘并掉。
            cx.background_executor().timer(EVENT_COALESCE_WINDOW).await;
            let mut batch = vec![first];
            // try_recv：收尽当前已就绪的事件（通道空 / 关闭都算收完）。
            while let Ok(event) = events.try_recv() {
                batch.push(event);
            }
            let (changed, rescan) = collapse_event_batch(batch);
            let dispatched = cx.update(|cx| {
                this.update(cx, |editor, cx| {
                    for path in &changed {
                        editor.on_watched_path_changed(path, cx);
                    }
                    if rescan && editor.workspace_root_path() == Some(watch_root.as_path()) {
                        editor.on_workspace_watch_rescan(cx);
                    }
                })
                .is_ok()
            });
            if !dispatched {
                // 编辑器实体已随窗口销毁：监听器挂在实体上，随后就会被 drop，
                // 这条泵没有继续存在的意义。
                break;
            }
        }
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{WorkspaceEvent, collapse_event_batch};

    /// 同一窗口内同一文件的多条事件只派发一次；Rescan 与 Changed 并存时两者
    /// 都保留（Rescan 补树，Changed 补正文重载）。
    #[test]
    fn event_batches_collapse_duplicate_paths_and_keep_rescan() {
        let batch = vec![
            WorkspaceEvent::Changed(PathBuf::from("/w/a.md")),
            WorkspaceEvent::Changed(PathBuf::from("/w/b.md")),
            WorkspaceEvent::Changed(PathBuf::from("/w/a.md")),
            WorkspaceEvent::Rescan,
            WorkspaceEvent::Changed(PathBuf::from("/w/b.md")),
        ];
        let (changed, rescan) = collapse_event_batch(batch);
        assert_eq!(
            changed,
            vec![
                PathBuf::from("/w/a.md"),
                PathBuf::from("/w/b.md"),
            ],
            "重复路径要去重，且保住首现顺序"
        );
        assert!(rescan, "同批里的 Rescan 不能被 Changed 顶掉");

        let (changed, rescan) = collapse_event_batch(vec![
            WorkspaceEvent::Rescan,
            WorkspaceEvent::Changed(PathBuf::from("/w/c.md")),
        ]);
        assert_eq!(changed, vec![PathBuf::from("/w/c.md")]);
        assert!(rescan);
    }
}
