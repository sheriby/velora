//! File URL parsing for platform open-file events, and the repository's only
//! percent-decoding rule.
//!
//! 图片目标（`src/components/markdown/image.rs:resolve_image_source`）与导出锚点
//! （`src/export/html.rs`）以前各自带一份 `%XX` 解码器，同一个写法在三个入口解出
//! 三个结果。解码口径只留这一份：任何「路径里可能有 percent 转义」的地方都调它。

use std::borrow::Cow;
use std::path::PathBuf;

/// Parses a local file URL into a path.
///
/// GPUI's macOS `on_open_urls` callback provides `file://` URLs, while tests
/// and other callers may pass plain paths. Only local `file://` URLs are
/// accepted; non-file schemes and remote file authorities are rejected.
pub(crate) fn parse_file_url(value: &str) -> Option<PathBuf> {
    if let Some(rest) = value.strip_prefix("file://") {
        let path = if let Some(path) = rest.strip_prefix("localhost/") {
            format!("/{path}")
        } else if rest.starts_with('/') {
            rest.to_string()
        } else {
            return None;
        };

        // `file:` URL 由程序生成，串里的裸 `%` 说明这个 URL 本身就是畸形的：
        // 不猜它想表达什么，直接拒绝（普通 Markdown 写法相反，见 `percent_decode`）。
        if has_invalid_percent_escape(&path) {
            return None;
        }

        return percent_decode(&path).map(|decoded| PathBuf::from(decoded.as_ref()));
    }

    if value.contains("://") {
        return None;
    }

    Some(PathBuf::from(value))
}

/// 唯一的 percent 解码：`%XX` 还原成对应字节。非法转义（`%` 后面不跟两个十六进制
/// 数字，含串尾的裸 `%`）按字面保留——`assets/100% done.png` 是真实存在的文件名，
/// 把它改坏就找不到文件了。解码结果不是合法 UTF-8 时返回 `None`。
fn percent_decode(value: &str) -> Option<Cow<'_, str>> {
    if !value.contains('%') {
        return Some(Cow::Borrowed(value));
    }

    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0usize;
    while let Some(byte) = bytes.get(index).copied() {
        match percent_escape_value(bytes, index) {
            Some(decoded_byte) => {
                decoded.push(decoded_byte);
                index += 3;
            }
            None => {
                decoded.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8(decoded).map(Cow::Owned).ok()
}

/// 解码路径里的 percent 转义；解不动（结果不是合法 UTF-8）就退回原文。
/// 图片目标与内部锚点都用这个口径：宁可找不到文件，也不把名字改坏。
pub(crate) fn percent_decode_or_raw(value: &str) -> Cow<'_, str> {
    percent_decode(value).unwrap_or(Cow::Borrowed(value))
}

/// 串里是否存在非法 `%` 转义（`%` 后不跟两个十六进制数字，含串尾的裸 `%`）。
fn has_invalid_percent_escape(value: &str) -> bool {
    let bytes = value.as_bytes();
    let mut index = 0usize;
    while let Some(byte) = bytes.get(index).copied() {
        if byte != b'%' {
            index += 1;
            continue;
        }
        if percent_escape_value(bytes, index).is_none() {
            return true;
        }
        index += 3;
    }
    false
}

/// `index` 处若是合法的 `%XX`，返回解码后的字节；不是 `%` 开头或数字非法给 `None`。
fn percent_escape_value(bytes: &[u8], index: usize) -> Option<u8> {
    if bytes.get(index) != Some(&b'%') {
        return None;
    }
    let high = hex_value(*bytes.get(index + 1)?)?;
    let low = hex_value(*bytes.get(index + 2)?)?;
    Some((high << 4) | low)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_file_url, percent_decode, percent_decode_or_raw};
    use std::path::PathBuf;

    #[test]
    fn parses_file_url_with_spaces() {
        assert_eq!(
            parse_file_url("file:///Users/example/My%20Notes/test%20file.md"),
            Some(PathBuf::from("/Users/example/My Notes/test file.md"))
        );
    }

    #[test]
    fn parses_file_url_with_unicode() {
        assert_eq!(
            parse_file_url("file:///Users/example/Notes/%E2%9C%93-%E6%96%87.md"),
            Some(PathBuf::from("/Users/example/Notes/✓-文.md"))
        );
    }

    #[test]
    fn parses_localhost_authority() {
        assert_eq!(
            parse_file_url("file://localhost/Users/example/test.md"),
            Some(PathBuf::from("/Users/example/test.md"))
        );
    }

    #[test]
    fn rejects_non_file_scheme() {
        assert_eq!(parse_file_url("https://example.com/test.md"), None);
    }

    #[test]
    fn passes_plain_path_through() {
        assert_eq!(
            parse_file_url("notes/100% literal.md"),
            Some(PathBuf::from("notes/100% literal.md"))
        );
    }

    #[test]
    fn rejects_remote_file_authority() {
        assert_eq!(parse_file_url("file://example.com/share/test.md"), None);
    }

    #[test]
    fn rejects_file_url_with_malformed_escape() {
        // 钉住既有行为：畸形的 `file:` URL 整条拒绝，不猜它想表达哪个文件。
        assert_eq!(parse_file_url("file:///Users/example/100%.md"), None);
        assert_eq!(parse_file_url("file:///Users/example/%zz.md"), None);
    }

    #[test]
    fn keeps_invalid_percent_escapes_literal_when_decoding() {
        assert_eq!(
            percent_decode_or_raw("assets/100% done.png").as_ref(),
            "assets/100% done.png"
        );
        // 同一串里合法与非法转义混着写，合法的照解、非法的原样留着。
        assert_eq!(
            percent_decode_or_raw("assets/100%25 blue%20card.png").as_ref(),
            "assets/100% blue card.png"
        );
    }

    #[test]
    fn gives_up_when_decoded_bytes_are_not_utf8() {
        assert!(percent_decode("%FF%FE").is_none());
        assert_eq!(percent_decode_or_raw("%FF%FE").as_ref(), "%FF%FE");
    }
}
