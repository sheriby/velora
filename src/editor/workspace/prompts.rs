use super::*;

impl Editor {
    pub(crate) fn prompt_delete_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(target) = self.selected_workspace_path() else {
            return;
        };
        if self.workspace.root.as_ref() == Some(&target) {
            return;
        }
        let target_is_directory = target.is_dir();
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        let has_unsaved_changes =
            self.document_dirty
                && self
                    .file_path
                    .as_ref()
                    .is_some_and(|path| path_is_affected(path, &target, target_is_directory))
                || self.workspace.open_documents.iter().any(|tab| {
                    tab.dirty && path_is_affected(&tab.path, &target, target_is_directory)
                });
        let mut detail = format!(
            "{}\n{}",
            strings.workspace_delete_confirm_message,
            target.display()
        );
        if has_unsaved_changes {
            detail.push_str("\n");
            detail.push_str(&strings.workspace_delete_unsaved_message);
        }
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        let background = cx.background_executor().clone();
        let delete_policy = crate::config::EditorSettings::delete_policy(cx);
        let title = strings.workspace_delete_confirm_title.clone();
        let confirm_label = strings.workspace_delete.clone();
        let cancel_label = strings.open_link_cancel.clone();

        // 删除确认走应用内模态（用户要求：全软件不用系统原生弹窗）。
        self.show_modal(
            ModalSpec {
                title: title.into(),
                detail: Some(detail.into()),
                buttons: vec![confirm_label.into(), cancel_label.into()],
                default_index: 0,
                cancel_index: 1,
            },
            move |choice, _editor, _window, cx| {
                if choice != 0 {
                    return;
                }
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let delete_target = target.clone();
            let delete_policy = delete_policy;
            let result = background.spawn(async move {
                match delete_policy {
                    crate::config::DeletePolicy::Permanent => {
                        permanent_delete(&delete_target, target_is_directory)
                    }
                    crate::config::DeletePolicy::Trash => {
                        move_to_trash(&delete_target, target_is_directory)
                    }
                }
            })
                .await;
            if let Err(err) = result {
                Self::show_workspace_file_error(window_handle, format!("无法删除：{}", err), cx);
                return;
            }

            let target_for_update = target.clone();
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |editor, cx| {
                        let active_recovery_id = editor.recovery_id;
                        let active_deleted = editor.file_path.as_ref().is_some_and(|path| {
                            path_is_affected(path, &target_for_update, target_is_directory)
                        });
                        editor.document_revision = editor.document_revision.wrapping_add(1);
                        editor.autosave_task = None;
                        if active_deleted {
                            editor.snapshot_current_document(cx);
                        }
                        let deleted_recovery_ids = editor
                            .workspace
                            .open_documents
                            .iter()
                            .filter(|tab| {
                                path_is_affected(
                                    &tab.path,
                                    &target_for_update,
                                    target_is_directory,
                                )
                            })
                            .map(|tab| tab.recovery_id)
                            .collect::<Vec<_>>();
                        for recovery_id in deleted_recovery_ids {
                            if let Err(error) =
                                crate::config::remove_recovery_snapshot(recovery_id)
                            {
                                eprintln!("failed to remove deleted tab recovery snapshot: {error}");
                            }
                        }
                        let next_tab = editor
                            .workspace
                            .open_documents
                            .iter()
                            .find(|tab| {
                                !path_is_affected(
                                    &tab.path,
                                    &target_for_update,
                                    target_is_directory,
                                )
                            })
                            .cloned();
                        editor.workspace.open_documents.retain(|tab| {
                            !path_is_affected(&tab.path, &target_for_update, target_is_directory)
                        });
                        if editor.workspace.root.as_ref().is_some_and(|root| {
                            path_is_affected(root, &target_for_update, target_is_directory)
                        }) {
                            editor.workspace.root = None;
                            editor.workspace.file_tree = None;
                            editor.workspace.tree_scan_root = None;
                        }
                        editor.workspace.selected = None;
                        if active_deleted {
                            if let Some(tab) = next_tab {
                                editor.recovery_id = tab.recovery_id;
                                let file_version = tab.file_version;
                                let view = tab.view.clone();
                                editor.recovery_source_path = None;
                                editor.is_recovered_document = false;
                                editor.workspace.active_document = Some(tab.path.clone());
                                if is_code_file(&tab.path) {
                                    editor.restore_document_from_code_source(
                                        tab.markdown, tab.path, view, cx,
                                    );
                                } else {
                                    editor.restore_document_from_markdown(
                                        tab.markdown, tab.path, view, cx,
                                    );
                                }
                                editor.document_dirty = tab.dirty;
                                editor.file_version = Some(file_version);
                                window.set_window_edited(tab.dirty);
                            } else {
                                editor.recovery_id = uuid::Uuid::new_v4();
                                editor.recovery_source_path = None;
                                editor.is_recovered_document = false;
                                if let Err(error) =
                                    crate::config::remove_recovery_snapshot(active_recovery_id)
                                {
                                    eprintln!("failed to remove deleted document recovery snapshot: {error}");
                                }
                                editor.workspace.active_document = None;
                                editor.replace_document_from_markdown(String::new(), None, cx);
                                window.set_window_edited(false);
                            }
                        }
                        editor.refresh_workspace_tree(cx);
                        if editor.document_dirty || editor.has_dirty_workspace_documents() {
                            editor.schedule_autosave(cx);
                        }
                        cx.notify();
                    });
                },
            );
        })
        .detach();
            },
            cx,
        );
    }
}
