//! Source-offset mapping between canonical Markdown and rendered blocks.

use std::ops::Range;

use super::table_edit::cell_content_range_in_line;
use super::*;

/// 逐行量出来的结果：每行在自己那一行里让开几个前缀字节，以及这一行在**文件里**占几个
/// 字节。后者与模型的 markdown 那行可以不等长（模型为保住一个字面反斜杠会多写一位），
/// 位置换算按文件的长度走，`content_to_source` 说的才是缓冲区的坐标。
pub(super) struct MeasuredBlockLines {
    pub prefixes: Vec<usize>,
    pub file_lens: Vec<usize>,
}

/// 围栏各行在它自己那一行里让开几个字节，按文件量（`measured_code_block_line_prefixes`）。
/// 模型不存围栏行，内容行前面被上级容器吃掉的那几列缩进也不在模型里，而映射的起点
/// 是整行的行首——这几位只能从缓冲区量回来。
pub(super) struct MeasuredFencePrefixes {
    pub open: usize,
    pub lines: Vec<usize>,
    pub close: usize,
}

fn fence_line_width(measured: Option<&MeasuredFencePrefixes>, uniform: usize, index: usize) -> usize {
    match measured {
        Some(prefixes) => prefixes.lines.get(index).copied().unwrap_or(0),
        None => uniform,
    }
}

/// 内容里第几个换行之后（= 第几条内容行，0 基）。
fn full_line_index_of(content: &str, offset: usize) -> usize {
    content[..offset].bytes().filter(|byte| *byte == b'\n').count()
}

fn leading_blank_bytes(line: &str) -> usize {
    line.bytes()
        .take_while(|byte| *byte == b' ' || *byte == b'\t')
        .count()
}

impl Editor {    /// 读取侧（搜索、大纲、状态栏、跳转）看到的文档文本。
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
        measured: Option<&MeasuredFencePrefixes>,
    ) -> (String, Vec<usize>, Vec<usize>) {
        let fence = self::persistence::safe_code_fence_with_info(
            content,
            language.map(|language| language.as_ref()),
        );
        let uniform = indentation.len();
        let mut full = String::new();
        let mut content_to_source = vec![0; content.len() + 1];
        let mut source_to_content = vec![0];

        let open_prefix = measured.map(|prefixes| prefixes.open).unwrap_or(0);
        if open_prefix > 0 {
            full.push_str(&" ".repeat(open_prefix));
            source_to_content.resize(full.len() + 1, 0);
        }

        full.push_str(&fence);
        if let Some(language) = language {
            full.push_str(language);
        }
        full.push('\n');
        source_to_content.resize(full.len() + 1, 0);

        let mut line_index = 0usize;
        let prefix_start = full.len();
        let width = fence_line_width(measured, uniform, line_index);
        if width > 0 {
            full.push_str(&" ".repeat(width));
        }
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
                line_index += 1;
                let line_prefix_start = full.len();
                let width = fence_line_width(measured, uniform, line_index);
                if width > 0 {
                    full.push_str(&" ".repeat(width));
                }
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
        let close_prefix = measured.map(|prefixes| prefixes.close).unwrap_or(0);
        if close_prefix > 0 {
            full.push_str(&" ".repeat(close_prefix));
            source_to_content.resize(full.len() + 1, content.len());
        }
        full.push_str(&fence);
        source_to_content.resize(full.len() + 1, content.len());
        source_to_content[full.len()] = content.len();

        (full, content_to_source, source_to_content)
    }

    /// 每一行按「量出来的前缀 + 这一行在**文件里**的长度」建位置表（多行内容的块用，
    /// `measured_block_line_prefixes`）。
    ///
    /// 内容偏移是模型 markdown 的坐标，落点必须是缓冲区的坐标，所以行内逐位对齐、
    /// 超出文件那一行长度的部分钳在行尾：模型为了保住一个字面反斜杠多写的那一位，
    /// 在文件里根本没有字节可对。以前这里按模型的 markdown 拼整段文本再按位置算，
    /// 于是行一旦带上这种长度差，后面每一行的落点都漂。
    fn build_line_prefixed_content_mapping(
        content: &str,
        line_prefixes: &[usize],
        file_lens: &[usize],
    ) -> (usize, Vec<usize>, Vec<usize>) {
        let model_lines: Vec<&str> = content.split('\n').collect();
        let mut content_to_source = vec![0usize; content.len() + 1];
        let mut total_len = 0usize;
        let mut markdown_offset = 0usize;
        for (index, model_line) in model_lines.iter().enumerate() {
            let prefix = line_prefixes.get(index).copied().unwrap_or(0);
            let file_len = file_lens.get(index).copied().unwrap_or(model_line.len());
            let content_start = total_len + prefix;
            for position in 0..=model_line.len() {
                content_to_source[markdown_offset + position] =
                    content_start + position.min(file_len);
            }
            markdown_offset += model_line.len();
            total_len = content_start + file_len;
            if index + 1 < model_lines.len() {
                content_to_source[markdown_offset] = total_len;
                markdown_offset += 1;
                total_len += 1;
            }
        }

        // 反表：内容偏移落在哪个字节上，那个字节就归它；行首记号与多字节字符内部的
        // 字节归到最近的那个内容起点。
        let mut source_to_content = vec![0usize; total_len + 1];
        for (content_offset, position) in content_to_source.iter().enumerate() {
            source_to_content[*position] = content_offset;
        }
        let mut previous = 0usize;
        for slot in source_to_content.iter_mut() {
            if *slot == 0 {
                *slot = previous;
            } else {
                previous = *slot;
            }
        }

        (total_len, content_to_source, source_to_content)
    }

    fn push_line_prefixed_inline_mapping(
        &self,
        block: &Entity<Block>,
        content_markdown: String,
        measured: &MeasuredBlockLines,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
    ) -> usize {
        let (full_len, content_to_source, source_to_content) =
            Self::build_line_prefixed_content_mapping(
                &content_markdown,
                &measured.prefixes,
                &measured.file_lens,
            );
        mappings.push(SourceTargetMapping {
            entity: block.clone(),
            full_source_range: absolute_start..absolute_start + full_len,
            content_to_source,
            source_to_content,
        });
        full_len
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

    /// 量出来：围栏的开行列、每一条内容行、闭合列各自在自己那一行里让开几个字节。
    ///
    /// 模型里存的是「内容行去掉上级容器吃掉的缩进」那一段，围栏行根本不在模型里，
    /// 而映射的起点是整行的行首。按 `render_depth` 拼「每级两个空格」在缩进两格、
    /// 四格、制表符的围栏里整体漂——实测在 `  ```rust` 围栏里打一个字，字节落在内容行
    /// 的第 0 位；列表项里四格的围栏更是把字写进了父项那一行。
    /// 内容行按「文件那一行以模型存下的这段结尾」来夹，差出来的一截必须是纯空白；
    /// 对不上（引用里的围栏、行内还改了别的、行数不齐）就 `None`，退回按模型拼。
    fn measured_code_block_line_prefixes(
        &self,
        content: &str,
        language: Option<&SharedString>,
        absolute_start: usize,
        quote_depth: usize,
    ) -> Option<MeasuredFencePrefixes> {
        // 引用里的围栏每一行还要让开 `>`，那是另一族账（`wrap_source_mapping_with_quotes`
        // 现在按每级 `> ` 拼），这里先不碰。
        if quote_depth > 0 || !self.buffer.is_char_boundary(absolute_start) {
            return None;
        }
        let fence = persistence::safe_code_fence_with_info(
            content,
            language.map(|language| language.as_ref()),
        );
        let mut line_index = self.buffer.line_of(absolute_start);
        let open_range = self.buffer.line_range(line_index);
        if open_range.start != absolute_start {
            return None;
        }
        let open_line = self.buffer.slice(open_range);
        let open_body = format!("{fence}{}", language.map(|l| l.as_str()).unwrap_or(""));
        let open_prefix = open_line.len().checked_sub(open_body.len())?;
        if !open_line[..open_prefix].chars().all(|ch| ch == ' ' || ch == '\t')
            || !open_line.ends_with(open_body.as_str())
        {
            return None;
        }

        let model_lines: Vec<&str> = if content.is_empty() {
            Vec::new()
        } else {
            content.split('\n').collect()
        };
        line_index += 1;
        let mut lines = Vec::with_capacity(model_lines.len());
        for model_line in &model_lines {
            let range = self.buffer.line_range(line_index);
            if range.start >= self.buffer.byte_len() {
                return None;
            }
            // 只比缩进，不比整行：这一次按键的字节已经进了模型、还没进缓冲区，
            // 要求「文件那行以模型这段结尾」在算光标时就成立、算完就又不成立了。
            let file_line = self.buffer.slice(range);
            let file_indent = leading_blank_bytes(&file_line);
            let model_indent = leading_blank_bytes(model_line);
            if model_indent > file_indent {
                return None;
            }
            lines.push(file_indent - model_indent);
            line_index += 1;
        }

        let close_range = self.buffer.line_range(line_index);
        if close_range.start > self.buffer.byte_len() {
            return None;
        }
        let close_line = self.buffer.slice(close_range);
        let close_prefix = close_line.len().checked_sub(fence.len())?;
        if !close_line.ends_with(fence.as_str())
            || !close_line[..close_prefix].chars().all(|ch| ch == ' ' || ch == '\t')
        {
            return None;
        }
        Some(MeasuredFencePrefixes {
            open: open_prefix,
            lines,
            close: close_prefix,
        })
    }

    /// 量出来：这一段缩进代码块的内容行各自在自己那一行里让开几个字节。
    ///
    /// 缩进代码块（四空格或制表符）在文件里就是那几行内容，没有围栏行；模型存的是
    /// 每行 dedent 之后那一段，dedent 吃掉几位是文件里的事。行数拿本块的 `source_span`
    /// 数——数不齐就说明这一块其实有围栏（围栏那两行不在内容里），交回围栏那条路。
    fn measured_indented_code_line_prefixes(
        &self,
        span: &Range<usize>,
        content: &str,
        absolute_start: usize,
        quote_depth: usize,
    ) -> Option<Vec<usize>> {
        if quote_depth > 0
            || span.start != absolute_start
            || !self.buffer.is_char_boundary(absolute_start)
        {
            return None;
        }
        let mut model_lines: Vec<&str> = content.split('\n').collect();
        if model_lines.last().is_some_and(|line| line.is_empty()) {
            model_lines.pop();
        }
        if model_lines.is_empty() {
            return None;
        }

        let mut line_index = self.buffer.line_of(span.start);
        let mut prefixes = Vec::with_capacity(model_lines.len());
        for model_line in &model_lines {
            let range = self.buffer.line_range(line_index);
            // 内容行必须在块自己的区间里：多一行就说明模型的行数与文件的行数不齐。
            if range.start >= span.end || range.start > self.buffer.byte_len() {
                return None;
            }
            let file_line = self.buffer.slice(range);
            let file_indent = leading_blank_bytes(&file_line);
            let model_indent = leading_blank_bytes(model_line);
            if model_indent > file_indent {
                return None;
            }
            prefixes.push(file_indent - model_indent);
            line_index += 1;
        }
        Some(prefixes)
    }

    /// 缩进代码块的映射：内容行按量出来的缩进起头，前后都不补围栏。
    fn build_indented_code_content_mapping(
        content: &str,
        line_prefixes: &[usize],
    ) -> (String, Vec<usize>, Vec<usize>) {
        let mut full = String::new();
        let mut content_to_source = vec![0; content.len() + 1];
        let mut source_to_content = vec![0usize];

        let push_prefix = |full: &mut String,
                               source_to_content: &mut Vec<usize>,
                               mark: usize,
                               index: usize| {
            let width = line_prefixes.get(index).copied().unwrap_or(0);
            let start = full.len();
            if width > 0 {
                full.push_str(&" ".repeat(width));
            }
            source_to_content.resize(full.len() + 1, mark);
            for position in start..=full.len() {
                source_to_content[position] = mark;
            }
        };

        push_prefix(&mut full, &mut source_to_content, 0, 0);
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
            for position in start..=full.len() {
                source_to_content[position] = content_offset;
            }
            content_offset += ch.len_utf8();
            if ch == '\n' && content_offset < content.len() {
                push_prefix(
                    &mut full,
                    &mut source_to_content,
                    content_offset,
                    full_line_index_of(content, content_offset),
                );
            }
        }
        content_to_source[content.len()] = full.len();
        source_to_content.resize(full.len() + 1, content.len());
        source_to_content[full.len()] = content.len();

        (full, content_to_source, source_to_content)
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

        let measured =
            self.measured_code_block_line_prefixes(&content, language.as_ref(), absolute_start, quote_depth);
        let (full_text, content_to_source, source_to_content) = match measured.as_ref() {
            Some(prefixes) => Self::build_code_block_content_mapping(
                &content,
                &indentation,
                language.as_ref(),
                Some(prefixes),
            ),
            None => {
                // 没有围栏的缩进代码块：文件里就是那几行内容，前后不该补 phantom 围栏行。
                let span = block.read(cx).record.source_span.clone();
                match span
                    .as_ref()
                    .and_then(|span| {
                        self.measured_indented_code_line_prefixes(
                            span,
                            &content,
                            absolute_start,
                            quote_depth,
                        )
                    }) {
                    Some(prefixes) => {
                        Self::build_indented_code_content_mapping(&content, &prefixes)
                    }
                    None => Self::build_code_block_content_mapping(
                        &content,
                        &indentation,
                        language.as_ref(),
                        None,
                    ),
                }
            }
        };
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
        for row_index in 0..table.rows.len() {
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

    /// 量出来：这一块的内容在它**那一行**里从第几个字节开始，顺带量出它的子块
    /// 在该行里要跟着让开几列（本块记号自己的宽度）。
    ///
    /// 逐层用解析器自己的剥记号函数（`strip_one_quote_level`、`parse_list_marker`、
    /// `parse_atx_heading_line_with_marker`、`strip_leading_columns`）把容器记号与本块
    /// 记号连同它们吃掉的空白剥掉，剩下的就是内容起点。按模型拼（列表每级两个空格、
    /// 引用一律 `> `）在缩进四格、制表符、`>引用` 这类写法里会整体漂——块内偏移错位
    /// 之后，打字会落进别的行（实测四空格嵌套列表里打一个字，字节进了父项那一行）。
    /// 引用每层都在自己那一行上，所以每行都重量；列表的上级只留下缩进，交给本块的
    /// 记号（或段落继承来的 `list_dedent`）吃掉。
    /// 这一行与模型对不上（剥不动、起点不在行首）就 `None`，调用方退回按模型拼。
    fn measured_block_prefix(
        &self,
        at: usize,
        quote_depth: usize,
        list_dedent: usize,
        kind: &BlockKind,
    ) -> Option<(usize, usize)> {
        // 起点落在多字节字符中间，说明上游的字节账已经错了，量不出这一行是啥。
        if !self.buffer.is_char_boundary(at) {
            return None;
        }
        let line_range = self.buffer.line_range(self.buffer.line_of(at));
        if line_range.start != at {
            return None;
        }
        let line = self.buffer.slice(line_range);
        let mut consumed = 0usize;
        for _ in 0..quote_depth {
            let stripped = super::document::strip_one_quote_level(&line[consumed..])?;
            consumed += line[consumed..].len() - stripped.len();
        }
        let rest = line[consumed..].to_string();
        let mut child_dedent = list_dedent;
        match kind {
            BlockKind::Quote => {
                let stripped = super::document::strip_one_quote_level(&rest)?;
                consumed += rest.len() - stripped.len();
            }
            BlockKind::BulletedListItem
            | BlockKind::TaskListItem { .. }
            | BlockKind::NumberedListItem => {
                let marker = super::document::parse_list_marker(&rest)?;
                consumed += rest.len() - marker.text.len();
                child_dedent = rest.len() - marker.text.len();
            }
            BlockKind::Heading { .. } => {
                // Setext 标题的内容行没有记号：导入把整行（连前导空白）当内容收，
                // 所以量到 0 才是对的。
                if let Some((_level, _content, marker_len)) =
                    BlockKind::parse_atx_heading_line_with_marker(&rest)
                {
                    consumed += marker_len;
                }
            }
            BlockKind::Paragraph => {
                if let Some(stripped) = super::document::strip_leading_columns(&rest, list_dedent) {
                    consumed += rest.len() - stripped.len();
                }
            }
            _ => return None,
        }
        Some((consumed, child_dedent))
    }

    /// 解析期就记在块上的每行记号宽度，直接拿来用（见 `BlockRecord::source_line_prefixes`）。
    ///
    /// 这是删掉「事后拿文件行与模型行比」那一层的正路：宽度是剥记号那段代码当场知道的
    /// 事实，不是比出来的猜测，而且带着上级容器（引用的 `>`、列表的缩进）已经吃掉的字节
    /// ——`collect_quote_block` 与 `dedent_lines_with_origins` 把每行的继承量一路传下来。
    /// 还没记到的形状交回 `measured_block_line_prefixes`：代码围栏、缩进代码块、表格格子，
    /// 以及容器自己那份多行正文（空行段折进行数就对不上，见下）。
    /// 行数对不上、块起点不在行首、记号宽度比那一行还长，都算「这份数据不能用」，交回量。
    fn recorded_block_line_prefixes(
        &self,
        block: &Entity<Block>,
        content_markdown: &str,
        absolute_start: usize,
        cx: &App,
    ) -> Option<MeasuredBlockLines> {
        let recorded = block.read(cx).record.source_line_prefixes.clone();
        let model_lines = content_markdown.split('\n').count();
        if recorded.is_empty() || recorded.len() != model_lines {
            return None;
        }
        let first_line = self.buffer.line_of(absolute_start);
        if self.buffer.line_start(first_line) != absolute_start {
            return None;
        }
        let mut prefixes = Vec::with_capacity(recorded.len());
        let mut file_lens = Vec::with_capacity(recorded.len());
        for (offset, prefix) in recorded.iter().enumerate() {
            let range = self.buffer.line_range(first_line + offset);
            let prefix = *prefix as usize;
            if prefix > range.len() {
                return None;
            }
            prefixes.push(prefix);
            file_lens.push(range.len() - prefix);
        }
        self.line_prefix_from_record
            .set(self.line_prefix_from_record.get() + prefixes.len() as u64);
        Some(MeasuredBlockLines { prefixes, file_lens })
    }

    /// 这一块的内容每一行在自己那一行里让开几个字节：首行走 `measured_block_prefix`
    /// （本块的记号在那里量），续行按「这一行的容器记号 + 缩进差」量。
    ///
    /// 多行内容以前整块退回按模型拼（续行一律 `> `、列表续段一律每级两个空格），于是
    /// 文件里写 `>引用二`（记号后没空格）就整体少一位、写 `>   引用三` 就多一位——字落进
    /// 上一行的内容里；四空格嵌套列表的续段更狠，整块重贴时把用户那四格缩进洗成两格。
    /// 续行没有自己的记号，被上级容器吃掉的缩进又不存在模型里，所以按「文件这行剥掉
    /// 容器记号后的缩进 − 模型这行的缩进」量：差出来那截就是被吃掉的字节数，这个口径
    /// 不问解析器逐级 dedent 的账怎么算。对不上（负数、引用记号剥不动、行不在缓冲区里）
    /// 整块退回按模型拼。
    fn measured_block_line_prefixes(
        &self,
        content_markdown: &str,
        absolute_start: usize,
        quote_depth: usize,
        list_dedent: usize,
        kind: &BlockKind,
    ) -> Option<MeasuredBlockLines> {
        let first = self.measured_block_prefix(absolute_start, quote_depth, list_dedent, kind)?;
        let model_lines: Vec<&str> = content_markdown.split('\n').collect();
        let open_line = self.buffer.slice(self.buffer.line_range(self.buffer.line_of(absolute_start)));
        let mut prefixes = vec![first.0];
        let mut file_lens = vec![open_line.len() - first.0];
        if model_lines.len() == 1 {
            return Some(MeasuredBlockLines { prefixes, file_lens });
        }

        let levels = quote_depth + usize::from(matches!(kind, BlockKind::Quote));
        let mut line_index = self.buffer.line_of(absolute_start);
        for model_line in model_lines.iter().skip(1) {
            line_index += 1;
            let range = self.buffer.line_range(line_index);
            if range.start >= self.buffer.byte_len() {
                return None;
            }
            let file_line = self.buffer.slice(range);
            let mut rest = file_line.clone();
            let mut consumed = 0usize;
            for _ in 0..levels {
                let stripped = super::document::strip_one_quote_level(&rest)?;
                consumed += rest.len() - stripped.len();
                rest = stripped;
            }
            let file_indent = leading_blank_bytes(&rest);
            let model_indent = leading_blank_bytes(model_line);
            if model_indent > file_indent {
                return None;
            }
            let prefix = consumed + (file_indent - model_indent);
            prefixes.push(prefix);
            file_lens.push(file_line.len() - prefix);
        }
        self.line_prefix_measured
            .set(self.line_prefix_measured.get() + prefixes.len() as u64);
        Some(MeasuredBlockLines { prefixes, file_lens })
    }

    /// 每一行的前缀都量得到的块按量出来的落笔（里头已含本行的容器记号，所以引用包裹
    /// 不再叠一层），任一行量不到就整块退回按模型拼的那对前缀。
    fn push_measured_inline_mapping(
        &self,
        block: &Entity<Block>,
        content_markdown: String,
        composed_first: String,
        composed_continuation: String,
        measured_lines: Option<MeasuredBlockLines>,
        quote_depth: usize,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
    ) -> usize {
        match measured_lines {
            Some(measured) => self.push_line_prefixed_inline_mapping(
                block,
                content_markdown,
                &measured,
                absolute_start,
                mappings,
            ),
            None => self.push_inline_block_mapping(
                block,
                content_markdown,
                composed_first,
                composed_continuation,
                quote_depth,
                absolute_start,
                mappings,
            ),
        }
    }

    /// 源码视图的块 ↔ 缓冲区切片：内容第 n 个字节就是「块起点 + n」，两侧都是恒等表。
    ///
    /// 返回这一份切片的长度，走查用它推进到下一根块（源码文档的块间只隔一个换行）。
    fn push_source_slice_mapping(
        &self,
        block: &Entity<Block>,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
        block_ranges: &mut HashMap<EntityId, Range<usize>>,
        cx: &App,
    ) -> usize {
        let len = block.read(cx).display_text().len();
        let identity: Vec<usize> = (0..=len).collect();
        let end = absolute_start + len;
        mappings.push(SourceTargetMapping {
            entity: block.clone(),
            full_source_range: absolute_start..end,
            content_to_source: identity.clone(),
            source_to_content: identity,
        });
        block_ranges.insert(block.entity_id(), absolute_start..end);
        len
    }

    pub(super) fn collect_single_block_source_mappings(
        &self,
        block: &Entity<Block>,
        list_depth: usize,
        list_dedent: usize,
        quote_depth: usize,
        absolute_start: usize,
        mappings: &mut Vec<SourceTargetMapping>,
        block_ranges: &mut HashMap<EntityId, Range<usize>>,
        cx: &App,
    ) -> usize {
        // 源码/代码文档里的块是**缓冲区的一段切片**：可见文本就是文件里的那几位字节，
        // 没有记号、没有围栏行可量。拿 markdown 的口径去量它（`行字节数 − 序列化出来的
        // 围栏长度`）会切进多字节字符中间——`def 甲():` 里 `甲` 占三位，切在第 5 位直接
        // panic。内容偏移与源码偏移只差一个块起点。
        if block.read(cx).is_source_raw_mode() {
            return self.push_source_slice_mapping(
                block,
                absolute_start,
                mappings,
                block_ranges,
                cx,
            );
        }
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

        // 记号在本行里占几位是文件里的事实。先问块自己在解析期记下的那份数据；
        // 还没记到的形状（引用/列表里的子块、代码围栏、表格格子）才退回按文件量
        // ——量的口径见 `measured_block_prefix`：按模型拼（列表每级两个空格、引用一律
        // `> `）在缩进四格、制表符、`>引用` 这类写法里会整体漂。
        // 子块的续行还要按上级列表的缩进量，那个数只能从本块的第一行量出来。
        let measured_first_line =
            self.measured_block_prefix(absolute_start, quote_depth, list_dedent, &kind);
        let kind_for_measure = kind.clone();
        let line_prefixes = |markdown: &str| {
            self.recorded_block_line_prefixes(block, markdown, absolute_start, cx)
            .or_else(|| {
                self.measured_block_line_prefixes(
                    markdown,
                    absolute_start,
                    quote_depth,
                    list_dedent,
                    &kind_for_measure,
                )
            })
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
            BlockKind::Heading { level } => {
                let markdown = title.expect("heading title").markdown().to_string();
                let measured_lines = line_prefixes(&markdown);
                self.push_measured_inline_mapping(
                    block,
                    markdown,
                    format!(
                        "{}{} ",
                        "  ".repeat(list_depth),
                        "#".repeat(level as usize)
                    ),
                    String::new(),
                    measured_lines,
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
            BlockKind::Paragraph => {
                let markdown = title.expect("paragraph title").markdown().to_string();
                let indentation = "  ".repeat(list_depth);
                let measured_lines = line_prefixes(&markdown);
                self.push_measured_inline_mapping(
                    block,
                    markdown,
                    indentation.clone(),
                    indentation,
                    measured_lines,
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
            BlockKind::BulletedListItem => {
                let markdown = title.expect("bullet title").markdown().to_string();
                let indentation = "  ".repeat(list_depth);
                let measured_lines = line_prefixes(&markdown);
                self.push_measured_inline_mapping(
                    block,
                    markdown,
                    format!("{indentation}- "),
                    format!("{indentation}  "),
                    measured_lines,
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
            BlockKind::TaskListItem { checked } => {
                let markdown = title.expect("task title").markdown().to_string();
                let indentation = "  ".repeat(list_depth);
                let measured_lines = line_prefixes(&markdown);
                self.push_measured_inline_mapping(
                    block,
                    markdown,
                    format!("{indentation}- [{}] ", if checked { "x" } else { " " }),
                    format!("{indentation}      "),
                    measured_lines,
                    quote_depth,
                    absolute_start,
                    mappings,
                )
            }
            BlockKind::NumberedListItem => {
                let markdown = title.expect("numbered title").markdown().to_string();
                let indentation = "  ".repeat(list_depth);
                let ordinal = list_ordinal.unwrap_or(1);
                let measured_lines = line_prefixes(&markdown);
                self.push_measured_inline_mapping(
                    block,
                    markdown,
                    format!("{indentation}{ordinal}. "),
                    format!("{indentation}   "),
                    measured_lines,
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
                    let measured_lines = line_prefixes(&title);
                    self.push_measured_inline_mapping(
                        block,
                        title,
                        String::new(),
                        String::new(),
                        measured_lines,
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
                    4,
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
        let child_list_dedent = measured_first_line
            .map(|(_, dedent)| dedent)
            .unwrap_or(list_dedent + 2 * usize::from(kind.is_list_item()));
        let child_quote_depth = quote_depth + usize::from(kind.is_quote_container());
        let mut total_len = own_len;
        for child in children {
            if total_len > 0 {
                total_len += 1;
            }
            total_len += self.collect_single_block_source_mappings(
                &child,
                child_list_depth,
                child_list_dedent,
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

    /// 这一块在缓冲区里占哪一段：根块用自己的 `source_span`，挂在容器里的子块只重建
    /// 它那**一根**块的映射，不为一个边界把整篇走查一遍。
    pub(super) fn block_source_range(
        &self,
        entity_id: EntityId,
        cx: &App,
    ) -> Option<Range<usize>> {
        let block = self.document.block_entity_by_id(entity_id)?;
        if let Some(span) = block.read(cx).record.source_span.clone() {
            return Some(span);
        }
        let root = self.document.root_ancestor_of(entity_id)?;
        if root.read(cx).record.source_span.is_some() {
            // 挂在容器里的子块：只重建它那**一根**块的映射。
            let mut mappings = Vec::new();
            let mut ranges = HashMap::new();
            self.push_root_source_mappings(&root, &mut mappings, &mut ranges, cx);
            return ranges.get(&entity_id).cloned().or_else(|| {
                mappings
                    .iter()
                    .find(|mapping| mapping.entity.entity_id() == entity_id)
                    .map(|mapping| mapping.full_source_range.clone())
            });
        }
        // 刚插进树、还没写回缓冲区的块没有区间：给一个就近的零宽锚点（前一根块末尾
        // 之后的那个字节），跨块选区的端点才解析得出来，删除不会因为一个空段落中止。
        let mut anchor = 0usize;
        for sibling in self.document.root_blocks().iter() {
            let id = sibling.entity_id();
            let span = sibling.read(cx).record.source_span.clone();
            if id == root.entity_id() {
                return Some(anchor..anchor);
            }
            if let Some(span) = span {
                anchor = (span.end + 1).min(self.buffer.byte_len());
            }
        }
        None
    }

    /// 一根块（连着它的子块）的映射，锚在它自己的 `source_span` 上。
    ///
    /// 返回 `false` = 这一根还没有区间（刚插进树、尚未写回缓冲区），什么都没写。
    /// 块内部的偏移仍由 `collect_single_block_source_mappings` 重建，所以映射要钳在
    /// 本块的区间里并落在字符边界上——写法不规范的块长度与缓冲区对不上时，既不能
    /// panic，也不能越界吃到邻居的字节。
    pub(super) fn push_root_source_mappings(
        &self,
        block: &Entity<Block>,
        mappings: &mut Vec<SourceTargetMapping>,
        block_ranges: &mut HashMap<EntityId, Range<usize>>,
        cx: &App,
    ) -> bool {
        let Some(span) = block.read(cx).record.source_span.clone() else {
            return false;
        };
        if Self::is_empty_root_paragraph(block.read(cx)) {
            // 空根块无文本映射，但要有零宽 span 让跨块选区边界能解析。
            block_ranges.insert(block.entity_id(), span.start..span.start);
            return true;
        }
        let prior_mapping_count = mappings.len();
        self.collect_single_block_source_mappings(
            block,
            0,
            0,
            0,
            span.start,
            mappings,
            block_ranges,
            cx,
        );
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
        block_ranges.insert(block.entity_id(), span);
        true
    }

    /// 只重建与 `range` 相交的那几根块的映射（再各带左右紧邻的一根）。
    ///
    /// 跳转、点大纲、恢复选区这类落点只可能落在被点到的那几块里，为它们把整篇重拼
    /// 一遍是 O(文档)：1 MiB 实测一次 227ms，10 MiB 就是秒级，而大纲跟随滚动每一帧
    /// 都在这条链上。多带紧邻的一根是因为端点落在两块之间的空行时，
    /// `endpoint_for_source_offset` 要按距离挑最近的一块，少带就挑到别处去了。
    pub(super) fn source_mappings_in_range(
        &self,
        range: &Range<usize>,
        cx: &App,
    ) -> Vec<SourceTargetMapping> {
        let started = std::time::Instant::now();
        self.source_mapping_builds
            .set(self.source_mapping_builds.get() + 1);
        let mut mappings = Vec::new();
        let mut block_ranges = HashMap::new();
        let roots = self.document.root_blocks();
        let mut visit = vec![false; roots.len()];
        let mut previous = None;
        let mut next = None;
        for (index, block) in roots.iter().enumerate() {
            let Some(span) = block.read(cx).record.source_span.clone() else {
                continue;
            };
            if span.start <= range.end && span.end >= range.start {
                visit[index] = true;
            } else if span.end < range.start {
                previous = Some(index);
            } else if next.is_none() {
                next = Some(index);
            }
        }
        for index in [previous, next].into_iter().flatten() {
            visit[index] = true;
        }

        let mut built = 0usize;
        for (index, block) in roots.iter().enumerate() {
            if !visit[index] {
                continue;
            }
            built += self.push_root_source_mappings(block, &mut mappings, &mut block_ranges, cx) as usize;
        }
        if built == 0 {
            // 窗口里一根有区间的块都没有（编辑中途刚插进树的块）：这时只能整篇走查，
            // 让零宽锚点把端点解出来。
            return self.build_source_target_mappings(cx);
        }
        self.source_mapping_nanos.set(
            self.source_mapping_nanos.get()
                + started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
        );
        mappings
    }

    /// 「这个字节落在哪一块」按块自己的区间回答。
    ///
    /// 根块的区间就挂在块上；只有落进容器（引用、列表）才把那**一根**块的子块映射
    /// 重建出来看是里头的哪一块。以前这类问题（点大纲、跳转、展开折叠）都是先整篇
    /// 重拼一遍，成本随文档长。
    pub(super) fn block_id_at_source_offset(&self, offset: usize, cx: &App) -> Option<EntityId> {
        let root = self.document.root_blocks().into_iter().find(|block| {
            block
                .read(cx)
                .record
                .source_span
                .as_ref()
                .is_some_and(|span| span.contains(&offset) || span.start == offset)
        })?;
        if root.read(cx).children.is_empty() {
            return Some(root.entity_id());
        }
        let mut mappings = Vec::new();
        let mut block_ranges = HashMap::new();
        self.push_root_source_mappings(&root, &mut mappings, &mut block_ranges, cx);
        mappings
            .iter()
            .find(|mapping| mapping.full_source_range.contains(&offset))
            .map(|mapping| mapping.entity.entity_id())
            .or_else(|| Some(root.entity_id()))
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

        // 位置与文本同源：两种视图都按「根块在缓冲区里的区间」起锚。源码/代码文档的块
        // 是缓冲区的一段切片（区间由 `attach_source_slice_spans` 挂上，块内偏移是恒等
        // 换算），渲染态的区间由导入器按行换算。以前源码模式另走一遍「按序列化长度累加
        // absolute」的记账，还要先把整篇缓冲区拷出来——一次按键 O(文档) 就在这两条上。
        //
        // 块内部的偏移仍由下面的重建算出，写法不规范的块（表格列宽填充）会在块内漂几个
        // 字节，但绝不会再漂到别的块里。
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
            let prior_mapping_count = mappings.len();
            if !self.push_root_source_mappings(block, &mut mappings, &mut block_ranges, cx) {
                // 这个根块还没有区间（刚插进树、尚未写回缓冲区）：给一个
                // 就近的零宽锚点，跨块选区的端点才解析得出来。真正的区间
                // 要等写回那一步才有。
                block_ranges.insert(id, next_anchor..next_anchor);
                continue;
            }
            next_anchor = block
                .read(cx)
                .record
                .source_span
                .as_ref()
                .map(|span| (span.end + 1).min(self.buffer.byte_len()))
                .unwrap_or(next_anchor);
            if target.is_some_and(|id| {
                mappings[prior_mapping_count..]
                    .iter()
                    .any(|mapping| mapping.entity.entity_id() == id)
            }) {
                break;
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
