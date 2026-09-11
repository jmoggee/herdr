# Fork maintenance guide

This fork (`jmoggee/herdr`) carries seven behavioral changes across its
implementation commits on top of `herdrdev/herdr`, plus companion commits that
only maintain documentation. This file tells an agent what they are, why they
exist, and what to verify after rebasing onto a newer upstream.

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

## Historical test-failure snapshot

The lists below were measured when rebasing onto `6045fe6a`. They are diagnostic
context, never an allowlist for an unattended push: upstream and this machine
have both changed since then. On every rebase, run the same suite in the rebased
fork and a detached pristine worktree at the exact new `origin/master`, then
compare sorted failures. Any fork-only failure blocks the push.

At that snapshot, 15 unit tests failed in both trees from environmental
workspace cwd discovery, git metadata, clipboard access, process, and PTY
spawning:

```
app::api::layouts::tests::*                                  (3)
app::api::tabs::tests::tab_create_follows_cached_*           (1)
app::api::workspaces::tests::workspace_create_*              (2)
app::api::worktrees::tests::*                                (3)
app::tests::pane_split_request_*                             (3)
detect::tests::foreground_job_detects_agent_behind_shell_wrapper
platform::linux::tests::failed_wl_copy_uses_x11_fallback
pty::backend::unix::tests::portable_pty_setup_leaves_one_parent_pty_fd
```

Six integration tests (`tests/*.rs`, run as `herdr::<binary>`) also failed in
both trees, from agent startup, live handoff, and cwd following:

```
api_ping::{new_terminal_cwd_follow_ignores_nonleader_group_member_cwd,
           pane_info_reports_foreground_cwd_without_changing_pane_cwd}
cli::cases::agents::agent_start_*                        (3)
live_handoff::live_handoff_keeps_unmanaged_agent_name_bound_to_saved_session
```

The `client_mode`, `cross_area`, and `multi_client` integration tests are
**not** on that list. They decode frames with hand-written `CellWire` mirrors
of `CellData`, so a `CellData` field that is missing from those mirrors fails
them with `InvalidIntegerType { expected: U16, found: U32 }`. That is ours.

Use a pristine worktree rather than a stash:
`git worktree add /tmp/herdr-pristine <upstream-sha> --detach`. Run the same
command there and diff the two sorted failure lists. That keeps the rebased tree
intact and lets both runs happen back to back. Always unregister it afterward
with `git worktree remove /tmp/herdr-pristine`, including when comparison fails.

The historical total was **21**, identical in the fork and pristine upstream.
Do not carry that number forward without a new comparison. Two details made that
run readable:

- Bound the hangs. `cases::plugins::plugin_install_*` can wedge indefinitely
  under load. `--config-file` a profile with
  `slow-timeout = { period = "60s", terminate-after = 4 }` so a stuck test is
  killed instead of stalling the suite for an hour.
- `client_mode`, `cross_area`, and `multi_client` passing is the signal that the
  `CellData`/`CellWire` work survived. They are the fork's canary; treat any
  failure there as ours until proven otherwise.

At that snapshot, `just check` could not run fully because the dev shell lacked
`python3` and `bun`. Use the current `just` recipes first; only narrow validation
when a current failure proves the environment still lacks a required tool, and
record the exact omission in the sync report.

## Implementation changes

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

**Current protocol state.** `PROTOCOL_VERSION` is **22** in the fork. Upstream
source is protocol 21, stable publishes protocol 20, and preview publishes
protocol 21. Because the fork's wider `CellData` differs from the now-published
preview protocol 21 wire layout, the fork owns the bump to 22. The historical
`fix: bump the wire protocol for the pane underline color field` commit remains
documentation-only after the latest rebase; do not infer protocol ownership or
necessity from its subject.

Recompute this after every rebase. Compare the rebased fork's wire layout and
version with upstream source and the `protocol` fields in `website/latest.json`
and `website/preview.json`. If the fork still differs from upstream at protocol
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
dirty-patch, ANSI-render, visual-equality, and hand-written `CellWire` paths
before dropping this fork change.

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

Dynamic labels change geometry. `compute_tab_bar_view()` precomputes each width
once before its scroll-search loops, and sizing, rendering, and mouse hit areas
must consume the same resolved label. Do not put terminal-state locking, title
formatting, or allocation back inside pane-scaled render loops.

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

## Documentation-only companion commits

- `docs: describe the fork's changes and how to rebase them`, and later commits
  that only refresh `UPDATE.md`, maintain this guide without runtime behavior.
- `docs: list the tab row settings in the default config` changes generated
  default-config comments in `src/main.rs`, not parser or runtime behavior.
- `fix: bump the wire protocol for the pane underline color field` is currently
  documentation-only after upstream supplied protocol 21. Its historical subject
  records why a bump was once needed; recompute the protocol state after every
  rebase rather than preserving a source change that no longer exists.

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

Conflict hotspots, in rough order of likelihood:

| File | Why it conflicts |
|---|---|
| `src/client/shell/tabs.rs` | Segment composition, dynamic widths, scrolling, mouse geometry, and full-rect body styling |
| `src/workspace.rs` | Canonical label precedence and source selection, with broad call-site fanout |
| `src/app/terminal_titles.rs` | Visibility-aware invalidation for both automatic sources |
| `src/pane.rs` | Detector hot path and the five-second command refresh |
| `src/pane/terminal.rs` | Both terminal fixes sit beside other query trackers |
| `src/protocol/wire.rs` | `CellData` is a hot struct upstream |
| `src/protocol/render_ansi.rs` | `build_sgr` signature gained a parameter |
| `src/app/window_title.rs` | `{tab}` indirectly depends on title or command state |
| `src/client/shell/overlay_input.rs`, `src/client/shell/context_menu.rs`, and `src/app/api/tabs.rs` | Rename freezes or clears automatic mode |
| `src/app/mod.rs` and `src/terminal/runtime_registry.rs` | Startup, handoff, reload, and runtime tracking toggle |
| `src/platform/windows.rs` | Agent selection and sole-child command selection share one process snapshot |
| `src/workspace/tab.rs` and `src/persist/restore.rs` | Empty custom-name normalization |
| `src/client/shell/mobile.rs`, `src/client/shell/agent_sidebar.rs`, and `src/ui/sidebar.rs` | Every label projection must use the configured source |
| `docs/next/CHANGELOG.md` | Everyone edits the top of this file |
| `docs/next/website/src/data/config-reference.json` | Adjacent key insertions |

### After rebasing, check in this order

1. **Recompute the inventory.** Run
   `git log --cherry-pick --right-only --no-merges --oneline origin/master...master`,
   classify implementation versus documentation-only commits, and update this
   guide when the count, ownership, or behavior changed during the rebase.
2. **Build correctly** (see the top of this file), then
   `cargo fmt --check` and `cargo clippy --all-targets --locked -- -D warnings`.
3. **Frame digest characterization tests will fail if upstream changed rendering.**
   `ui::tab_surface::tests::desktop_full_app_semantic_frame_is_characterized` and
   its `mobile_` sibling SHA-256 the bincode-encoded `FrameData`. Any change to
   `CellData` or the tab row moves them. Read the new digest from the failure and
   update it — but first confirm the test's other assertions (geometry, cursor,
   hyperlinks) still pass, because those failing means something real broke.
4. **Run the suite** in both trees and compare their sorted failure lists. Remove
   the pristine worktree afterward. Any fork-only failure blocks the push; the
   historical snapshot above cannot authorize it.
5. **Check `PROTOCOL_VERSION`** as described in implementation change 1.
6. **Run the automatic-name characterizations.** At minimum:

```bash
just test-one automatic_tab_name_tracks_focused_pane_until_explicitly_named
just test-one saving_rename_makes_the_current_automatic_name_static
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
