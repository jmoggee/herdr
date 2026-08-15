//! DECRQSS (`DCS $ q ... ST`) request tracking for pane output.
//!
//! Neovim decides whether it may emit extended underlines by writing
//! `CSI 4:3 m`, asking the terminal to report its current SGR state with
//! DECRQSS, and checking whether the answer echoes the curly style back. A
//! terminal that stays silent is treated as having no extended underline
//! support, so Neovim falls back to a plain `CSI 4 m` and undercurl never
//! reaches Herdr at all.
//!
//! libghostty-vt parses DECRQSS but builds its reply in `termio`, which is not
//! part of the C surface Herdr links against, and exposes no accessor for the
//! terminal pen. So this module shadows the pen from the pane's output stream
//! and answers the query itself, mirroring `xtgettcap` in shape: observe bytes,
//! drain responses that the caller interleaves at the recorded offsets.
//!
//! SGR semantics are not reimplemented here — parameters are handed to
//! libghostty's own SGR parser through [`crate::ghostty::sgr`].

use bytes::Bytes;

use crate::ghostty::sgr::{Attribute, SgrParser};
use crate::ghostty::Error;

/// Upper bound on parameters collected from a single CSI sequence.
///
/// Real SGR sequences are a handful of parameters long. A sequence past this
/// bound is not applied at all, because truncating it could split a colon
/// sub-parameter group and synthesize an attribute the application never sent.
const MAX_PARAMS: usize = 256;

/// Upper bound on the DECRQSS setting name (`m`, ` q`, `r`, `s`).
const MAX_DCS_BODY: usize = 8;

#[derive(Debug)]
pub(super) struct DecrqssQueryTracker {
    parser: SgrParser,
    pen: SgrPen,
    /// Pen saved by DECSC (`ESC 7`), restored by DECRC (`ESC 8`).
    saved_pen: Option<SgrPen>,
    /// Pen saved when entering the alternate screen with mode 1049.
    alternate_saved_pen: Option<SgrPen>,
    state: State,
    params: Vec<u16>,
    separators: Vec<u8>,
    param_value: u16,
    param_seen: bool,
    param_overflow: bool,
    private: u8,
    intermediate: u8,
    body: Vec<u8>,
    pending: Vec<DecrqssResponse>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DecrqssResponse {
    pub(super) end_offset: usize,
    pub(super) bytes: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    Escape,
    CsiParams,
    DcsIntro,
    DcsDollar,
    DcsBody,
    DcsEscape,
    IgnoreOsc,
    IgnoreOscEscape,
    IgnoreString,
    IgnoreStringEscape,
}

impl DecrqssQueryTracker {
    pub(super) fn new() -> Result<Self, Error> {
        Ok(Self {
            parser: SgrParser::new()?,
            pen: SgrPen::default(),
            saved_pen: None,
            alternate_saved_pen: None,
            state: State::Ground,
            params: Vec::new(),
            separators: Vec::new(),
            param_value: 0,
            param_seen: false,
            param_overflow: false,
            private: 0,
            intermediate: 0,
            body: Vec::new(),
            pending: Vec::new(),
        })
    }

    pub(super) fn observe(&mut self, bytes: &[u8]) {
        for (index, &byte) in bytes.iter().enumerate() {
            match self.state {
                State::Ground => match byte {
                    0x1b => self.state = State::Escape,
                    0x90 => self.enter_dcs(),
                    0x9b => self.enter_csi(),
                    0x9d => self.state = State::IgnoreOsc,
                    0x98 | 0x9e | 0x9f => self.state = State::IgnoreString,
                    _ => {}
                },
                State::Escape => match byte {
                    b'[' => self.enter_csi(),
                    b'P' => self.enter_dcs(),
                    b']' => self.state = State::IgnoreOsc,
                    b'_' | b'^' | b'X' => self.state = State::IgnoreString,
                    // DECSC / DECRC save and restore the pen along with the cursor.
                    b'7' => {
                        self.saved_pen = Some(self.pen);
                        self.state = State::Ground;
                    }
                    b'8' => {
                        self.pen = self.saved_pen.unwrap_or_default();
                        self.state = State::Ground;
                    }
                    // RIS clears every attribute, including anything saved.
                    b'c' => {
                        self.pen = SgrPen::default();
                        self.saved_pen = None;
                        self.alternate_saved_pen = None;
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::Escape,
                    _ => self.state = State::Ground,
                },
                State::CsiParams => match byte {
                    b'0'..=b'9' => {
                        self.param_value = self
                            .param_value
                            .saturating_mul(10)
                            .saturating_add(u16::from(byte - b'0'));
                        self.param_seen = true;
                    }
                    b';' | b':' => self.push_param(byte),
                    0x3c..=0x3f => self.private = byte,
                    0x20..=0x2f => self.intermediate = byte,
                    0x40..=0x7e => {
                        self.dispatch_csi(byte);
                        self.state = State::Ground;
                    }
                    0x1b => self.state = State::Escape,
                    _ => self.state = State::Ground,
                },
                State::DcsIntro => match byte {
                    b'0'..=b'9' | b';' | b':' => {}
                    b'$' => self.state = State::DcsDollar,
                    0x1b => self.state = State::IgnoreStringEscape,
                    0x9c => self.state = State::Ground,
                    _ => self.state = State::IgnoreString,
                },
                State::DcsDollar => match byte {
                    b'q' => {
                        self.body.clear();
                        self.state = State::DcsBody;
                    }
                    0x1b => self.state = State::IgnoreStringEscape,
                    0x9c => self.state = State::Ground,
                    _ => self.state = State::IgnoreString,
                },
                State::DcsBody => match byte {
                    0x1b => self.state = State::DcsEscape,
                    0x9c => {
                        self.finalize(index + 1);
                        self.state = State::Ground;
                    }
                    _ => {
                        self.body.push(byte);
                        if self.body.len() > MAX_DCS_BODY {
                            self.body.clear();
                            self.state = State::IgnoreString;
                        }
                    }
                },
                State::DcsEscape => {
                    if byte == b'\\' {
                        self.finalize(index + 1);
                        self.state = State::Ground;
                    } else if byte != 0x1b {
                        self.body.clear();
                        self.state = State::IgnoreString;
                    }
                }
                State::IgnoreOsc => {
                    if byte == 0x1b {
                        self.state = State::IgnoreOscEscape;
                    } else if matches!(byte, 0x07 | 0x9c) {
                        self.state = State::Ground;
                    }
                }
                State::IgnoreOscEscape => {
                    if byte == b'\\' {
                        self.state = State::Ground;
                    } else if byte != 0x1b {
                        self.state = State::IgnoreOsc;
                    }
                }
                State::IgnoreString => {
                    if byte == 0x1b {
                        self.state = State::IgnoreStringEscape;
                    } else if byte == 0x9c {
                        self.state = State::Ground;
                    }
                }
                State::IgnoreStringEscape => {
                    if byte == b'\\' {
                        self.state = State::Ground;
                    } else if byte != 0x1b {
                        self.state = State::IgnoreString;
                    }
                }
            }
        }
    }

    pub(super) fn drain_pending(&mut self) -> Vec<DecrqssResponse> {
        std::mem::take(&mut self.pending)
    }

    fn enter_csi(&mut self) {
        self.params.clear();
        self.separators.clear();
        self.param_value = 0;
        self.param_seen = false;
        self.param_overflow = false;
        self.private = 0;
        self.intermediate = 0;
        self.state = State::CsiParams;
    }

    fn enter_dcs(&mut self) {
        self.body.clear();
        self.state = State::DcsIntro;
    }

    fn push_param(&mut self, separator: u8) {
        if self.params.len() < MAX_PARAMS {
            self.params.push(self.param_value);
            self.separators.push(separator);
        } else {
            self.param_overflow = true;
        }
        self.param_value = 0;
        self.param_seen = false;
    }

    fn dispatch_csi(&mut self, final_byte: u8) {
        if self.param_seen || !self.params.is_empty() {
            self.push_param(b';');
        }

        match (self.private, self.intermediate, final_byte) {
            (0, 0, b'm') => self.apply_sgr(),
            // DECSTR returns the pen to its default state.
            (0, b'!', b'p') => self.pen = SgrPen::default(),
            (b'?', 0, mode @ (b'h' | b'l')) => self.apply_private_mode(mode),
            _ => {}
        }
    }

    fn apply_sgr(&mut self) {
        if self.param_overflow {
            return;
        }

        // `CSI m` with no parameters is `CSI 0 m`.
        if self.params.is_empty() {
            self.params.push(0);
            self.separators.push(b';');
        }

        let pen = &mut self.pen;
        let _ = self
            .parser
            .for_each_attribute(&self.params, &self.separators, |attr| pen.apply(attr));
    }

    fn apply_private_mode(&mut self, mode: u8) {
        // Mode 1049 saves the pen with the cursor when entering the alternate
        // screen and restores it on the way out. Modes 47 and 1047 switch
        // screens without touching the saved cursor, so the pen carries over.
        if !self.params.contains(&1049) {
            return;
        }

        if mode == b'h' {
            self.alternate_saved_pen = Some(self.pen);
            self.pen = SgrPen::default();
        } else {
            self.pen = self.alternate_saved_pen.take().unwrap_or_default();
        }
    }

    fn finalize(&mut self, end_offset: usize) {
        if self.body == b"m" {
            self.pending.push(DecrqssResponse {
                end_offset,
                bytes: self.pen.decrpss_response(),
            });
        }
        self.body.clear();
    }
}

/// The subset of SGR state Herdr can report through DECRQSS.
///
/// Mirrors libghostty's own `printAttributes`, with two deliberate additions:
/// the underline *style* is reported exactly rather than flattened to `4`, and
/// the underline color is included. Ghostty can afford to omit both because it
/// ships a terminfo entry with `Smulx`; over `TERM=xterm-256color` this reply
/// is the only channel through which Neovim can learn that extended underlines
/// are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct SgrPen {
    bold: bool,
    faint: bool,
    italic: bool,
    underline: u8,
    blink: bool,
    inverse: bool,
    invisible: bool,
    strikethrough: bool,
    fg: PenColor,
    bg: PenColor,
    underline_color: PenColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum PenColor {
    #[default]
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

impl SgrPen {
    fn apply(&mut self, attr: Attribute) {
        match attr {
            Attribute::Unset => *self = Self::default(),
            Attribute::Bold => self.bold = true,
            // SGR 22 clears both bold and faint.
            Attribute::ResetBold => {
                self.bold = false;
                self.faint = false;
            }
            Attribute::Faint => self.faint = true,
            Attribute::Italic => self.italic = true,
            Attribute::ResetItalic => self.italic = false,
            Attribute::Underline(style) => self.underline = style,
            Attribute::UnderlineColor(r, g, b) => self.underline_color = PenColor::Rgb(r, g, b),
            Attribute::UnderlineColor256(index) => {
                self.underline_color = PenColor::Indexed(index);
            }
            Attribute::ResetUnderlineColor => self.underline_color = PenColor::Default,
            Attribute::Blink => self.blink = true,
            Attribute::ResetBlink => self.blink = false,
            Attribute::Inverse => self.inverse = true,
            Attribute::ResetInverse => self.inverse = false,
            Attribute::Invisible => self.invisible = true,
            Attribute::ResetInvisible => self.invisible = false,
            Attribute::Strikethrough => self.strikethrough = true,
            Attribute::ResetStrikethrough => self.strikethrough = false,
            Attribute::DirectColorFg(r, g, b) => self.fg = PenColor::Rgb(r, g, b),
            Attribute::DirectColorBg(r, g, b) => self.bg = PenColor::Rgb(r, g, b),
            // libghostty already folds the bright variants into 8..15.
            Attribute::Fg8(index) | Attribute::BrightFg8(index) | Attribute::Fg256(index) => {
                self.fg = PenColor::Indexed(index);
            }
            Attribute::Bg8(index) | Attribute::BrightBg8(index) | Attribute::Bg256(index) => {
                self.bg = PenColor::Indexed(index);
            }
            Attribute::ResetFg => self.fg = PenColor::Default,
            Attribute::ResetBg => self.bg = PenColor::Default,
            Attribute::Overline | Attribute::ResetOverline | Attribute::Other => {}
        }
    }

    /// Builds the full DECRPSS reply for this pen.
    fn decrpss_response(&self) -> Bytes {
        let mut response = String::from("\x1bP1$r");
        // DECRPSS SGR replies always open with a 0. See
        // https://vt100.net/docs/vt510-rm/DECRPSS
        response.push('0');

        if self.bold {
            response.push_str(";1");
        }
        if self.faint {
            response.push_str(";2");
        }
        if self.italic {
            response.push_str(";3");
        }
        match self.underline {
            0 => {}
            1 => response.push_str(";4"),
            style => {
                response.push_str(";4:");
                response.push_str(&style.to_string());
            }
        }
        if self.blink {
            response.push_str(";5");
        }
        if self.inverse {
            response.push_str(";7");
        }
        if self.invisible {
            response.push_str(";8");
        }
        if self.strikethrough {
            response.push_str(";9");
        }

        push_color(&mut response, self.fg, 30, 90, "38");
        push_color(&mut response, self.bg, 40, 100, "48");
        if let PenColor::Indexed(index) = self.underline_color {
            response.push_str(&format!(";58:5:{index}"));
        } else if let PenColor::Rgb(r, g, b) = self.underline_color {
            response.push_str(&format!(";58:2::{r}:{g}:{b}"));
        }

        response.push('m');
        response.push_str("\x1b\\");
        Bytes::from(response.into_bytes())
    }
}

/// Appends one color in the spelling libghostty's `printAttributes` uses.
fn push_color(response: &mut String, color: PenColor, base: u8, bright_base: u8, extended: &str) {
    match color {
        PenColor::Default => {}
        PenColor::Indexed(index) if index >= 16 => {
            response.push_str(&format!(";{extended}:5:{index}"));
        }
        PenColor::Indexed(index) if index >= 8 => {
            response.push_str(&format!(
                ";{}",
                u16::from(bright_base) + u16::from(index) - 8
            ));
        }
        PenColor::Indexed(index) => {
            response.push_str(&format!(";{}", u16::from(base) + u16::from(index)));
        }
        PenColor::Rgb(r, g, b) => {
            response.push_str(&format!(";{extended}:2::{r}:{g}:{b}"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn responses(bytes: &[u8]) -> Vec<Bytes> {
        let mut tracker = DecrqssQueryTracker::new().expect("tracker");
        tracker.observe(bytes);
        tracker
            .drain_pending()
            .into_iter()
            .map(|response| response.bytes)
            .collect()
    }

    fn sole_response(bytes: &[u8]) -> Bytes {
        let mut responses = responses(bytes);
        assert_eq!(responses.len(), 1, "expected exactly one response");
        responses.remove(0)
    }

    #[test]
    fn answers_sgr_query_with_default_pen() {
        assert_eq!(
            sole_response(b"\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0m\x1b\\")
        );
    }

    #[test]
    fn answers_neovim_extended_underline_probe_with_the_curly_style() {
        // The exact bytes Neovim 0.12 writes to decide whether extended
        // underlines are available.
        assert_eq!(
            sole_response(b"\x1b[4:3m\x1bP$qm\x1b\\\x1b[0m"),
            Bytes::from_static(b"\x1bP1$r0;4:3m\x1b\\")
        );
    }

    #[test]
    fn reports_plain_underline_without_a_sub_parameter() {
        assert_eq!(
            sole_response(b"\x1b[4m\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;4m\x1b\\")
        );
    }

    #[test]
    fn reports_rgb_underline_color() {
        assert_eq!(
            sole_response(b"\x1b[4:3m\x1b[58:2::255:0:0m\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;4:3;58:2::255:0:0m\x1b\\")
        );
    }

    #[test]
    fn reports_indexed_underline_color() {
        assert_eq!(
            sole_response(b"\x1b[4:3;58:5:9m\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;4:3;58:5:9m\x1b\\")
        );
    }

    #[test]
    fn reports_flags_and_colors_in_ghostty_order() {
        assert_eq!(
            sole_response(b"\x1b[1;3;31;44m\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;1;3;31;44m\x1b\\")
        );
    }

    #[test]
    fn reports_direct_colors() {
        assert_eq!(
            sole_response(b"\x1b[38;2;1;2;3;48;5;200m\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;38:2::1:2:3;48:5:200m\x1b\\")
        );
    }

    #[test]
    fn sgr_reset_clears_the_pen() {
        assert_eq!(
            sole_response(b"\x1b[1;4:3m\x1b[0m\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0m\x1b\\")
        );
    }

    #[test]
    fn empty_sgr_sequence_resets_the_pen() {
        assert_eq!(
            sole_response(b"\x1b[1m\x1b[m\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0m\x1b\\")
        );
    }

    #[test]
    fn ignores_decrqss_requests_other_than_sgr() {
        // DECSCUSR: Herdr has no answer it can stand behind, so it stays quiet
        // rather than reporting a value it did not track.
        assert!(responses(b"\x1bP$q q\x1b\\").is_empty());
    }

    #[test]
    fn keeps_split_query_until_string_terminator() {
        let mut tracker = DecrqssQueryTracker::new().expect("tracker");

        tracker.observe(b"\x1b[4:3m\x1bP$q");
        assert!(tracker.drain_pending().is_empty());
        tracker.observe(b"m\x1b");
        assert!(tracker.drain_pending().is_empty());
        tracker.observe(b"\\");

        assert_eq!(
            tracker
                .drain_pending()
                .into_iter()
                .map(|response| response.bytes)
                .collect::<Vec<_>>(),
            vec![Bytes::from_static(b"\x1bP1$r0;4:3m\x1b\\")]
        );
    }

    #[test]
    fn accepts_eight_bit_dcs_and_string_terminator() {
        assert_eq!(
            sole_response(b"\x1b[4:3m\x90$qm\x9c"),
            Bytes::from_static(b"\x1bP1$r0;4:3m\x1b\\")
        );
    }

    #[test]
    fn ignores_sgr_bytes_inside_an_osc_string() {
        // An OSC payload such as a window title may contain arbitrary bytes;
        // they must not be mistaken for pen changes.
        assert_eq!(
            sole_response(b"\x1b[1m\x1b]0;\x1b[3m\x07\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;1m\x1b\\")
        );
    }

    #[test]
    fn decsc_and_decrc_save_and_restore_the_pen() {
        assert_eq!(
            sole_response(b"\x1b[1m\x1b7\x1b[0;3m\x1b8\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;1m\x1b\\")
        );
    }

    #[test]
    fn ris_clears_the_pen() {
        assert_eq!(
            sole_response(b"\x1b[1;4:3m\x1bc\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0m\x1b\\")
        );
    }

    #[test]
    fn decstr_clears_the_pen() {
        assert_eq!(
            sole_response(b"\x1b[1;4:3m\x1b[!p\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0m\x1b\\")
        );
    }

    #[test]
    fn alternate_screen_entry_starts_from_a_default_pen() {
        assert_eq!(
            sole_response(b"\x1b[1m\x1b[?1049h\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0m\x1b\\")
        );
    }

    #[test]
    fn leaving_the_alternate_screen_restores_the_saved_pen() {
        assert_eq!(
            sole_response(b"\x1b[1m\x1b[?1049h\x1b[3m\x1b[?1049l\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;1m\x1b\\")
        );
    }

    #[test]
    fn private_mode_without_1049_leaves_the_pen_alone() {
        assert_eq!(
            sole_response(b"\x1b[1m\x1b[?25l\x1bP$qm\x1b\\"),
            Bytes::from_static(b"\x1bP1$r0;1m\x1b\\")
        );
    }

    /// Representative alternate-screen output: mostly plain glyphs with an SGR
    /// change every few cells, which is the shape that makes this tracker's
    /// per-byte cost multiplicative across panes and clients.
    fn styled_output_stream(rows: usize) -> Vec<u8> {
        let mut stream = Vec::new();
        for row in 0..rows {
            stream.extend_from_slice(b"\x1b[H\x1b[2K");
            for col in 0..40 {
                let fg = 30 + (col % 8);
                stream.extend_from_slice(format!("\x1b[{fg};1m").as_bytes());
                stream.extend_from_slice(b"word ");
            }
            stream.extend_from_slice(format!("\x1b[{};1H\x1b[0m", row % 24 + 1).as_bytes());
        }
        stream
    }

    #[test]
    #[ignore = "manual DECRQSS pen-tracking cost profile for the PTY parse path"]
    fn decrqss_observe_scale_profile() {
        let stream = styled_output_stream(2_000);
        let mut tracker = DecrqssQueryTracker::new().expect("tracker");

        // Warm the allocator and instruction cache before measuring.
        tracker.observe(&stream);

        let started = std::time::Instant::now();
        tracker.observe(&stream);
        let elapsed = started.elapsed();

        // Baseline: what the pane already spends parsing the same bytes.
        let mut terminal = crate::ghostty::Terminal::new(80, 24, 0).expect("terminal");
        terminal.write(&stream);
        let baseline_started = std::time::Instant::now();
        terminal.write(&stream);
        let baseline = baseline_started.elapsed();

        let megabytes = stream.len() as f64 / (1024.0 * 1024.0);
        println!(
            "decrqss observe: {} bytes in {elapsed:?} ({:.0} MiB/s)",
            stream.len(),
            megabytes / elapsed.as_secs_f64()
        );
        println!(
            "libghostty write (baseline): {baseline:?} ({:.0} MiB/s)",
            megabytes / baseline.as_secs_f64()
        );
        println!(
            "decrqss marginal cost: {:.1}% of the existing parse",
            elapsed.as_secs_f64() / baseline.as_secs_f64() * 100.0
        );
    }

    #[test]
    fn reports_response_end_offsets() {
        let mut tracker = DecrqssQueryTracker::new().expect("tracker");

        tracker.observe(b"before\x1bP$qm\x1b\\after");

        assert_eq!(
            tracker.drain_pending(),
            vec![DecrqssResponse {
                end_offset: 13,
                bytes: Bytes::from_static(b"\x1bP1$r0m\x1b\\"),
            }]
        );
    }
}
