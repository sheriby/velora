use super::*;

/// Links in mixed inline-visual blocks (math, scripts, inline images) render as
/// plain divs, so this sets `PointingHand` whenever its hitbox is hovered, like
/// `BlockTextElement` does for normal text. Layout and painting are delegated
/// to the child.
pub(crate) struct LinkFollowCursor {
    pub(crate) child: AnyElement,
}

impl IntoElement for LinkFollowCursor {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for LinkFollowCursor {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if hitbox.is_hovered(window) {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        }
        self.child.paint(window, cx);
    }
}

use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn inline_word_chunks(text: &str, code: bool, has_background: bool) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    if code || has_background {
        return vec![text];
    }
    let mut chunks = Vec::new();
    for word in text.split_inclusive(char::is_whitespace) {
        // CJK 没有空格，整段会成为一个 flex 项：超宽后内部换行，相邻的行内
        // 公式/图片被整体挤到下一行。按字素簇限量切成可折行的块；拉丁长词
        // 保持原样（不在词内断开）。
        let needs_grapheme_chunks = !word.chars().any(char::is_whitespace)
            && word.chars().any(is_cjk)
            && word.chars().count() > CJK_CHUNK_CHARS;
        if !needs_grapheme_chunks {
            chunks.push(word);
            continue;
        }
        let mut start = 0usize;
        let mut count = 0usize;
        for (index, _) in word.grapheme_indices(true) {
            if count == CJK_CHUNK_CHARS {
                chunks.push(&word[start..index]);
                start = index;
                count = 0;
            }
            count += 1;
        }
        if start < word.len() {
            chunks.push(&word[start..]);
        }
    }
    chunks
}

/// 一个 CJK 折行块最多几个字素。
const CJK_CHUNK_CHARS: usize = 12;

fn is_cjk(ch: char) -> bool {
    matches!(
        ch as u32,
        0x2E80..=0x9FFF | 0xAC00..=0xD7AF | 0x20000..=0x2FA1F
    )
}
