use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use super::scrollbar::{render_pane_scrollbar, should_show_scrollbar};
use super::widgets::panel_contrast_fg;
use crate::app::state::{Palette, PaneBorderStyle};
use crate::app::{AppState, Mode};
use crate::layout::PaneInfo;
use crate::terminal::{TerminalRuntime, TerminalRuntimeRegistry};
use crate::workspace::Workspace;

const BORDER_LEFT: u8 = 0b0001;
const BORDER_RIGHT: u8 = 0b0010;
const BORDER_UP: u8 = 0b0100;
const BORDER_DOWN: u8 = 0b1000;

pub(crate) fn pane_is_scrolled_back(rt: &TerminalRuntime) -> bool {
    rt.scroll_metrics()
        .is_some_and(|metrics| metrics.offset_from_bottom > 0)
}

fn truncate_label(text: &str, max_width: usize) -> String {
    let len = text.chars().count();
    if len <= max_width {
        return text.to_string();
    }
    if max_width == 0 {
        return String::new();
    }
    if max_width == 1 {
        return "…".to_string();
    }
    let prefix: String = text.chars().take(max_width.saturating_sub(1)).collect();
    format!("{prefix}…")
}

fn pane_border_title(label: &str, pane_width: u16) -> Option<String> {
    let label = label.trim();
    if label.is_empty() || pane_width <= 4 {
        return None;
    }
    let max_label_width = pane_width.saturating_sub(4) as usize;
    Some(format!(" {} ", truncate_label(label, max_label_width)))
}

fn stable_terminal_inner_rect(pane_inner: Rect) -> Rect {
    if pane_inner.width <= 4 {
        return pane_inner;
    }

    Rect::new(
        pane_inner.x,
        pane_inner.y,
        pane_inner.width.saturating_sub(1),
        pane_inner.height,
    )
}

fn pane_inner_rect(area: Rect, framed: bool) -> Rect {
    if framed {
        Block::default().borders(Borders::ALL).inner(area)
    } else {
        area
    }
}

fn ranges_overlap(a_start: u16, a_len: u16, b_start: u16, b_len: u16) -> bool {
    a_start < b_start.saturating_add(b_len) && a_start.saturating_add(a_len) > b_start
}

fn single_pane_inner_rect(rect: Rect, splits: &[crate::layout::SplitBorder]) -> Rect {
    let mut inner = rect;
    if inner.width == 0 || inner.height == 0 {
        return inner;
    }

    let has_left_divider = splits.iter().any(|split| {
        split.direction == ratatui::layout::Direction::Horizontal
            && split.pos == rect.x
            && ranges_overlap(split.area.y, split.area.height, rect.y, rect.height)
    });
    let has_top_divider = splits.iter().any(|split| {
        split.direction == ratatui::layout::Direction::Vertical
            && split.pos == rect.y
            && ranges_overlap(split.area.x, split.area.width, rect.x, rect.width)
    });

    if has_left_divider {
        inner.x = inner.x.saturating_add(1);
        inner.width = inner.width.saturating_sub(1);
    }
    if has_top_divider {
        inner.y = inner.y.saturating_add(1);
        inner.height = inner.height.saturating_sub(1);
    }

    inner
}

fn runtime_for_tab_pane<'a>(
    terminal_runtimes: &'a TerminalRuntimeRegistry,
    tab: &'a crate::workspace::Tab,
    pane_id: crate::layout::PaneId,
) -> Option<(&'a crate::terminal::TerminalId, &'a TerminalRuntime)> {
    let terminal_id = tab.terminal_id(pane_id)?;
    #[cfg(test)]
    if let Some(runtime) = tab.runtimes.get(&pane_id) {
        return Some((terminal_id, runtime));
    }
    terminal_runtimes
        .get(terminal_id)
        .map(|runtime| (terminal_id, runtime))
}

fn stable_scrollbar_gutter(rt: &TerminalRuntime, pane_inner: Rect) -> (Rect, Option<Rect>) {
    let inner_rect = stable_terminal_inner_rect(pane_inner);
    if inner_rect == pane_inner {
        return (inner_rect, None);
    }
    let gutter = Rect::new(
        pane_inner.x + pane_inner.width.saturating_sub(1),
        pane_inner.y,
        1,
        pane_inner.height,
    );
    let scrollbar_rect = rt
        .scroll_metrics()
        .filter(|metrics| should_show_scrollbar(*metrics))
        .map(|_| gutter);

    (inner_rect, scrollbar_rect)
}

/// Resize every visible runtime in a tab to the geometry it would receive if the tab were selected.
pub(super) fn resize_tab_panes(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    tab: &crate::workspace::Tab,
    area: Rect,
    cell_size: crate::kitty_graphics::HostCellSize,
) {
    let multi_pane = tab.layout.pane_count() > 1;

    if tab.zoomed {
        let focused_id = tab.layout.focused();
        if let Some((terminal_id, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, focused_id) {
            let pane_inner = pane_inner_rect(area, multi_pane);
            let inner_rect = stable_terminal_inner_rect(pane_inner);
            if !app.direct_attach_resize_locks.contains(terminal_id) {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
        return;
    }

    for info in tab.layout.panes(area) {
        let pane_inner = if multi_pane {
            if app.pane_border_style == PaneBorderStyle::Single {
                let splits = tab.layout.splits(area);
                single_pane_inner_rect(info.rect, &splits)
            } else {
                Block::default().borders(Borders::ALL).inner(info.rect)
            }
        } else {
            area
        };

        if let Some((terminal_id, rt)) = runtime_for_tab_pane(terminal_runtimes, tab, info.id) {
            let inner_rect = stable_terminal_inner_rect(pane_inner);
            if !app.direct_attach_resize_locks.contains(terminal_id) {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
    }
}

/// Compute pane layout info and optionally resize pane runtimes to match.
pub(super) fn compute_pane_infos(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    area: Rect,
    resize_panes: bool,
    cell_size: crate::kitty_graphics::HostCellSize,
) -> Vec<PaneInfo> {
    let Some(ws_idx) = app.active else {
        return Vec::new();
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        return Vec::new();
    };

    let multi_pane = ws.layout.pane_count() > 1;
    let terminal_active = app.mode == Mode::Terminal;

    if ws.zoomed {
        let focused_id = ws.layout.focused();
        let pane_inner = pane_inner_rect(area, multi_pane);
        let mut inner_rect = pane_inner;
        let mut scrollbar_rect = None;
        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, focused_id) {
            (inner_rect, scrollbar_rect) = stable_scrollbar_gutter(rt, pane_inner);
            if resize_panes
                && ws.terminal_id(focused_id).is_some_and(|terminal_id| {
                    !app.direct_attach_resize_locks.contains(terminal_id)
                })
            {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }
        return vec![PaneInfo {
            id: focused_id,
            rect: area,
            inner_rect,
            scrollbar_rect,
            is_focused: true,
        }];
    }

    let mut pane_infos = ws.layout.panes(area);

    for info in &mut pane_infos {
        let pane_inner = if multi_pane {
            if app.pane_border_style == PaneBorderStyle::Single {
                let splits = ws.layout.splits(area);
                single_pane_inner_rect(info.rect, &splits)
            } else {
                let border_set = if info.is_focused && terminal_active {
                    ratatui::symbols::border::THICK
                } else {
                    ratatui::symbols::border::PLAIN
                };
                let block = Block::default()
                    .borders(Borders::ALL)
                    .border_set(border_set);
                block.inner(info.rect)
            }
        } else {
            area
        };

        let mut inner_rect = pane_inner;
        let mut scrollbar_rect = None;
        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, info.id) {
            (inner_rect, scrollbar_rect) = stable_scrollbar_gutter(rt, pane_inner);
            if resize_panes
                && ws.terminal_id(info.id).is_some_and(|terminal_id| {
                    !app.direct_attach_resize_locks.contains(terminal_id)
                })
            {
                rt.resize(
                    inner_rect.height,
                    inner_rect.width,
                    cell_size.width_px,
                    cell_size.height_px,
                );
            }
        }

        info.inner_rect = inner_rect;
        info.scrollbar_rect = scrollbar_rect;
    }

    pane_infos
}

pub(super) fn render_panes(
    app: &AppState,
    terminal_runtimes: &TerminalRuntimeRegistry,
    frame: &mut Frame,
    area: Rect,
) {
    let Some(ws_idx) = app.active else {
        render_empty(app, frame, area);
        return;
    };
    let Some(ws) = app.workspaces.get(ws_idx) else {
        render_empty(app, frame, area);
        return;
    };

    let multi_pane = ws.layout.pane_count() > 1;
    let terminal_active = app.mode == Mode::Terminal;

    if multi_pane && app.pane_border_style == PaneBorderStyle::Single {
        render_single_pane_borders(app, ws, frame, area);
    }

    for info in &app.view.pane_infos {
        if let Some(rt) = app.runtime_for_pane_in_workspace(terminal_runtimes, ws_idx, info.id) {
            if multi_pane && app.pane_border_style == PaneBorderStyle::Boxed {
                let (border_style, border_set) = if info.is_focused && terminal_active {
                    (
                        Style::default().fg(app.palette.accent),
                        ratatui::symbols::border::THICK,
                    )
                } else if info.is_focused {
                    (
                        Style::default().fg(app.palette.accent),
                        ratatui::symbols::border::PLAIN,
                    )
                } else {
                    (
                        Style::default().fg(app.palette.overlay0),
                        ratatui::symbols::border::PLAIN,
                    )
                };

                let mut block = Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .border_set(border_set);
                if let Some(title) = ws
                    .pane_state(info.id)
                    .and_then(|pane| app.terminals.get(&pane.attached_terminal_id))
                    .and_then(|terminal| {
                        terminal.border_label(app.show_agent_labels_on_pane_borders)
                    })
                    .and_then(|label| pane_border_title(&label, info.rect.width))
                {
                    block = block.title(Line::from(Span::styled(title, border_style)));
                }
                frame.render_widget(block, info.rect);
            }

            let show_cursor = info.is_focused
                && terminal_active
                && !pane_is_scrolled_back(rt)
                && app.pane_exposes_host_cursor(ws_idx, info.id);
            rt.render(frame, info.inner_rect, show_cursor);
            render_pane_scrollbar(app, frame, info, rt);

            let should_dim = !info.is_focused && multi_pane && !terminal_active;
            if should_dim {
                let inner = info.inner_rect;
                let buf = frame.buffer_mut();
                for y in inner.y..inner.y + inner.height {
                    for x in inner.x..inner.x + inner.width {
                        let cell = &mut buf[(x, y)];
                        cell.set_style(cell.style().add_modifier(Modifier::DIM));
                    }
                }
            }

            render_selection_highlight(
                &app.selection,
                frame,
                info.id,
                info.inner_rect,
                rt.scroll_metrics(),
                &app.palette,
                app.host_terminal_theme,
            );
            render_copy_mode_cursor(app, frame, info);
        }
    }
}

fn border_symbol(mask: u8) -> &'static str {
    match mask {
        BORDER_LEFT | BORDER_RIGHT => "─",
        BORDER_UP | BORDER_DOWN => "│",
        x if x == (BORDER_LEFT | BORDER_RIGHT) => "─",
        x if x == (BORDER_UP | BORDER_DOWN) => "│",
        x if x == BORDER_RIGHT | BORDER_DOWN => "┌",
        x if x == BORDER_LEFT | BORDER_DOWN => "┐",
        x if x == BORDER_RIGHT | BORDER_UP => "└",
        x if x == BORDER_LEFT | BORDER_UP => "┘",
        x if x == BORDER_LEFT | BORDER_RIGHT | BORDER_DOWN => "┬",
        x if x == BORDER_LEFT | BORDER_RIGHT | BORDER_UP => "┴",
        x if x == BORDER_RIGHT | BORDER_UP | BORDER_DOWN => "├",
        x if x == BORDER_LEFT | BORDER_UP | BORDER_DOWN => "┤",
        x if x == BORDER_LEFT | BORDER_RIGHT | BORDER_UP | BORDER_DOWN => "┼",
        _ => " ",
    }
}

fn add_single_border_cell(
    cells: &mut std::collections::BTreeMap<(u16, u16), (u8, bool)>,
    x: u16,
    y: u16,
    mask: u8,
    focused: bool,
) {
    let entry = cells.entry((x, y)).or_insert((0, false));
    entry.0 |= mask;
    entry.1 |= focused;
}

fn solidify_single_border_junctions(
    cells: &mut std::collections::BTreeMap<(u16, u16), (u8, bool)>,
) {
    let snapshot = cells.clone();
    for (&(x, y), &(mask, focused)) in &snapshot {
        let has_vertical = mask & (BORDER_UP | BORDER_DOWN) != 0;
        let has_horizontal = mask & (BORDER_LEFT | BORDER_RIGHT) != 0;
        let mut extra_mask = 0;
        let mut extra_focused = focused;

        if has_vertical {
            if let Some((left_mask, left_focused)) =
                x.checked_sub(1).and_then(|left| snapshot.get(&(left, y)))
            {
                if left_mask & (BORDER_LEFT | BORDER_RIGHT) != 0 {
                    extra_mask |= BORDER_LEFT;
                    extra_focused |= *left_focused;
                }
            }
            if let Some((right_mask, right_focused)) = snapshot.get(&(x.saturating_add(1), y)) {
                if right_mask & (BORDER_LEFT | BORDER_RIGHT) != 0 {
                    extra_mask |= BORDER_RIGHT;
                    extra_focused |= *right_focused;
                }
            }
        }

        if has_horizontal {
            if let Some((up_mask, up_focused)) =
                y.checked_sub(1).and_then(|up| snapshot.get(&(x, up)))
            {
                if up_mask & (BORDER_UP | BORDER_DOWN) != 0 {
                    extra_mask |= BORDER_UP;
                    extra_focused |= *up_focused;
                }
            }
            if let Some((down_mask, down_focused)) = snapshot.get(&(x, y.saturating_add(1))) {
                if down_mask & (BORDER_UP | BORDER_DOWN) != 0 {
                    extra_mask |= BORDER_DOWN;
                    extra_focused |= *down_focused;
                }
            }
        }

        if extra_mask != 0 {
            let entry = cells.entry((x, y)).or_insert((0, false));
            entry.0 |= extra_mask;
            entry.1 |= extra_focused;
        }
    }
}

fn two_pane_split_cell_focused(
    split: &crate::layout::SplitBorder,
    focused: Rect,
    x: u16,
    y: u16,
) -> bool {
    match split.direction {
        ratatui::layout::Direction::Horizontal => {
            let midpoint = split.area.y.saturating_add(split.area.height / 2);
            if focused.x < split.pos {
                y < midpoint
            } else {
                y >= midpoint
            }
        }
        ratatui::layout::Direction::Vertical => {
            let midpoint = split.area.x.saturating_add(split.area.width / 2);
            if focused.y < split.pos {
                x < midpoint
            } else {
                x >= midpoint
            }
        }
    }
}

fn split_cell_touches_focused_pane(
    split: &crate::layout::SplitBorder,
    focused: Rect,
    pane_count: usize,
    x: u16,
    y: u16,
) -> bool {
    if pane_count == 2 {
        return two_pane_split_cell_focused(split, focused, x, y);
    }

    match split.direction {
        ratatui::layout::Direction::Horizontal => {
            (split.pos == focused.x || split.pos == focused.x.saturating_add(focused.width))
                && ranges_overlap(y, 1, focused.y, focused.height)
        }
        ratatui::layout::Direction::Vertical => {
            (split.pos == focused.y || split.pos == focused.y.saturating_add(focused.height))
                && ranges_overlap(x, 1, focused.x, focused.width)
        }
    }
}

fn render_single_pane_borders(app: &AppState, ws: &Workspace, frame: &mut Frame, area: Rect) {
    let mut cells = std::collections::BTreeMap::new();
    let splits = ws.layout.splits(area);
    let focused = app.view.pane_infos.iter().find(|info| info.is_focused);
    let pane_count = app.view.pane_infos.len();

    for split in &splits {
        let Some(focused) = focused else {
            break;
        };
        match split.direction {
            ratatui::layout::Direction::Horizontal => {
                let x = split.pos;
                let y1 = split.area.y;
                let y2 = split
                    .area
                    .y
                    .saturating_add(split.area.height)
                    .saturating_sub(1);
                for y in y1..=y2 {
                    let mut mask = 0;
                    if y > y1 {
                        mask |= BORDER_UP;
                    }
                    if y < y2 {
                        mask |= BORDER_DOWN;
                    }
                    let cell_focused =
                        split_cell_touches_focused_pane(split, focused.rect, pane_count, x, y);
                    add_single_border_cell(&mut cells, x, y, mask, cell_focused);
                }
            }
            ratatui::layout::Direction::Vertical => {
                let y = split.pos;
                let x1 = split.area.x;
                let x2 = split
                    .area
                    .x
                    .saturating_add(split.area.width)
                    .saturating_sub(1);
                for x in x1..=x2 {
                    let mut mask = 0;
                    if x > x1 {
                        mask |= BORDER_LEFT;
                    }
                    if x < x2 {
                        mask |= BORDER_RIGHT;
                    }
                    let cell_focused =
                        split_cell_touches_focused_pane(split, focused.rect, pane_count, x, y);
                    add_single_border_cell(&mut cells, x, y, mask, cell_focused);
                }
            }
        }
    }

    solidify_single_border_junctions(&mut cells);

    let buffer = frame.buffer_mut();
    for ((x, y), (mask, focused)) in cells {
        let style = if focused {
            Style::default().fg(app.palette.accent)
        } else {
            Style::default().fg(app.palette.overlay0)
        };
        buffer[(x, y)]
            .set_symbol(border_symbol(mask))
            .set_style(style);
    }
}

fn render_copy_mode_cursor(app: &AppState, frame: &mut Frame, info: &PaneInfo) {
    if app.mode != Mode::Copy {
        return;
    }
    let Some(copy_mode) = app.copy_mode else {
        return;
    };
    if copy_mode.pane_id != info.id
        || copy_mode.cursor_row >= info.inner_rect.height
        || copy_mode.cursor_col >= info.inner_rect.width
    {
        return;
    }

    let x = info.inner_rect.x + copy_mode.cursor_col;
    let y = info.inner_rect.y + copy_mode.cursor_row;
    let cell = &mut frame.buffer_mut()[(x, y)];
    cell.set_style(
        Style::default()
            .fg(panel_contrast_fg(&app.palette))
            .bg(app.palette.accent)
            .add_modifier(Modifier::BOLD),
    );
}

fn render_selection_highlight(
    selection: &Option<crate::selection::Selection>,
    frame: &mut Frame,
    pane_id: crate::layout::PaneId,
    inner: Rect,
    scroll_metrics: Option<crate::pane::ScrollMetrics>,
    p: &Palette,
    host_theme: crate::terminal_theme::TerminalTheme,
) {
    if let Some(sel) = selection {
        if sel.is_visible() && sel.pane_id == pane_id {
            let buf = frame.buffer_mut();
            let style = automatic_selection_style(p, host_theme);
            for y in 0..inner.height {
                for x in 0..inner.width {
                    if sel.contains(y, x, scroll_metrics) {
                        let cell = &mut buf[(inner.x + x, inner.y + y)];
                        cell.set_style(style);
                    }
                }
            }
        }
    }
}

type Rgb = (u8, u8, u8);

fn automatic_selection_style(
    p: &Palette,
    host_theme: crate::terminal_theme::TerminalTheme,
) -> Style {
    let bg = automatic_selection_bg(p, host_theme);
    Style::reset().fg(selection_fg_for_bg(bg, p)).bg(bg)
}

fn automatic_selection_bg(p: &Palette, host_theme: crate::terminal_theme::TerminalTheme) -> Color {
    let Some(background) = host_theme.background.map(terminal_theme_to_rgb) else {
        return selection_palette_background(p);
    };

    let target = if relative_luminance(background) < 0.5 {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };
    let selected = mix_rgb(background, target, 0.28);
    Color::Rgb(selected.0, selected.1, selected.2)
}

fn selection_palette_background(p: &Palette) -> Color {
    if p.panel_bg == Color::Reset {
        p.surface_dim
    } else {
        p.panel_bg
    }
}

fn terminal_theme_to_rgb(color: crate::terminal_theme::RgbColor) -> Rgb {
    (color.r, color.g, color.b)
}

fn selection_fg_for_bg(bg: Color, p: &Palette) -> Color {
    color_to_rgb(bg)
        .map(|bg| {
            if relative_luminance(bg) < 0.5 {
                Color::White
            } else {
                Color::Black
            }
        })
        .unwrap_or_else(|| panel_contrast_fg(p))
}

fn mix_rgb(base: Rgb, target: Rgb, amount: f32) -> Rgb {
    fn channel(base: u8, target: u8, amount: f32) -> u8 {
        (f32::from(base) + (f32::from(target) - f32::from(base)) * amount).round() as u8
    }
    (
        channel(base.0, target.0, amount),
        channel(base.1, target.1, amount),
        channel(base.2, target.2, amount),
    )
}

fn relative_luminance(color: Rgb) -> f32 {
    fn channel(value: u8) -> f32 {
        let value = f32::from(value) / 255.0;
        if value <= 0.03928 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(color.0) + 0.7152 * channel(color.1) + 0.0722 * channel(color.2)
}

fn color_to_rgb(color: Color) -> Option<Rgb> {
    match color {
        Color::Reset => None,
        Color::Black => Some((0, 0, 0)),
        Color::Red => Some((128, 0, 0)),
        Color::Green => Some((0, 128, 0)),
        Color::Yellow => Some((128, 128, 0)),
        Color::Blue => Some((0, 0, 128)),
        Color::Magenta => Some((128, 0, 128)),
        Color::Cyan => Some((0, 128, 128)),
        Color::Gray => Some((192, 192, 192)),
        Color::DarkGray => Some((128, 128, 128)),
        Color::LightRed => Some((255, 0, 0)),
        Color::LightGreen => Some((0, 255, 0)),
        Color::LightYellow => Some((255, 255, 0)),
        Color::LightBlue => Some((0, 0, 255)),
        Color::LightMagenta => Some((255, 0, 255)),
        Color::LightCyan => Some((0, 255, 255)),
        Color::White => Some((255, 255, 255)),
        Color::Rgb(r, g, b) => Some((r, g, b)),
        Color::Indexed(_) => None,
    }
}

fn render_empty(app: &AppState, frame: &mut Frame, area: Rect) {
    let p = &app.palette;
    let lines = vec![
        Line::from(""),
        Line::from(""),
        Line::from(Span::styled(
            "  No workspaces yet",
            Style::default().fg(p.overlay0),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "  A workspace is one project context.",
            Style::default().fg(p.overlay1),
        )),
        Line::from(Span::styled(
            "  Its root pane (top-left) sets the default repo or folder name.",
            Style::default().fg(p.overlay1),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("  Press ", Style::default().fg(p.overlay0)),
            Span::styled(
                app.keybinds
                    .new_workspace
                    .label()
                    .unwrap_or_else(|| "unset".to_string()),
                Style::default().fg(p.accent).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to create one", Style::default().fg(p.overlay0)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(p.surface_dim)),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::PaneId;
    use crate::selection::Selection;
    use crate::terminal::TerminalRuntime;
    use crate::workspace::Workspace;

    #[test]
    fn pane_border_title_trims_and_truncates() {
        assert_eq!(
            pane_border_title(" claude ", 20).as_deref(),
            Some(" claude ")
        );
        assert_eq!(pane_border_title("", 20), None);
        assert_eq!(pane_border_title("abcdef", 8).as_deref(), Some(" abc… "));
        assert_eq!(pane_border_title("abcdef", 4), None);
    }

    #[tokio::test]
    async fn pane_scrollbar_gutter_is_reserved_before_scrollback_exists() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[tokio::test]
    async fn single_pane_borders_use_plain_internal_divider_with_accent_color() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        let right_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root_pane);
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(18, 4, 128, b"left\n"),
        );
        workspace.tabs[0].runtimes.insert(
            right_pane,
            TerminalRuntime::test_with_scrollback_bytes(18, 4, 128, b"right\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.pane_border_style = crate::app::state::PaneBorderStyle::Single;

        let area = Rect::new(0, 0, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );

        let backend = ratatui::backend::TestBackend::new(40, 8);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(20, 0)].symbol(), "│");
        assert_ne!(buffer[(20, 0)].symbol(), "┃");
        assert_eq!(buffer[(20, 0)].style().fg, Some(app.palette.accent));
    }

    #[tokio::test]
    async fn single_pane_borders_draw_only_internal_divider_without_outer_frame() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        let right_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root_pane);
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(10, 4, 128, b"left\n"),
        );
        workspace.tabs[0].runtimes.insert(
            right_pane,
            TerminalRuntime::test_with_scrollback_bytes(10, 4, 128, b"right\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.pane_border_style = crate::app::state::PaneBorderStyle::Single;

        let area = Rect::new(0, 0, 20, 6);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );

        let backend = ratatui::backend::TestBackend::new(20, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .unwrap();

        let buffer = terminal.backend().buffer();
        assert_ne!(buffer[(0, 0)].symbol(), "┌");
        assert_eq!(buffer[(10, 0)].symbol(), "│");
        assert_eq!(buffer[(10, 5)].symbol(), "│");
        assert_eq!(app.view.pane_infos[0].inner_rect, Rect::new(0, 0, 9, 6));
        assert_eq!(app.view.pane_infos[1].inner_rect, Rect::new(11, 0, 8, 6));
    }

    #[tokio::test]
    async fn single_pane_two_column_split_highlights_top_or_bottom_half_for_focus() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        let right_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(10, 4, 128, b"left\n"),
        );
        workspace.tabs[0].runtimes.insert(
            right_pane,
            TerminalRuntime::test_with_scrollback_bytes(10, 4, 128, b"right\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.pane_border_style = crate::app::state::PaneBorderStyle::Single;

        let area = Rect::new(0, 0, 20, 6);
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        app.workspaces[0].tabs[0].layout.focus_pane(root_pane);
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let backend = ratatui::backend::TestBackend::new(20, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(10, 1)].style().fg, Some(app.palette.accent));
        assert_eq!(buffer[(10, 4)].style().fg, Some(app.palette.overlay0));

        app.workspaces[0].tabs[0].layout.focus_pane(right_pane);
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let backend = ratatui::backend::TestBackend::new(20, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(10, 1)].style().fg, Some(app.palette.overlay0));
        assert_eq!(buffer[(10, 4)].style().fg, Some(app.palette.accent));
    }

    #[tokio::test]
    async fn single_pane_two_row_split_highlights_left_or_right_half_for_focus() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        let bottom_pane = workspace.test_split(ratatui::layout::Direction::Vertical);
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(10, 4, 128, b"top\n"),
        );
        workspace.tabs[0].runtimes.insert(
            bottom_pane,
            TerminalRuntime::test_with_scrollback_bytes(10, 4, 128, b"bottom\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.pane_border_style = crate::app::state::PaneBorderStyle::Single;

        let area = Rect::new(0, 0, 20, 6);
        let terminal_runtimes = TerminalRuntimeRegistry::new();

        app.workspaces[0].tabs[0].layout.focus_pane(root_pane);
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let backend = ratatui::backend::TestBackend::new(20, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(4, 3)].style().fg, Some(app.palette.accent));
        assert_eq!(buffer[(16, 3)].style().fg, Some(app.palette.overlay0));

        app.workspaces[0].tabs[0].layout.focus_pane(bottom_pane);
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let backend = ratatui::backend::TestBackend::new(20, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(4, 3)].style().fg, Some(app.palette.overlay0));
        assert_eq!(buffer[(16, 3)].style().fg, Some(app.palette.accent));
    }

    #[tokio::test]
    async fn single_pane_grid_split_renders_solid_cross_junction() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        let top_right = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.tabs[0].layout.focus_pane(root_pane);
        let bottom_left = workspace.test_split(ratatui::layout::Direction::Vertical);
        workspace.tabs[0].layout.focus_pane(top_right);
        let bottom_right = workspace.test_split(ratatui::layout::Direction::Vertical);

        for (pane, label) in [
            (root_pane, b"top-left\n".as_slice()),
            (top_right, b"top-right\n".as_slice()),
            (bottom_left, b"bottom-left\n".as_slice()),
            (bottom_right, b"bottom-right\n".as_slice()),
        ] {
            workspace.tabs[0].runtimes.insert(
                pane,
                TerminalRuntime::test_with_scrollback_bytes(10, 4, 128, label),
            );
        }
        app.workspaces = vec![workspace];
        app.active = Some(0);
        app.pane_border_style = crate::app::state::PaneBorderStyle::Single;

        let area = Rect::new(0, 0, 20, 6);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        app.view.pane_infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );

        let backend = ratatui::backend::TestBackend::new(20, 6);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| render_panes(&app, &terminal_runtimes, frame, area))
            .unwrap();

        assert_eq!(terminal.backend().buffer()[(10, 3)].symbol(), "┼");
    }

    #[tokio::test]
    async fn zoomed_pane_scrollbar_gutter_is_reserved_before_scrollback_exists() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        workspace.zoomed = true;
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[tokio::test]
    async fn zoomed_multi_pane_keeps_border_space() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let focused_pane = workspace.test_split(ratatui::layout::Direction::Horizontal);
        workspace.zoomed = true;
        workspace.tabs[0].runtimes.insert(
            focused_pane,
            TerminalRuntime::test_with_scrollback_bytes(40, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.id, focused_pane);
        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, Rect::new(11, 4, 37, 6));
    }

    #[tokio::test]
    async fn tiny_pane_does_not_reserve_scrollbar_gutter() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(4, 8, 1024, b"ready\n"),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 4, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, None);
        assert_eq!(info.inner_rect, area);
    }

    #[tokio::test]
    async fn pane_scrollbar_reserves_last_column_from_terminal_area() {
        let mut app = AppState::test_new();
        let mut workspace = Workspace::test_new("test");
        let root_pane = workspace.tabs[0].root_pane;
        workspace.tabs[0].runtimes.insert(
            root_pane,
            TerminalRuntime::test_with_scrollback_bytes(
                40,
                8,
                1024,
                b"one\ntwo\nthree\nfour\nfive\nsix\nseven\neight\nnine\nten\n",
            ),
        );
        app.workspaces = vec![workspace];
        app.active = Some(0);

        let area = Rect::new(10, 3, 40, 8);
        let terminal_runtimes = TerminalRuntimeRegistry::new();
        let infos = compute_pane_infos(
            &app,
            &terminal_runtimes,
            area,
            false,
            crate::kitty_graphics::HostCellSize::default(),
        );
        let info = &infos[0];

        assert_eq!(info.rect, area);
        assert_eq!(info.scrollbar_rect, Some(Rect::new(49, 3, 1, 8)));
        assert_eq!(info.inner_rect, Rect::new(10, 3, 39, 8));
    }

    #[test]
    fn selection_highlight_uses_one_uniform_style() {
        let palette = Palette::catppuccin();
        let host_theme = crate::terminal_theme::TerminalTheme {
            foreground: None,
            background: Some(crate::terminal_theme::RgbColor {
                r: 12,
                g: 14,
                b: 16,
            }),
        };
        let expected_style = automatic_selection_style(&palette, host_theme);
        let selection = Some(Selection::range(PaneId::from_raw(1), 0, 0, 2, None));
        let backend = ratatui::backend::TestBackend::new(4, 1);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();

        terminal
            .draw(|frame| {
                let buf = frame.buffer_mut();
                buf[(0, 0)].set_style(
                    Style::default()
                        .fg(Color::Rgb(10, 220, 120))
                        .bg(Color::Black),
                );
                buf[(1, 0)].set_style(
                    Style::default()
                        .fg(Color::Rgb(220, 180, 40))
                        .bg(Color::DarkGray)
                        .add_modifier(Modifier::BOLD),
                );
                buf[(2, 0)].set_style(Style::default().fg(Color::Blue).bg(Color::Reset));
                render_selection_highlight(
                    &selection,
                    frame,
                    PaneId::from_raw(1),
                    Rect::new(0, 0, 4, 1),
                    None,
                    &palette,
                    host_theme,
                );
            })
            .unwrap();

        let buffer = terminal.backend().buffer();
        let first = buffer[(0, 0)].style();
        let second = buffer[(1, 0)].style();
        let third = buffer[(2, 0)].style();

        assert_eq!(first.fg, expected_style.fg);
        assert_eq!(second.fg, expected_style.fg);
        assert_eq!(third.fg, expected_style.fg);
        assert_eq!(first.bg, expected_style.bg);
        assert_eq!(second.bg, expected_style.bg);
        assert_eq!(third.bg, expected_style.bg);
        assert_eq!(first.add_modifier, expected_style.add_modifier);
        assert_eq!(second.add_modifier, expected_style.add_modifier);
        assert_eq!(third.add_modifier, expected_style.add_modifier);
        assert!(!second.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn automatic_selection_background_uses_host_background() {
        let bg = automatic_selection_bg(
            &Palette::terminal(),
            crate::terminal_theme::TerminalTheme {
                foreground: Some(crate::terminal_theme::RgbColor {
                    r: 230,
                    g: 230,
                    b: 230,
                }),
                background: Some(crate::terminal_theme::RgbColor {
                    r: 12,
                    g: 14,
                    b: 16,
                }),
            },
        );

        let Color::Rgb(r, g, b) = bg else {
            panic!("selection background should resolve to rgb");
        };
        assert!(relative_luminance((r, g, b)) > relative_luminance((12, 14, 16)));
    }
}
