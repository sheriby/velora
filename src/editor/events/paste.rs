use super::*;

impl Editor {
    /// 多行粘贴在缓冲区里是一次替换：剪贴板文本覆盖原选择，别的块一个字节
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
        soft_wrapped: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(mapping) = self.source_mapping_for_entity(block.entity_id(), cx) else {
            return false;
        };
        let original = block.read(cx);
        let clean_start = leading.visible_len();
        let clean_end = original.clean_visible_len().saturating_sub(trailing.visible_len());
        if clean_end < clean_start {
            return false;
        }
        let markdown_range = original.record.title.markdown_offset_map()
            .visible_to_markdown_range(clean_start..clean_end);
        let Some(source_start) = mapping.content_to_source.get(markdown_range.start).copied() else { return false; };
        let Some(source_end) = mapping.content_to_source.get(markdown_range.end).copied() else { return false; };
        let source_range = mapping.full_source_range.start + source_start
            ..mapping.full_source_range.start + source_end;
        // 软换行的连续行就是同一段的一部分：原样插在光标处，不补空行。
        // 结构粘贴保留剪贴板原样的换行，但结构块必须独占一行，所以只在
        // 光标那一侧还留着文字时补空行把它和前后隔开。
        let mut inserted = String::new();
        if !soft_wrapped {
            if leading.visible_len() > 0 {
                inserted.push_str("\n\n");
            }
        }
        inserted.push_str(&lines.join("\n"));
        if !soft_wrapped && trailing.visible_len() > 0 {
            inserted.push_str("\n\n");
        }
        if inserted.is_empty() {
            return false;
        }

        self.prepare_undo_capture(crate::components::UndoCaptureKind::NonCoalescible, cx);
        let applied = self.buffer.edit(source_range, &inserted);
        // 光标停在粘贴文本的末尾：插入点之后、原文剩下的那截之前。
        let caret = applied.new_range.end;
        self.record_buffer_edit(applied);
        self.rebuild_document_from_buffer(cx);
        if let Some(last_root) = self.document.root_blocks().last().cloned() {
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
        // 没有文档路径也没有工作区根时，图片没有「文档旁边」可落。退回进程当前
        // 目录会让文件位置随启动方式漂移（可能是 `/` 或应用包目录），文档一旦
        // 保存到别处引用全断——先让用户给文档一个位置。
        anyhow::bail!(
            "文档还没有保存位置：先保存文档或打开一个工作区，再粘贴图片"
        )
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

                // 探测必须有上界：目录名空间被占满时不能在 UI 线程上无限循环，
                // 到顶就报错走应用内模态。
                const FOLDER_PROBE_LIMIT: u32 = 1000;
                for index in 0..=FOLDER_PROBE_LIMIT {
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
                Err(anyhow::anyhow!(
                    "no free '{base}.assets' folder name after {FOLDER_PROBE_LIMIT} probes"
                ))
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

    pub(crate) fn unique_file_path(dir: &Path, preferred_name: &str) -> anyhow::Result<PathBuf> {
        let preferred = Path::new(preferred_name);
        let stem = preferred
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("image");
        let extension = preferred.extension().and_then(|ext| ext.to_str());
        // 探测必须有上界：文件名空间被占满时既不能在 UI 线程上无限循环，
        // 也不能放弃防撞名直接覆盖既有文件，到顶就报错走应用内模态。
        const NAME_PROBE_LIMIT: u32 = 1000;
        for index in 0..=NAME_PROBE_LIMIT {
            let file_name = if index == 0 {
                preferred_name.to_string()
            } else if let Some(extension) = extension {
                format!("{stem}{index}.{extension}")
            } else {
                format!("{stem}{index}")
            };
            let candidate = dir.join(file_name);
            if !candidate.exists() {
                return Ok(candidate);
            }
        }
        Err(anyhow::anyhow!(
            "no free file name for '{preferred_name}' after {NAME_PROBE_LIMIT} probes"
        ))
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
                let target = Self::unique_file_path(&target_dir, file_name)
                    .with_context(|| format!("failed to reserve a file name in '{}'", target_dir.display()))?;
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
                let target = Self::unique_file_path(&target_dir, &file_name)
                    .with_context(|| format!("failed to reserve a file name in '{}'", target_dir.display()))?;
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
        let escaped: String = path
            .chars()
            .flat_map(|ch| match ch {
                '\\' | '(' | ')' | '"' => ['\\', ch].into_iter().collect::<Vec<_>>(),
                _ => [ch].into_iter().collect::<Vec<_>>(),
            })
            .collect();
        // CommonMark 的裸目的地不许含空格，`\ ` 也不是合法转义：目的地址带空格
        //（截图默认名、Windows 复制路径）时整体包 `<>`，应用内
        // `angle_bracket_destination` 与导出侧 pulldown 都按规范剥壳。尖括号
        // 内部不许出现未转义的 `<`/`>`，一并转义。
        if escaped.contains(' ') {
            let inner = escaped.replace('<', "\\<").replace('>', "\\>");
            format!("<{inner}>")
        } else {
            escaped
        }
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
        if self.is_read_only() {
            return false;
        }

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
        if self.is_read_only() {
            return false;
        }

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
        if self.is_read_only() {
            return;
        }
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
            if let Some(path) = created_path
                && let Err(error) = std::fs::remove_file(&path)
            {
                // 清理失败多半是文件被占用（Windows 索引器）或权限问题：孤儿
                // 文件会一直留在磁盘上，至少让它可排查。
                eprintln!(
                    "velora: 未能删除未接收的粘贴图片 {}：{error}",
                    path.display()
                );
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
