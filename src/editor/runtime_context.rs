//! Runtime context synchronization for blocks, references, images, and focus.

use super::*;

impl Editor {
    /// 这次编辑**可能**改动引用定义集或脚注引用表吗？
    ///
    /// 定义只有两种写法，两种都带 `]:`：`[label]: url`（图片与链接各一张表）与
    /// `[^label]: text`（脚注定义）。脚注的**引用** `[^label]` 不带 `]:`，但它决定
    /// 「哪几块引用了这个脚注」，注册表里的 `block_occurrences` 也要跟着重算。所以屏幕
    /// 就两条：`]:` 或 `[^`。
    ///
    /// 扫的是**块自己的源码字节**（`buffer.slice(块区间)`）：解析器扫的就是这份缓冲区
    /// 文本，屏幕得跟它看同一份东西，不能拿行内树里的片段去猜——定义行在行内树里未必
    /// 还留着 `]:`。行内树那条只留给没有源码区间的块（子块、刚建出来还没落笔的）。
    ///
    /// 旧口径是「片段里出现 `[` 或 `<`，或块是表格/HTML/frontmatter/裸 markdown」：
    /// 一个带 `[文字](链接)` 的段落、一句 `a < b` 都会让每键触发一次整篇注册表重建
    /// （10 MiB 档一次 766ms）。收窄之后，定义行与脚注引用该动的仍然会动。
    fn block_may_change_document_references(&self, block: &Entity<Block>, cx: &App) -> bool {
        // 区间不可信时（端点落在多字节字符中间、或越过缓冲区末尾）不要拿它去切片：
        // 那会让整篇注册表重建当场 panic，而这里只是个**屏幕**——退回行内树那份保守
        // 口径就够了（多重建一次，换的是不崩）。区间本身的问题由
        // `root_spans_stay_on_char_boundaries_across_edits_and_undo` 那条不变式闸门报。
        // 区间不可信时（端点落在多字节字符中间、越过缓冲区末尾、或区间倒置）不要
        // 拿它去切片：那会让整篇注册表重建当场 panic，而这里只是个**屏幕**——退回
        // 行内树那份保守口径就够了（多重建一次，换的是不崩）。判据只有一处：
        // `TextBuffer::span_is_trustworthy`。区间本身的问题由
        // `root_spans_stay_on_char_boundaries_across_edits_and_undo` 那条不变式闸门报。
        let source = self
            .document
            .source_span_of(block.entity_id())
            .filter(|span| self.buffer.span_is_trustworthy(span))
            .map(|span| self.buffer.slice(span));
        let Some(source) = source else {
            // 表格格子是唯一一类「宽口径可以更窄」的无区间块：定义行不可能长在表格行
            // 里（解析器只认行首的 `[label]:`，而表格行首是 `|`），格子这一格只可能
            // 改**脚注引用表**——`[^label]` 决定「哪几块引用了这个脚注」。旧口径对含
            // `[文字](链接)` 或 `a < b` 的格子每键付一次整篇重建（10 MiB 档 766ms，
            // 闸门 `typing_in_a_table_cell_does_not_rebuild_the_reference_registry`）。
            if self.table_cells.contains_key(&block.entity_id()) {
                return block.read_with(cx, |block, _cx| {
                    block.record.title.fragments.iter().any(|fragment| {
                        fragment.footnote.is_some() || fragment.text.contains("[^")
                    })
                });
            }
            // 其余无区间块（列表项、引用容器的子块）退回**原来的**宽口径（宁可多重建
            // 一次，也别漏掉定义行的更新）。这一档里两类都要看：
            // 1. 定义类的块无条件算「可能改」（`FootnoteDefinition` 这类块的行内树标题
            //    就是标签本身，既没有 `]:` 也没有 `[^`——容器里的脚注定义是子块，改标签
            //    时只能靠这一条认出来，闸门
            //    `renaming_a_footnote_definition_sub_block_refreshes_the_registry`）；
            // 2. 行内树片段里出现 `[` 或 `<`。
            return block.read_with(cx, |block, _cx| {
                if matches!(
                    block.kind(),
                    BlockKind::RawMarkdown
                        | BlockKind::FrontMatter
                        | BlockKind::HtmlBlock
                        | BlockKind::FootnoteDefinition
                        | BlockKind::Table
                ) {
                    return true;
                }
                block.record.title.fragments.iter().any(|fragment| {
                    fragment.link.is_some()
                        || fragment.footnote.is_some()
                        || fragment.text.contains('[')
                        || fragment.text.contains('<')
                })
            });
        };
        contains_reference_definition_marker(&source)
    }

    pub(super) fn changed_block_needs_runtime_context_refresh(
        &self,
        block: &Entity<Block>,
        cx: &App,
    ) -> bool {
        // P5：源码直编文档（.log/.lock/.toml 等整文件等宽编辑）没有
        // markdown 引用语义，编辑后无需全文档重建图片/链接/脚注注册表
        // ——此前含 `[`/`<` 的日志文本会让每次按键都触发 O(文档) 重建。
        if self.code_document {
            return false;
        }
        self.runtime_context_sensitive_blocks
            .contains(&block.entity_id())
            || self.block_may_change_document_references(block, cx)
    }

    pub(super) fn current_edit_target_entity_id_from_state(&self, cx: &App) -> Option<EntityId> {
        if self.image_preview.is_some() {
            return None;
        }
        self.active_entity_id
            .filter(|entity_id| self.focusable_entity_by_id(*entity_id).is_some())
            .or_else(|| {
                self.pending_focus
                    .filter(|entity_id| self.focusable_entity_by_id(*entity_id).is_some())
            })
            .or_else(|| self.first_focusable_entity_id(cx))
    }

    pub(super) fn current_edit_target_from_state(&self, cx: &App) -> Option<Entity<Block>> {
        self.current_edit_target_entity_id_from_state(cx)
            .and_then(|entity_id| self.focusable_entity_by_id(entity_id))
    }

    fn end_block_pointer_selection_sessions_inner(
        &mut self,
        cx: &mut Context<Self>,
        notify: bool,
    ) -> bool {
        let mut changed = false;

        if let Some(target) = self.current_edit_target_from_state(cx) {
            target.update(cx, |block, _cx| {
                changed |= block.end_pointer_selection_session();
            });
        }

        for visible in self.document.visible_blocks().to_vec() {
            visible.entity.update(cx, |block, _cx| {
                changed |= block.end_pointer_selection_session();
            });
        }

        // Collect only the cell Entity handles, not the whole TableCellBinding
        // (3-field struct of Entity<Block> + TableCellPosition). The collect()
        // exists to drop the &self borrow before the .update() loop; cloning
        // an Entity is an Arc bump, so we pay that once per cell either way —
        // skipping the surrounding struct clone makes the per-frame work
        // proportional to "cell count" not "binding count + position copy".
        let cells: Vec<Entity<Block>> = self
            .table_cells
            .values()
            .map(|binding| binding.cell.clone())
            .collect();
        for cell in cells {
            cell.update(cx, |block, _cx| {
                changed |= block.end_pointer_selection_session();
            });
        }

        if changed && notify {
            cx.notify();
        }
        changed
    }

    pub(super) fn end_block_pointer_selection_sessions(&mut self, cx: &mut Context<Self>) -> bool {
        self.end_block_pointer_selection_sessions_inner(cx, true)
    }

    /// Creates a new block entity and subscribes this editor to its
    /// [`BlockEvent`](crate::components::BlockEvent) stream.
    pub(super) fn new_block(cx: &mut Context<Self>, record: BlockRecord) -> Entity<Block> {
        let block = cx.new(|cx| Block::with_record(cx, record));
        cx.subscribe(&block, Self::on_block_event).detach();
        block
    }

    pub(super) fn new_table_cell_block(
        cx: &mut Context<Self>,
        title: InlineTextTree,
        position: TableCellPosition,
        alignment: TableColumnAlignment,
        reading_mode: Option<std::rc::Rc<std::cell::Cell<bool>>>,
    ) -> Entity<Block> {
        let block = Self::new_block(cx, BlockRecord::new(BlockKind::Paragraph, title));
        block.update(cx, |block, _cx| {
            block.reading_mode = reading_mode;
            block.set_table_cell_mode(position, alignment);
        });
        block
    }

    /// 相对链接与图片目标的解析基准，只有这一处：已落盘文档按其父目录，未落盘按
    /// 工作区根，两者都没有才返回 `None`（交各消费方自己的回退）。以前图片那路落
    /// 进程 CWD（`image_base_dir`）、链接那路落 `PathBuf::from(".")`，未落盘文档里
    /// 的相对引用解析到与工作区无关的位置，点链接静默无反应、图加载不出。
    pub(super) fn local_link_base_dir(&self) -> Option<PathBuf> {
        self.file_path
            .as_ref()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .or_else(|| self.workspace.root.clone())
    }

    pub(super) fn sync_runtime_context_for_block(
        &self,
        block: &Entity<Block>,
        base_dir: Option<&Path>,
        cx: &mut Context<Self>,
    ) {
        let next_base_dir = base_dir.map(Path::to_path_buf);
        let image_reference_definitions = self.image_reference_definitions.clone();
        let link_reference_definitions = self.link_reference_definitions.clone();
        let footnote_registry = self.footnote_registry.clone();
        let document_selection_active = self.document_selection_active.clone();
        let reading_mode = self.reading_mode.clone();
        block.update(cx, move |block, cx| {
            block.document_selection_active = Some(document_selection_active.clone());
            block.reading_mode = Some(reading_mode.clone());
            block.set_runtime_context(
                next_base_dir.clone(),
                image_reference_definitions.clone(),
                link_reference_definitions.clone(),
                footnote_registry.clone(),
            );
            cx.notify();
        });
    }

    fn editable_blocks_in_document_order(&self, cx: &App) -> Vec<Entity<Block>> {
        let mut blocks = Vec::new();
        for visible in self.document.visible_blocks() {
            blocks.push(visible.entity.clone());
            let block = visible.entity.read(cx);
            if block.kind() == BlockKind::Table && let Some(runtime) = &block.table_runtime {
                blocks.extend(runtime.header.iter().cloned());
                blocks.extend(runtime.rows.iter().flatten().cloned());
            }
        }
        blocks
    }

    pub(super) fn rebuild_footnote_registry(&mut self, cx: &App) {
        let mut definitions = HashMap::new();
        let entities = self.editable_blocks_in_document_order(cx);
        for entity in &entities {
            let block = entity.read(cx);
            if block.kind() != BlockKind::FootnoteDefinition {
                continue;
            }

            let allow_definition = self
                .document
                .find_block_location(entity.entity_id())
                .is_some_and(|location| {
                    location.parent.is_none()
                        || location
                            .parent
                            .as_ref()
                            .is_some_and(|parent| parent.read(cx).kind().is_quote_container())
                });
            if !allow_definition {
                continue;
            }

            definitions
                .entry(block.record.title.visible_text().to_string())
                .or_insert(entity.entity_id());
        }

        let mut bindings = HashMap::<String, FootnoteDefinitionBinding>::new();
        for (id, entity_id) in definitions {
            bindings.insert(
                id,
                FootnoteDefinitionBinding {
                    ordinal: None,
                    definition_entity_id: entity_id,
                    first_reference: None,
                },
            );
        }

        let mut next_ordinal = 1usize;
        let mut occurrence_index = 0usize;
        let mut block_occurrences = HashMap::<uuid::Uuid, Vec<FootnoteResolvedOccurrence>>::new();
        for entity in entities {
            let block = entity.read(cx);
            let block_id = block.record.id;
            for fragment in &block.record.title.fragments {
                let Some(footnote) = fragment.footnote.as_ref() else {
                    continue;
                };
                let ordinal = if let Some(binding) = bindings.get_mut(&footnote.id) {
                    if binding.ordinal.is_none() {
                        binding.ordinal = Some(next_ordinal);
                        next_ordinal += 1;
                    }
                    if binding.first_reference.is_none() {
                        binding.first_reference = Some(FootnoteReferenceLocation {
                            entity_id: entity.entity_id(),
                            occurrence_index,
                        });
                    }
                    binding.ordinal
                } else {
                    None
                };
                block_occurrences
                    .entry(block_id)
                    .or_default()
                    .push(FootnoteResolvedOccurrence {
                        id: footnote.id.clone(),
                        ordinal,
                        occurrence_index,
                    });
                if ordinal.is_none() {
                    occurrence_index += 1;
                    continue;
                }
                occurrence_index += 1;
            }
        }

        self.footnote_registry = Arc::new(FootnoteRegistry {
            bindings,
            block_occurrences,
        });
    }

    pub(super) fn rebuild_image_runtimes(&mut self, cx: &mut Context<Self>) {
        self.reference_registry_rebuilds
            .set(self.reference_registry_rebuilds.get() + 1);
        let base_dir = self.local_link_base_dir();
        // 引用定义按**文件**算：保存出去的是缓冲区，重新打开时解析器读到的就是这几
        // 个字节，渲染时必须用同一份文本。块树的序列化不保证这一点——列表项里的定义
        // `- [logo]: /a.png` 会被写成 `- [logo][logo]: /a.png`。顺带这也把一整篇
        // 序列化从每次结构变更的关键路径上摘掉了。
        let markdown = self.buffer.text();
        self.image_reference_definitions = Arc::new(parse_image_reference_definitions(&markdown));
        self.link_reference_definitions = Arc::new(parse_link_reference_definitions(&markdown));
        self.rebuild_footnote_registry(cx);
        self.runtime_context_sensitive_blocks.clear();
        for entity in self.editable_blocks_in_document_order(cx) {
            let is_sensitive = self.block_may_change_document_references(&entity, cx);
            if is_sensitive {
                self.runtime_context_sensitive_blocks
                    .insert(entity.entity_id());
            }
            self.sync_runtime_context_for_block(&entity, base_dir.as_deref(), cx);
        }
    }

    pub(super) fn focusable_entity_by_id(&self, entity_id: EntityId) -> Option<Entity<Block>> {
        self.document.block_entity_by_id(entity_id).or_else(|| {
            self.table_cells
                .get(&entity_id)
                .map(|binding| binding.cell.clone())
        })
    }

    pub(super) fn first_focusable_entity_id(&self, cx: &App) -> Option<EntityId> {
        let first_root = self.document.first_root()?.clone();
        if first_root.read(cx).kind() == BlockKind::Table {
            return first_root
                .read(cx)
                .table_runtime
                .as_ref()
                .and_then(|runtime| runtime.header.first())
                .map(|cell| cell.entity_id())
                .or_else(|| Some(first_root.entity_id()));
        }
        Some(first_root.entity_id())
    }

    pub(super) fn focused_edit_target_entity_id(
        &self,
        window: &Window,
        cx: &App,
    ) -> Option<EntityId> {
        if self.image_preview.is_some() {
            return None;
        }
        self.document
            .focused_block_entity_id(window, cx)
            .or_else(|| {
                self.table_cells
                    .values()
                    .find(|binding| binding.cell.read(cx).focus_handle.is_focused(window))
                    .map(|binding| binding.cell.entity_id())
            })
    }

    /// 窗口焦点此刻是不是落在某一个编辑目标（正文块或表格单元格）上。
    ///
    /// 命令面板与快速打开的输入框会把焦点借走，块那一层的处理者就此不在派发路径里。
    /// 实测：面板里回车执行时当场派发，动作只走到文档根那一条链（编辑器层的捕获
    /// 处理者收得到，块收不到，缓冲区不变）；等一次重绘之后再对同一块派发，块的处理者
    /// 才触发。所以借走焦点的这两个入口，命令要在编辑器层收掉。
    pub(super) fn block_focus_is_live(&self, window: &Window, cx: &App) -> bool {
        self.focused_edit_target_entity_id(window, cx).is_some()
    }

    pub(super) fn focused_edit_target(&self, window: &Window, cx: &App) -> Option<Entity<Block>> {
        self.focused_edit_target_entity_id(window, cx)
            .and_then(|entity_id| self.focusable_entity_by_id(entity_id))
    }

    pub(super) fn table_cell_binding(&self, entity_id: EntityId) -> Option<TableCellBinding> {
        self.table_cells.get(&entity_id).cloned()
    }

    pub(super) fn table_block_by_id(&self, entity_id: EntityId, cx: &App) -> Option<Entity<Block>> {
        self.document
            .block_entity_by_id(entity_id)
            .filter(|block| block.read(cx).kind() == BlockKind::Table)
    }
}

/// 源码字节里有没有可能构成引用定义或脚注引用的记号。
///
/// 两条对应解析器与脚注注册表真正会读的东西：定义行必含 `]:`（label 的右括号紧跟
/// 冒号），脚注引用必含 `[^`。围栏或 HTML 块里出现的这些字面量会被算进来——那是
/// **故意**的过近似：代价是多重建一次，换来的是不必在这里重放解析器的跨行状态
/// （围栏、HTML 注释、容器前缀）。判据只有这一处，解析器那边是真正认定定义的地方。
fn contains_reference_definition_marker(source: &str) -> bool {
    source.contains("]:") || source.contains("[^")
}
