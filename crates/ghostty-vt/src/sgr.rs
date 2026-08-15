//! Safe wrapper over libghostty-vt's standalone SGR parser.
//!
//! Herdr needs to understand SGR parameters outside of a terminal write in one
//! place: answering DECRQSS SGR queries, which must report the pane's current
//! graphic rendition. Rather than reimplement SGR semantics — colon versus
//! semicolon sub-parameters, extended underline styles, the three color
//! spellings — this delegates to the same parser libghostty uses internally.

use std::ptr;

use super::{ffi, Error, GhosttyResultExt};

/// A single parsed SGR attribute.
///
/// Only the attributes Herdr can report are named; everything else the parser
/// recognizes collapses into [`Attribute::Other`], which callers ignore.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribute {
    /// SGR 0 — reset every attribute to its default.
    Unset,
    Bold,
    ResetBold,
    Italic,
    ResetItalic,
    Faint,
    /// Underline style as a `GHOSTTY_SGR_UNDERLINE_*` value (0 = none).
    Underline(u8),
    UnderlineColor(u8, u8, u8),
    UnderlineColor256(u8),
    ResetUnderlineColor,
    Overline,
    ResetOverline,
    Blink,
    ResetBlink,
    Inverse,
    ResetInverse,
    Invisible,
    ResetInvisible,
    Strikethrough,
    ResetStrikethrough,
    DirectColorFg(u8, u8, u8),
    DirectColorBg(u8, u8, u8),
    Fg8(u8),
    Bg8(u8),
    BrightFg8(u8),
    BrightBg8(u8),
    Fg256(u8),
    Bg256(u8),
    ResetFg,
    ResetBg,
    /// A parameter the parser understood but Herdr does not model.
    Other,
}

/// Owned handle to a libghostty SGR parser.
#[derive(Debug)]
pub struct SgrParser {
    raw: ffi::GhosttySgrParser,
}

// SAFETY: the parser owns its allocation and is only reachable through `&mut
// self` methods, so it can move between threads but never be shared.
unsafe impl Send for SgrParser {}

impl SgrParser {
    pub fn new() -> Result<Self, Error> {
        let mut raw = ptr::null_mut();
        // SAFETY: valid out pointer, null allocator means default allocator.
        unsafe {
            ffi::ghostty_sgr_new(ptr::null(), &mut raw).into_result()?;
        }
        Ok(Self { raw })
    }

    /// Parses one SGR parameter list, invoking `visit` for each attribute.
    ///
    /// `separators[i]` is the byte that *followed* parameter `i` in the
    /// original sequence — `b':'` for a sub-parameter, anything else for a
    /// plain separator. This mirrors libghostty's own convention, where
    /// `4:3` arrives as params `[4, 3]` with a colon recorded at index 0.
    ///
    /// Takes a visitor rather than returning a collection so that parsing a
    /// sequence costs no allocation on Herdr's side.
    pub fn for_each_attribute(
        &mut self,
        params: &[u16],
        separators: &[u8],
        mut visit: impl FnMut(Attribute),
    ) -> Result<(), Error> {
        debug_assert_eq!(params.len(), separators.len());
        if params.is_empty() {
            return Ok(());
        }

        // SAFETY: both slices have the same length as `len`, and libghostty
        // copies them before returning.
        unsafe {
            ffi::ghostty_sgr_set_params(
                self.raw,
                params.as_ptr(),
                separators.as_ptr().cast::<std::os::raw::c_char>(),
                params.len(),
            )
            .into_result()?;
        }

        loop {
            let mut attr = ffi::GhosttySgrAttribute::default();
            // SAFETY: `attr` is a valid out pointer for the parser's lifetime.
            let more = unsafe { ffi::ghostty_sgr_next(self.raw, &mut attr) };
            if !more {
                break;
            }
            visit(attribute_from_ffi(&attr));
        }

        Ok(())
    }
}

impl Drop for SgrParser {
    fn drop(&mut self) {
        // SAFETY: `raw` came from `ghostty_sgr_new` and is freed exactly once.
        unsafe { ffi::ghostty_sgr_free(self.raw) };
    }
}

fn attribute_from_ffi(attr: &ffi::GhosttySgrAttribute) -> Attribute {
    // SAFETY: each union field is read only under its matching tag, as the
    // libghostty header requires.
    unsafe {
        match attr.tag {
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_UNSET => Attribute::Unset,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_BOLD => Attribute::Bold,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_BOLD => Attribute::ResetBold,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_ITALIC => Attribute::Italic,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_ITALIC => Attribute::ResetItalic,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_FAINT => Attribute::Faint,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_UNDERLINE => {
                Attribute::Underline(attr.value.underline as u8)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_UNDERLINE_COLOR => {
                let color = attr.value.underline_color;
                Attribute::UnderlineColor(color.r, color.g, color.b)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_UNDERLINE_COLOR_256 => {
                Attribute::UnderlineColor256(attr.value.underline_color_256)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_UNDERLINE_COLOR => {
                Attribute::ResetUnderlineColor
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_OVERLINE => Attribute::Overline,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_OVERLINE => Attribute::ResetOverline,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_BLINK => Attribute::Blink,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_BLINK => Attribute::ResetBlink,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_INVERSE => Attribute::Inverse,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_INVERSE => Attribute::ResetInverse,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_INVISIBLE => Attribute::Invisible,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_INVISIBLE => {
                Attribute::ResetInvisible
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_STRIKETHROUGH => Attribute::Strikethrough,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_STRIKETHROUGH => {
                Attribute::ResetStrikethrough
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_DIRECT_COLOR_FG => {
                let color = attr.value.direct_color_fg;
                Attribute::DirectColorFg(color.r, color.g, color.b)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_DIRECT_COLOR_BG => {
                let color = attr.value.direct_color_bg;
                Attribute::DirectColorBg(color.r, color.g, color.b)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_FG_8 => Attribute::Fg8(attr.value.fg_8),
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_BG_8 => Attribute::Bg8(attr.value.bg_8),
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_BRIGHT_FG_8 => {
                Attribute::BrightFg8(attr.value.bright_fg_8)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_BRIGHT_BG_8 => {
                Attribute::BrightBg8(attr.value.bright_bg_8)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_FG_256 => {
                Attribute::Fg256(attr.value.fg_256)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_BG_256 => {
                Attribute::Bg256(attr.value.bg_256)
            }
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_FG => Attribute::ResetFg,
            ffi::GhosttySgrAttributeTag_GHOSTTY_SGR_ATTR_RESET_BG => Attribute::ResetBg,
            _ => Attribute::Other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(params: &[u16], separators: &[u8]) -> Vec<Attribute> {
        let mut parser = SgrParser::new().expect("sgr parser");
        let mut attributes = Vec::new();
        parser
            .for_each_attribute(params, separators, |attr| attributes.push(attr))
            .expect("parse succeeds");
        attributes
    }

    #[test]
    fn parses_bold() {
        assert_eq!(parse(&[1], b";"), vec![Attribute::Bold]);
    }

    #[test]
    fn parses_curly_underline_from_colon_sub_parameter() {
        assert_eq!(parse(&[4, 3], b":;"), vec![Attribute::Underline(3)]);
    }

    #[test]
    fn separator_marks_the_parameter_it_follows_not_the_one_it_precedes() {
        // Guards the convention the whole shadow pen depends on: a colon
        // recorded at index 0 binds params 0 and 1 into `4:3`. Flipping the
        // colon to index 1 must not produce a curly underline, otherwise the
        // separator array could be built either way and still look correct.
        assert_ne!(parse(&[4, 3], b";:"), vec![Attribute::Underline(3)]);
    }

    #[test]
    fn parses_plain_underline_without_sub_parameter() {
        assert_eq!(parse(&[4], b";"), vec![Attribute::Underline(1)]);
    }

    #[test]
    fn parses_rgb_underline_color() {
        assert_eq!(
            parse(&[58, 2, 0, 255, 0, 0], b":::::;"),
            vec![Attribute::UnderlineColor(255, 0, 0)]
        );
    }

    #[test]
    fn parses_indexed_underline_color() {
        assert_eq!(
            parse(&[58, 5, 9], b"::;"),
            vec![Attribute::UnderlineColor256(9)]
        );
    }

    #[test]
    fn parses_underline_color_reset() {
        assert_eq!(parse(&[59], b";"), vec![Attribute::ResetUnderlineColor]);
    }

    #[test]
    fn parses_multiple_attributes_in_one_sequence() {
        assert_eq!(
            parse(&[1, 3, 0], b";;;"),
            vec![Attribute::Bold, Attribute::Italic, Attribute::Unset]
        );
    }

    #[test]
    fn parses_direct_color_foreground() {
        assert_eq!(
            parse(&[38, 2, 1, 2, 3], b";;;;;"),
            vec![Attribute::DirectColorFg(1, 2, 3)]
        );
    }

    #[test]
    fn empty_parameter_list_yields_no_attributes() {
        assert_eq!(parse(&[], b""), vec![]);
    }
}
