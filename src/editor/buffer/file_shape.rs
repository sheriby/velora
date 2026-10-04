//! 打开文件时从磁盘字节记下的「形状」，保存时用它把文本还原回**同样的字节**。
//!
//! 缓冲区内部一律是解码后的规范文本、行尾一律 LF。所以 CRLF 与非 UTF-8 是
//! 形状信息而不是文本内容——这样解析器、搜索、行号都不用去容忍 `\r\n`，
//! 字节保真也不必给每种怪形状加分支。
//!
//! 有无末行换行**不在这里记**：缓冲区文本末尾有没有 `\n` 就是它的答案。
//! （旧实现是在导入时把尾部空行段丢掉，那才是末行换行丢失的原因。）

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
    /// 中文 Windows 常见；解码走 encoding_rs GB18030，保存按同编码写回。
    Gb18030,
    /// 无法无损解码（BOM 或坏字节，走了 lossy 兜底）。保存按 UTF-8 写出。
    Lossy,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct FileShape {
    pub(crate) encoding: TextEncoding,
    pub(crate) line_ending: LineEnding,
}

impl FileShape {
    /// 从磁盘原始字节判定形状。`text` 是这些字节解码并把行尾规范成 LF 之后的文本。
    pub(crate) fn detect(raw: &[u8]) -> Self {
        Self {
            encoding: detect_encoding(raw),
            line_ending: detect_line_ending(raw),
        }
    }

    /// 把规范文本还原成磁盘字节。
    ///
    /// 缓冲区本该是 LF 规范文本；万一有路径漏了归一（比如剪贴板带进来的 `\r`），
    /// 这里是最后一道闸：先折平再升格，绝不把 `\r\r\n` 写上磁盘——那不只是多出
    /// 空行，还会让版本号校验把自己写的文件误判成外部修改。
    pub(crate) fn encode(&self, text: &str) -> Vec<u8> {
        let text: Cow<'_, str> = if text.contains('\r') {
            Cow::Owned(text.replace("\r\n", "\n").replace('\r', "\n"))
        } else {
            Cow::Borrowed(text)
        };
        let body = match self.line_ending {
            LineEnding::LF => text,
            LineEnding::CRLF => Cow::Owned(text.replace('\n', "\r\n")),
        };
        match self.encoding {
            TextEncoding::Utf8 | TextEncoding::Lossy => body.into_owned().into_bytes(),
            TextEncoding::Gb18030 => encoding_rs::GB18030.encode(&body).0.into_owned(),
        }
    }
}

fn detect_encoding(raw: &[u8]) -> TextEncoding {
    if std::str::from_utf8(raw).is_ok() {
        return TextEncoding::Utf8;
    }
    if raw.starts_with(&[0xFF, 0xFE]) || raw.starts_with(&[0xFE, 0xFF]) {
        return TextEncoding::Lossy;
    }
    let (_, _, had_errors) = encoding_rs::GB18030.decode(raw);
    if had_errors {
        TextEncoding::Lossy
    } else {
        TextEncoding::Gb18030
    }
}

/// 只有「所有换行都是 `\r\n`、且没有单独的 `\r`」才算 CRLF：混进别的形态时
/// 无法由 LF 文本还原，按 LF 处理（宁可少还原一处，也不要把 LF 升格成 CRLF）。
fn detect_line_ending(raw: &[u8]) -> LineEnding {
    let cr = raw.iter().filter(|byte| **byte == b'\r').count();
    let lf = raw.iter().filter(|byte| **byte == b'\n').count();
    let crlf = raw.windows(2).filter(|pair| pair == b"\r\n").count();
    if crlf > 0 && cr == crlf && lf == crlf {
        LineEnding::CRLF
    } else {
        LineEnding::LF
    }
}
