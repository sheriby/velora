//! GPUI [`EntityInputHandler`] implementation for Block.
//!
//! Bridges between GPUI's UTF-16-based IME subsystem and the block's
//! internal UTF-8 representation.  All range arguments from GPUI arrive
//! as UTF-16 offsets and are converted through `range_from_utf16` before
//! operating on the block's title.

use std::ops::Range;

use gpui::*;

use super::Block;
use super::element;
use crate::components::{BlockEvent, UndoCaptureKind};

impl EntityInputHandler for Block {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        if self.code_language_focus_handle.is_focused(_window) {
            let range = self.code_language_range_from_utf16(&range_utf16);
            actual_range.replace(self.code_language_range_to_utf16(&range));
            return Some(self.code_language_text()[range].to_string());
        }

        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.display_text()[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        if self.code_language_focus_handle.is_focused(_window) {
            return Some(UTF16Selection {
                range: self.code_language_range_to_utf16(&self.code_language_selected_range),
                reversed: self.code_language_selection_reversed,
            });
        }

        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        if self.code_language_focus_handle.is_focused(window) {
            return self
                .code_language_marked_range
                .as_ref()
                .map(|range| self.code_language_range_to_utf16(range));
        }

        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, _cx: &mut Context<Self>) {
        if self.code_language_focus_handle.is_focused(window) {
            self.code_language_marked_range = None;
            return;
        }

        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.code_language_focus_handle.is_focused(_window) {
            let visible_range = range_utf16
                .as_ref()
                .map(|range| self.code_language_range_from_utf16(range))
                .or(self.code_language_marked_range.clone())
                .unwrap_or(self.code_language_selected_range.clone());
            self.replace_code_language_text_in_range(visible_range, new_text, None, false, cx);
            return;
        }

        if self.editor_selection_range.is_some() {
            cx.emit(BlockEvent::RequestReplaceCrossBlockSelection {
                text: new_text.to_string(),
                selected_range_relative: None,
                mark_inserted_text: false,
                undo_kind: if self.marked_range.is_some() {
                    UndoCaptureKind::ImeCompositionCommit
                } else {
                    UndoCaptureKind::CoalescibleText
                },
            });
            return;
        }

        let undo_kind = if self.marked_range.is_some() {
            UndoCaptureKind::ImeCompositionCommit
        } else {
            UndoCaptureKind::CoalescibleText
        };
        self.prepare_undo_capture(undo_kind, cx);
        let mut visible_range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let smart_punctuation = crate::config::EditorSettings::smart_punctuation(cx)
            && !self.kind().is_code_block()
            && !self.uses_raw_text_editing();
        // Wrap a non-empty selection with the typed paired symbol (roadmap
        // B4): typing `*` over a selection becomes *selection*, etc.
        if let Some((mut open, mut close)) = wrap_pair_for(new_text)
            && !visible_range.is_empty()
            && !self.display_text()[visible_range.clone()].contains('\n')
        {
            if smart_punctuation && new_text == "\"" {
                open = "\u{201c}";
                close = "\u{201d}";
            } else if smart_punctuation && new_text == "'" {
                open = "\u{2018}";
                close = "\u{2019}";
            }
            let selected = self.display_text()[visible_range.clone()].to_string();
            let wrapped = format!("{open}{selected}{close}");
            self.replace_text_in_visible_range(
                visible_range.clone(),
                &wrapped,
                None,
                false,
                cx,
            );
            let inner_start = visible_range.start + open.len();
            self.selected_range = inner_start..inner_start + selected.len();
            cx.notify();
            return;
        }
        let replacement = if smart_punctuation {
            smart_punctuation_replacement(new_text, &self.display_text()[..visible_range.start])
                .map(|(consumed_prefix, replacement)| {
                    visible_range.start -= consumed_prefix;
                    replacement
                })
        } else {
            None
        };
        match replacement {
            Some(replacement) => {
                self.replace_text_in_visible_range(visible_range, &replacement, None, false, cx)
            }
            None => {
                self.replace_text_in_visible_range(visible_range, new_text, None, false, cx)
            }
        }
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.code_language_focus_handle.is_focused(_window) {
            let visible_range = range_utf16
                .as_ref()
                .map(|range| self.code_language_range_from_utf16(range))
                .or(self.code_language_marked_range.clone())
                .unwrap_or(self.code_language_selected_range.clone());
            let sanitized_new_text = new_text.replace("\r\n", " ").replace(['\r', '\n'], " ");
            let selected_range_relative = new_selected_range_utf16
                .as_ref()
                .map(|range_utf16| Self::utf16_range_to_utf8_in(&sanitized_new_text, range_utf16))
                .map(|relative| relative.start..relative.end);

            self.replace_code_language_text_in_range(
                visible_range,
                &sanitized_new_text,
                selected_range_relative,
                !sanitized_new_text.is_empty(),
                cx,
            );
            return;
        }

        if self.editor_selection_range.is_some() {
            let selected_range_relative = new_selected_range_utf16
                .as_ref()
                .map(|range_utf16| Self::utf16_range_to_utf8_in(new_text, range_utf16))
                .map(|relative| relative.start..relative.end);
            cx.emit(BlockEvent::RequestReplaceCrossBlockSelection {
                text: new_text.to_string(),
                selected_range_relative,
                mark_inserted_text: !new_text.is_empty(),
                undo_kind: UndoCaptureKind::ImeComposition,
            });
            return;
        }

        if self.marked_range.is_some() {
            if new_text.is_empty() {
                self.prepare_undo_capture(UndoCaptureKind::ImeCompositionCommit, cx);
            }
        } else if !new_text.is_empty() {
            self.prepare_undo_capture(UndoCaptureKind::ImeComposition, cx);
        }
        let visible_range = range_utf16
            .as_ref()
            .map(|range| self.range_from_utf16(range))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        let selected_range_relative = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| Self::utf16_range_to_utf8_in(new_text, range_utf16))
            .map(|relative| relative.start..relative.end);

        self.replace_text_in_visible_range(
            visible_range,
            new_text,
            selected_range_relative,
            !new_text.is_empty(),
            cx,
        );
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        if self.code_language_focus_handle.is_focused(_window) {
            let line = self.code_language_last_layout.as_ref()?;
            let range = self.code_language_range_from_utf16(&range_utf16);
            let start_x = line.x_for_index(range.start);
            let end_x = line.x_for_index(range.end);
            return Some(Bounds::from_corners(
                point(bounds.left() + start_x, bounds.top()),
                point(bounds.left() + end_x, bounds.bottom()),
            ));
        }

        let lines = self.last_layout.as_ref()?;
        let range = self.range_from_utf16(&range_utf16);
        let line_height = self.last_line_height;
        let text = self.display_text();
        element::range_bounds(lines, bounds, line_height, text, range, self.text_align())
    }

    fn character_index_for_point(
        &mut self,
        pt: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        if self.code_language_focus_handle.is_focused(_window) {
            let index = self.code_language_index_for_mouse_position(pt);
            return Some(Self::utf8_to_utf16_in(self.code_language_text(), index));
        }

        let bounds = self.last_bounds?;
        let lines = self.last_layout.as_ref()?;
        let text = self.display_text();
        let ranges = element::hard_line_ranges(text);
        let relative = Point {
            x: pt.x - bounds.left(),
            y: pt.y - bounds.top(),
        };
        let (line_idx, y_in_line) =
            element::wrapped_line_for_y(lines, self.last_line_height, relative.y)?;
        let layout = &lines[line_idx];
        let origin_x = element::aligned_line_left(layout, bounds, self.text_align());
        let utf8_offset_in_line = match layout
            .closest_index_for_position(point(pt.x - origin_x, y_in_line), self.last_line_height)
        {
            Ok(idx) | Err(idx) => idx,
        };
        let utf8_index = ranges[line_idx].start + utf8_offset_in_line;
        Some(Self::utf8_to_utf16_in(self.display_text(), utf8_index))
    }
}

/// Typographic substitution applied while typing (roadmap B6). Returns the
/// replacement plus how many bytes before the insertion point it consumes
/// (`--` becomes one em dash).
fn smart_punctuation_replacement(text: &str, before: &str) -> Option<(usize, String)> {
    let preceding = before.chars().next_back();
    match text {
        "\"" => Some((
            0,
            if is_opening_quote_context(preceding) {
                "\u{201c}".to_string()
            } else {
                "\u{201d}".to_string()
            },
        )),
        "'" => Some((
            0,
            if is_opening_quote_context(preceding) {
                "\u{2018}".to_string()
            } else {
                "\u{2019}".to_string()
            },
        )),
        "-" if preceding == Some('-') => Some((1, "\u{2014}".to_string())),
        _ => None,
    }
}

fn is_opening_quote_context(preceding: Option<char>) -> bool {
    const OPENERS: &str = "([{<\u{201c}\u{2018}\u{300c}\u{300e}\u{ff08}\u{3010}\u{300a}\u{3008}\u{3001}\u{3002}\u{ff0c}\u{ff1a}\u{ff1b}\u{ff01}\u{ff1f}\u{2026},;:.!?";
    match preceding {
        None => true,
        Some(ch) => ch.is_whitespace() || OPENERS.contains(ch),
    }
}

/// Paired symbols that wrap a selection instead of replacing it (roadmap B4).
fn wrap_pair_for(text: &str) -> Option<(&'static str, &'static str)> {
    match text {
        "*" => Some(("*", "*")),
        "_" => Some(("_", "_")),
        "`" => Some(("`", "`")),
        "\"" => Some(("\"", "\"")),
        "'" => Some(("'", "'")),
        "(" => Some(("(", ")")),
        "[" => Some(("[", "]")),
        "{" => Some(("{", "}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{Block, smart_punctuation_replacement};
    use crate::components::{BlockKind, BlockRecord, InlineTextTree};
    use crate::config::EditorSettings;
    use gpui::{Entity, EntityInputHandler, TestAppContext, VisualTestContext};

    #[test]
    fn smart_punctuation_maps_quotes_and_dashes() {
        assert_eq!(
            smart_punctuation_replacement("\"", ""),
            Some((0, "\u{201c}".to_string()))
        );
        assert_eq!(
            smart_punctuation_replacement("\"", "word"),
            Some((0, "\u{201d}".to_string()))
        );
        assert_eq!(
            smart_punctuation_replacement("\"", "中文"),
            Some((0, "\u{201d}".to_string()))
        );
        assert_eq!(
            smart_punctuation_replacement("\"", "，"),
            Some((0, "\u{201c}".to_string()))
        );
        assert_eq!(
            smart_punctuation_replacement("'", "word"),
            Some((0, "\u{2019}".to_string()))
        );
        assert_eq!(
            smart_punctuation_replacement("-", "-"),
            Some((1, "\u{2014}".to_string()))
        );
        assert_eq!(smart_punctuation_replacement("-", "a"), None);
        assert_eq!(smart_punctuation_replacement("x", "a"), None);
    }

    fn typing_block<'a>(
        cx: &'a mut TestAppContext,
        text: &str,
    ) -> (Entity<Block>, &'a mut VisualTestContext) {
        cx.add_window_view(|_window, cx| {
            Block::with_record(
                cx,
                BlockRecord::new(BlockKind::Paragraph, InlineTextTree::plain(text)),
            )
        })
    }

    #[gpui::test]
    async fn typing_with_smart_punctuation_substitutes_quotes_and_dashes(
        cx: &mut TestAppContext,
    ) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            EditorSettings::install_test_settings(cx, true);
        });
        let (block, cx) = typing_block(cx, "word");

        cx.update(|window, cx| {
            block.update(cx, |block, block_cx| {
                block.selected_range = 4..4;
                <Block as EntityInputHandler>::replace_text_in_range(
                    block, None, "\"", window, block_cx,
                );
            });
        });
        assert_eq!(
            block.read_with(cx, |block, _| block.display_text().to_string()),
            "word\u{201d}"
        );

        cx.update(|window, cx| {
            block.update(cx, |block, block_cx| {
                <Block as EntityInputHandler>::replace_text_in_range(
                    block, None, "-", window, block_cx,
                );
                <Block as EntityInputHandler>::replace_text_in_range(
                    block, None, "-", window, block_cx,
                );
            });
        });
        assert_eq!(
            block.read_with(cx, |block, _| block.display_text().to_string()),
            "word\u{201d}\u{2014}"
        );

    }

    #[gpui::test]
    async fn typing_without_smart_punctuation_keeps_straight_quotes(cx: &mut TestAppContext) {
        cx.update(|cx| {
            crate::i18n::I18nManager::init(cx);
            crate::theme::ThemeManager::init(cx);
            EditorSettings::install_test_settings(cx, false);
        });
        let (block, cx) = typing_block(cx, "word");

        cx.update(|window, cx| {
            block.update(cx, |block, block_cx| {
                block.selected_range = 4..4;
                <Block as EntityInputHandler>::replace_text_in_range(
                    block, None, "\"", window, block_cx,
                );
                <Block as EntityInputHandler>::replace_text_in_range(
                    block, None, "-", window, block_cx,
                );
                <Block as EntityInputHandler>::replace_text_in_range(
                    block, None, "-", window, block_cx,
                );
            });
        });
        assert_eq!(
            block.read_with(cx, |block, _| block.display_text().to_string()),
            "word\"--"
        );
    }
}
