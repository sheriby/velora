use super::*;
use crate::editor::encoding;

/// 真实打开漏斗里的规范文本：解码 → 行尾统一成 LF。
fn canonical(raw: &[u8]) -> String {
    encoding::decode_document_bytes(raw.to_vec())
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn utf8_bytes(text: &str) -> Vec<u8> {
    text.as_bytes().to_vec()
}

/// UTF-16 夹具：BOM + 逐 code unit 的 LE/BE 字节。
fn utf16_bytes(text: &str, little_endian: bool, bom: bool) -> Vec<u8> {
    let mut bytes = Vec::new();
    if bom {
        bytes.extend_from_slice(if little_endian { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
    }
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&if little_endian {
            unit.to_le_bytes()
        } else {
            unit.to_be_bytes()
        });
    }
    bytes
}

#[test]
fn detect_reads_encoding_and_line_ending_from_the_bytes() {
    assert_eq!(
        FileShape::detect(&utf8_bytes("# 标题\n\n正文\n")),
        FileShape {
            encoding: TextEncoding::Utf8,
            bom: false,
            line_ending: LineEnding::LF,
        }
    );
    assert_eq!(
        FileShape::detect(&utf8_bytes("a\r\nb\r\n")),
        FileShape {
            encoding: TextEncoding::Utf8,
            bom: false,
            line_ending: LineEnding::CRLF,
        }
    );

    let gbk = encoding_rs::GB18030.encode("中文笔记\n").0;
    assert_eq!(
        FileShape::detect(gbk.as_ref()).encoding,
        TextEncoding::Gb18030
    );

    // UTF-16 BOM 开头：现在是真解码，形状必须说得出自己是谁，保存才按同编码写回。
    assert_eq!(
        FileShape::detect(&[0xFF, 0xFE, 0x68, 0x00]).encoding,
        TextEncoding::Utf16Le
    );
    assert_eq!(
        FileShape::detect(&[0xFE, 0xFF, 0x00, 0x68]).encoding,
        TextEncoding::Utf16Be
    );
}

#[test]
fn detect_reads_a_utf8_bom_as_a_shape_not_as_text() {
    // 报修 8：BOM 记在形状里，正文里没有那个零宽字符，解析器才看得见首行的 `#`。
    let raw = utf8_bytes("\u{feff}# 标题\n");
    let (shape, text) = FileShape::detect_and_decode(&raw);
    assert_eq!(
        shape,
        FileShape {
            encoding: TextEncoding::Utf8,
            bom: true,
            line_ending: LineEnding::LF,
        }
    );
    assert_eq!(text, "# 标题\n");
    assert_eq!(shape.encode(&text), raw, "BOM 要按形状贴回去");
}

#[test]
fn line_ending_is_measured_on_the_decoded_text_not_on_utf16_bytes() {
    // 「不」的 UTF-16LE 字节是 `0D 4E`：按字节数 `\r` 会把每个中文 UTF-16 文件
    // 判成混排行尾，于是编辑后保存把 `0D 4E` 换成 `0A` —— 字节损坏。
    let crlf = utf16_bytes("# 不\r\n\r\n正文\r\n", true, true);
    let (shape, text) = FileShape::detect_and_decode(&crlf);
    assert_eq!(shape.encoding, TextEncoding::Utf16Le);
    assert_eq!(shape.line_ending, LineEnding::CRLF, "中文 UTF-16 的 CRLF 被漏判：{text:?}");
    assert_eq!(shape.encode(&canonical(&crlf)), crlf);

    let lf = utf16_bytes("# 不\n\n正文\n", false, true);
    let (lf_shape, _) = FileShape::detect_and_decode(&lf);
    assert_eq!(lf_shape.line_ending, LineEnding::LF);
    assert_eq!(lf_shape.encode(&canonical(&lf)), lf);
}

#[test]
fn encoding_a_canonical_buffer_reproduces_the_original_bytes() {
    let gbk = encoding_rs::GB18030
        .encode("中文笔记\n第二行\n")
        .0
        .into_owned();
    let cases = vec![
        utf8_bytes("# 标题\n\n正文\n"),
        utf8_bytes("# 标题\n\n正文"),         // 无末行换行
        utf8_bytes("正文\n\n\n"),             // 多个末行换行
        utf8_bytes("a\r\nb\r\n"),             // 全 CRLF
        utf8_bytes("a\r\nb"),                 // CRLF 且无末行换行
        utf8_bytes("\u{feff}# BOM 标题\n"),   // UTF-8 BOM：形状里的 BOM 要贴回去
        utf8_bytes("   \n\n\t\n"),            // 只有空白
        utf8_bytes(""),                       // 空文件
        gbk,                                  // GB18030 中文
        utf16_bytes("# 标题\n\n正文 🎉\n", true, true),
        utf16_bytes("# 标题\r\n\r\n正文\r\n", true, true),
        utf16_bytes("正文", false, true),
        utf16_bytes("中文 🎉 混排", false, true),
        vec![0xFF, 0xFE], // 只有 BOM 的空文件
    ];

    for raw in cases {
        let shape = FileShape::detect(&raw);
        let back = shape.encode(&canonical(&raw));
        assert_eq!(back, raw, "原字节 {raw:?} 被判为 {shape:?}，还原失败");
    }
}

#[test]
fn an_unrepresentable_shape_never_claims_it_can_write_back() {
    // 坏字节 / 被截断的 UTF-16 / 孤立代理项 / 含 NUL 的字节流（无 BOM 的 UTF-16）：
    // 判定为不可无损表示，上层因此拒绝打开，磁盘上的文件一动不动。
    for raw in [
        vec![0xFF, 0x00, 0xFF, 0xFF],
        vec![0xFF, 0xFE, 0x68],
        vec![0xFF, 0xFE, 0x00, 0xD8, 0x00, 0x00],
        vec![0x41, 0x00, 0x42, 0x00],
    ] {
        let (shape, _) = FileShape::detect_and_decode(&raw);
        assert_eq!(shape.encoding, TextEncoding::Lossy, "{raw:?} → {shape:?}");
        assert!(!shape.is_lossless(), "{raw:?} 被判成可无损写回");
    }
}

#[test]
fn a_file_with_lone_carriage_returns_bytes_as_lf_instead_of_corrupting_them() {
    // 混进单独的 \r 或两种行尾并存时无法由 LF 文本还原：按 LF 写出，
    // 绝不把 LF 升格成 CRLF。
    let mixed = utf8_bytes("混合\r\n换行\n结束\n");
    let shape = FileShape::detect(&mixed);
    assert_eq!(shape.line_ending, LineEnding::LF);
    assert_eq!(
        shape.encode(&canonical(&mixed)),
        utf8_bytes("混合\n换行\n结束\n")
    );

    let lone_cr = utf8_bytes("旧式 Mac\r换行\r");
    assert_eq!(
        FileShape::detect(&lone_cr).line_ending,
        LineEnding::LF,
        "单独的 \\r 不该被判成 CRLF"
    );
}

#[test]
fn encode_folds_stray_carriage_returns_before_upgrading_line_endings() {
    // 缓冲区里混进了 `\r`（比如剪贴板带进来的）时，CRLF 升格绝不能再套一层：
    // `\r\n` → `\r\r\n` 是字节损坏，还会让版本号校验把自己写的文件误判成
    // 外部修改。encode 是最后一道闸：先折平再升格。
    let shape = FileShape {
        encoding: TextEncoding::Utf8,
        bom: false,
        line_ending: LineEnding::CRLF,
    };
    assert_eq!(shape.encode("a\r\nb"), utf8_bytes("a\r\nb"));
    assert_eq!(shape.encode("a\rb"), utf8_bytes("a\r\nb"));
    assert_eq!(shape.encode("a\r\nb\rc\n"), utf8_bytes("a\r\nb\r\nc\r\n"));
    let lf_shape = FileShape {
        encoding: TextEncoding::Utf8,
        bom: false,
        line_ending: LineEnding::LF,
    };
    assert_eq!(lf_shape.encode("a\r\nb"), utf8_bytes("a\nb"));
}

#[test]
fn utf16_encode_is_the_mirror_of_utf16_decode() {
    // 对称性是不变式，不是巧合：同一份文本按形状编码再解码必须回到原文本，
    // 形状本身也必须判回同一个（否则「保存后重开」就会漂）。
    for little_endian in [true, false] {
        for text in [
            "# 会议纪要\n\n中文正文 🎉\n",
            "# 标题\r\n\r\n正文\r\n第二行\r\n",
            "无末行换行",
            "\n\n",
            "",
        ] {
            let raw = utf16_bytes(text, little_endian, true);
            let (shape, decoded) = FileShape::detect_and_decode(&raw);
            assert_eq!(decoded, text);
            assert_eq!(shape.encode(&decoded), raw);
            let (again, _) = FileShape::detect_and_decode(&shape.encode(&decoded));
            assert_eq!(again, shape, "形状不是自己的不动点：{shape:?} vs {again:?}");
        }
    }
}
