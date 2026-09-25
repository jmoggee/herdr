# Fork maintenance guide

This fork (`jmoggee/herdr`) carries seven behavioral changes across its
implementation commits on top of `herdrdev/herdr`, plus companion commits that
only maintain documentation. This file tells an agent what they are, why they
exist, and what to verify after rebasing onto a newer upstream.

The latest comparison is against upstream `8d95e9bd` on 2026-09-25. Upstream
still lacks a complete equivalent for every behavior below, so the fork cannot
yet be retired. Upstream remains on private protocol 22; the fork remains on 23
because its `CellData` wire layout still carries underline color.

Since the previous comparison, upstream replaced its custom pane-graphics API
with server-owned native Kitty image rendering and source retention, while
keeping the stable endpoint generation at 1 and the private protocol at 22. It
also avoids rebuilding identical ANSI SGR sequences, avoids repeated scrollback
page lookups, keeps copy mode active through projection updates, matches
Navigator search terms independently, strengthens Win32 and Kitty input
qualification, and documents the tmux auto-attach guard. The fork's ANSI style
cache now includes underline color, preserving upstream's encoding optimization
without treating color-only undercurl changes as identical. The graphics,
scrollback, copy-mode, Navigator, Windows-input, and documentation changes do not
implement any of the fork's seven behaviors. All seven equivalence assessments
below were rechecked against the final upstream tree rather than carried forward
as a historical allowlist.

The partial equivalents and integration points found in earlier comparisons
remain: automatic names use upstream's client-specific title target and bounded
process scan; tab chips use upstream's full-tab scroll limit and readable
inactive styling; rename overlays use upstream's `TextEditor`; and underline
colors preserve the frozen `endpoint.surface-delta.v1` layout by falling back to
a full protocol-23 frame when a delta would otherwise discard SGR 58.

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

## Current pristine comparison

The full, non-fail-fast suite was compared against a detached pristine worktree
at exact upstream `8d95e9bd` on 2026-09-25. The fork ran 3,826 tests: 3,764
passed, 62 failed, and ten were skipped. Pristine upstream ran 3,798 tests:
3,735 passed, 63 failed, and ten were skipped. Every fork failure also occurred
in pristine upstream, so there were no fork-only failures. Pristine alone failed
`federated_client_starts_without_local_and_survives_its_restart`; the fork's
corresponding test passed. The client-mode, cross-area, and multi-client wire
canaries all passed in the fork.

The shared failures are environmental on this machine: process/cwd discovery,
git worktree setup, clipboard access, PTY spawning, headless shell startup,
agent startup, machine-bridge fixtures, and live handoff. Grouped by test
surface, the identical failures were:

```
api_ping::*cwd*                                              (2)
cli::cases::agents::agent_start_*                            (3)
machine_api::*                                               (15)
machine_setup::*                                             (5)
remote_attach::ssh_check_message_is_visible_while_authentication_waits (1)
app::api::layouts::tests::*                                  (3)
app::api::tabs::tests::tab_create_follows_cached_*           (1)
app::api::tests::pane_died_respawns_shell_*                  (1)
app::api::workspaces::tests::workspace_create_*              (2)
app::api::worktrees::tests::*                                (8)
app::tests::{pane_exit_checkpoint_*,pane_split_request_*}     (4)
live_handoff::live_handoff_keeps_unmanaged_agent_name_*       (1)
detect::tests::foreground_job_detects_agent_behind_shell_*    (1)
integration::tests::{install_hermes_*,uninstall_hermes_*}     (10)
pane::terminal::migration_tests::*ed3_for_droid*              (1)
platform::linux::tests::failed_wl_copy_uses_x11_fallback      (1)
pty::backend::unix::tests::portable_pty_setup_*               (1)
server::headless::tests::*                                    (2)
```

This snapshot is diagnostic context, never an allowlist for an unattended push.
Upstream and the machine can both change. On every rebase, rerun the same suite
in the fork and a detached pristine worktree at the exact new `origin/master`,
then compare sorted test names. Any fork-only failure blocks the push.

Use a pristine worktree rather than a stash:
`git worktree add /tmp/herdr-pristine <upstream-sha> --detach`. Run the same
command there and diff the two sorted failure lists. That keeps the rebased tree
intact and lets both runs happen back to back. Always unregister it afterward
with `git worktree remove /tmp/herdr-pristine`, including when comparison fails.

Do not carry the current total forward without a new comparison. Two details
make the paired run readable:

- Bound the hangs. `cases::plugins::plugin_install_*` can wedge indefinitely
  under load. `--config-file` a profile with
  `slow-timeout = { period = "60s", terminate-after = 4 }` so a stuck test is
  killed instead of stalling the suite for an hour.
- `client_mode`, `cross_area`, and `multi_client` passing is the signal that the
  `CellData` wire change survived. They are the fork's canary; treat any failure
  there as ours until proven otherwise.

`just check` stops at its fail-fast nextest step on the two shared `api_ping` cwd
failures above. Run the remaining maintenance, architecture, integration-asset,
Windows lint, and docs recipes separately so that baseline failure does not hide
their results. If a later run fails elsewhere, compare that exact command in the
pristine worktree rather than assuming this snapshot still applies.

### Validation at `8d95e9bd`

- `cargo fmt --check`, Clippy with warnings denied, the six UI hot-path
  architecture tests, the generated API schema check from the full suite, and
  19 focused wire, surface-delta, tab-render, automatic-name, and DECRQSS
  regressions passed.
- `just bench-render-scale` passed. At 15 panes the combined render pipeline was
  1.00× the one-pane median for background workspaces and 1.13× for active panes;
  client-shell composition was 1.04× and 0.97× respectively. The benchmark also
  exercised upstream's surface reuse and delta paths with 1 and 15 panes.
- `just check` stopped on the same two `api_ping` cwd failures in both trees. The
  complete non-fail-fast comparison above establishes that the remaining suite
  has no fork-only failure.
- `just maintenance-test` ran 149 tests and had the same single
  missing-`openssl` host failure in both trees. `just integration-assets-test`
  and `just docs-contract-test` could not start in either tree because `bun` is
  absent; `just windows-lint` could not start in either tree because this machine
  has no Windows SDK/Zig libc configuration. These are toolchain baselines, not
  fork exceptions, and must be rechecked from scratch on the next sync.
- Live checks could not reach an API-ready disposable session on this host.
  Both the checkout and pristine `herdr 0.9.1` clients timed out waiting for the
  named-session client socket when launched in dedicated PTYs. The fork server
  log showed its valid NixOS Bash child exiting immediately with `SIGHUP`; the
  paired suites also share the `portable_pty_setup_leaves_one_parent_pty_fd`
  failure and the broader pane-spawn baseline above. No runtime code was changed
  to accommodate the host. The stopped fork and pristine named sessions,
  isolated config/state, and reproduction directory were deleted.

## Implementation changes

### 1. `fix: carry pane underline colors to the host terminal` (refs #1252, #1169)

SGR 58 underline colors were dropped between the pane and the client, so
colored undercurls rendered in the text foreground color.

- `src/protocol/wire.rs` — `CellData` gained `underline_color: u32`, packed with
  the existing `color_to_u32`. Read in `from_ratatui_cell`, restored in
  `to_ratatui_buffer`.
- `src/protocol/render_ansi.rs` — `build_sgr` emits `58:2::r:g:b` / `58:5:n`.
  No `59` is emitted because every emitted cell-style SGR opens with a full
  reset. `cells_visually_equal` compares the field so a color-only change
  repaints. Upstream's packed-style cache includes `underline_color` in its key,
  so it retains the redundant-encoding optimization without hiding color-only
  changes.
- `src/pane/terminal.rs` — `cell_data_from_style` carries it too. **There are two
  independent paths**: `from_ratatui_cell` (full-frame) and `cell_data_from_style`
  (dirty-patch). Both need the field; fixing one silently leaves the other broken.
- `src/protocol/surface_delta.rs` and `surface_delta/decode.rs` — upstream's
  named `endpoint.surface-delta.v1` codec is frozen with the original six-field
  cell layout. A zero-allocation `CellV1` serializer keeps those bytes unchanged,
  and the decoder restores `underline_color` as zero. A changed row or popup
  replacement containing a nonzero underline color declines the delta so the
  server sends the full protocol-23 frame instead of silently dropping SGR 58.

Regression tests: `cell_data_carries_underline_color_from_ratatui_cell`,
`frame_data_restores_underline_color_into_ratatui_buffer`,
`render_preserves_underline_color`, `build_sgr_emits_rgb_underline_color`, and
`cells_visually_equal_detects_underline_color_change` cover the full-frame,
dirty-patch, ANSI, and repaint paths.
`surface_delta_uses_full_frame_for_underline_color_changes` pins the safe delta
fallback. The `PaneSurface` bincode digest tests pin the changed positional
layout, while the upstream surface-delta fixture digest remains unchanged.

Upstream equivalent: partial only. Upstream `src/ghostty/mod.rs` exposes
libghostty's underline color and `src/pane/terminal.rs` restores it into a pane
style. Its surface reuse and delta codecs avoid many complete cell payloads, and
its ANSI renderer now caches identical packed styles, but upstream `CellData`
still has no underline-color field and its ANSI client renderer cannot emit SGR
58. The color is still lost at the pane/client boundary without this fork
change.

**Current protocol state.** `PROTOCOL_VERSION` is **23** in the fork. Upstream
source, stable 0.9.1, and the current preview all publish protocol 22. Because
the fork's wider `CellData` is incompatible with that published layout, the fork
owns the bump to 23.

Recompute this after every rebase. Compare the rebased fork's wire layout and
version with upstream source and the `protocol` fields in
`distribution/latest.json` and `distribution/preview.json`. If the fork still
differs from upstream at protocol
`V` and either channel has published `V`, bump the fork to `V+1`. A bump touches
four things besides the constant, and missing any one fails a large and
misleading block of tests:

- `src/protocol/wire.rs::PROTOCOL_VERSION` — the constant itself.
- `tests/support/mod.rs::CURRENT_PROTOCOL` — shared by `client_mode`,
  `cross_area`, `multi_client`, `detach_reattach`, `server_headless`, and
  `tests/cli/harness.rs`. Stale here fails about 30 tests at once, all with
  `server should report current protocol version`.
- `tests/api_ping.rs` and `tests/cli/sessions.rs` — hardcoded on purpose so that
  a bump has to be acknowledged rather than absorbed.
- `docs/next/api/herdr-api.schema.json` — generated; do not hand-edit. Regenerate
  with `HERDR_UPDATE_API_SCHEMA=1 cargo nextest run generated_protocol_schema_artifact_is_current`.

Leave upstream's `// Freeze the protocol 20 input envelope` comment and its
frozen bytes in `wire.rs` alone. That test pins the *input* envelope, which this
fork does not touch, so the bytes stay correct and rewriting them only invites a
conflict.

Both release metadata files belong to upstream and must never be edited here.
If upstream adds equivalent underline-color transport, check the full-frame,
dirty-patch, ANSI-render, visual-equality, and bincode digest paths before
dropping this fork change.

### 2. `fix: answer decrqss sgr queries in panes` (refs #1178)

Neovim decides whether it may emit undercurl by writing `CSI 4:3 m`, asking for
the SGR state with DECRQSS, and checking whether the reply echoes the curly
style. Older upstream Herdr stayed silent, so Neovim fell back to a plain
underline. Upstream now answers and preserves `4:3`, but its response omits the
active SGR 58 underline color. This fork keeps the complete style query.

- `src/ghostty/mod.rs` — `Terminal::cursor_style()` wraps upstream's
  `GHOSTTY_TERMINAL_DATA_CURSOR_STYLE`; libghostty remains the single owner of
  SGR state.
- `src/pane/decrqss.rs` — a small tracker records only DECRQSS SGR query end
  offsets, including split and eight-bit control forms. It does not parse SGR
  or hold a shadow pen.
- `src/pane/terminal.rs` — the ordered response stream writes through each query,
  drains libghostty's native reply, reads the live cursor style at that boundary,
  and inserts `58:5:n` or `58:2::r:g:b` when libghostty has not already supplied
  one. It observes upstream's original, unfiltered PTY bytes after the Droid ED3
  compatibility filter was removed.

Regression tests: the `pane::decrqss` tests pin query boundaries, split writes,
eight-bit controls, and false-positive avoidance.
`process_pty_bytes_answers_neovim_extended_underline_probe` and
`process_pty_bytes_orders_decrqss_reply_before_following_xtgettcap_reply` pin
one augmented reply and its order relative to native XTGETTCAP.

Upstream equivalent: partial. The libghostty upgrade supplies the DECRQSS reply,
exact extended underline style, live cursor-style accessor, and ordinary
XTGETTCAP responses. This rebase deleted the fork's full SGR parser, shadow pen,
and duplicate response generation. Upstream still omits underline color from
`Terminal.printAttributes`, so the query-boundary augmentation remains.

### 3. `feat(ui): show tab numbers as chips in the desktop tab row`

A named tab hid its position, which is also its `prefix+<n>` switch key.

- `src/config/model.rs` — `ui.tab_numbers` = `auto` (default) | `always` | `never`.
- `src/client/shell/tabs.rs` — `tab_segments()` composes `[Number | Name | Zoom]`, and both
  `tab_width()` and the render loop consume it, so a tab can never be sized from
  one label and drawn from another.
- `src/config/tab_theme.rs` (new) — `[theme.tabs]`, kept **out** of
  `CustomThemeColors` on purpose: that struct mirrors the semantic palette every
  widget draws from, and tab chrome colors have no meaning elsewhere.

`auto` adds a chip when the tab has a name, `always` also turns an otherwise
unnamed tab into a compact chip, and `never` leaves the number in the body label.
Chips are left-anchored; unchipped labels stay centered. The eight foreground
and background fields under `[theme.tabs]` independently cover active/inactive
number chips and names, with unset values following the semantic palette.

Two invariants worth protecting:

- **Width uses `display_width`, never `.chars().count()`.** An earlier
  implementation of this feature swapped it and broke CJK and emoji tab names.
- **The chip shows the tab's position, never `Tab::number`.** The stored number
  backs the public `w1:t<n>` id and deliberately does not renumber; using it
  shows stale numbers after a tab is closed.

Regression tests: `tab_numbers_default_to_auto_and_parse_every_policy` pins the
setting. `number_chip_uses_visual_position_and_unicode_display_width` uses a
stable public number of 99 at visual position 3 and a CJK-plus-emoji label to
pin both invariants. `themed_tab_bar_paints_number_body_and_surrounding_bar`
checks the rendered chip separately from the body. Upstream's
`trailing_scroll_limit_accounts_for_full_widths_and_separators` remains in the
merged renderer and pins its full-last-tab scroll behavior with segment widths.

Upstream equivalent: partial only. Upstream now reveals the full final tab in an
overflowing strip and provides the `max_tab_scroll` algorithm this fork uses.
It still centers one combined label and has no `ui.tab_numbers` policy,
number/name segmentation, or chip styling.

### 4. `feat(ui): let the tab row background be themed separately`

`[theme.tabs] bar_bg` detaches the row's background from `panel_bg`, which also
paints menus, popups, and panel shells. Three sites paint the row (fill, status
separator, status segments) use the same resolved `bar_bg` value.

Implementation: `TabThemeConfig::resolve()` parses `bar_bg`; the client-owned
`render_tab_bar()` fills the row with it and passes it to the right-side status
renderer, whose separators and segment backgrounds use the same color.

Regression tests: `resolves_every_element_independently` and
`a_tmux_window_status_style_can_be_expressed_in_full` pin parsing;
`themed_tab_bar_paints_number_body_and_surrounding_bar` inspects the composed
frame and proves surrounding cells use `bar_bg`.

Upstream equivalent: none. Upstream still paints the desktop tab row and status
segments with `palette.panel_bg`, which also controls other panel surfaces.

### 5. `feat(ui): name tabs from focused terminal titles`

Tabs are automatic until explicitly named. `Workspace::tab_display_name()` is
the authority and resolves, in order:

1. A non-empty `Tab::custom_name`.
2. The tab's focused pane's stripped OSC terminal title.
3. The tab's current one-based position, never stable `Tab::number`.

Every projection must use that resolver: desktop and mobile tabs, Navigator,
sidebar pane details, notifications, API `TabInfo`, plugin contexts, rename
prefill, and the outer `{tab}` window-title token. A split tab follows whichever
pane is focused, including when the tab is not the workspace's active tab.

`ui.prompt_new_tab_name` now defaults to `false`. Saving the rename dialog
freezes its prefilled automatic label as a custom name. Saving exactly `""`,
including through the tab API, clears `custom_name` and restores automatic
naming. `src/workspace/tab.rs` and `src/persist/restore.rs` normalize empty
persisted names to `None`; only custom names persist.

PTY parsing records dirty title sources and `src/app/terminal_titles.rs` updates
pure `TerminalState`. Spinner-frame changes that leave the stripped title
unchanged must not publish stripped-title metadata changes or redraw automatic
labels. A sidebar row configured with raw `terminal_title` is the deliberate
exception and follows raw spinner frames; `terminal_title_stripped` stays quiet.
Changes in hidden workspaces still update state and events without generic
rendering. In the active workspace, the desktop tab row treats every automatic
tab as visible whenever the row exists, including tabs scrolled offscreen;
Navigator, mobile navigation, relevant sidebar tokens, and an outer title using
`{tab}` have their own visibility paths.

Dynamic labels change geometry. Client-shell projection resolves each label
before rendering, and `render_tab_bar()` computes every segment list and width
once before its scroll-search loops. Sizing, rendering, and mouse hit areas must
consume those same values. Do not put terminal-state locking, title formatting,
or allocation back inside pane-scaled render loops.

Regression tests: `automatic_tab_name_tracks_focused_pane_until_explicitly_named`
pins focus, custom-name precedence, clearing, and both sources.
`saving_rename_makes_the_current_automatic_name_static` and
`saving_empty_tab_rename_restores_automatic_name` pin client rename semantics.
Window-title, API, plugin-context, notification, mobile, Navigator, and sidebar
tests exercise the shared resolver through their existing projections.

Upstream equivalent: partial only. Upstream now scopes outer window titles to
each attached client's workspace and tab, and its rename overlays use a shared
cursor-based `TextEditor`. Its Navigator now matches search terms independently,
but still searches the label projected by `Workspace::tab_display_name()` rather
than supplying a new label source. This fork uses those upstream structures.
Upstream's resolver still returns a custom label or visual position only; it does
not use the focused pane's OSC title, restore automation after an empty rename,
or project dynamic names to the other surfaces.

### 6. `feat(ui): allow command-based automatic tab names`

`ui.automatic_tab_name_source` selects `terminal_title` (default) or `command`.
Custom names override either source. Command mode initially falls back to the
basename of the pane's launch command, then to the tab position, until process
detection supplies a foreground command.

Foreground-command selection follows the foreground process group. Linux and
macOS choose its leader, or the first job member when the leader is absent. On
Windows, existing agent-aware selection remains authoritative; when it still
selects the pane shell, its sole direct child is used, while zero or multiple
children deliberately fall back to the shell. Unsupported platforms fall back
to launch command or position.

Tracking is opt-in. `TerminalRuntimeRegistry::set_track_foreground_commands()`
must reach existing, restored, and newly inserted runtimes. Stable process
groups are reprobed every five seconds only in command mode so a same-PGID
`exec` eventually changes the name. Initial construction, server handoff, and
config reload all set the flag; switching source clears stale cached commands.

`ForegroundCommandChanged` updates cached `TerminalState`, increments its
revision, and publishes existing `PaneUpdated` API projection rather than a new
wire field. It redraws the focused automatic label when the active workspace's
tab row or another label projection is present, or when the outer title uses
`{tab}`. Custom-named tabs and tabs in hidden workspaces remain render-quiet.
Preserve this performance boundary: title mode must not gain periodic
process-tree probes, and lifecycle-authority shortcuts must not suppress the
five-second command refresh while command mode is enabled.

Regression tests: `automatic_tab_name_source_defaults_to_title_and_parses_command`,
`foreground_command_updates_command_named_tabs_only_when_visible`,
`foreground_command_updates_command_named_outer_title_without_tab_bar`, and
`command_tracking_rechecks_stable_process_groups_periodically` pin the config,
invalidation boundary, outer title, and refresh cadence. Windows-only selection
tests pin sole-child versus ambiguous-child fallback.
`promoted_client_window_title_uses_its_own_view` now also pins command-based
title invalidation for a client viewing a non-global tab, without requesting a
generic full render.

Upstream equivalent: partial only. Upstream now bounds Linux foreground
process-tree scans and scopes window titles to each client's view; the fork uses
those APIs instead of retaining its former scan or global-title assumptions.
Upstream still does not cache a foreground command in `TerminalState`, opt
runtimes into periodic command probes, expose this setting, or use commands as
tab labels.

### 7. `fix(ui): apply tab body style to full tab rect`

The client-owned tab renderer must paint the resolved body style across the
entire tab rectangle before drawing the number, name, and zoom segments. Without
that fill, unused cells retain the tab-row background and active/custom tab body
colors appear as narrow islands behind text instead of covering the whole tab.

- `src/client/shell/tabs.rs` — `render_tab_bar()` calls
  `buffer.set_style(rect, body)` before rendering its segments.

Keep this ordering when upstream changes tab segment rendering: the fill uses
the body style, then the chip and text segments deliberately override only the
cells they occupy.

Regression test: `themed_tab_bar_paints_number_body_and_surrounding_bar`
inspects the final unused cell in a named tab rectangle and confirms it has the
active body background, while the number and surrounding row keep their own
backgrounds.

Upstream equivalent: partial only. Upstream now leaves inactive automatic tab
labels undimmed so host-terminal faint styling cannot stack and make them
unreadable; the fork retains that behavior for both the body and number chip.
Upstream still fills only the row, then draws centered label text without first
applying a separately themed tab body style to the whole hit rectangle.

## Documentation-only companion commits

- `docs: describe the fork's changes and how to rebase them`, and later commits
  that only refresh `UPDATE.md`, maintain this guide without runtime behavior.
- `docs: list the tab row settings in the default config` changes generated
  default-config comments in `src/main.rs`, not parser or runtime behavior.

Do not classify the fork from old commit subjects alone. Rebase conflict
resolution has moved small pieces between commits over time. The final diff
against `origin/master` is authoritative. In this run, protocol 23 is runtime
behavior owned by the underline-color change, while old protocol-bump commits
contain only guide, generated-reference, or import cleanup after replay.

Normal fork syncs do not add entries to `docs/next/CHANGELOG.md`. Upstream owns
release curation, and carrying fork-only entries there creates conflicts without
preserving behavior.

## Updating onto a newer upstream

```bash
git fetch origin master
git fetch fork master
git rev-parse fork/master > "$(git rev-parse --git-path herdr-fork-before)"

if git merge-base --is-ancestor fork/master master; then
  : # local master already contains every fork commit
elif git merge-base --is-ancestor master fork/master; then
  git merge --ff-only fork/master
else
  echo "local master and fork/master diverged; stop for human reconciliation" >&2
  exit 1
fi

git rebase origin/master
```

The file under Git's private directory keeps the observed fork SHA across agent
shell calls for the landing command. Fetching the fork is not permission to
overwrite commits found there: fast-forward when the fork is simply ahead, and
stop when histories diverge.

An interrupted earlier sync can leave local `master` rebased while `fork/master`
still points at the old series. Resume that prepared rebase only when the saved
`herdr-fork-before` value exactly equals the freshly fetched fork tip, reflog
identifies the prior upstream base, and `git range-diff` accounts for every fork
commit with no independent local work. Otherwise use the divergence stop above.

Conflict hotspots, in rough order of likelihood:

| File | Why it conflicts |
|---|---|
| `src/client/shell/tabs.rs` | Segment composition, dynamic widths, scrolling, mouse geometry, and full-rect body styling |
| `src/workspace.rs` | Canonical label precedence and source selection, with broad call-site fanout |
| `src/app/terminal_titles.rs` | Visibility-aware invalidation for both automatic sources |
| `src/pane.rs` | Detector hot path and the five-second command refresh |
| `src/pane/terminal.rs` | Both terminal fixes sit beside other query trackers |
| `src/protocol/wire.rs` | `CellData` is a hot struct upstream |
| `src/protocol/surface_delta.rs` and `src/protocol/surface_delta/decode.rs` | Frozen generation-1 delta cells must keep the upstream six-field layout; underline changes fall back to full frames |
| `src/protocol/render_ansi.rs` | `build_sgr` signature gained a parameter |
| `src/app/window_title.rs` | `{tab}` indirectly depends on title or command state |
| `src/client/shell/overlay_input.rs`, `src/client/shell/context_menu.rs`, and `src/app/api/tabs.rs` | Rename freezes or clears automatic mode |
| `src/app/mod.rs` and `src/terminal/runtime_registry.rs` | Startup, handoff, reload, and runtime tracking toggle |
| `src/platform/windows.rs` | Agent selection and sole-child command selection share one process snapshot |
| `src/workspace/tab.rs` and `src/persist/restore.rs` | Empty custom-name normalization |
| `src/client/shell/mobile.rs`, `src/client/shell/agent_sidebar.rs`, and `src/ui/sidebar.rs` | Every label projection must use the configured source |
| `docs/next/website/src/data/config-reference.json` | Adjacent key insertions |

### After rebasing, check in this order

1. **Recompute the inventory.** Run
   `git log --cherry-pick --right-only --no-merges --oneline origin/master...master`,
   classify implementation versus documentation-only commits, and update this
   guide when the count, ownership, or behavior changed during the rebase.
2. **Build correctly** (see the top of this file), then
   `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings`.
3. **Run the wire and tab-render characterizations early.** The `PaneSurface`
   bincode digests in `src/protocol/wire.rs` move when `CellData` changes.
   `number_chip_uses_visual_position_and_unicode_display_width` and
   `themed_tab_bar_paints_number_body_and_surrounding_bar` pin the current
   client-shell tab geometry and styles. Treat a digest or frame change as a
   review point, not a value to update blindly.
4. **Run the suite** in both trees and compare their sorted failure lists. Remove
   the pristine worktree afterward. Any fork-only failure blocks the push; the
   historical snapshot above cannot authorize it.
5. **Check `PROTOCOL_VERSION`** as described in implementation change 1.
6. **Run the automatic-name characterizations.** At minimum:

```bash
just test-one automatic_tab_name_tracks_focused_pane_until_explicitly_named
just test-one saving_rename_makes_the_current_automatic_name_static
just test-one saving_empty_tab_rename_restores_automatic_name
just test-one automatic_tab_name_source_defaults_to_title_and_parses_command
just test-one foreground_command_updates_command_named_tabs_only_when_visible
just test-one foreground_command_updates_command_named_outer_title_without_tab_bar
just test-one command_tracking_rechecks_stable_process_groups_periodically
```

   On Windows, also run
   `just test-one windows_command_name_falls_back_to_shell_for_ambiguous_children`;
   the test is compile-gated out of Unix builds.
7. **Check projection and docs parity.** Desktop/mobile tabs, Navigator, sidebar
   tab tokens, API tab info, plugin and notification contexts, rename prefill,
   and outer `{tab}` must resolve the same label. Defaults and accepted values
   must agree across `src/config/model.rs`, `src/main.rs`, the configuration
   guide, and `config-reference.json`.
8. **Live-verify the terminal fixes.** Use the project-local
   `herdr-throwaway-repro` skill with the built binary and a unique named session.
   Clear inherited socket, session, workspace, tab, and pane variables when
   launching it, and explicitly address that session on every control command.
   Run the colored-undercurl `printf` inside the disposable pane through
   `pane run`, not in the maintenance shell. Drive a real Neovim DECRQSS probe in
   that pane and inspect it with `pane read --format ansi`. Expect `4:3` for the
   curly style and `58;2;…` for the color; plain `4` with no `58` is a regression.

9. **Live-verify automatic names.** With the default source, emit OSC 0/2 titles
   from two split panes and confirm the label follows focus. Rename it and confirm
   title changes no longer affect it; save an empty rename and confirm automation
   resumes. Switch to `automatic_tab_name_source = "command"`, reload config,
   and verify shell → command → shell transitions, allowing five seconds for a
   stable-process-group replacement. Switch back and confirm stale command state
   does not leak into title mode.
10. **Protect the hot paths.** Hidden title/command changes must not cause generic
    renders, title mode must not periodically inspect process trees, and tab
    widths must be computed once per view computation. Run `just bench-render-scale`
    whenever conflict resolution touches these paths, with at least 15 populated
    panes as required by `AGENTS.md`.
11. **Clean up** through `herdr-throwaway-repro`: stop and delete the exact named
    session after checking current CLI help, then close only the outer pane that
    reproduction created.

### Landing it

```bash
git push \
  --force-with-lease=refs/heads/master:"$(cat "$(git rev-parse --git-path herdr-fork-before)")" \
  fork master &&
  rm "$(git rev-parse --git-path herdr-fork-before)"
```

The explicit lease pins the remote SHA observed before rebasing. It catches a
fork that moves during the run; the pre-rebase ancestry check protects work that
was already on the fork when the run began.
