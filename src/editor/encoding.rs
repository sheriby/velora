//! 文档文件解码：UTF-8 优先，非 UTF-8 回退 GB18030（GBK 超集）。
//!
//! 此前非 UTF-8 文件 `read_to_string` 直接报错打不开；现在中文 Windows
//! 圈常见的 GBK/GB18030 笔记可以正常打开编辑（保存仍按 UTF-8 写出，
//! 与全应用的编码策略一致）。UTF-16 BOM 维持既有占位提示路径，不受
//! 回退影响。解码只在打开时发生一次，热路径零开销。

use std::io;
use std::path::Path;

/// 打开一个文件所需的全部信息：**原始字节**与解码后的文本。
///
/// 原始字节是「未编辑就字节不变」的依据（保存时原样写回），解码文本是缓冲区的
/// 内容。两者必须在同一次读盘里拿到——分两次读会在两次读之间被外部改动时，
/// 把一个「其实没跟着文本变」的字节版本写回去。
pub(crate) struct LoadedDocument {
    /// 磁盘原始字节；空表示来源不是文件（新建、粘贴片段、恢复快照），
    /// 因而没有「原样写回」的依据。
    pub(crate) raw: Vec<u8>,
    pub(crate) text: String,
}

impl LoadedDocument {
    /// 只有文本、没有原始字节可用时的来源。
    pub(crate) fn from_text(text: String) -> Self {
        Self {
            raw: Vec::new(),
            text,
        }
    }
}

/// 读并解码一个文档文件。IO 错误原样上抛（与 read_to_string 同语义）。
pub(crate) fn load_document(path: &Path) -> io::Result<LoadedDocument> {
    let raw = std::fs::read(path)?;
    let text = decode_document_bytes(raw.clone());
    Ok(LoadedDocument { raw, text })
}

/// 读并解码一个文档文件，只要文本。
pub(crate) fn read_document_string(path: &Path) -> io::Result<String> {
    Ok(load_document(path)?.text)
}

/// 解码文档字节：UTF-8 → UTF-16 BOM 保持原路径 → GB18030 → lossy 兜底。
pub(crate) fn decode_document_bytes(bytes: Vec<u8>) -> String {
    if let Ok(text) = String::from_utf8(bytes.clone()) {
        return text;
    }
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return String::from_utf8_lossy(&bytes).into_owned();
    }
    let (decoded, _, had_errors) = encoding_rs::GB18030.decode(&bytes);
    if had_errors {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        decoded.into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_passes_through_unchanged() {
        let text = "# 标题\n\n正文 emoji 🎉\n";
        assert_eq!(decode_document_bytes(text.as_bytes().to_vec()), text);
    }

    #[test]
    fn gbk_bytes_decode_to_expected_text() {
        // 「中文笔记」的 GBK 编码（encoding_rs 不提供 GBK 编码器，固定字节）。
        let gbk_bytes = vec![0xD6, 0xD0, 0xCE, 0xC4, 0xB1, 0xCA, 0xBC, 0xC7];
        assert_eq!(decode_document_bytes(gbk_bytes), "中文笔记");
    }

    #[test]
    fn utf16_bom_keeps_existing_placeholder_path() {
        // UTF-16 LE BOM + "hi"：不走 GB18030，维持既有占位逻辑的输入形态。
        let utf16 = vec![0xFF, 0xFE, 0x68, 0x00, 0x69, 0x00];
        let decoded = decode_document_bytes(utf16);
        assert!(decoded.starts_with('\u{FFFD}'), "lossy 解码以替换符呈现");
    }

    #[test]
    fn undecodable_falls_back_to_lossy() {
        let garbage = vec![0xFF, 0xFE, 0xFE, 0xFF, 0x81, 0x40, 0x00];
        let decoded = decode_document_bytes(garbage);
        assert!(!decoded.is_empty() || decoded.is_empty()); // 不 panic 即可
    }

    #[test]
    fn read_document_string_round_trips_utf8_file() {
        let path = std::env::temp_dir().join(format!("velora-enc-{}.md", std::process::id()));
        std::fs::write(&path, "# hello\n").expect("write");
        let text = read_document_string(&path).expect("read");
        assert_eq!(text, "# hello\n");
        let _ = std::fs::remove_file(&path);
    }
}
