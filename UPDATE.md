# Fork maintenance guide

This fork (`jmoggee/herdr`) carries four commits on top of upstream
`herdrdev/herdr`. This file tells an agent what they are, why they exist, and
what to verify after rebasing onto a newer upstream.

`master` here is the integration branch. Upstream is `origin`; the fork is
`fork`. Never push to `origin` — the authenticated account is not a maintainer
of the canonical repository, and `CLAUDE.md`'s external contributor guardrail
forbids it.

## Build correctly first — this one wastes hours

The dev shell sets `LIBGHOSTTY_VT_OPTIMIZE = "Debug"` (`flake.nix`), while
`build.rs` defaults to `ReleaseFast`. `nix develop` applies the flake's `env`
block, so a variable passed as a *prefix* is silently overridden:

```bash
# WRONG — the flake overrides this, you get a debug VT library
LIBGHOSTTY_VT_OPTIMIZE=ReleaseFast nix develop --command cargo test

# RIGHT — set it inside the shell
nix develop --command bash -c 'LIBGHOSTTY_VT_OPTIMIZE=ReleaseFast cargo test'
```

With the debug VT library, `ghostty::tests::deep_scrollback_resize_preserves_unicode_and_hyperlinks`
takes **over an hour** and dozens of integration tests fail on timeouts. Built
correctly it takes **0.08 s** and those failures disappear. If tests look
catastrophically broken or slow, check this before investigating anything else.

## Expected test failures

Roughly 15 tests fail in this environment regardless of our changes. They are
pre-existing and environmental (workspace cwd discovery, git metadata, process
and pty spawning), not regressions:

```
app::api::layouts::tests::*            (3)
app::api::tabs / workspaces / worktrees tests
app::input::mouse::tests::keyboard_context_menu_split_keeps_new_runtime
app::tests::pane_split_request_*       (4)
detect::tests::foreground_job_detects_agent_behind_shell_wrapper
pty::backend::unix::tests::portable_pty_setup_leaves_one_parent_pty_fd
workspace::tests::new_workspace_retains_discovered_git_metadata
```

About 19 integration tests (`tests/*.rs`, run as `herdr::<binary>`) also fail
here on a pristine upstream tree, for the same environmental reasons (agent
hook session identity, process termination, live handoff, cwd following):

```
api_ping::{new_terminal_cwd_follow_ignores_nonleader_group_member_cwd,
           pane_info_reports_foreground_cwd_without_changing_pane_cwd}
cli::cases::agents::agent_start_*                        (3)
cli::cases::hooks::*                                     (6)
cli::cases::panes::closing_{pane,workspace}_terminates_processes_inside_it
cli::cases::workspace::forced_worktree_remove_terminates_processes_inside_checkout
live_handoff::*                                          (5)
```

The `client_mode`, `cross_area`, and `multi_client` integration tests are
**not** on that list. They decode frames with hand-written `CellWire` mirrors
of `CellData`, so a `CellData` field that is missing from those mirrors fails
them with `InvalidIntegerType { expected: U16, found: U32 }`. That is ours.

Confirm the set is unchanged rather than assuming: stash our work, run the same
filter on a pristine tree, and compare. Anything failing beyond this list is ours.

`just check` cannot run fully here — the maintenance-script tests need
`python3` and the integration-asset tests need `bun`, neither of which is in the
dev shell. Run the cargo half plus `cargo fmt --check` and
`cargo clippy --all-targets --locked -- -D warnings`.

## The four commits

### 1. `fix: carry pane underline colors to the host terminal` (refs #1252, #1169)

SGR 58 underline colors were dropped between the pane and the client, so
colored undercurls rendered in the text foreground color.

- `src/protocol/wire.rs` — `CellData` gained `underline_color: u32`, packed with
  the existing `color_to_u32`. Read in `from_ratatui_cell`, restored in
  `to_ratatui_buffer`.
- `src/protocol/render_ansi.rs` — `build_sgr` emits `58:2::r:g:b` / `58:5:n`.
  No `59` is emitted because every cell's SGR opens with a full reset.
  `cells_visually_equal` compares the field so a color-only change repaints.
- `src/pane/terminal.rs` — `cell_data_from_style` carries it too. **There are two
  independent paths**: `from_ratatui_cell` (full-frame) and `cell_data_from_style`
  (dirty-patch). Both need the field; fixing one silently leaves the other broken.
- `tests/client_mode.rs`, `tests/cross_area.rs`, `tests/multi_client.rs` — each
  has a hand-written `CellWire` mirror of `CellData` used to decode frames off
  the socket. `underline_color` sits between `modifier` and `skip` there too;
  bincode is positional, so the field order must match `CellData` exactly.

No `PROTOCOL_VERSION` bump: source was already at 20 and both stable and preview
publish 19, so the unreleased protocol absorbed the change. **Re-check this after
a rebase** — if upstream has since published protocol 20, adding a field needs a
bump. Compare `src/protocol/wire.rs::PROTOCOL_VERSION` against `website/latest.json`
and `website/preview.json`.

### 2. `fix: answer decrqss sgr queries in panes` (refs #1178)

Neovim decides whether it may emit undercurl by writing `CSI 4:3 m`, asking for
the SGR state with DECRQSS, and checking whether the reply echoes the curly
style. Herdr never answered, so Neovim fell back to a plain underline.

- `src/pane/decrqss.rs` (new) — `DecrqssQueryTracker`, shaped exactly like
  `xtgettcap.rs`: `observe(&[u8])` then `drain_pending()`, with responses
  interleaved at recorded byte offsets. Holds a shadow SGR pen, and honours
  DECSC/DECRC, RIS, DECSTR, and mode 1049 so the pen cannot drift.
- `src/ghostty/sgr.rs` (new) — safe wrapper over libghostty's own SGR parser
  (`ghostty_sgr_set_params` / `ghostty_sgr_next`). SGR semantics are deliberately
  **not** reimplemented. Note the separator convention: `separators[i]` is the
  byte *following* parameter `i`, so `4:3` arrives as params `[4, 3]` with a
  colon at index 0. A test pins this both ways.
- `src/pane/terminal.rs` — a field on the core, an observe/drain pair beside the
  other trackers, and a third `OrderedPtyResponseEvent` variant.

The reply mirrors libghostty's `printAttributes` with two deliberate
divergences: the underline *style* is reported exactly (`4:3`, not flattened to
`4`) and the underline color is included. Ghostty can omit both because it ships
terminfo with `Smulx`; over `TERM=xterm-256color` this reply is Neovim's only
channel.

Cost: about **5.2%** on top of the parse the pane already does, measured on an
SGR-dense stream. `pane::decrqss::tests::decrqss_observe_scale_profile` is an
ignored profile that re-measures it against libghostty's own parse.

### 3. `feat(ui): show tab numbers as chips in the desktop tab row`

A named tab hid its position, which is also its `prefix+<n>` switch key.

- `src/config/model.rs` — `ui.tab_numbers` = `auto` (default) | `always` | `never`.
- `src/ui/tabs.rs` — `tab_segments()` composes `[Number | Name | Zoom]`, and both
  `tab_width()` and the render loop consume it, so a tab can never be sized from
  one label and drawn from another.
- `src/config/tab_theme.rs` (new) — `[theme.tabs]`, kept **out** of
  `CustomThemeColors` on purpose: that struct mirrors the semantic palette every
  widget draws from, and tab chrome colors have no meaning elsewhere.

Two invariants worth protecting:

- **Width uses `display_width_u16`, never `.chars().count()`.** An earlier
  implementation of this feature swapped it and broke CJK and emoji tab names.
- **The chip shows the tab's position, never `Tab::number`.** The stored number
  backs the public `w1:t<n>` id and deliberately does not renumber; using it
  shows stale numbers after a tab is closed. Pinned by a test built on
  `AppState::test_with_adversarial_identity_state()`.

### 4. `feat(ui): let the tab row background be themed separately`

`[theme.tabs] bar_bg` detaches the row's background from `panel_bg`, which also
paints menus, popups, and panel shells. Three sites paint the row (fill, status
separator, status segments) and all route through `tab_bar_bg()`.

## Updating onto a newer upstream

```bash
git fetch origin master
git rebase origin/master        # on master
```

Conflict hotspots, in rough order of likelihood:

| File | Why it conflicts |
|---|---|
| `src/ui/tabs.rs` | Largest change; we restructured label composition |
| `src/pane/terminal.rs` | Touched by both features, near other trackers |
| `src/protocol/wire.rs` | `CellData` is a hot struct upstream |
| `src/protocol/render_ansi.rs` | `build_sgr` signature gained a parameter |
| `docs/next/CHANGELOG.md` | Everyone edits the top of this file |
| `docs/next/website/src/data/config-reference.json` | Adjacent key insertions |

### After rebasing, check in this order

1. **Build correctly** (see the top of this file), then
   `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings`.
2. **Frame digest characterization tests will fail if upstream changed rendering.**
   `ui::tab_surface::tests::desktop_full_app_semantic_frame_is_characterized` and
   its `mobile_` sibling SHA-256 the bincode-encoded `FrameData`. Any change to
   `CellData` or the tab row moves them. Read the new digest from the failure and
   update it — but first confirm the test's other assertions (geometry, cursor,
   hyperlinks) still pass, because those failing means something real broke.
3. **Run the suite** and compare failures against the expected list above.
4. **Check `PROTOCOL_VERSION`** as described in commit 1.
5. **Live-verify both fixes.** Unit tests do not prove the escape sequences reach
   a real terminal:

```bash
# start a disposable session with the built binary
HERDR_SESSION=verify ./target/release/herdr server &
./target/release/herdr workspace create --cwd /var/tmp --focus

# undercurl color, needs no editor (issue #1252)
printf '\033[38;2;0;255;0m\033[4:3m\033[58;2;255;0;0mGREEN TEXT, RED CURL\033[59m\033[24m\033[39m\n'

# neovim's DECRQSS probe (issue #1178) — read the pane back and expect 4:3 and 58
./target/release/herdr pane read <pane> --format ansi
```

   Expect `4:3` for the curly style and `58;2;…` for the color. A plain `4` with
   no `58` means the DECRQSS responder regressed.

6. **Clean up** the disposable session: `session stop` then `session delete`.

### Landing it

```bash
git push --force-with-lease fork master
```

`--force-with-lease` rather than `--force`: the fork's master is rewritten on
every rebase, and the lease catches a fork that moved underneath you.
