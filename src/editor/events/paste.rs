use super::*;

impl Editor {
    /// 多行粘贴在缓冲区里就是一次插入：把剪贴板文本插到光标处，别的块一个字节
    /// 都不动，块结构交给导入器从缓冲区重新认（与打开文件同一套规则）。
    ///
    /// 旧路径自己按行拼块、再 `mark_dirty` 让重同步把整篇从块树重新序列化，于是
    /// 一次粘贴会把不相干的块洗成规范化写法（表格列宽填充重算、`__强调__` 变
    /// `**…**`、Setext 转 ATX、CRLF 与末行换行丢失），撤销条目也变成全文副本。
    ///
    /// 返回 false 表示这次插入没法按区间算（块内偏移映射不到缓冲区、容器结构要
    /// 自己那套规范化、整段都是空行），留给原来的整篇重投影路径。
    pub(crate) fn paste_multiline_through_buffer(
        &mut self,
        block: &Entity<crate::editor::Block>,
        leading: &crate::components::InlineTextTree,
        lines: &[String],
        trailing: &crate::components::InlineTextTree,
        split_physical_lines: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(at) = self.caret_source_offset(block.entity_id(), leading.visible_len(), cx) else {
            return false;
        };
        // 物理行粘贴是「一段一行」：空行丢掉，行与行之间补分隔空行。
        // 结构粘贴保留剪贴板原样的换行，但结构块必须独占一行，所以只在光标那一侧
        // 还留着文字时补空行把它和前后隔开。
        let inserted = if split_physical_lines {
            lines
                .iter()
                .filter(|line| !line.trim().is_empty())
                .cloned()
                .collect::<Vec<_>>()
                .join("\n\n")
        } else {
            let mut text = String::new();
            if leading.visible_len() > 0 {
                text.push_str("\n\n");
            }
            text.push_str(&lines.join("\n"));
            if trailing.visible_len() > 0 {
                text.push_str("\n\n");
            }
            text
        };
        if inserted.is_empty() {
            return false;
        }

        self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        let applied = self.buffer.edit(at..at, &inserted);
        // 光标停在粘贴文本的末尾：插入点之后、原文剩下的那截之前。
        let caret = applied.new_range.end;
        self.record_buffer_edit(applied);
        self.rebuild_document_from_buffer(cx);
        if !split_physical_lines
            && let Some(last_root) = self.document.root_blocks().last().cloned()
        {
            // 结构块落在文档末尾时下面没有行，光标得有段落落脚。
            self.ensure_trailing_paragraph_after_structural(&last_root, cx);
        }
        self.apply_selection_snapshot_in_current_mode(
            &crate::editor::UndoSelectionSnapshot {
                range: caret..caret,
                reversed: false,
            },
            cx,
        );
        self.mark_dirty_written_back(cx);
        self.finalize_pending_undo_capture(cx);
        cx.notify();
        true
    }

    pub(crate) fn build_plain_paste_blocks_from_lines(
        cx: &mut Context<Self>,
        lines: &[String],
    ) -> Vec<Entity<crate::editor::Block>> {
        let mut blocks = lines
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| {
                Self::new_block(
                    cx,
                    BlockRecord::new(BlockKind::Paragraph, InlineTextTree::from_markdown(line)),
                )
            })
            .collect::<Vec<_>>();

        if blocks.is_empty() && !lines.is_empty() {
            blocks.push(Self::new_block(
                cx,
                BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain(String::new())),
            ));
        }

        blocks
    }
    pub(crate) fn current_image_paste_behavior() -> ImagePasteBehavior {
        read_app_preferences()
            .map(|preferences| preferences.image_paste_behavior)
            .unwrap_or(ImagePasteBehavior::None)
    }

    pub(crate) fn image_paste_root_dir(&self) -> anyhow::Result<PathBuf> {
        if let Some(root) = Self::image_paste_base_dir(
            self.file_path.as_deref(),
            self.workspace_root_for_image_paste().as_deref(),
        ) {
            return Ok(root);
        }
        std::env::current_dir().context("failed to resolve current working directory")
    }

    pub(crate) fn image_paste_base_dir(
        file_path: Option<&Path>,
        workspace_root: Option<&Path>,
    ) -> Option<PathBuf> {
        file_path
            .and_then(Path::parent)
            .or(workspace_root)
            .map(Path::to_path_buf)
    }


    pub(crate) fn image_target_dir(
        &self,
        behavior: ImagePasteBehavior,
        root_dir: &Path,
        source: &PastedImageSource,
    ) -> anyhow::Result<PathBuf> {
        match behavior {
            ImagePasteBehavior::None | ImagePasteBehavior::CopyToDocumentFolder => {
                Ok(root_dir.to_path_buf())
            }
            ImagePasteBehavior::CopyToAssetsFolder => Ok(root_dir.join("assets")),
            ImagePasteBehavior::CopyToNamedAssetsFolder => {
                let base = self
                    .file_path
                    .as_ref()
                    .and_then(|path| path.file_stem())
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| !stem.trim().is_empty())
                    .unwrap_or("untitle");
                if self.file_path.is_some() {
                    return Ok(root_dir.join(format!("{base}.assets")));
                }

                for index in 0.. {
                    let folder = if index == 0 {
                        "untitle.assets".to_string()
                    } else {
                        format!("untitle{index}.assets")
                    };
                    let path = root_dir.join(folder);
                    if !path.exists() {
                        return Ok(path);
                    }
                    if matches!(source, PastedImageSource::LocalPath(_)) {
                        continue;
                    }
                }
                unreachable!("unbounded search should always return");
            }
        }
    }

    /// Stable 8-hex-char digest of pasted image bytes for `YYYY-MM-DD-hash` names.
    pub(crate) fn pasted_image_hash(bytes: &[u8]) -> String {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut hasher = DefaultHasher::new();
        bytes.hash(&mut hasher);
        format!("{:08x}", hasher.finish() as u32)
    }

    pub(crate) fn unique_file_path(dir: &Path, preferred_name: &str) -> PathBuf {
        let preferred = Path::new(preferred_name);
        let stem = preferred
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("image");
        let extension = preferred.extension().and_then(|ext| ext.to_str());
        for index in 0.. {
            let file_name = if index == 0 {
                preferred_name.to_string()
            } else if let Some(extension) = extension {
                format!("{stem}{index}.{extension}")
            } else {
                format!("{stem}{index}")
            };
            let candidate = dir.join(file_name);
            if !candidate.exists() {
                return candidate;
            }
        }
        unreachable!("unbounded search should always return");
    }

    pub(crate) fn path_parent_eq(left: &Path, right: &Path) -> bool {
        let Some(parent) = left.parent() else {
            return false;
        };
        let left = parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf());
        let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());
        left == right
    }

    pub(crate) fn materialize_pasted_image(
        &self,
        source: &PastedImageSource,
    ) -> anyhow::Result<(PathBuf, bool)> {
        let behavior = Self::current_image_paste_behavior();
        let root_dir = self.image_paste_root_dir()?;

        if matches!(behavior, ImagePasteBehavior::None)
            && let PastedImageSource::LocalPath(path) = source
        {
            return Ok((path.clone(), false));
        }

        let target_dir = self.image_target_dir(behavior, &root_dir, source)?;
        fs::create_dir_all(&target_dir)
            .with_context(|| format!("failed to create '{}'", target_dir.display()))?;

        match source {
            PastedImageSource::LocalPath(path) => {
                if Self::path_parent_eq(path, &target_dir) {
                    return Ok((path.clone(), behavior != ImagePasteBehavior::None));
                }
                let file_name = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("image");
                let target = Self::unique_file_path(&target_dir, file_name);
                fs::copy(path, &target).with_context(|| {
                    format!(
                        "failed to copy '{}' to '{}'",
                        path.display(),
                        target.display()
                    )
                })?;
                Ok((target, behavior != ImagePasteBehavior::None))
            }
            PastedImageSource::ClipboardImage(image) => {
                let file_name = format!(
                    "{}-{}.{}",
                    crate::config::today_local_date(),
                    Self::pasted_image_hash(&image.bytes),
                    crate::editor::workspace::clipboard_image_extension(image.format)
                );
                let target = Self::unique_file_path(&target_dir, &file_name);
                fs::write(&target, &image.bytes)
                    .with_context(|| format!("failed to write '{}'", target.display()))?;
                Ok((target, behavior != ImagePasteBehavior::None))
            }
        }
    }

    pub(crate) fn markdown_path_string(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    pub(crate) fn markdown_image_target(path: &str) -> String {
        path.chars()
            .flat_map(|ch| match ch {
                '\\' | '(' | ')' | '"' => ['\\', ch].into_iter().collect::<Vec<_>>(),
                _ => [ch].into_iter().collect::<Vec<_>>(),
            })
            .collect()
    }

    pub(crate) fn markdown_image_alt(path: &Path) -> String {
        let alt = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("image");
        alt.chars()
            .flat_map(|ch| match ch {
                '\\' | ']' => ['\\', ch].into_iter().collect::<Vec<_>>(),
                _ => [ch].into_iter().collect::<Vec<_>>(),
            })
            .collect()
    }

    pub(crate) fn relative_markdown_path(root_dir: &Path, path: &Path) -> Option<String> {
        let relative = path.strip_prefix(root_dir).ok()?;
        Some(format!("./{}", Self::markdown_path_string(relative)))
    }

    /// 物化粘贴的图片并产出引用它的 markdown。`created` 为 `Some(路径)` 时表示
    /// 这次调用**新建**了图片文件（剪贴板写入或复制），调用方若最终没有把它写进
    /// 文档，应当删掉它，别留孤儿资源。
    pub(crate) fn pasted_image_markdown(
        &self,
        source: &PastedImageSource,
    ) -> anyhow::Result<(String, Option<std::path::PathBuf>)> {
        let root_dir = self.image_paste_root_dir()?;
        let (path, relative) = self.materialize_pasted_image(source)?;
        let created = Self::materialized_image_was_created(source, &path);
        let path_text = if relative {
            Self::relative_markdown_path(&root_dir, &path)
                .ok_or_else(|| anyhow!("failed to create a relative image path"))?
        } else {
            Self::markdown_path_string(&path)
        };
        let markdown = format!(
            "![{}]({})",
            Self::markdown_image_alt(&path),
            Self::markdown_image_target(&path_text)
        );
        Ok((markdown, created.then_some(path)))
    }

    /// 物化结果是不是这次调用新建的文件（决定失败时能否安全删除）。
    fn materialized_image_was_created(source: &PastedImageSource, materialized: &PathBuf) -> bool {
        match source {
            // behavior = None 时直接引用原路径，没有新建。
            PastedImageSource::LocalPath(path) => path != materialized,
            PastedImageSource::ClipboardImage(_) => true,
        }
    }

    pub(crate) fn show_image_paste_error(&mut self, err: anyhow::Error, cx: &mut Context<Self>) {
        // 应用内模态（用户要求：全软件不用系统原生弹窗）。
        let strings = cx.global::<crate::i18n::I18nManager>().strings().clone();
        self.show_message_modal(
            strings.image_paste_failed_title.clone(),
            err.to_string(),
            cx,
        );
    }

    pub(crate) fn inserted_image_tree_for_block(block: &crate::editor::Block, markdown: &str) -> InlineTextTree {
        if block.uses_raw_text_editing() || block.kind().is_code_block() {
            InlineTextTree::plain(markdown.to_string())
        } else {
            InlineTextTree::from_markdown(markdown)
        }
    }

    pub(crate) fn replace_current_block_selection_with_image_text(
        &mut self,
        block: &Entity<crate::editor::Block>,
        leading: &InlineTextTree,
        markdown: &str,
        trailing: &InlineTextTree,
        cx: &mut Context<Self>,
    ) -> bool {
        let (kind, title, cursor) = block.read_with(cx, |block, _cx| {
            let mut title = leading.clone();
            title.append_tree(Self::inserted_image_tree_for_block(block, markdown));
            let cursor = title.visible_len();
            title.append_tree(trailing.clone());
            (block.kind(), title, cursor)
        });
        Self::set_block_title_and_kind(block, kind, title, cursor, cx);
        if let Some(binding) = self.table_cell_binding(block.entity_id()) {
            self.sync_table_record_from_runtime(&binding.table_block, cx);
        }
        self.focus_block(block.entity_id());
        self.rebuild_image_runtimes(cx);
        true
    }

    pub(crate) fn insert_image_block_after_paragraph(
        &mut self,
        block: &Entity<crate::editor::Block>,
        leading: &InlineTextTree,
        markdown: &str,
        trailing: &InlineTextTree,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(location) = self.document.find_block_location(block.entity_id()) else {
            return false;
        };
        let leading_empty = leading.visible_len() == 0;
        let trailing_empty = trailing.visible_len() == 0;

        if leading_empty {
            Self::set_block_title_and_kind(
                block,
                BlockKind::Paragraph,
                InlineTextTree::plain(markdown.to_string()),
                markdown.len(),
                cx,
            );
            let image_block = block.clone();
            if !trailing_empty {
                let trailing_block =
                    Self::new_block(cx, BlockRecord::new(BlockKind::Paragraph, trailing.clone()));
                self.document.insert_blocks_at(
                    location.parent,
                    location.index + 1,
                    vec![trailing_block],
                    cx,
                );
            }
            self.focus_block(image_block.entity_id());
            self.rebuild_image_runtimes(cx);
            return true;
        }

        Self::set_block_title_and_kind(
            block,
            BlockKind::Paragraph,
            leading.clone(),
            leading.visible_len(),
            cx,
        );
        let image_block = Self::new_block(cx, BlockRecord::paragraph(markdown.to_string()));
        let mut inserted = vec![image_block.clone()];
        if !trailing_empty {
            inserted.push(Self::new_block(
                cx,
                BlockRecord::new(BlockKind::Paragraph, trailing.clone()),
            ));
        }
        self.document
            .insert_blocks_at(location.parent, location.index + 1, inserted, cx);
        self.focus_block(image_block.entity_id());
        self.rebuild_image_runtimes(cx);
        true
    }

    pub(crate) fn handle_paste_image_request(
        &mut self,
        block: Entity<crate::editor::Block>,
        leading: &InlineTextTree,
        source: &PastedImageSource,
        trailing: &InlineTextTree,
        cx: &mut Context<Self>,
    ) {
        let (markdown, created_path) = match self.pasted_image_markdown(source) {
            Ok(result) => result,
            Err(err) => {
                self.show_image_paste_error(err, cx);
                return;
            }
        };

        if self.replace_cross_block_selection_with_text(
            &markdown,
            None,
            false,
            crate::components::UndoCaptureKind::NonCoalescible,
            cx,
        ) {
            return;
        }

        self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        let roots_before = self.document.root_layout();
        let can_insert_image_block = self.view_mode == crate::editor::ViewMode::Rendered
            && block.read(cx).kind() == BlockKind::Paragraph
            && self.table_cell_binding(block.entity_id()).is_none()
            && !block.read(cx).uses_raw_text_editing();

        let inserted = if can_insert_image_block {
            self.insert_image_block_after_paragraph(&block, leading, &markdown, trailing, cx)
        } else {
            self.replace_current_block_selection_with_image_text(
                &block, leading, &markdown, trailing, cx,
            )
        };
        if !inserted {
            // 图片文件已经落盘、文档却没接收（锚点不在树里等边角）：删掉刚建的
            // 文件，别在磁盘上留孤儿资源。
            if let Some(path) = created_path {
                let _ = std::fs::remove_file(&path);
            }
        }

        // 一段变三段（前面 + 图片行 + 后面）改的是根块序列，但变的只有落点那一段：
        // 只重写那一段，别处的 `__强调__` 写法、表格列宽、CRLF 才不会跟着被洗。
        if self.write_back_structural_change(&block, Some(&roots_before), cx) {
            self.mark_dirty_written_back(cx);
        } else {
            self.mark_dirty(cx);
        }
        self.finalize_pending_undo_capture(cx);
        cx.notify();
    }
}
