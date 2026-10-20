//! File URL parsing for platform open-file events, and the repository's only
//! rules for percent decoding and for Windows verbatim (`\\?\`) paths.
//!
//! 图片目标（`src/components/markdown/image.rs:resolve_image_source`）与导出锚点
//! （`src/export/html.rs`）以前各自带一份 `%XX` 解码器，同一个写法在三个入口解出
//! 三个结果。解码口径只留这一份：任何「路径里可能有 percent 转义」的地方都调它。

use std::borrow::Cow;
use std::path::{Path, PathBuf};

pub(crate) fn is_markdown_document(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        let extension = extension.to_string_lossy();
        extension.eq_ignore_ascii_case("md") || extension.eq_ignore_ascii_case("markdown")
    })
}

/// 文本类点文件：整个名字就是「扩展名」，`Path::extension()` 对它们返回
/// None，只能按文件名认。
const TEXT_DOTFILES: &[&str] = &[".env", ".gitignore", ".dockerignore", ".editorconfig"];

/// 判断是否为文本文档（AI 工具支持的文件类型）。
///
/// 包括 Markdown、纯文本、配置文件等常见文本格式。二进制文件（图片、视频、
/// 压缩包等）不在此列。
pub(crate) fn is_text_document(path: &Path) -> bool {
    match path.extension() {
        Some(extension) => {
            let extension = extension.to_string_lossy();
            matches!(
                extension.to_lowercase().as_str(),
                // Markdown
                "md" | "markdown" |
                // 纯文本
                "txt" |
                // 配置文件
                "json" | "toml" | "yaml" | "yml" | "xml" | "ini" | "conf" | "cfg" |
                // 代码文件（常见语言）
                "rs" | "js" | "ts" | "jsx" | "tsx" | "py" | "java" | "c" | "cpp" | "h" | "hpp" |
                "go" | "rb" | "php" | "swift" | "kt" | "cs" | "sh" | "bash" | "zsh" |
                // 标记语言
                "html" | "css" | "scss" | "less" | "svg" |
                // 其他文本格式
                "log" | "csv" | "env" | "gitignore" | "dockerignore" | "editorconfig"
            )
        }
        None => path.file_name().is_some_and(|name| {
            let name = name.to_string_lossy().to_ascii_lowercase();
            TEXT_DOTFILES.contains(&name.as_str())
        }),
    }
}

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

/// RFC 3986 的 scheme 语法：字母开头，后跟字母 / 数字 / `+` / `-` / `.`，以 `:` 收尾。
/// 返回不含冒号的 scheme 切片。`note:backup.png` 的 `note` 同样是合法 scheme——
/// 认不认由调用方决定，这里只管语法。
pub(crate) fn url_scheme(value: &str) -> Option<&str> {
    let colon = value.find(':')?;
    let scheme = &value[..colon];
    let mut chars = scheme.chars();
    if !chars.next()?.is_ascii_alphabetic() {
        return None;
    }
    if !chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.')) {
        return None;
    }
    Some(scheme)
}

/// 剥掉 `file:` scheme 前缀，按 RFC 3986 大小写不敏感（`FILE:` 与 `file:` 是同一个
/// scheme）。值不带 `file:` scheme 时返回 `None`，调用方保留原文——带冒号的相对
/// 文件名（`note:backup.png`）不许在这里被误剥。
pub(crate) fn strip_file_scheme_prefix(value: &str) -> Option<&str> {
    let is_file = url_scheme(value).is_some_and(|scheme| scheme.eq_ignore_ascii_case("file"));
    // scheme 恰为 4 个 ASCII 字母加冒号，第 5 字节必是字符边界。
    is_file.then(|| &value["file:".len()..])
}

/// 链接目标里的锚点切分：第一个裸 `#` 前是路径/目标，后面是锚点。
pub(crate) fn split_link_anchor(target: &str) -> (&str, Option<&str>) {
    match target.find('#') {
        Some(index) => (&target[..index], Some(&target[index + 1..])),
        None => (target, None),
    }
}

/// 本地链接的落点：绝对路径直接用，相对路径按基准目录解析。基准由
/// `Editor::local_link_base_dir` 一处决定（文档父目录 → 工作区根），与图片
/// base_dir 同源；两边都没有基准时退相对路径（按进程 CWD 判存在性）。
pub(crate) fn resolve_local_link_path(base_dir: Option<&Path>, path: &str) -> PathBuf {
    let candidate = PathBuf::from(path);
    if candidate.is_absolute() {
        return candidate;
    }
    base_dir.unwrap_or(Path::new(".")).join(candidate)
}

/// 绝对 `file:` URL → 本地绝对路径。scheme、authority 与 Windows 盘符是 URL
/// 语法，交给 `url` crate（与粘贴图片同一套）；它同时负责 percent 解码。
/// 畸形 URL 与远程 `file:` authority 返回 `None`。
pub(crate) fn local_path_from_file_url(destination: &str) -> Option<PathBuf> {
    let file_url = url::Url::parse(destination).ok()?;
    let path = file_url.to_file_path().ok()?;
    path.is_absolute().then_some(path)
}

/// Windows 的 verbatim 前缀的口径（见 [`strip_verbatim_prefix`]）。
///
/// `canonicalize` + 去前缀合成一处，是因为两件事总是要一起做：解析 `.`/软链要靠前者，
/// 落到状态与界面里要用后者。丢掉前缀不会丢掉长路径能力——大于 MAX_PATH 时 `std` 自己
/// 会临时拼上 verbatim 前缀（`std::sys::path` 内部做的），调用方不必操心。
pub(crate) fn canonical_path(path: impl AsRef<Path>) -> PathBuf {
    let path = path.as_ref();
    strip_verbatim_prefix(&std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf()))
}

/// 去掉 Windows 的 verbatim 前缀，还原成用户认得的写法。
///
/// `std::fs::canonicalize` 在 Windows 上返回 `\\?\C:\Users\…`（UNC 是
/// `\\?\UNC\server\share\…`）。这个前缀是给 Win32 绕过 MAX_PATH 与规范化用的，
/// 不该出现在最近列表、会话文件或标签标题里——「打开最近」整列 `\\?\` 就是这么来的。
/// 只剥前缀：`\\?\C:\a` 与 `\\?\UNC\server\share` 剥完都仍是同一条路径，可用。
///
/// 非 Windows 平台与不带前缀的路径原样返回（判断只此一处，别在调用点再写一遍）。
pub(crate) fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let Some(text) = path.to_str() else {
        // 非法 UTF-8 的路径（Windows 上可能出现）没法安全剥前缀，原样留着。
        return path.to_path_buf();
    };
    let Some(rest) = text.strip_prefix(r"\\?\") else {
        return path.to_path_buf();
    };
    // UNC：`\\?\UNC\server\share\…` → `\\server\share\…`。
    if rest
        .get(..4)
        .is_some_and(|head| head.eq_ignore_ascii_case("UNC\\"))
    {
        return PathBuf::from(format!(r"\\{}", &rest[4..]));
    }
    // 盘符：`\\?\C:\…` → `C:\…`。没盘符的 verbatim 路径（`\\?\Volume{…}\`）剥完
    // 就不再是绝对路径了，宁可不剥——它还不到用户眼前，但至少能用。
    let is_drive = matches!(
        rest.as_bytes(),
        [letter, b':', b'\\', ..] if letter.is_ascii_alphabetic()
    );
    if is_drive {
        PathBuf::from(rest)
    } else {
        path.to_path_buf()
    }
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
    use super::{
        is_text_document, parse_file_url, percent_decode, percent_decode_or_raw,
        strip_verbatim_prefix, url_scheme,
    };
    use crate::file_url::strip_file_scheme_prefix;
    use std::path::{Path, PathBuf};

    /// AI 文档工具的文件类型判据只看扩展名：常见文本格式进名单，二进制与
    /// 无扩展名的文件不进。
    #[test]
    fn text_document_accepts_text_extensions() {
        for name in [
            "note.md",
            "NOTE.MD",
            "readme.markdown",
            "todo.txt",
            "config.json",
            "Cargo.toml",
            "ci.yaml",
            "feed.xml",
            "main.rs",
            "app.tsx",
            "script.py",
            "index.html",
            "theme.css",
            "trace.log",
            "table.csv",
            "notes/子目录/deep.md",
        ] {
            assert!(is_text_document(Path::new(name)), "{name} 应识别为文本文档");
        }
    }

    /// 点文件的整个名字就是「扩展名」，`Path::extension()` 返回 None，
    /// 判据必须自己看文件名，否则名单里永远缺这几个配置文件。
    #[test]
    fn text_document_accepts_config_dotfiles() {
        for name in [
            ".env",
            ".gitignore",
            ".dockerignore",
            ".editorconfig",
            "deploy/.env",
            ".ENV",
        ] {
            assert!(is_text_document(Path::new(name)), "{name} 应识别为文本文档");
        }
    }

    #[test]
    fn text_document_rejects_binaries_and_extension_less_names() {
        for name in [
            "cover.png",
            "photo.jpeg",
            "clip.mp4",
            "bundle.zip",
            "report.pdf",
            "archive.tar.gz",
            "LICENSE",
            "Makefile",
            "notes",
        ] {
            assert!(
                !is_text_document(Path::new(name)),
                "{name} 不是文本文档：二进制或没有扩展名"
            );
        }
    }

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

    #[test]
    fn scheme_syntax_follows_rfc3986() {
        // 合法：字母开头，后跟字母/数字/+/-/.。
        assert_eq!(url_scheme("https://example.com"), Some("https"));
        assert_eq!(url_scheme("FILE:///tmp/a.png"), Some("FILE"));
        assert_eq!(url_scheme("x+y-z.1:rest"), Some("x+y-z.1"));
        // `note:backup.png` 的 `note` 语法上就是 scheme，认不认由调用方定。
        assert_eq!(url_scheme("note:backup.png"), Some("note"));
        // 非法：数字开头、空 scheme、无冒号。
        assert_eq!(url_scheme("1note:x"), None);
        assert_eq!(url_scheme(":rest"), None);
        assert_eq!(url_scheme("plain-path.png"), None);
    }

    #[test]
    fn strips_file_scheme_prefix_case_insensitively() {
        assert_eq!(strip_file_scheme_prefix("file:relative.png"), Some("relative.png"));
        assert_eq!(strip_file_scheme_prefix("FILE:relative.png"), Some("relative.png"));
        assert_eq!(strip_file_scheme_prefix("File:///tmp/a.png"), Some("///tmp/a.png"));
        // 非 file 的 scheme 与无 scheme 都不动原文。
        assert_eq!(strip_file_scheme_prefix("note:backup.png"), None);
        assert_eq!(strip_file_scheme_prefix("assets/a.png"), None);
    }

    #[test]
    fn strips_the_windows_verbatim_prefix() {
        // canonicalize 在 Windows 上给的两种形态：盘符与 UNC。
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\C:\Users\me\notes")),
            PathBuf::from(r"C:\Users\me\notes")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\UNC\server\share\notes")),
            PathBuf::from(r"\\server\share\notes")
        );
        // 已是正常写法、以及 Unix 路径：原样返回，不动字节。
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"C:\Users\me\notes")),
            PathBuf::from(r"C:\Users\me\notes")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new("/Users/me/notes")),
            PathBuf::from("/Users/me/notes")
        );
        // 剥完没内容的（光秃秃的前缀）、以及没有盘符的 volume 路径：原样留着，
        // 不能交出空路径或相对路径。
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\")),
            PathBuf::from(r"\\?\")
        );
        assert_eq!(
            strip_verbatim_prefix(Path::new(r"\\?\Volume{9f2c}\notes")),
            PathBuf::from(r"\\?\Volume{9f2c}\notes")
        );
    }
}
