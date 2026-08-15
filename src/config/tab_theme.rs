//! Optional per-element colors for the desktop tab row.
//!
//! These live outside [`crate::config::CustomThemeColors`] on purpose. That
//! struct mirrors the semantic palette — `accent`, `surface1`, `text` — which
//! every widget draws from. Tab chrome needs colors that have no semantic
//! meaning elsewhere (the number chip's background, the inactive tab's body),
//! and folding them into the palette would grow it by a handful of entries per
//! widget that ever wants direct control.
//!
//! Every field is optional. Unset fields fall back to the palette-derived
//! defaults in the tab bar, so a theme that says nothing here keeps working and
//! keeps following whatever base theme is active.

use serde::Deserialize;

/// Raw, unparsed tab colors as written in `config.toml`.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct TabThemeConfig {
    /// Inactive tab body.
    pub fg: Option<String>,
    pub bg: Option<String>,
    /// Active tab body.
    pub active_fg: Option<String>,
    pub active_bg: Option<String>,
    /// Inactive tab's number chip.
    pub number_fg: Option<String>,
    pub number_bg: Option<String>,
    /// Active tab's number chip.
    pub active_number_fg: Option<String>,
    pub active_number_bg: Option<String>,
}

/// Tab colors resolved to terminal colors, ready for rendering.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TabTheme {
    pub fg: Option<ratatui::style::Color>,
    pub bg: Option<ratatui::style::Color>,
    pub active_fg: Option<ratatui::style::Color>,
    pub active_bg: Option<ratatui::style::Color>,
    pub number_fg: Option<ratatui::style::Color>,
    pub number_bg: Option<ratatui::style::Color>,
    pub active_number_fg: Option<ratatui::style::Color>,
    pub active_number_bg: Option<ratatui::style::Color>,
}

impl TabThemeConfig {
    pub fn resolve(&self) -> TabTheme {
        let parse = |value: &Option<String>| value.as_deref().map(super::parse_color);
        TabTheme {
            fg: parse(&self.fg),
            bg: parse(&self.bg),
            active_fg: parse(&self.active_fg),
            active_bg: parse(&self.active_bg),
            number_fg: parse(&self.number_fg),
            number_bg: parse(&self.number_bg),
            active_number_fg: parse(&self.active_number_fg),
            active_number_bg: parse(&self.active_number_bg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Color;

    #[test]
    fn unset_colors_resolve_to_none_so_the_palette_defaults_win() {
        assert_eq!(TabThemeConfig::default().resolve(), TabTheme::default());
    }

    #[test]
    fn resolves_every_element_independently() {
        let config = TabThemeConfig {
            fg: Some("#c0caf5".into()),
            bg: Some("#2f3549".into()),
            active_fg: Some("#c0caf5".into()),
            active_bg: Some("#414868".into()),
            number_fg: Some("#1a1b26".into()),
            number_bg: Some("#787c99".into()),
            active_number_fg: Some("#1a1b26".into()),
            active_number_bg: Some("#bb9af7".into()),
        };

        let resolved = config.resolve();

        assert_eq!(resolved.bg, Some(Color::Rgb(0x2f, 0x35, 0x49)));
        assert_eq!(resolved.number_bg, Some(Color::Rgb(0x78, 0x7c, 0x99)));
        assert_eq!(
            resolved.active_number_bg,
            Some(Color::Rgb(0xbb, 0x9a, 0xf7))
        );
        assert_eq!(resolved.active_bg, Some(Color::Rgb(0x41, 0x48, 0x68)));
    }

    #[test]
    fn a_tmux_window_status_style_can_be_expressed_in_full() {
        // Mirrors a tmux config of the shape
        //   window-status-format         '#[fg=A,bg=B] #I #[fg=C,bg=D] #W '
        //   window-status-current-format '#[fg=A,bg=E] #I #[fg=C,bg=F] #W '
        // Every element tmux styles separately has a key here, so a tab row can
        // be matched to an existing status line exactly.
        let config: crate::config::Config = toml::from_str(
            r##"
[theme.tabs]
number_fg = "#1a1b26"
number_bg = "#787c99"
fg = "#c0caf5"
bg = "#2f3549"
active_number_fg = "#1a1b26"
active_number_bg = "#bb9af7"
active_fg = "#c0caf5"
active_bg = "#414868"
"##,
        )
        .unwrap();

        let resolved = config.theme.tabs.as_ref().unwrap().resolve();

        assert_eq!(resolved.number_bg, Some(Color::Rgb(0x78, 0x7c, 0x99)));
        assert_eq!(resolved.number_fg, Some(Color::Rgb(0x1a, 0x1b, 0x26)));
        assert_eq!(resolved.bg, Some(Color::Rgb(0x2f, 0x35, 0x49)));
        assert_eq!(
            resolved.active_number_bg,
            Some(Color::Rgb(0xbb, 0x9a, 0xf7))
        );
        assert_eq!(resolved.active_bg, Some(Color::Rgb(0x41, 0x48, 0x68)));
        assert_eq!(resolved.active_fg, Some(Color::Rgb(0xc0, 0xca, 0xf5)));
    }

    #[test]
    fn accepts_the_same_color_spellings_as_the_rest_of_the_theme() {
        let config = TabThemeConfig {
            number_bg: Some("rgb(120, 124, 153)".into()),
            bg: Some("reset".into()),
            ..Default::default()
        };

        let resolved = config.resolve();

        assert_eq!(resolved.number_bg, Some(Color::Rgb(120, 124, 153)));
        assert_eq!(resolved.bg, Some(Color::Reset));
    }
}
