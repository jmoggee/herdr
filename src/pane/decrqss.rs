//! Tracks DECRQSS SGR query boundaries in pane output.
//!
//! Libghostty answers DECRQSS through its write-PTY callback. Herdr only needs
//! the end offset of each SGR query so it can read libghostty's live cursor
//! style at that exact point and add the underline color omitted by the native
//! reply.

const MAX_DCS_BODY: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DecrqssQuery {
    pub(super) end_offset: usize,
}

#[derive(Debug, Default)]
pub(super) struct DecrqssQueryTracker {
    state: State,
    body: Vec<u8>,
    pending: Vec<DecrqssQuery>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum State {
    #[default]
    Ground,
    Escape,
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
    pub(super) fn observe(&mut self, bytes: &[u8]) {
        for (index, &byte) in bytes.iter().enumerate() {
            match self.state {
                State::Ground => match byte {
                    0x1b => self.state = State::Escape,
                    0x90 => self.enter_dcs(),
                    0x9d => self.state = State::IgnoreOsc,
                    0x98 | 0x9e | 0x9f => self.state = State::IgnoreString,
                    _ => {}
                },
                State::Escape => match byte {
                    b'P' => self.enter_dcs(),
                    b']' => self.state = State::IgnoreOsc,
                    b'_' | b'^' | b'X' => self.state = State::IgnoreString,
                    0x1b => self.state = State::Escape,
                    _ => self.state = State::Ground,
                },
                State::DcsIntro => match byte {
                    b'$' => self.state = State::DcsDollar,
                    0x30..=0x3f => {}
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

    pub(super) fn drain_pending(&mut self) -> Vec<DecrqssQuery> {
        std::mem::take(&mut self.pending)
    }

    fn enter_dcs(&mut self) {
        self.body.clear();
        self.state = State::DcsIntro;
    }

    fn finalize(&mut self, end_offset: usize) {
        if self.body == b"m" {
            self.pending.push(DecrqssQuery { end_offset });
        }
        self.body.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_sgr_query_end_offsets() {
        let mut tracker = DecrqssQueryTracker::default();
        let bytes = b"a\x1bP$qm\x1b\\bc\x90$qm\x9c";

        tracker.observe(bytes);

        assert_eq!(
            tracker.drain_pending(),
            vec![
                DecrqssQuery { end_offset: 8 },
                DecrqssQuery {
                    end_offset: bytes.len()
                },
            ]
        );
    }

    #[test]
    fn keeps_a_query_split_across_writes() {
        let mut tracker = DecrqssQueryTracker::default();

        tracker.observe(b"\x1bP$q");
        assert!(tracker.drain_pending().is_empty());
        tracker.observe(b"m\x1b\\tail");

        assert_eq!(
            tracker.drain_pending(),
            vec![DecrqssQuery { end_offset: 3 }]
        );
    }

    #[test]
    fn ignores_other_queries_and_string_payloads() {
        let mut tracker = DecrqssQueryTracker::default();

        tracker.observe(b"\x1bP$q q\x1b\\\x1b]0;\x1bP$qm\x1b\\\x07");

        assert!(tracker.drain_pending().is_empty());
    }
}
