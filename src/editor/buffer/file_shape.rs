//! 打开文件时从磁盘字节记下的「形状」，保存时用它把文本还原回**同样的字节**。
//!
//! 形状 = 编码 + 有没有 BOM + 行尾，三样都是**磁盘上的形状**，不是文本内容：
//! 缓冲区内部一律是解码后的规范文本（BOM 已剥、行尾 LF）。所以每个编码都必须
//! 同时有解码与编码两半——`detect_and_decode` 与 `encode` 是同一个 `FileShape`
//! 的两面，加新编码只在这一处加，两边自然对称。不对称就是数据丢失的形状：
//! UTF-16 以前只有 lossy 兜底没有解码，保存又按 UTF-8 写出去，用户按一个键
//! 就把整篇中文洗成替换符（报修第 1 条）。
//!
//! 有无末行换行**不在这里记**：缓冲区文本末尾有没有 `\n` 就是它的答案。
//! （旧实现是在导入时把尾部空行段丢掉，那才是末行换行丢失的原因。）
//!
//! 「无 BOM 的 UTF-16」刻意不去猜：它与 GB18030／带 NUL 的二进制流在字节上没有
//! 可靠区分法，猜错就是把用户的 GB18030 笔记按 UTF-16 写出去。这类字节由
//! [`TextEncoding::Lossy`] 兜住——正文里出现 NUL 就判定为不可无损表示，
//! 上层（`encoding::load_document`）拒绝打开，绝不写回。

#[cfg(test)]
mod tests;

use std::borrow::Cow;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineEnding {
    LF,
    CRLF,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TextEncoding {
    Utf8,
    /// UTF-16 小端（带 BOM 才会被认出来，见模块头）。
    Utf16Le,
    /// UTF-16 大端（带 BOM 才会被认出来，见模块头）。
    Utf16Be,
    /// 中文 Windows 常见；解码走 encoding_rs GB18030，保存按同编码写回。
    Gb18030,
    /// 无法无损解码：坏字节、被截断的 UTF-16、孤立代理项、含 NUL 的字节流。
    /// 这种形状**不写回磁盘**（见 `encoding::load_document` 与
    /// `TextBuffer::set_file_origin`），保存按 UTF-8 兜出去只是给测试用的退路。
    Lossy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileShape {
    pub(crate) encoding: TextEncoding,
    /// 文件开头那个 BOM 是不是存在过。BOM 属于形状不属于正文：读的时候剥掉，
    /// 写的时候按 `encoding` 对应的字节贴回去，一趟下来字节不变。
    pub(crate) bom: bool,
    pub(crate) line_ending: LineEnding,
}

const UTF8_BOM: &[u8] = &[0xEF, 0xBB, 0xBF];
const UTF16_LE_BOM: &[u8] = &[0xFF, 0xFE];
const UTF16_BE_BOM: &[u8] = &[0xFE, 0xFF];

/// 这段字节开头是不是 UTF-16 的 BOM。
///
/// 「哪些字节算 UTF-16」这份表只准存在一处：判定形状要用它，「这个文件值不值得
/// 预览」那套嗅探也要用它。各处自己抄一份 `[0xFF, 0xFE]` 字面量时，加一种编码
/// （UTF-32、无 BOM 的 UTF-16）就会有一处漏掉，表现为「能打开却搜不到」。
pub(crate) fn starts_with_utf16_bom(head: &[u8]) -> bool {
    head.starts_with(UTF16_LE_BOM) || head.starts_with(UTF16_BE_BOM)
}

impl FileShape {
    /// 从磁盘原始字节判定形状。
    pub(crate) fn detect(raw: &[u8]) -> Self {
        Self::detect_and_decode(raw).0
    }

    /// 唯一的嗅探口：一次给出形状，以及「BOM 已剥、行尾还是原样」的解码文本。
    ///
    /// 判定与解码必须同在这一趟里做完：分两次判就会让「文本是按 A 编码解的、
    /// 形状记的是 B」这种错位有处可生，而错位就是保存改写文件的开始。
    /// 返回 [`TextEncoding::Lossy`] 表示「这份字节我们不能无损写回」——上层据此
    /// 拒绝打开，绝不覆盖原文件。
    pub(crate) fn detect_and_decode(raw: &[u8]) -> (Self, String) {
        sniff(raw).unwrap_or_else(|| (lossy_shape(), String::from_utf8_lossy(raw).into_owned()))
    }

    /// 这个形状的文档能不能安全写回磁盘。假的一律不写（见 `TextEncoding::Lossy`）。
    pub(crate) fn is_lossless(&self) -> bool {
        self.encoding != TextEncoding::Lossy
    }

    /// 把规范文本还原成磁盘字节。
    ///
    /// 缓冲区本该是 LF 规范文本；万一有路径漏了归一（比如剪贴板带进来的 `\r`），
    /// 这里是最后一道闸：先折平再升格，绝不把 `\r\r\n` 写上磁盘——那不只是多出
    /// 空行，还会让版本号校验把自己写的文件误判成外部修改。
    pub(crate) fn encode(&self, text: &str) -> Vec<u8> {
        let normalized: Cow<'_, str> = if text.contains('\r') {
            Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
        } else {
            Cow::Borrowed(text)
        };
        let body = match self.line_ending {
            LineEnding::LF => normalized,
            LineEnding::CRLF => Cow::Owned(normalized.replace('\n', "\r\n")),
        };
        let mut out: Vec<u8> = Vec::with_capacity(body.len() + 2);
        if self.bom {
            out.extend_from_slice(bom_bytes(self.encoding));
        }
        match self.encoding {
            // UTF-8（含 lossy 退路）：正文本身就是 UTF-8 字节，BOM 已经贴在上面。
            TextEncoding::Utf8 | TextEncoding::Lossy => {
                out.extend_from_slice(body.as_bytes());
            }
            // UTF-16 自己拼 code unit：encoding_rs 的 UTF-16 `encode` 不是解码的
            // 逆操作（它出去的是 UTF-8 字节），只有 `decode` 那半边能用。
            TextEncoding::Utf16Le => {
                for unit in body.encode_utf16() {
                    out.extend_from_slice(&unit.to_le_bytes());
                }
            }
            TextEncoding::Utf16Be => {
                for unit in body.encode_utf16() {
                    out.extend_from_slice(&unit.to_be_bytes());
                }
            }
            TextEncoding::Gb18030 => {
                out.extend_from_slice(&encoding_rs::GB18030.encode(&body).0);
            }
        }
        out
    }
}

/// 一次嗅探：形状 + 「BOM 已剥、行尾原样」的文本；`None` 是不能无损写回。
fn sniff(raw: &[u8]) -> Option<(FileShape, String)> {
    for (marker, encoding) in [
        (UTF16_LE_BOM, TextEncoding::Utf16Le),
        (UTF16_BE_BOM, TextEncoding::Utf16Be),
    ] {
        if !raw.starts_with(marker) {
            continue;
        }
        let decoder = match encoding {
            TextEncoding::Utf16Le => encoding_rs::UTF_16LE,
            _ => encoding_rs::UTF_16BE,
        };
        // BOM 自己不进正文（encoding_rs 的 UTF-16 解码器不会替我们剥它）。
        let (text, _, had_errors) = decoder.decode(&raw[marker.len()..]);
        // 被截断的 code unit、孤立代理项：重编码回去已经不是这几个字节了。
        return if had_errors {
            None
        } else {
            shape_for_text(text.into_owned(), encoding, true)
        };
    }

    if raw.starts_with(UTF8_BOM) {
        if let Some(candidate) = utf8_candidate(&raw[UTF8_BOM.len()..], true) {
            return Some(candidate);
        }
    }

    if let Some(candidate) = utf8_candidate(raw, false) {
        return Some(candidate);
    }

    let (decoded, _, had_errors) = encoding_rs::GB18030.decode(raw);
    if had_errors {
        return None;
    }
    shape_for_text(decoded.into_owned(), TextEncoding::Gb18030, false)
}

/// UTF-8 那一档：字节得先是合法 UTF-8，`bom` 说开头那三个字节算不算形状。
fn utf8_candidate(raw: &[u8], bom: bool) -> Option<(FileShape, String)> {
    let text = std::str::from_utf8(raw).ok()?;
    shape_for_text(text.to_string(), TextEncoding::Utf8, bom)
}

/// 解码文本 + 编码 → 形状；`None` 表示这份文本不能无损写回。
///
/// 一条对所有编码都成立的规则：**正文里有 NUL 的字节流不是文档**。无 BOM 的
/// UTF-16 正好长成这个样子（拉丁文本一半是 NUL），它和二进制没法区分，猜错就是
/// 拿别种编码把用户的文件写回去。行尾在这一起量，量的是解码后的文本。
fn shape_for_text(text: String, encoding: TextEncoding, bom: bool) -> Option<(FileShape, String)> {
    if text.contains('\u{0}') {
        return None;
    }
    let shape = FileShape {
        encoding,
        bom,
        line_ending: detect_line_ending(&text),
    };
    Some((shape, text))
}

fn lossy_shape() -> FileShape {
    FileShape {
        encoding: TextEncoding::Lossy,
        bom: false,
        line_ending: LineEnding::LF,
    }
}

fn bom_bytes(encoding: TextEncoding) -> &'static [u8] {
    match encoding {
        TextEncoding::Utf16Le => UTF16_LE_BOM,
        TextEncoding::Utf16Be => UTF16_BE_BOM,
        _ => UTF8_BOM,
    }
}

/// 只有「所有换行都是 `\r\n`、且没有单独的 `\r`」才算 CRLF：混进别的形态时
/// 无法由 LF 文本还原，按 LF 处理（宁可少还原一处，也不要把 LF 升格成 CRLF）。
///
/// 量的是**解码后的文本**，不是磁盘字节：UTF-16 里 `0x0D`/`0x0A` 只是某个汉字
/// 的半个 code unit（「不」U+4E0D 的小端字节就是 `0D 4E`），按字节数会把 UTF-16
/// 文件判成混排行尾。
fn detect_line_ending(text: &str) -> LineEnding {
    let cr = text.matches('\r').count();
    let lf = text.matches('\n').count();
    let crlf = text.match_indices("\r\n").count();
    if crlf > 0 && cr == crlf && lf == crlf {
        LineEnding::CRLF
    } else {
        LineEnding::LF
    }
}
