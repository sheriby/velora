//! 本地补丁（velora）：见 `docs/architecture/testing-and-build.md` §7 第 9 条。
//!
//! `CF_HTML`, the Windows clipboard flavor that carries copied HTML.
//!
//! The payload is a set of ASCII `Key:Value` header lines followed by the
//! markup; `StartFragment`/`EndFragment` are byte offsets that mark the part
//! the user selected. Only that part belongs in `ClipboardItem::html`, the way
//! the macOS pasteboard's `public.html` flavor carries markup alone.

/// Extracts the HTML from a `CF_HTML` clipboard payload. Returns `None` when
/// the payload holds no markup.
pub fn cf_html_fragment(payload: &[u8]) -> Option<String> {
    // The header is ASCII key/value lines; the markup starts at the first `<`.
    let header_end = payload
        .iter()
        .position(|byte| *byte == b'<')
        .unwrap_or(payload.len());

    let start = offset(payload, header_end, "StartFragment")
        .or_else(|| offset(payload, header_end, "StartHTML"))
        .unwrap_or(header_end);
    let end = offset(payload, header_end, "EndFragment")
        .or_else(|| offset(payload, header_end, "EndHTML"))
        .unwrap_or(payload.len());
    let (start, end) = (start.min(payload.len()), end.min(payload.len()));
    if start >= end {
        return None;
    }

    // A payload that is not valid UTF-8 (a Chinese-locale Word or Write clipboard
    // writes its declared code page) is not a flavor we can show: decoding it
    // lossily would put U+FFFD runs into the document and let them be saved back.
    // Reporting no flavor lets the paste fall back to the plain-text one.
    let fragment = String::from_utf8(payload[start..end].to_vec()).ok()?;
    let fragment = fragment.trim().to_string();
    if fragment.is_empty() {
        None
    } else {
        Some(fragment)
    }
}

/// Reads one header value. `header_end` bounds the search to the header, so a
/// `StartHTML` mentioned inside the markup itself cannot be picked up.
fn offset(payload: &[u8], header_end: usize, key: &str) -> Option<usize> {
    let header = std::str::from_utf8(&payload[..header_end]).ok()?;
    header.split(['\r', '\n']).find_map(|line| {
        let value = line.strip_prefix(key)?.strip_prefix(':')?;
        value.trim().parse().ok()
    })
}
