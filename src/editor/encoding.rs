//! 文档文件的读盘漏斗：一次嗅探给出「原始字节 + 规范文本 + 文件形状」。
//!
//! 三条规则都在这里，也只有这里：
//! 1. **真解码**。UTF-8、带 BOM 的 UTF-16LE/BE、GB18030 各按各自的编码解；
//!    UTF-8 BOM 与 UTF-16 BOM 都剥掉（BOM 属于形状，见 `buffer::file_shape`），
//!    解析器因此永远看不到那个零宽字符（报修第 8 条：首行 `# 标题` 被 BOM 挡成段落）。
//! 2. **能无损写回才收**。形状不能把文本重编码成原字节（坏字节、被截断的
//!    UTF-16、孤立代理项、含 NUL 的字节流）就直接拒绝打开——这是报修第 1 条
//!    「打开 UTF-16、打一个字、保存，中文没了」的根因所在：以前 lossy 文本照样
//!    进编辑器，保存再按 UTF-8 写回去，等于把用户的文件覆盖成垃圾。
//! 3. **所有入口共用这一趟**。工作区标签、拖拽替换、菜单/CLI 开窗、外部改动
//!    重载都调 [`load_document`]，字节与文本来自同一次读盘，形状由文本一起带出去。
//!    将来新增入口只需记住走这里，就不会再漏掉编码。
//!
//! 解码只在打开时发生一次，热路径零开销。

use std::io;
use std::path::Path;

use super::buffer::FileShape;

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
///
/// 不能无损写回磁盘的编码返回 [`io::ErrorKind::InvalidData`]，理由是给用户的
/// 文案（各入口都把它送进应用内模态/错误条，不写盘、不覆盖）。
pub(crate) fn load_document(path: &Path) -> io::Result<LoadedDocument> {
    let raw = std::fs::read(path)?;
    let (shape, text) = FileShape::detect_and_decode(&raw);
    if !shape.is_lossless() {
        return Err(unrepresentable_encoding_error(path));
    }
    Ok(LoadedDocument { raw, text })
}

/// 读并解码一个文档文件，只要文本。
///
/// 与 [`load_document`] 同样拒绝不能无损写回的字节：凡是要把这份文本再写回磁盘
/// 的调用方（跨文件替换、恢复快照比对）都不该拿到它；只读的调用方（索引）本来
/// 就该用 [`decode_document_bytes`]。
pub(crate) fn read_document_string(path: &Path) -> io::Result<String> {
    Ok(load_document(path)?.text)
}

/// 只解码，不判定「能不能保存」：工作区搜索/索引用它读任意文件，读不出来的
/// 字节按 lossy 呈现即可，反正那一趟不写盘。
pub(crate) fn decode_document_bytes(bytes: Vec<u8>) -> String {
    FileShape::detect_and_decode(&bytes).1
}

/// 拒绝打开的理由：说清楚「为什么不能打开」和「文件还在原地」。
fn unrepresentable_encoding_error(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "无法以不损坏文件的方式打开「{}」：这个文件的编码不能无损写回（可能是二进制文件，\
             或被截断／含孤立代理项的 UTF-16）。原文件保持不动；需要的话请先用其他工具转成 \
             UTF-8 再打开。",
            path.display()
        ),
    )
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

    /// UTF-16 夹具：BOM + 逐 code unit 的 LE/BE 字节。
    fn utf16_le(text: &str, bom: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        if bom {
            bytes.extend_from_slice(&[0xFF, 0xFE]);
        }
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    fn utf16_be(text: &str, bom: bool) -> Vec<u8> {
        let mut bytes = Vec::new();
        if bom {
            bytes.extend_from_slice(&[0xFE, 0xFF]);
        }
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        bytes
    }

    #[test]
    fn utf16_le_with_bom_decodes_to_the_users_text() {
        // 报修 1：UTF-16 必须真解码。以前这里走 `from_utf8_lossy`，用户的中文
        // 原文在打开那一刻就被换成替换字符，保存再按 UTF-8 写回去——文件没了。
        let text = "# 会议纪要\n\n中文正文 🎉\n第二行\n";
        assert_eq!(decode_document_bytes(utf16_le(text, true)), text);
    }

    #[test]
    fn utf16_be_with_bom_decodes_to_the_users_text() {
        let text = "# 会议纪要\n\n中文正文 🎉\n第二行\n";
        assert_eq!(decode_document_bytes(utf16_be(text, true)), text);
    }

    #[test]
    fn utf16_surrogate_pairs_and_cjk_decode_without_replacement_chars() {
        for text in ["emoji 🎉🚀 混排 中文\n", "\u{1F600}\u{1F601}\n"] {
            for bytes in [utf16_le(text, true), utf16_be(text, true)] {
                let decoded = decode_document_bytes(bytes);
                assert!(!decoded.contains('\u{FFFD}'), "解码出替换符：{decoded:?}");
                assert_eq!(decoded, text);
            }
        }
    }

    #[test]
    fn utf8_bom_is_stripped_from_the_decoded_text() {
        // 报修 8：BOM 不是正文。留着它，首行的 `# 标题` 就成了「\u{feff}# 标题」，
        // 解析器认不出标记，整行按段落渲染。剥在解码这一层，保存按形状贴回去。
        assert_eq!(
            decode_document_bytes("\u{feff}# Title\n\n正文\n".as_bytes().to_vec()),
            "# Title\n\n正文\n"
        );
        // BOM 只剥行首那一个：文中出现的零宽不换行空格是用户的字，不许动。
        assert_eq!(
            decode_document_bytes("正文\u{feff}尾\n".as_bytes().to_vec()),
            "正文\u{feff}尾\n"
        );
    }

    #[test]
    fn truncated_or_unpaired_utf16_is_not_decoded_as_a_document() {
        // 孤立代理项、被截断的奇数长度：重编码回去不是原字节 → 不可无损表示。
        for bytes in [
            vec![0xFF, 0xFE, 0x00, 0xD8, 0x00, 0x00],
            vec![0xFF, 0xFE, 0x68],
            vec![0xFE, 0xFF, 0x00],
        ] {
            let (shape, _) = FileShape::detect_and_decode(&bytes);
            assert!(!shape.is_lossless(), "这些字节不该被收下：{shape:?}");
        }
    }

    #[test]
    fn bom_less_utf16_is_refused_instead_of_guessed() {
        // 无 BOM 的 UTF-16 不猜编码：它的字节在 GB18030 眼里是「拉丁字母 + NUL」，
        // 按 GB18030 收下就等于承诺能写回，而正文里的 NUL 一经过滤行尾就改写了字节。
        // 判定为不可无损表示，交给上层拒绝打开。
        let bytes: Vec<u8> = vec![0x41, 0x00, 0x42, 0x00, 0x43, 0x00];
        let (shape, _) = FileShape::detect_and_decode(&bytes);
        assert!(!shape.is_lossless(), "{shape:?}");
    }

    #[test]
    fn load_document_refuses_bytes_it_cannot_write_back() {
        let path = std::env::temp_dir().join(format!("velora-enc-refuse-{}.md", uuid::Uuid::new_v4()));
        std::fs::write(&path, [0xFF, 0x00, 0xFF, 0xFF]).expect("write");
        let Err(error) = load_document(&path) else {
            panic!("坏字节不该被当成可编辑文档");
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("无损"), "理由要说清为什么：{error}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn read_document_string_round_trips_utf8_file() {
        let path = std::env::temp_dir().join(format!("velora-enc-{}.md", uuid::Uuid::new_v4()));
        std::fs::write(&path, "# hello\n").expect("write");
        let text = read_document_string(&path).expect("read");
        assert_eq!(text, "# hello\n");
        let _ = std::fs::remove_file(&path);
    }
}
