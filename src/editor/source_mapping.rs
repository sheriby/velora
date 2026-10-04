//! Source-offset mapping between canonical Markdown and rendered blocks.

use std::ops::Range;

use super::table_edit::cell_content_range_in_line;
use super::*;

impl Editor {
    /// 读取侧（搜索、大纲、状态栏、跳转）看到的文档文本。
    ///
    /// 就是缓冲区里的那份——也就是文件里的那份。以前这里重新序列化块树，
    /// 于是搜索命中的行号与字节区间说的是「模型眼里的文档」，用户看到的
    /// 却是磁盘上的文件：Setext 标题少一行、表格列宽被重新填充，两边就对不上。
    /// 偏移也不再靠反推，块的位置由 `source_span` 说了算。
    pub(super) fn current_document_source(&self, _cx: &App) -> String {
        self.buffer.text()
    }

    pub(super) fn is_empty_paragraph_separator(block: &Block) -> bool {
        block.kind() == BlockKind::Paragraph
            && block.record.title.visible_text().is_empty()
            && block.children.is_empty()
    }

    pub(super) fn is_empty_root_paragraph(block: &Block) -> bool {
        Self::is_empty_paragraph_separator(block)
    }

    pub(super) fn build_prefixed_content_mapping(
        content: &str,
        first_prefix: &str,
        continuation_prefix: &str,
    ) -> (String, Vec<usize>, Vec<usize>) {
        let mut full = String::new();
        let mut content_to_source = vec![0; content.len() + 1];
        let mut source_to_content = vec![0];

        full.push_str(first_prefix);
        source_to_content.resize(full.len() + 1, 0);

        let mut content_offset = 0usize;
        while content_offset < content.len() {
            content_to_source[content_offset] = full.len();
            let ch = content[content_offset..]
                .chars()
                .next()
                .expect("content offset should stay on char boundaries");
            let start = full.len();
            full.push(ch);
            source_to_content.resize(full.len() + 1, content_offset);
            for index in start..=full.len() {
                source_to_content[index] = content_offset;
            }
            content_offset += ch.len_utf8();
            if ch == '\n' {
                let prefix_start = full.len();
                full.push_str(continuation_prefix);
                source_to_content.resize(full.len() + 1, content_offset);
                for index in prefix_start..=full.len() {
                    source_to_content[index] = content_offset;
                }
            }
        }
        content_to_source[content.len()] = full.len();
        source_to_content[full.len()] = content.len();

        (full, content_to_source, source_to_content)
    }

    pub(super) fn build_code_block_content_mapping(
        content: &str,
        indentation: &str,
        language: Option<&SharedString>,
    ) -> (String, Vec<usize>, Vec<usize>) {
        let fence = self::persistence::safe_code_fence_with_info(
            content,
            language.map(|language| language.as_ref()),
        );
        let mut full = String::new();
        let mut content_to_source = vec![0; content.len() + 1];
        let mut source_to_content = vec![0];

        full.push_str(&fence);
        if let Some(language) = language {
            full.push_str(language);
        }
        full.push('\n');
        source_to_content.resize(full.len() + 1, 0);

        let prefix_start = full.len();
        full.push_str(indentation);
        source_to_content.resize(full.len() + 1, 0);
        for index in prefix_start..=full.len() {
            source_to_content[index] = 0;
        }

        let mut content_offset = 0usize;
        while content_offset < content.len() {
            content_to_source[content_offset] = full.len();
            let ch = content[content_offset..]
                .chars()
                .next()
                .expect("content offset should stay on char boundaries");
            let start = full.len();
            full.push(ch);
            source_to_content.resize(full.len() + 1, content_offset);
            for index in start..=full.len() {
                source_to_content[index] = content_offset;
            }
            content_offset += ch.len_utf8();
            if ch == '\n' {
                let line_prefix_start = full.len();
                full.push_str(indentation);
                source_to_content.resize(full.len() + 1, content_offset);
                for index in line_prefix_start..=full.len() {
                    source_to_content[index] = content_offset;
                }
            }
        }
        content_to_source[content.len()] = full.len();
        source_to_content[full.len()] = content.len();

        full.push('\n');
        source_to_content.resize(full.len() + 1, content.len());
        full.push_str(&fence);
        source_to_content.resize(full.len() + 1, content.len());
        source_to_content[full.len()] = content.len();

        (full, content_to_source, source_to_content)
    }

    pub(super) fn push_inline_block_mapping(
        &self,
        block: &Entity<Block>,
        content_markdown: String,
        first_prefix: String,
        continuation_prefix: String,
        quote_depth: usize,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
    ) -> usize {
        let (full_text, content_to_source, source_to_content) =
            Self::build_prefixed_content_mapping(
                &content_markdown,
                &first_prefix,
                &continuation_prefix,
            );
        let (full_text, content_to_source, source_to_content) =
            Self::wrap_source_mapping_with_quotes(
                full_text,
                content_to_source,
                source_to_content,
                quote_depth,
            );
        mappings.push(SourceTargetMapping {
            entity: block.clone(),
            full_source_range: absolute_start..absolute_start + full_text.len(),
            content_to_source,
            source_to_content,
        });
        full_text.len()
    }

    pub(super) fn push_footnote_definition_head_mapping(
        block: &Entity<Block>,
        footnote_id: &str,
        include_trailing_space: bool,
        quote_depth: usize,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
    ) -> usize {
        let mut full_text = format!("[^{footnote_id}]:");
        if include_trailing_space {
            full_text.push(' ');
        }

        let mut content_to_source = vec![0; footnote_id.len() + 1];
        let mut source_to_content = vec![0; full_text.len() + 1];
        let id_start = 2usize;
        for offset in 0..=footnote_id.len() {
            content_to_source[offset] = id_start + offset;
        }
        for source_offset in 0..=full_text.len() {
            source_to_content[source_offset] = if source_offset <= id_start {
                0
            } else if source_offset >= id_start + footnote_id.len() {
                footnote_id.len()
            } else {
                source_offset - id_start
            };
        }

        let (full_text, content_to_source, source_to_content) =
            Self::wrap_source_mapping_with_quotes(
                full_text,
                content_to_source,
                source_to_content,
                quote_depth,
            );
        mappings.push(SourceTargetMapping {
            entity: block.clone(),
            full_source_range: absolute_start..absolute_start + full_text.len(),
            content_to_source,
            source_to_content,
        });
        full_text.len()
    }

    pub(super) fn push_raw_block_mapping(
        &self,
        block: &Entity<Block>,
        quote_depth: usize,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
        cx: &App,
    ) -> usize {
        let (content, indentation) = {
            let block_ref = block.read(cx);
            (
                block_ref.display_text().to_string(),
                if block_ref.render_depth == 0 {
                    String::new()
                } else {
                    "  ".repeat(block_ref.render_depth)
                },
            )
        };
        let (full_text, content_to_source, source_to_content) =
            Self::build_prefixed_content_mapping(&content, &indentation, &indentation);
        let (full_text, content_to_source, source_to_content) =
            Self::wrap_source_mapping_with_quotes(
                full_text,
                content_to_source,
                source_to_content,
                quote_depth,
            );
        mappings.push(SourceTargetMapping {
            entity: block.clone(),
            full_source_range: absolute_start..absolute_start + full_text.len(),
            content_to_source,
            source_to_content,
        });
        full_text.len()
    }

    pub(super) fn push_code_block_mapping(
        &self,
        block: &Entity<Block>,
        quote_depth: usize,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
        cx: &App,
    ) -> usize {
        let (language, indentation, content) = {
            let block_ref = block.read(cx);
            (
                match block_ref.kind() {
                    BlockKind::CodeBlock { language } => language.clone(),
                    _ => None,
                },
                "  ".repeat(block_ref.render_depth),
                block_ref.display_text().to_string(),
            )
        };

        let (full_text, content_to_source, source_to_content) =
            Self::build_code_block_content_mapping(&content, &indentation, language.as_ref());
        let (full_text, content_to_source, source_to_content) =
            Self::wrap_source_mapping_with_quotes(
                full_text,
                content_to_source,
                source_to_content,
                quote_depth,
            );
        mappings.push(SourceTargetMapping {
            entity: block.clone(),
            full_source_range: absolute_start..absolute_start + full_text.len(),
            content_to_source,
            source_to_content,
        });
        full_text.len()
    }

    pub(super) fn wrap_source_mapping_with_quotes(
        mut full_text: String,
        mut content_to_source: Vec<usize>,
        mut source_to_content: Vec<usize>,
        quote_depth: usize,
    ) -> (String, Vec<usize>, Vec<usize>) {
        for _ in 0..quote_depth {
            let (wrapped_text, inner_to_wrapped, wrapped_to_inner) =
                Self::build_prefixed_content_mapping(&full_text, "> ", "> ");
            let max_inner_to_wrapped = inner_to_wrapped.len().saturating_sub(1);
            let max_source_to_content = source_to_content.len().saturating_sub(1);

            let wrapped_content_to_source = content_to_source
                .iter()
                .map(|offset| inner_to_wrapped[(*offset).min(max_inner_to_wrapped)])
                .collect::<Vec<_>>();
            let wrapped_source_to_content = wrapped_to_inner
                .iter()
                .map(|offset| source_to_content[(*offset).min(max_source_to_content)])
                .collect::<Vec<_>>();

            full_text = wrapped_text;
            content_to_source = wrapped_content_to_source;
            source_to_content = wrapped_source_to_content;
        }

        (full_text, content_to_source, source_to_content)
    }

    pub(super) fn push_table_mappings(
        &self,
        block: &Entity<Block>,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
        cx: &App,
    ) -> usize {
        let block_ref = block.read(cx);
        let (Some(_table), Some(runtime)) = (
            block_ref.record.table.clone(),
            block_ref.table_runtime.clone(),
        ) else {
            return 0;
        };
        let Some(span) = block_ref.record.source_span.clone() else {
            // 挂在容器里的表格没有自己的源码区间（区间只挂在根块上），那就到**所在根块**
            // 的原文里量同一套格子。
            return self.push_table_mappings_in_root(block, absolute_start, mappings, cx);
        };

        // 单元格的位置按结构从缓冲区里这张表的原文量（第几行第几列，夹在管道符之间），
        // 既不按「列宽 = 内容长 + 3」猜，也不拿这一格序列化出来的文字回原文里搜。
        let raw = self.buffer.slice(span.clone());
        let mut raw_lines = raw.split('\n');
        let mut line_start = span.start;

        if let Some(header_line) = raw_lines.next() {
            self.push_table_row_mappings(header_line, line_start, &runtime.header, mappings);
            line_start += header_line.len() + 1;
        }
        if let Some(separator_line) = raw_lines.next() {
            line_start += separator_line.len() + 1;
        }
        for row_cells in runtime.rows.iter() {
            let Some(row_line) = raw_lines.next() else { break };
            self.push_table_row_mappings(row_line, line_start, row_cells, mappings);
            line_start += row_line.len() + 1;
        }

        span.len()
    }

    /// 把一行表格里的每个单元格映射到它在缓冲区原文里的字节区间。
    ///
    /// 按**结构**量：第几列就从原文的管道符之间夹第几段（与写回同一把尺）。以前是拿
    /// 这一格序列化出来的文字回原文里搜——空格子序列化出空串，于是这一格压根没有映射
    /// （光标停在空格里时 `caret_source_offset` 算不出，粘贴/跳转/行列号只能退回默认），
    /// 写法与序列化口径不一致时同样是静默失配。
    fn push_table_row_mappings(
        &self,
        row_line: &str,
        line_start: usize,
        cells: &[Entity<Block>],
        mappings: &mut Vec<SourceTargetMapping>,
    ) {
        // 挂在容器里的行写着 `> | 甲 | 乙 |`：容器记号不是第一列。
        let prefix = table_row_container_prefix(row_line);
        let core = &row_line[prefix..];
        for (column, cell) in cells.iter().enumerate() {
            let Some(range) = cell_content_range_in_line(core, line_start + prefix, column) else {
                continue;
            };
            let len = range.len();
            mappings.push(SourceTargetMapping {
                entity: cell.clone(),
                full_source_range: range,
                content_to_source: (0..=len).collect(),
                source_to_content: (0..=len).collect(),
            });
        }
    }

    /// 容器里的表格（挂在引用/列表根块下，没有自己的源码区间）的映射：在它**所在根块**的
    /// 原文里先按表头那一行定位这张表，再把每行的格子按原文量出来。
    ///
    /// 不按「列宽 = 内容长 + 3」推算——那条口径在用户填过宽度的列上会漂（实测漂 3 字节，
    /// 命中选中的是 `" | 苹"` 而不是 `苹果`）。同一个根块里有几张表头相同的表时，取离本块
    /// 在走树时算出的起点最近的那一张。定位不到表头、行数与模型对不上，就什么都不推，命中
    /// 退回宿主表格块。
    fn push_table_mappings_in_root(
        &self,
        block: &Entity<Block>,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
        cx: &App,
    ) -> usize {
        let (Some(table), Some(runtime)) = (
            block.read(cx).record.table.clone(),
            block.read(cx).table_runtime.clone(),
        ) else {
            return 0;
        };
        let Some(root) = self.document.root_ancestor_of(block.entity_id()) else {
            return 0;
        };
        let Some(root_span) = root.read(cx).record.source_span.clone() else {
            return 0;
        };
        let header = table
            .header
            .iter()
            .map(serialize_table_cell_markdown)
            .collect::<Vec<_>>();
        let mut outer_header = vec![String::new()];
        outer_header.extend(header.iter().cloned());
        outer_header.push(String::new());

        let raw = self.buffer.slice(root_span.clone());
        let mut lines: Vec<(usize, String)> = Vec::new();
        let mut line_start = root_span.start;
        for line in raw.split('\n') {
            lines.push((line_start, line.to_string()));
            line_start += line.len() + 1;
        }
        let mut candidates = lines.iter().enumerate().filter_map(|(index, (start, line))| {
            let cells = table_row_cells(line);
            (cells == header || cells == outer_header).then_some((index, *start))
        });
        let (header_index, header_start) = match candidates.next() {
            Some(first) => candidates
                .fold(first, |best, candidate| {
                    if (candidate.1 as i64 - absolute_start as i64).abs()
                        < (best.1 as i64 - absolute_start as i64).abs()
                    {
                        candidate
                    } else {
                        best
                    }
                },
                ),
            None => return 0,
        };
        // 表头之后是分隔行，再往后才是数据行；行数与模型对不上就什么都不推。
        let body_start = header_index + 2;
        if lines.len() < body_start + table.rows.len() {
            return 0;
        }
        self.push_table_row_mappings(
            &lines[header_index].1,
            header_start,
            &runtime.header,
            mappings,
        );
        for (row_index, row) in table.rows.iter().enumerate() {
            let Some(cells) = runtime.rows.get(row_index) else {
                continue;
            };
            let (start, line) = &lines[body_start + row_index];
            self.push_table_row_mappings(line, *start, cells, mappings);
        }
        let last_index = if table.rows.is_empty() {
            header_index + 1
        } else {
            body_start + table.rows.len() - 1
        }
        .min(lines.len() - 1);
        (lines[last_index].0 + lines[last_index].1.len()) - header_start
    }

    pub(super) fn collect_single_block_source_mappings(
        &self,
        block: &Entity<Block>,
        list_depth: usize,
        quote_depth: usize,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
        block_ranges: &mut HashMap<EntityId, Range<usize>>,
        cx: &App,
    ) -> usize {
        let (kind, list_ordinal, title, children) = {
            let block_ref = block.read(cx);
            let kind = block_ref.kind();
            let title = (!matches!(
                kind,
                BlockKind::Table
                    | BlockKind::CodeBlock { .. }
                    | BlockKind::Comment
                    | BlockKind::HtmlBlock
                    | BlockKind::MathBlock
                    | BlockKind::MermaidBlock
                    | BlockKind::RawMarkdown
                    | BlockKind::FrontMatter
                    | BlockKind::Separator
            ))
            .then(|| block_ref.record.title.markdown_offset_map());
            (
                kind,
                block_ref.list_ordinal,
                title,
                block_ref.children.clone(),
            )
        };

        let own_len = match kind {
            BlockKind::Table => self.push_table_mappings(
                block,
                absolute_start,
                mappings,
                cx,
            ),
            BlockKind::CodeBlock { .. } => {
                self.push_code_block_mapping(block, quote_depth, absolute_start, mappings, cx)
            }
            BlockKind::RawMarkdown
            | BlockKind::FrontMatter
            | BlockKind::Comment
            | BlockKind::HtmlBlock
            | BlockKind::MathBlock
            | BlockKind::MermaidBlock => {
                self.push_raw_block_mapping(block, quote_depth, absolute_start, mappings, cx)
            }
            BlockKind::Separator => {
                let line = block
                    .read(cx)
                    .record
                    .markdown_line(list_depth, list_ordinal);
                if quote_depth == 0 {
                    line.len()
                } else {
                    Self::wrap_source_mapping_with_quotes(
                        line.clone(),
                        (0..=line.len()).collect(),
                        (0..=line.len()).collect(),
                        quote_depth,
                    )
                    .0
                    .len()
                }
            }
            BlockKind::Heading { level } => self.push_inline_block_mapping(
                block,
                title.expect("heading title").markdown().to_string(),
                format!("{}{} ", "  ".repeat(list_depth), "#".repeat(level as usize)),
                String::new(),
                quote_depth,
                absolute_start,
                mappings,
            ),
            BlockKind::Paragraph => {
                let indentation = "  ".repeat(list_depth);
                self.push_inline_block_mapping(
                    block,
                    title.expect("paragraph title").markdown().to_string(),
                    indentation.clone(),
                    indentation,
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
            BlockKind::BulletedListItem => {
                let indentation = "  ".repeat(list_depth);
                self.push_inline_block_mapping(
                    block,
                    title.expect("bullet title").markdown().to_string(),
                    format!("{indentation}- "),
                    format!("{indentation}  "),
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
            BlockKind::TaskListItem { checked } => {
                let indentation = "  ".repeat(list_depth);
                self.push_inline_block_mapping(
                    block,
                    title.expect("task title").markdown().to_string(),
                    format!("{indentation}- [{}] ", if checked { "x" } else { " " }),
                    format!("{indentation}      "),
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
            BlockKind::NumberedListItem => {
                let indentation = "  ".repeat(list_depth);
                let ordinal = list_ordinal.unwrap_or(1);
                self.push_inline_block_mapping(
                    block,
                    title.expect("numbered title").markdown().to_string(),
                    format!("{indentation}{ordinal}. "),
                    format!("{indentation}   "),
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
            BlockKind::Quote => {
                let title = title.expect("quote title").markdown().to_string();
                if title.is_empty() && !children.is_empty() {
                    0
                } else {
                    self.push_inline_block_mapping(
                        block,
                        title,
                        String::new(),
                        String::new(),
                        quote_depth + 1,
                        absolute_start,
                        mappings,
                    )
                }
            }
            BlockKind::Callout(variant) => {
                let title_markdown = title.expect("callout title").markdown().to_string();
                if title_markdown.is_empty() {
                    let full_text = Self::wrap_source_mapping_with_quotes(
                        format!("[!{}]", variant.marker()),
                        vec![0],
                        vec![0; format!("[!{}]", variant.marker()).len() + 1],
                        quote_depth + 1,
                    )
                    .0;
                    mappings.push(SourceTargetMapping {
                        entity: block.clone(),
                        full_source_range: absolute_start..absolute_start + full_text.len(),
                        content_to_source: vec![full_text.len()],
                        source_to_content: vec![0; full_text.len() + 1],
                    });
                    full_text.len()
                } else {
                    self.push_inline_block_mapping(
                        block,
                        title_markdown,
                        format!("[!{}] ", variant.marker()),
                        String::new(),
                        quote_depth + 1,
                        absolute_start,
                        mappings,
                    )
                }
            }
            BlockKind::FootnoteDefinition => {
                let footnote_id = title.expect("footnote id").markdown().to_string();
                let first_child = children.first().cloned();
                let first_is_paragraph = first_child
                    .as_ref()
                    .is_some_and(|child| child.read(cx).kind() == BlockKind::Paragraph);
                Self::push_footnote_definition_head_mapping(
                    block,
                    &footnote_id,
                    first_is_paragraph,
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
        };

        if kind == BlockKind::FootnoteDefinition {
            let mut total_len = own_len;
            let mut child_index = 0usize;
            if let Some(first_child) = children.first()
                && first_child.read(cx).kind() == BlockKind::Paragraph
            {
                total_len = self.push_inline_block_mapping(
                    first_child,
                    first_child
                        .read(cx)
                        .record
                        .title
                        .markdown_offset_map()
                        .markdown()
                        .to_string(),
                    block
                        .read(cx)
                        .footnote_definition_id()
                        .map(|id| format!("[^{id}]: "))
                        .unwrap_or_else(|| "[^]: ".to_string()),
                    "    ".to_string(),
                    quote_depth,
                    absolute_start,
                    mappings,
                );
                child_index = 1;
            }

            let mut previous_kind = if child_index > 0 {
                Some(BlockKind::Paragraph)
            } else {
                None
            };
            for child in children.iter().skip(child_index) {
                let current_kind = child.read(cx).kind();
                if total_len > 0 {
                    total_len += if previous_kind.is_none() {
                        1
                    } else if previous_kind.as_ref().is_some_and(|previous| {
                        previous.is_list_item() && current_kind.is_list_item()
                    }) {
                        1
                    } else {
                        2
                    };
                }
                total_len += self.collect_single_block_source_mappings(
                    child,
                    2,
                    quote_depth,
                    absolute_start + total_len,
                    mappings,
                    block_ranges,
                    cx,
                );
                previous_kind = Some(current_kind);
            }
            block_ranges.insert(
                block.entity_id(),
                absolute_start..absolute_start + total_len,
            );
            return total_len;
        }

        let child_list_depth = list_depth + usize::from(kind.is_list_item());
        let child_quote_depth = quote_depth + usize::from(kind.is_quote_container());
        let mut total_len = own_len;
        for child in children {
            if total_len > 0 {
                total_len += 1;
            }
            total_len += self.collect_single_block_source_mappings(
                &child,
                child_list_depth,
                child_quote_depth,
                absolute_start + total_len,
                mappings,
                block_ranges,
                cx,
            );
        }

        block_ranges.insert(
            block.entity_id(),
            absolute_start..absolute_start + total_len,
        );
        total_len
    }

    pub(super) fn build_source_target_mappings(&self, cx: &App) -> Vec<SourceTargetMapping> {
        self.build_source_target_mappings_with_block_ranges(cx).0
    }

    /// Like [`Self::build_source_target_mappings`], but also returns the source
    /// span of every block keyed by entity id. Atomic blocks (e.g. tables) have
    /// no per-block text mapping, so this is the only way to recover their full
    /// source extent for selection/deletion.
    pub(super) fn build_source_target_mappings_with_block_ranges(
        &self,
        cx: &App,
    ) -> (Vec<SourceTargetMapping>, HashMap<EntityId, Range<usize>>) {
        self.build_source_target_mappings_until(cx, None)
    }

    /// Finds one caret mapping without visiting roots after the target block.
    pub(super) fn source_mapping_for_entity(
        &self,
        entity_id: EntityId,
        cx: &App,
    ) -> Option<SourceTargetMapping> {
        self.build_source_target_mappings_until(cx, Some(entity_id))
            .0
            .into_iter()
            .find(|mapping| mapping.entity.entity_id() == entity_id)
    }

    fn build_source_target_mappings_until(
        &self,
        cx: &App,
        target: Option<EntityId>,
    ) -> (Vec<SourceTargetMapping>, HashMap<EntityId, Range<usize>>) {
        self.source_mapping_builds
            .set(self.source_mapping_builds.get() + 1);
        if target.is_none() {
            self.source_mapping_full_builds
                .set(self.source_mapping_full_builds.get() + 1);
        }
        let started = std::time::Instant::now();
        let result = self.build_source_target_mappings_inner(cx, target);
        self.source_mapping_nanos.set(
            self.source_mapping_nanos.get()
                + started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        );
        result
    }

    fn build_source_target_mappings_inner(
        &self,
        cx: &App,
        target: Option<EntityId>,
    ) -> (Vec<SourceTargetMapping>, HashMap<EntityId, Range<usize>>) {
        let mut mappings = Vec::new();
        let mut block_ranges = HashMap::new();

        match self.view_mode {
            ViewMode::Rendered => {
                // 锚点就是每个根块在缓冲区里的区间：位置与文本同源，不再按
                // 「块间必有空行」自行记账——那条路在非规范输入（相邻根块、
                // 编辑后状态）会累积漂移，甚至切进多字节字符中间直接 coredump
                // （用户报修）。块内部的偏移仍由下面的重建算出，写法不规范的
                // 块（表格列宽填充）会在块内漂几个字节，但绝不会再漂到别的块里。
                let mut next_anchor = 0usize;
                let roots = self.document.root_blocks();
                // 只要某一根的映射时，前面那些根块不必走查：锚点就是它自己的区间，
                // 走查它没有意义，而这一跳正是「打字 = O(这一块)」的关键。
                let start_index = match target {
                    Some(id) => self
                        .document
                        .root_ancestor_of(id)
                        .and_then(|root| {
                            roots.iter().position(|block| block.entity_id() == root.entity_id())
                        })
                        .unwrap_or(0),
                    None => 0,
                };
                if start_index > 0 {
                    next_anchor = roots[start_index - 1]
                        .read(cx)
                        .record
                        .source_span
                        .as_ref()
                        .map(|span| (span.end + 1).min(self.buffer.byte_len()))
                        .unwrap_or(0);
                }
                for block in roots.iter().skip(start_index) {
                    let id = block.entity_id();
                    let Some(span) = block.read(cx).record.source_span.clone() else {
                        // 这个根块还没有区间（刚插进树、尚未写回缓冲区）：给一个
                        // 就近的零宽锚点，跨块选区的端点才解析得出来。真正的区间
                        // 要等写回那一步才有。
                        block_ranges.insert(id, next_anchor..next_anchor);
                        continue;
                    };
                    next_anchor = (span.end + 1).min(self.buffer.byte_len());
                    if Self::is_empty_root_paragraph(block.read(cx)) {
                        // 空根块无文本映射，但要有零宽 span 让跨块选区边界
                        // 能解析（否则删除选区会中止）。
                        block_ranges.insert(id, span.start..span.start);
                        continue;
                    }
                    let prior_mapping_count = mappings.len();
                    self.collect_single_block_source_mappings(
                        block,
                        0,
                        0,
                        span.start,
                        &mut mappings,
                        &mut block_ranges,
                        cx,
                    );
                    // 保险：块内映射钳在本块的区间里并落在字符边界上。锚点已
                    // 精确，但个别块的映射记账长度与缓冲区跨度的任何微小出入
                    // 都不允许变成 panic，也不允许越界吃到邻居的字节。
                    for mapping in &mut mappings[prior_mapping_count..] {
                        let start = mapping.full_source_range.start.min(span.end);
                        let mut end = mapping.full_source_range.end.min(span.end);
                        while end > start && !self.buffer.is_char_boundary(end) {
                            end -= 1;
                        }
                        mapping.full_source_range = start..end;
                    }
                    // 根块的范围就是它在缓冲区里的区间：跨块选区按它取整块，
                    // 不能按重建出来的长度算（写法不规范时两者长度不同）。
                    block_ranges.insert(id, span.clone());
                    if target.is_some_and(|id| {
                        mappings[prior_mapping_count..]
                            .iter()
                            .any(|mapping| mapping.entity.entity_id() == id)
                    }) {
                        break;
                    }
                }
            }
            ViewMode::Source => {
                // 源码模式：块即原始行，保持记账走查；边界钳制防越界。
                let source = self.current_document_source(cx);
                let mut absolute = 0usize;
                for block in self.document.root_blocks() {
                    let is_empty_root = Self::is_empty_root_paragraph(block.read(cx));
                    if is_empty_root {
                        block_ranges.insert(block.entity_id(), absolute..absolute);
                        continue;
                    }
                    let prior_mapping_count = mappings.len();
                    absolute += self.collect_single_block_source_mappings(
                        block,
                        0,
                        0,
                        absolute,
                        &mut mappings,
                        &mut block_ranges,
                        cx,
                    );
                    for mapping in &mut mappings[prior_mapping_count..] {
                        let start = mapping.full_source_range.start;
                        let mut end = mapping.full_source_range.end.min(source.len());
                        while end > start && !source.is_char_boundary(end) {
                            end -= 1;
                        }
                        mapping.full_source_range = start..end;
                    }
                    absolute += 1;
                }
            }
        }

        (mappings, block_ranges)
    }

}

/// 一行里按管道符切出的格文本：先去掉引用前缀 `>` 与缩进，再逐格 trim。
fn table_row_cells(line: &str) -> Vec<String> {
    let mut core = line.trim_start();
    while let Some(rest) = core.strip_prefix('>') {
        core = rest.trim_start();
    }
    core.split('|').map(|part| part.trim().to_string()).collect()
}

/// 一行表格前挂着多少**容器记号**：缩进、引用块的 `>`（可以嵌套），到表格真正开始为止。
///
/// 量格子要从表格里量：`> | 甲 | 乙 |` 的 `> ` 不是第 0 列。
fn table_row_container_prefix(line: &str) -> usize {
    let mut consumed = 0usize;
    let mut rest = line;
    loop {
        let trimmed = rest.trim_start();
        consumed += rest.len() - trimmed.len();
        rest = trimmed;
        match rest.strip_prefix('>') {
            Some(after_marker) => {
                consumed += 1;
                rest = after_marker;
            }
            None => return consumed,
        }
    }
}
