
use std::time::{Duration, Instant};

use super::*;

/// std 的 `str::floor_char_boundary` 还没稳定，测试自己往回退到字符边界。
fn floor_char_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while offset > 0 && !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

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
    let start = floor_char_boundary(&text, 10);
    let end = floor_char_boundary(&text, MAX_CHUNK_BYTES * 4 + 7);
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
fn an_unrepresentable_shape_refuses_to_write_new_bytes_over_the_file() {
    // 最后一道闸：形状不能无损写回时，编辑之后 `file_bytes` 还是那份原字节——
    // 保存这种文档就是一次什么都不写。正常入口在 `encoding::load_document` 就把
    // 它们拒在门外；这条守的是「万一有路径绕过去，用户的文件也不会被覆盖」。
    let raw = vec![0xFF, 0x00, 0xFF, 0xFF];
    let mut buffer = TextBuffer::from_text(&String::from_utf8_lossy(&raw));
    buffer.set_file_origin(raw.clone(), FileShape::detect(&raw));

    buffer.edit(0..0, "用户打的字");
    assert_eq!(buffer.file_bytes(), raw, "不可无损表示的形状重编码写回了磁盘");
}


// 墙钟预算闸门，依赖机器速度，随整族性能测试默认 #[ignore]；单独跑
// `cargo test --bin velora -- --ignored`。
#[test]
#[ignore = "墙钟预算闸门，依赖机器速度；单独跑：cargo test --bin velora -- --ignored"]
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
        "200 次编辑把块数从 {} 切到 {}",
        chunks_at_start,
        buffer.chunks.len()
    );
}

#[test]
fn a_buffer_without_a_file_origin_writes_its_text_as_utf8() {
    let buffer = TextBuffer::from_text("# 标题\n\n正文\n");
    assert!(!buffer.is_pristine());
    assert_eq!(buffer.file_bytes(), "# 标题\n\n正文\n".as_bytes());
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

/// 行号换算读了多少字节要数得出来：问得越深，读得越多。这条在优化之后仍然成立
/// （两边都是 0），它只钉住「计数器不是摆设」，不给优化设门槛——门槛写在
/// `asking_for_line_numbers_does_not_read_the_text` 里。
#[test]
fn the_line_probe_counter_follows_how_deep_the_offsets_are() {
    let line = "line 01234 with some english text and a bit more\n";
    let text = format!("{}# heading at the end\n", line.repeat(4000));
    let buffer = TextBuffer::from_text(&text);
    buffer.take_line_probe_bytes();
    buffer.lines_and_line_starts(&[0usize, 1, 40]);
    let shallow = buffer.take_line_probe_bytes();
    buffer.lines_and_line_starts(&[0usize, 1, 40, buffer.byte_len() - 1, buffer.byte_len()]);
    let deep = buffer.take_line_probe_bytes();
    assert!(
        deep >= shallow,
        "问整篇的行号反而比问开头读得少：{deep} < {shallow}，计数器漏档了"
    );
    assert_eq!(
        buffer.take_line_probe_bytes(),
        0,
        "取走一次之后没再问任何东西，读数该是 0"
    );
}

/// 换算行号本该只问「这块有几个换行、都落在哪儿」，不该把正文读一遍。
///
/// 大纲按根块走一圈时要把每块起点的字节偏移换成整篇行号：10 MiB 代码文档实测一次
/// 同步 44ms，全花在这个换算上（沿块把字节数一遍）。这笔必须与文档多大无关。
#[test]
fn asking_for_line_numbers_does_not_read_the_text() {
    let line = "line 01234 with some english text and a bit more\n";
    let text = format!("{}# heading at the end\n", line.repeat(4000));
    let buffer = TextBuffer::from_text(&text);
    assert!(buffer.chunks.len() > 30, "夹具得跨很多块才测得出名堂");
    let total = buffer.byte_len();
    buffer.take_line_probe_bytes();

    // 批量问：偏移全挤在文档开头那一小段里（大纲按根块走一圈就是这个形状）。
    let offsets = [0usize, 1, 40, line.len(), line.len() * 2];
    let answers = buffer.lines_and_line_starts(&offsets);
    assert_eq!(answers[4], (2, line.len() * 2), "顺手钉一下结果本身");
    let batched = buffer.take_line_probe_bytes();
    assert_eq!(
        batched, 0,
        "批量行号换算读了 {batched} 字节正文：它该只问每块的换行表，\
         与文档多大无关（10 MiB 一次大纲同步的 44ms 就是这么来的）"
    );

    // 单点问：`line_of` 与 `line_start` 也不许碰正文。
    for offset in [0usize, 1, 40, line.len(), total - 1] {
        buffer.line_of(offset);
    }
    let single = buffer.take_line_probe_bytes();
    assert_eq!(single, 0, "line_of 读了 {single} 字节正文");
    for line_index in [1usize, 2, 39, 4000] {
        buffer.line_start(line_index);
    }
    let starts = buffer.take_line_probe_bytes();
    assert_eq!(starts, 0, "line_start 读了 {starts} 字节正文");
}

/// 换行索引的**维护**成本只跟着改动走：切块是把两张表分开（一个字节都不重读），
/// 只有新进来的那段文本要现数一遍换行。
#[test]
fn rebuilding_the_line_index_costs_only_the_edited_bytes() {
    let line = "line 01234 with some english text and a bit more\n";
    let text = format!("{}# heading at the end\n", line.repeat(4000));
    let mut buffer = TextBuffer::from_text(&text);
    buffer.take_line_probe_bytes();

    // 在文档中间插一个换行：正文一个字节都没挪，只数了插进去的那一行。
    buffer.edit(text.len() / 2..text.len() / 2, "\n");
    let inserted = buffer.take_line_probe_bytes();
    assert_eq!(inserted, 1, "插一行却读了 {inserted} 字节正文");

    // 删一段也照样只按删掉的那段计（这里删的是既有换行，不新增文本）。
    buffer.edit(text.len() / 2..text.len() / 2 + 1, "");
    assert_eq!(buffer.take_line_probe_bytes(), 0);

    // 行号还是对的：切块之后两张表拼起来等于原文的换行位置。
    assert_eq!(
        buffer.line_count(),
        4002,
        "插入又删掉一个换行，行数该回到原样（4000 行正文 + 最后一行 + 末尾空行）"
    );
    assert_eq!(
        buffer.line_start(3000),
        line.len() * 3000,
        "换行表切过之后行首偏移漂了"
    );
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
