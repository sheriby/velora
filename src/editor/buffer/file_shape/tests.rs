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

#[test]
fn detect_reads_encoding_and_line_ending_from_the_bytes() {
    assert_eq!(
        FileShape::detect(&utf8_bytes("# 标题\n\n正文\n")),
        FileShape {
            encoding: TextEncoding::Utf8,
            line_ending: LineEnding::LF,
        }
    );
    assert_eq!(
        FileShape::detect(&utf8_bytes("a\r\nb\r\n")),
        FileShape {
            encoding: TextEncoding::Utf8,
            line_ending: LineEnding::CRLF,
        }
    );

    let gbk = encoding_rs::GB18030.encode("中文笔记\n").0;
    assert_eq!(
        FileShape::detect(gbk.as_ref()).encoding,
        TextEncoding::Gb18030
    );

    // UTF-16 BOM 开头：解码走 lossy，形状必须承认不可无损还原。
    assert_eq!(
        FileShape::detect(&[0xFF, 0xFE, 0x68, 0x00]).encoding,
        TextEncoding::Lossy
    );
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
        utf8_bytes("\u{feff}# BOM 标题\n"),   // UTF-8 BOM 就是普通合法 UTF-8 字节
        utf8_bytes("   \n\n\t\n"),            // 只有空白
        utf8_bytes(""),                       // 空文件
        gbk,                                  // GB18030 中文
    ];

    for raw in cases {
        let shape = FileShape::detect(&raw);
        let back = shape.encode(&canonical(&raw));
        assert_eq!(back, raw, "原字节 {raw:?} 被判为 {shape:?}，还原失败");
    }
}

#[test]
fn a_file_with_lone_carriage_returns_bytes_as_lf_instead_of_corrupting_them() {
    // 混进单独的 \r 或两种行尾并存时无法由 LF 文本还原：按 LF 写出，
    // 绝不把 LF 升格成 CRLF。
    let mixed = utf8_bytes("混合\r\n换行\n结束\n");
    let shape = FileShape::detect(&mixed);
    assert_eq!(shape.line_ending, LineEnding::LF);
    assert_eq!(shape.encode(&canonical(&mixed)), utf8_bytes("混合\n换行\n结束\n"));

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
        line_ending: LineEnding::CRLF,
    };
    assert_eq!(shape.encode("a\r\nb"), utf8_bytes("a\r\nb"));
    assert_eq!(shape.encode("a\rb"), utf8_bytes("a\r\nb"));
    assert_eq!(shape.encode("a\r\nb\rc\n"), utf8_bytes("a\r\nb\r\nc\r\n"));
    let lf_shape = FileShape {
        encoding: TextEncoding::Utf8,
        line_ending: LineEnding::LF,
    };
    assert_eq!(lf_shape.encode("a\r\nb"), utf8_bytes("a\nb"));
}
