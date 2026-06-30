//! Formatting helpers for plain preedit display.
//!
//! Port of `src/ui/preedit.c`. Joins a composition's segments into the flat
//! string shown inline by the compositor and resolves the display cursor.
//! Pure — no I/O.

/// A single preedit segment. Mirrors `TypioPreeditSegment`; only the text
/// matters for plain (unformatted) display.
#[derive(Debug, Clone, Copy)]
pub struct PreeditSegment<'a> {
    pub text: &'a str,
}

/// A composition preedit. Mirrors `TypioPreedit`.
#[derive(Debug, Clone, Copy)]
pub struct Preedit<'a> {
    pub segments: &'a [PreeditSegment<'a>],
    /// Cursor position in characters. Negative means "place at the end".
    pub cursor_pos: i32,
}

/// Resolve a preedit cursor position against a joined-segment text.
///
/// Converts the character-based `cursor_pos` into a byte offset.
/// Non-negative `cursor_pos` is resolved to the byte boundary of that
/// character; negative falls back to `text.len()` (place the cursor at the end).
pub fn resolve_cursor(cursor_pos: i32, text: &str) -> usize {
    if cursor_pos >= 0 {
        text.char_indices()
            .nth(cursor_pos as usize)
            .map(|(i, _)| i)
            .unwrap_or_else(|| text.len())
    } else {
        text.len()
    }
}

/// Build the flat plain-preedit string from a composition.
///
/// Returns the joined segment text and the resolved cursor. An absent or
/// empty composition yields `(None, -1)`. When `cursor_pos` is non-negative
/// it is preserved; otherwise the cursor is placed at the end of the joined
/// text via [`resolve_cursor`].
pub fn build_plain_preedit(preedit: Option<&Preedit>) -> (Option<String>, i32) {
    let Some(preedit) = preedit else {
        return (None, -1);
    };
    if preedit.segments.is_empty() {
        return (None, -1);
    }

    let mut buffer = String::new();
    for segment in preedit.segments {
        buffer.push_str(segment.text);
    }

    let cursor = resolve_cursor(preedit.cursor_pos, &buffer) as i32;

    (Some(buffer), cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_preedit_only() {
        let segments = [
            PreeditSegment { text: "zhong" },
            PreeditSegment { text: "wen" },
        ];
        let preedit = Preedit {
            segments: &segments,
            cursor_pos: 5,
        };
        let (display, cursor) = build_plain_preedit(Some(&preedit));
        assert_eq!(display.as_deref(), Some("zhongwen"));
        assert_eq!(cursor, 5);
    }

    #[test]
    fn plain_preedit_none() {
        let (display, cursor) = build_plain_preedit(None);
        assert_eq!(display, None);
        assert_eq!(cursor, -1);
    }

    #[test]
    fn plain_preedit_empty_segments() {
        let preedit = Preedit {
            segments: &[],
            cursor_pos: 3,
        };
        let (display, cursor) = build_plain_preedit(Some(&preedit));
        assert_eq!(display, None);
        assert_eq!(cursor, -1);
    }

    #[test]
    fn negative_cursor_resolves_to_end() {
        let segments = [
            PreeditSegment { text: "ni" },
            PreeditSegment { text: "hao" },
        ];
        let preedit = Preedit {
            segments: &segments,
            cursor_pos: -1,
        };
        let (display, cursor) = build_plain_preedit(Some(&preedit));
        assert_eq!(display.as_deref(), Some("nihao"));
        assert_eq!(cursor, 5);
    }

    #[test]
    fn resolve_cursor_preserves_non_negative() {
        assert_eq!(resolve_cursor(0, "0123456789"), 0);
        assert_eq!(resolve_cursor(3, "0123456789"), 3);
        assert_eq!(resolve_cursor(10, "0123456789"), 10);
    }

    #[test]
    fn resolve_cursor_negative_falls_back_to_len() {
        assert_eq!(resolve_cursor(-1, "0123456789"), 10);
        assert_eq!(resolve_cursor(i32::MIN, ""), 0);
    }

    #[test]
    fn resolve_cursor_multibyte_chars() {
        assert_eq!(resolve_cursor(0, "你好"), 0);
        assert_eq!(resolve_cursor(1, "你好"), 3);
        assert_eq!(resolve_cursor(2, "你好"), 6);
        assert_eq!(resolve_cursor(3, "你好"), 6); // clamped
    }
}
