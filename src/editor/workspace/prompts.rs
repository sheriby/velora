use super::*;

impl Editor {
    pub(crate) fn prompt_create_workspace_file(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.selected_workspace_directory() else {
            return;
        };
        let prompt = cx.prompt_for_new_path(&directory, Some("untitled.md"));
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            if let Err(err) = create_workspace_file(&path) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法新建文件：{}", err),
                    cx,
                );
                return;
            }
            let _ = editor.update(cx, |editor, cx| editor.refresh_workspace_tree(cx));
            let _ = cx.update_window(
                window_handle,
                move |_view: AnyView, window: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |editor, cx| {
                        editor.open_workspace_file(path, window, cx);
                    });
                },
            );
        })
        .detach();
    }

    pub(crate) fn prompt_create_workspace_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(directory) = self.selected_workspace_directory() else {
            return;
        };
        let prompt = cx.prompt_for_new_path(&directory, Some("New Folder"));
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(path))) = prompt.await else {
                return;
            };
            if let Err(err) = create_workspace_folder(&path) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法新建文件夹：{}", err),
                    cx,
                );
                return;
            }
            let folder_path = path.clone();
            let _ = editor.update(cx, move |editor, cx| {
                editor.refresh_workspace_tree(cx);
                editor.workspace.expanded.insert(file_node_id(&folder_path));
                editor.workspace.selected = Some(WorkspaceSelection::Directory(folder_path));
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn prompt_rename_or_move_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(source) = self.selected_workspace_path() else {
            return;
        };
        let Some(parent) = source.parent().map(Path::to_path_buf) else {
            return;
        };
        let suggested_name = source
            .file_name()
            .map(|name| name.to_string_lossy().into_owned());
        let prompt = cx.prompt_for_new_path(&parent, suggested_name.as_deref());
        let editor = cx.entity().downgrade();
        let window_handle = window.window_handle();
        let source_is_directory = source.is_dir();
        let background = cx.background_executor().clone();

        cx.spawn(async move |_this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let Ok(Ok(Some(destination))) = prompt.await else {
                return;
            };
            if destination == source {
                return;
            }
            if destination.exists() {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("目标路径已存在：{}", destination.display()),
                    cx,
                );
                return;
            }

            let source_directory = source.parent().map(Path::to_path_buf);
            let destination_directory = destination.parent().map(Path::to_path_buf);
            let markdown_move = !source_is_directory
                && is_markdown_file(&source)
                && source_directory != destination_directory;
            let open_markdown = if markdown_move {
                editor
                    .update(cx, |editor, cx| editor.markdown_state_for_path(&source, cx))
                    .ok()
                    .flatten()
            } else {
                None
            };
            let source_for_read = source.clone();
            let disk_markdown = if markdown_move {
                match background
                    .spawn(async move { crate::editor::encoding::read_document_string(source_for_read.as_path()) })
                    .await
                {
                    Ok(markdown) => Some(markdown),
                    Err(error) => {
                        Self::show_workspace_file_error(
                            window_handle,
                            format!("无法读取待移动的 Markdown 文件：{error}"),
                            cx,
                        );
                        return;
                    }
                }
            } else {
                None
            };
            if let (Some((_, _, expected_version)), Some(markdown)) =
                (open_markdown.as_ref(), disk_markdown.as_ref())
                && crate::editor::persistence::file_content_version(markdown) != *expected_version
            {
                Self::show_external_change_error(
                    window_handle,
                    format!("检测到外部修改：{}", source.display()),
                    cx,
                );
                return;
            }
            let rewritten_disk_markdown = match (
                markdown_move,
                disk_markdown.as_deref(),
                source_directory.as_deref(),
                destination_directory.as_deref(),
            ) {
                (true, Some(markdown), Some(source_directory), Some(destination_directory)) => {
                    Some(rewrite_relative_image_targets(
                        markdown,
                        source_directory,
                        destination_directory,
                    ))
                }
                _ => None,
            };
            let moved_disk_markdown = rewritten_disk_markdown
                .clone()
                .or_else(|| disk_markdown.clone());
            let moved_disk_version = moved_disk_markdown
                .as_deref()
                .map(crate::editor::persistence::file_content_version);

            if let Err(err) = std::fs::rename(&source, &destination) {
                Self::show_workspace_file_error(
                    window_handle,
                    format!("无法移动或重命名：{}", err),
                    cx,
                );
                return;
            }
            if let Some(rewritten) = rewritten_disk_markdown
                .as_ref()
                .zip(disk_markdown.as_ref())
                .and_then(|(rewritten, original)| (rewritten != original).then_some(rewritten))
                && let Err(error) = fs::write(&destination, rewritten)
            {
                let rollback_error = std::fs::rename(&destination, &source).err();
                let detail = if let Some(rollback_error) = rollback_error {
                    format!("无法更新图片相对路径：{error}；回滚也失败：{rollback_error}")
                } else {
                    format!("无法更新图片相对路径：{error}")
                };
                Self::show_workspace_file_error(window_handle, detail, cx);
                return;
            }

            let _ = editor.update(cx, move |editor, cx| {
                editor.document_revision = editor.document_revision.wrapping_add(1);
                editor.autosave_task = None;
                for tab in &mut editor.workspace.open_documents {
                    if markdown_move && tab.path == source {
                        if let (Some(source_directory), Some(destination_directory)) = (
                            source_directory.as_deref(),
                            destination_directory.as_deref(),
                        ) {
                            tab.markdown = rewrite_relative_image_targets(
                                &tab.markdown,
                                source_directory,
                                destination_directory,
                            );
                        }
                        if let Some(file_version) = moved_disk_version {
                            tab.file_version = file_version;
                        }
                    }
                    if let Some(path) =
                        remap_moved_path(&tab.path, &source, &destination, source_is_directory)
                    {
                        tab.path = path;
                    }
                }
                let active_markdown = if markdown_move && editor.file_path.as_ref() == Some(&source)
                {
                    Some((editor.serialized_document_text(cx), editor.document_dirty))
                } else {
                    None
                };
                if let Some(path) = editor.file_path.as_ref().and_then(|path| {
                    remap_moved_path(path, &source, &destination, source_is_directory)
                }) {
                    editor.file_path = Some(path);
                    editor.pending_window_title_refresh = true;
                }
                if let Some(path) = editor.workspace.active_document.as_ref().and_then(|path| {
                    remap_moved_path(path, &source, &destination, source_is_directory)
                }) {
                    editor.workspace.active_document = Some(path);
                }
                if let Some(root) = editor.workspace.root.as_ref().and_then(|root| {
                    remap_moved_path(root, &source, &destination, source_is_directory)
                }) {
                    editor.workspace.root = Some(root.clone());
                }
                editor.workspace.selected = match editor.workspace.selected.take() {
                    Some(WorkspaceSelection::File(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::File)
                    }
                    Some(WorkspaceSelection::Directory(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::Directory)
                    }
                    Some(WorkspaceSelection::WorkspaceRoot(path)) => {
                        remap_moved_path(&path, &source, &destination, source_is_directory)
                            .map(WorkspaceSelection::WorkspaceRoot)
                    }
                    other => other,
                };
                if let Some((markdown, was_dirty)) = active_markdown {
                    if let (Some(source_directory), Some(destination_directory)) = (
                        source_directory.as_deref(),
                        destination_directory.as_deref(),
                    ) {
                        let rewritten = rewrite_relative_image_targets(
                            &markdown,
                            source_directory,
                            destination_directory,
                        );
                        editor.file_version = moved_disk_version;
                        if rewritten != markdown {
                            editor.replace_document_from_markdown(
                                rewritten.clone(),
                                Some(destination.clone()),
                                cx,
                            );
                            editor.file_version = moved_disk_version;
                            editor.document_dirty = was_dirty;
                            if was_dirty {
                                editor.mark_dirty(cx);
                            }
                            if let Some(tab) =
                                editor.workspace.open_documents.iter_mut().find(|tab| {
                                    tab.path == destination && tab.recovery_id == editor.recovery_id
                                })
                            {
                                tab.markdown = rewritten;
                                tab.dirty = editor.document_dirty;
                                tab.file_version = moved_disk_version.unwrap_or_else(|| {
                                    crate::editor::persistence::file_content_version(&tab.markdown)
                                });
                            }
                        } else if was_dirty {
                            editor.document_dirty = true;
                            editor.schedule_autosave(cx);
                        }
                    }
                }
                editor.refresh_workspace_tree(cx);
                if editor.document_dirty || editor.has_dirty_workspace_documents() {
                    editor.schedule_autosave(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

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
                                editor.recovery_source_path = None;
                                editor.is_recovered_document = false;
                                editor.workspace.active_document = Some(tab.path.clone());
                                if is_code_file(&tab.path) {
                                    editor.replace_document_from_code_source(tab.markdown, tab.path, cx);
                                } else {
                                    editor.replace_document_from_markdown(tab.markdown, Some(tab.path), cx);
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
