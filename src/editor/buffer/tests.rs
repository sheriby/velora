use std::time::{Duration, Instant};

use super::*;

#[test]
fn buffer_holds_exactly_the_text_it_was_given() {
    let text = "# 标题\n\n正文 with emoji 🎉 和中文标点，以及 `code`。\n";

    let buffer = TextBuffer::from_text(text);

    assert_eq!(buffer.text(), text);
    assert_eq!(buffer.byte_len(), text.len());
}

#[test]
fn an_edit_replaces_exactly_the_given_byte_range() {
    let mut buffer = TextBuffer::from_text("hello world");

    buffer.edit(5..5, " big");
    assert_eq!(buffer.text(), "hello big world");

    buffer.edit(5..9, "");
    assert_eq!(buffer.text(), "hello world");

    buffer.edit(0..5, "HELLO");
    assert_eq!(buffer.text(), "HELLO world");
    assert_eq!(buffer.byte_len(), "HELLO world".len());

    buffer.edit("HELLO world".len().."HELLO world".len(), "!\n");
    assert_eq!(buffer.text(), "HELLO world!\n");
}

#[test]
fn editing_edges_of_an_empty_buffer_behaves_like_a_plain_string() {
    let mut buffer = TextBuffer::from_text("");
    assert_eq!(buffer.byte_len(), 0);

    buffer.edit(0..0, "第一段");
    assert_eq!(buffer.text(), "第一段");

    let end = buffer.byte_len();
    buffer.edit(end..end, "\n第二段");
    assert_eq!(buffer.text(), "第一段\n第二段");

    buffer.edit(0..0, "前置\n");
    assert_eq!(buffer.text(), "前置\n第一段\n第二段");

    let all = buffer.byte_len();
    buffer.edit(0..all, "");
    assert_eq!(buffer.text(), "");
    assert_eq!(buffer.byte_len(), 0);
}

#[test]
fn an_edit_spanning_several_chunks_still_replaces_only_that_range() {    // 造一份远超单块预算的文本，保证它真被切成了多块。
    let line = "行内容 abcdefghijklmnopqrstuvwxyz 中文标点，句号。\n";
    let text = line.repeat(600);
    let mut buffer = TextBuffer::from_text(&text);
    assert!(
        buffer.chunks.len() > 6,
        "夹具应被切成多块，实际 {} 块",
        buffer.chunks.len()
    );

    // 跨越若干块的删除。
    let start = text.floor_char_boundary(10);
    let end = text.floor_char_boundary(MAX_CHUNK_BYTES * 4 + 7);
    let removed = &text[start..end];
    buffer.edit(start..end, "");
    assert_eq!(buffer.text(), format!("{}{}", &text[..start], &text[end..]));
    assert_eq!(buffer.byte_len(), text.len() - removed.len());

    // 再插入一段比单块还长的文本（对应粘贴大段内容的路径）。
    let paste = "粘贴一整段长文本。".repeat(2000);
    buffer.edit(start..start, &paste);
    assert_eq!(
        buffer.text(),
        format!("{}{}{}", &text[..start], paste, &text[end..])
    );
}

#[test]
fn chunk_splitting_and_edits_never_cut_a_multibyte_character() {    // 每字 3 字节 + emoji 4 字节：4096 的块预算必然落在字符中间。
    let text = "测".repeat(3000) + "🎉" + &"试".repeat(2000);
    let mut buffer = TextBuffer::from_text(&text);
    assert_eq!(buffer.text(), text);

    let emoji_at = text.find('🎉').expect("fixture contains emoji");
    buffer.edit(emoji_at..emoji_at, "!");
    buffer.edit(emoji_at + 1..emoji_at + 5, "?");

    assert_eq!(
        buffer.text(),
        "测".repeat(3000) + "!?" + &"试".repeat(2000)
    );
}

#[test]
fn line_offsets_resolve_both_ways_and_survive_edits() {
    let text = "# 标题\n\n正文 in 中文\n\n末行";
    let mut buffer = TextBuffer::from_text(text);

    // 行数按 split('\n') 计：末行没有换行符也算一行。
    assert_eq!(buffer.line_count(), 5);
    assert_eq!(buffer.line_start(0), 0);
    assert_eq!(buffer.line_start(1), "# 标题".len() + 1);
    assert_eq!(buffer.line_start(2), "# 标题\n\n".len());
    assert_eq!(buffer.line_of(buffer.line_start(2)), 2);
    assert_eq!(buffer.line_of(1), 0);
    assert_eq!(buffer.line_of(buffer.byte_len()), 4);

    // 末行换行不额外凭空造出一行内容，但会多出那条空行 —— 与磁盘行数一致。
    buffer.edit(buffer.byte_len()..buffer.byte_len(), "\n");
    assert_eq!(buffer.line_count(), 6);
    assert_eq!(buffer.line_start(5), text.len() + 1);

    // 在前面插一行，之后的行号整体后移，但仍自洽。
    buffer.edit(0..0, "新前言\n");
    assert_eq!(buffer.line_count(), 7);
    assert_eq!(buffer.line_of(0), 0);
    assert_eq!(buffer.line_of(buffer.line_start(6)), 6);
    let last_text_line = buffer.line_start(5);
    assert_eq!(&buffer.text()[last_text_line..], "末行\n");
}

#[test]
fn an_anchor_shifts_with_edits_before_it_and_stays_put_after() {
    let mut buffer = TextBuffer::from_text("第一段\n第二段\n第三段");
    let second_at = buffer.text().find("第二段").expect("fixture");
    let anchor = buffer.anchor_at(second_at);
    let tail = buffer.anchor_at(buffer.byte_len());

    // 在它之前插入：锚点右移，且仍指向「第二段」开头。
    buffer.edit(0..0, "前言\n");
    let moved = buffer.resolve(anchor);
    assert!(moved > second_at, "锚点没有随前面的插入右移");
    let after = buffer.text();
    assert_eq!(&after[moved..moved + "第二段".len()], "第二段");

    // 在它之后插入：锚点不动。
    let before = buffer.resolve(anchor);
    let end = buffer.byte_len();
    buffer.edit(end..end, "\n后记");
    assert_eq!(buffer.resolve(anchor), before);
    assert_eq!(buffer.resolve(tail), end);
}

#[test]
fn an_anchor_inside_deleted_text_clamps_to_a_valid_offset() {
    let mut buffer = TextBuffer::from_text("keep this and drop that");
    let doomed = buffer.anchor_at("keep this and ".len());
    let kept = buffer.anchor_at(0);

    buffer.edit("keep this ".len()..buffer.byte_len(), "");

    assert_eq!(buffer.text(), "keep this ");
    assert_eq!(buffer.resolve(kept), 0);
    // 指向被删内容的锚点必须落在合法位置，并且解析出的文本不再是旧内容。
    let resolved = buffer.resolve(doomed);
    assert!(resolved <= buffer.byte_len(), "解析越界：{resolved}");
    assert_eq!(buffer.slice(resolved..buffer.byte_len()), "");
}

#[test]
fn a_span_between_two_anchors_still_yields_its_original_text() {
    let filler = "填充行 filler line。\n";
    let mut buffer = TextBuffer::from_text(&filler.repeat(3000));

    // 取中间某一行当「块 span」，然后在它之前大量编辑。
    let start = buffer.line_start(1500);
    let end = buffer.line_start(1501);
    let original = buffer.slice(start..end);
    let span_start = buffer.anchor_at(start);
    let span_end = buffer.anchor_at(end);

    for _ in 0..50 {
        buffer.edit(0..0, "插一行\n");
    }
    let last = buffer.byte_len();
    buffer.edit(last - 1..last, "");

    assert_eq!(buffer.slice_span(span_start..span_end), original);
    assert_eq!(buffer.resolve(span_start), buffer.line_start(1550));
}

#[test]
fn an_edit_returns_the_inverse_that_restores_the_previous_text() {
    let mut buffer = TextBuffer::from_text("第一段\n第二段\n第三段");
    let start = buffer.line_start(1);
    let end = buffer.byte_len();
    let original = buffer.slice(start..end);

    let applied = buffer.edit(start..end, "只剩这一段\n");
    assert_eq!(applied.removed, original);
    assert_eq!(buffer.slice(applied.new_range.clone()), "只剩这一段\n");

    // 逆操作就是把换掉的文本放回去。
    buffer.edit(applied.new_range.clone(), &applied.removed);
    assert_eq!(buffer.text(), "第一段\n第二段\n第三段");

    // 撤销/重做交替，位置仍自洽（历史栈就靠这条性质工作）。
    let redo = buffer.line_start(1)..buffer.byte_len();
    let removed = buffer.slice(redo.clone());
    let again = buffer.edit(redo, "只剩这一段\n");
    buffer.edit(again.new_range.clone(), &again.removed);
    assert_eq!(buffer.text(), "第一段\n第二段\n第三段");
    let _ = removed;
}

#[test]
fn freeing_an_anchor_returns_its_slot_and_shifts_stay_correct() {
    let mut buffer = TextBuffer::from_text("aaaaaaaa");
    let first = buffer.anchor_at(1);
    let second = buffer.anchor_at(2);
    buffer.free_anchor(second);

    buffer.edit(0..0, "X");
    assert_eq!(buffer.resolve(first), 2);

    // 释放后的槽位被新锚点复用，不会无限增长。
    let slots_before = buffer.anchors.len();
    let third = buffer.anchor_at(0);
    buffer.free_anchor(first);
    let reused = buffer.anchor_at(3);
    assert_eq!(buffer.anchors.len(), slots_before);
    assert_eq!(reused.slot, first.slot);
    assert_eq!(buffer.resolve(third), 0);
    assert_eq!(buffer.resolve(reused), 3);
}

#[test]
fn an_unedited_buffer_saves_back_the_exact_bytes_it_was_opened_with() {
    let raw = "行尾 CRLF 且没有末行换行\r\n第二行".as_bytes().to_vec();
    let text = String::from_utf8_lossy(&raw).replace("\r\n", "\n");
    let mut buffer = TextBuffer::from_text(&text);
    buffer.set_file_origin(raw.clone(), FileShape::detect(&raw));

    assert!(buffer.is_pristine());
    assert_eq!(buffer.file_bytes(), raw);

    // 落一次编辑，原始字节依据就作废；此后按形状编码写回。
    buffer.edit(0..0, "新行\n\n");
    assert!(!buffer.is_pristine());
    assert_eq!(
        buffer.file_bytes(),
        "新行\r\n\r\n行尾 CRLF 且没有末行换行\r\n第二行".as_bytes()
    );
}

#[test]
fn a_buffer_without_a_file_origin_writes_its_text_as_utf8() {
    let buffer = TextBuffer::from_text("# 标题\n\n正文\n");
    assert!(!buffer.is_pristine());
    assert_eq!(buffer.file_bytes(), "# 标题\n\n正文\n".as_bytes());
}

#[test]
fn an_edit_on_an_eight_mib_buffer_costs_nothing_proportional_to_the_text() {
    // 结构性保证：每次编辑的工作量是 O(块数)，不是 O(文本字节数)。
    // 今天的实现在 10 MiB 上打一个字要 13 秒（perf_budgets.rs 注释），所以这条
    // 断言是整次重构性能收益的底线；debug 构建下留了很大余量，避免抖动。
    let line = "这是一行混合中文和 english 的内容。\n";
    let text = line.repeat(200_000); // ≈ 8 MiB
    assert!(text.len() >= 6 * 1024 * 1024, "夹具应达数 MiB");
    let mut buffer = TextBuffer::from_text(&text);
    let chunks_at_start = buffer.chunks.len();

    let started = Instant::now();
    for _ in 0..200 {
        buffer.edit(0..0, "插一行\n");
    }
    let elapsed = started.elapsed();

    assert!(
        elapsed < Duration::from_millis(200),
        "8 MiB 缓冲区上 200 次编辑用了 {elapsed:?}"
    );
    // 反复切分不能把块切碎到失控（每次编辑最多新增常数个块）。
    assert!(
        buffer.chunks.len() <= chunks_at_start + 400,
        "块数从 {chunks_at_start} 涨到 {}",
        buffer.chunks.len()
    );
    assert_eq!(buffer.line_count(), 200_201);
}

#[test]
fn line_offsets_stay_correct_across_many_chunks() {
    let line = "第 12345 行，内容里有一些中文和标点，还有 english。\n";
    let text = line.repeat(4000); // 约 250 KiB，必然跨很多块
    let buffer = TextBuffer::from_text(&text);
    assert!(buffer.chunks.len() > 30);

    assert_eq!(buffer.line_count(), 4001);
    for probe in [0usize, 1, 999, 2000, 3999, 4000] {
        assert_eq!(
            buffer.line_of(buffer.line_start(probe)),
            probe,
            "行 {probe} 的起点换算不成对"
        );
    }
    let last_start = buffer.line_start(3999);
    assert_eq!(&buffer.text()[last_start..last_start + line.len()], line);
}

#[test]
fn lines_and_line_starts_agrees_with_asking_one_offset_at_a_time() {
    // ASCII 的夹具行：探针偏移要点点在字符边界上才好对照单点版。
    let line = "line 01234 with some english text and a bit more\n";
    let text = format!("{}# heading at the end\n", line.repeat(4000));
    let mut buffer = TextBuffer::from_text(&text);
    assert!(buffer.chunks.len() > 30, "夹具得跨很多块才测得出批量换算");
    // 编辑把块切开，于是「一块的边界」既有多块的行也有碎块的行。
    buffer.edit(line.len()..line.len(), "!");
    let total = buffer.byte_len();

    // 行首、行中、块边界、文末都点一遍；升序传给批量版。
    let mut offsets = vec![0usize, 1, 7, 40, 41, 1000, 8192, 9000, total - 1, total];
    for line_index in [1usize, 2, 39, 40, 4001] {
        offsets.push(buffer.line_start(line_index));
    }
    offsets.sort_unstable();
    offsets.dedup();

    for (offset, (line_of_batch, line_start_of_batch)) in offsets
        .iter()
        .copied()
        .zip(buffer.lines_and_line_starts(&offsets))
    {
        assert_eq!(
            (line_of_batch, line_start_of_batch),
            (buffer.line_of(offset), buffer.line_start(buffer.line_of(offset))),
            "偏移 {offset} 的批量行号与单点问的不一样"
        );
    }
}

#[test]
fn byte_len_survives_many_edits_without_counting_the_chunks() {
    let mut buffer = TextBuffer::from_text("first\nsecond\nthird\n");
    for _ in 0..200 {
        buffer.edit(6..6, "inserted text\n");
        buffer.edit(0..5, "");
    }
    assert_eq!(
        buffer.byte_len(),
        buffer.text().len(),
        "byte_len 与内容对不上了"
    );
}

#[test]
fn an_edit_forces_reencoding_by_shape_and_ends_the_pristine_copy() {
    let raw = "正文一\r\n正文二\r\n".as_bytes().to_vec();
    let mut buffer = TextBuffer::from_text("正文一\n正文二\n");
    buffer.set_file_origin(raw.clone(), FileShape::detect(&raw));
    assert_eq!(buffer.file_bytes(), raw);

    // 唯一写入口落下改动：原字节依据就此作废，写回改按文件形状编码。
    buffer.edit(0..9, "改过的正文一");
    assert!(!buffer.is_pristine());
    assert_eq!(buffer.file_bytes(), "改过的正文一\r\n正文二\r\n".as_bytes());

    // 无形状信息时退回 UTF-8 文本本身。
    let plain = TextBuffer::from_text("正文\n");
    assert_eq!(plain.file_bytes(), "正文\n".as_bytes());
}

/// 行号→字节区间是读取侧（搜索跳转、大纲点击）唯一的换算入口，必须在
/// 缓冲区自己的坐标里成立，且不含行尾换行符。
#[test]
fn line_range_gives_the_bytes_of_that_line_without_the_newline() {
    let mut buffer = TextBuffer::from_text("first\n\n第三行 with 中文\ntail\n");
    assert_eq!(buffer.line_range(0), 0..5);
    assert_eq!(buffer.line_range(1), 6..6);
    assert_eq!(buffer.line_range(2), 7..7 + "第三行 with 中文".len());
    assert_eq!(buffer.line_range(3), 29..33);

    // 末行没有换行符时区间就到文末。
    buffer = TextBuffer::from_text("甲\n乙");
    assert_eq!(buffer.line_range(0), 0..3);
    assert_eq!(buffer.line_range(1), 4..7);
    // 越界钳到文末，不 panic。
    assert_eq!(buffer.line_range(9), 7..7);
}

/// 比较一份等长但内容不同的文本时只能返回 `false`，不许 panic。
///
/// 分块的边界是按缓冲区自己的字符切的；换一份内容时同样的偏移可能正好落在某个多
/// 字节字符中间（撤销表格「移动一行」就是这种：两行互换，整篇长度不变、内容变了）。
/// 逐字节比较走 `bytes`，跟字符边界无关。
#[test]
fn matching_a_same_length_but_different_text_returns_false_instead_of_panicking() {
    let text = "x".repeat(3000) + &"甲".repeat(1500);
    let buffer = TextBuffer::from_text(&text);
    assert!(
        buffer.chunks.len() > 1,
        "夹具应被切成多块，实际 {} 块",
        buffer.chunks.len()
    );
    assert!(buffer.matches_text(&text));

    // 等长、内容不同，而且块边界落在对方文本的字符中间。
    let other = "x".repeat(2999) + &"甲".repeat(1500) + "y";
    assert_eq!(other.len(), text.len());
    assert!(!buffer.matches_text(&other));
}
