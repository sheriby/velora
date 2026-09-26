//! Workspace file watching (roadmap D3): externally modified open documents
//! are reloaded when clean; dirty documents keep the existing save-time
//! conflict protection.

use std::path::{Path, PathBuf};

use futures::channel::mpsc;
use futures::StreamExt;
use notify::{RecursiveMode, Watcher};

use super::Editor;

/// Starts a recursive watcher on `root`, forwarding modified file paths into
/// the editor. Any previous watcher is dropped (replacing the workspace root).
pub(crate) fn start_watching(editor: &mut Editor, root: &Path, cx: &mut gpui::Context<Editor>) {
    editor.external_watcher = None;

    let (tx, rx) = mpsc::unbounded::<PathBuf>();
    let watcher = notify::recommended_watcher(
        move |res: Result<notify::Event, notify::Error>| {
            let Ok(event) = res else { return };
            if !matches!(
                event.kind,
                notify::EventKind::Modify(_) | notify::EventKind::Create(_)
            ) {
                return;
            }
            for path in event.paths {
                let _ = tx.unbounded_send(path);
            }
        },
    );
    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(error) => {
            eprintln!("failed to watch workspace {}: {error}", root.display());
            return;
        }
    };
    if let Err(error) = watcher.watch(root, RecursiveMode::Recursive) {
        eprintln!("failed to watch workspace {}: {error}", root.display());
        return;
    }
    // Store the watcher (kept alive) and spawn the event pump. The receiver
    // must move into the spawn, so the editor field is filled from a clone
    // stored beforehand: use an Option swap instead.
    editor.external_watcher = Some(watcher);
    let mut events = rx;
    cx.spawn(async move |this, cx| {
        while let Some(path) = events.next().await {
            let Ok(()) = cx.update(|cx| {
                let _ = this.update(cx, |editor, cx| {
                    editor.reload_externally_changed_document(&path, cx);
                });
            }) else {
                return;
            };
        }
    })
    .detach();
}
