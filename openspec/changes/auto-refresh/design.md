## Context

See proposal.md — Why. Panels re-read on their own only after FileCommand's jobs, on Ctrl+R / menu Re-read (`Command::RereadPanel` → `begin_listing`), on return from an external editor, and when a stale tab is activated (`refresh_activated_stale_tab` → `begin_listing_inner`). The design doc explicitly left filesystem watching out of v1. The reducer is pure (application-shell "The reducer performs no filesystem I/O"): every read happens on a worker thread and comes back as a `Command`; async replies carry request ids and stale ones are dropped (`listing_request`, `info_request`, `git_request`). The event loop is idle-aware: it wakes only for input, worker results, or a deadline from `anim::next_wake_ms` and the header clock's minute flip, and redraws only when something was applied (application-shell "Idle event loop does not redraw").

The F-key bar is a ladder of complete forms chosen by width (`views/keybar.rs`: 67/50/20 columns); clicks map slot numbers through `keybar_command` to the same commands as the keys.

## Goals / Non-Goals

**Goals:**
- A panel showing a directory that changed on disk re-reads within about one interval without user action, keeping the user's place.
- Zero cost when nothing changed: no state change, no redraw, no flicker.
- Never interfere with an interaction in progress — in particular never move the cursor under an open confirmation dialog.
- A visible, clickable `12Refresh` slot equivalent to Ctrl+R.
- Interval configurable; `0` disables.

**Non-Goals:**
- Real-time change notification (a watcher). Polling at a minute granularity is deliberate: it needs no platform API, no extra thread per directory, and no handling of event storms.
- Probing background tabs, Tree/Info/QuickView panels, or the viewer/editor.
- Changing Ctrl+R's navigation bookkeeping (it records a frecency visit today; F12 mirrors it exactly).
- A `--norefresh` launch flag; F11; Ctrl/Alt+F12.

## Decisions

**D1 — Detect by content digest, not directory mtime.** On NTFS a directory's own modified time changes when children are added, removed, or renamed, but not when a child's size or content changes; on network shares and FAT it is less reliable still. The probe therefore enumerates the directory with the same reader and classification the listing uses (`StdFsReader`, `classify_entry`) and hashes the entries sorted by name: `(name, kind, size, modified, hidden, link)` plus the count. The reducer keeps the digest of the listing it shows (`PanelState::content_digest`, computed once at `ListingComplete` from `entries` minus `..`, cleared by `begin_new_listing`), so comparing is a `u64` equality — no per-minute O(n) work on the UI thread. Both sides call one `listing::content_digest` so the two digests are comparable by construction; entries the listing skipped as unreadable are skipped identically by the probe. *Alternative considered:* a directory-mtime probe — rejected because it misses in-place modifications, which are exactly what a build or a log writer produces.

**D2 — Scheduling lives in the reducer clock.** `State::next_probe_ms` is set from `clock_ms + refresh_interval_ms` at startup and after every probe round. `Command::Tick` with `clock_ms >= next_probe_ms` emits `Effect::ProbeDirectory` for each eligible panel (D3), records `probe_request`, and advances `next_probe_ms`. `anim::next_wake_ms` returns `next_probe_ms` as a fourth deadline when `refresh_interval_ms > 0` and at least one panel is eligible, so the idle loop wakes once a minute for it and never otherwise. With `refresh_interval = 0` no deadline exists and nothing changes from today. A suspended session (external editor, Ctrl+O) resumes with a clock jump, so a probe falls due immediately on return — harmless, since the editor return already re-reads the active panel. *Alternative considered:* a TUI-side timer thread — rejected because it would bypass the reducer's single clock and make the behavior untestable in the reducer tests.

**D3 — Eligibility is checked when a probe is issued and again when its result arrives.** A probe is issued for a panel only when: `phase == UiPhase::Panels`; `overlay.is_none()`; `drag.is_none()`; `quick_search.is_none()`; the panel's `progress` is `Complete` and `last_error.is_none()`; `display_mode` is `Full` or `Brief`; `probe_request.is_none()`. A `DirectoryProbed` is applied only if its `request` matches the panel's `probe_request` and its `path` equals the panel's `cwd`, and every condition above still holds (the request is cleared either way). Re-checking at arrival is what makes the F8 case safe: if a delete confirmation opened between probe and reply, the reply is dropped rather than re-reading under the dialog and letting the cursor land on a different file. The `probe_request.is_none()` guard means a slow share never accumulates probes; the next round simply waits.

**D4 — A changed directory re-reads through a purpose-named in-place helper.** `begin_listing_inner`'s doc comment says a third non-navigation caller should prompt extracting a helper. `reread_in_place(state, side)` becomes that helper: `begin_listing_inner(side, cwd, capture_anchor = !streaming)` — exactly what `refresh_activated_stale_tab` does today, which now calls it too. The automatic refresh is therefore not a navigation: no `record_visit`, no `PersistHistory`; it does capture the re-read anchor (panel-navigation "Same-directory re-read preserves cursor and scroll position"), keeps the selection set (keyed by identity), keeps the sort mode, and re-issues the git-info query as every re-read does. F12 and the key-bar click, by contrast, dispatch `Command::RereadPanel(active)` and so behave byte-for-byte like Ctrl+R, including its frecency visit. *Alternative considered:* having the probe carry the fresh entries and apply them directly — rejected because it would duplicate the streaming listing's anchor-restore and request-guard logic for a second entry path.

**D5 — An unchanged probe is invisible.** The reducer applies a matching no-change `DirectoryProbed` by clearing `probe_request` and returning no effects. The TUI's worker-result path marks the frame dirty for every applied command today; it is amended so a `DirectoryProbed` marks it dirty only when the reducer returned at least one effect (i.e. a `StartListing` round began). This keeps application-shell's "Idle screen is not redrawn" true between probes and extends it across a no-change probe.

**D6 — Probe failure is silent.** `digest: None` (directory unreadable or gone) clears `probe_request` and nothing else. Flipping the panel into the inline read-error state on a timer would be startling and, for a transient share hiccup, wrong; the user's next navigation or Ctrl+R/F12 surfaces the real error through the existing path (filesystem-error-handling "Inline panel read-error state offering re-read or drive change").

**D7 — F12 as the eleventh slot; F11 skipped.** NC 5.5's bar had ten slots; F12 reads naturally as "the key past Quit" and is unassigned in every terminal we target. The forms become `1Help … 10Quit 12Refresh` (77 columns — fits the 80-column nominal width), `1Hlp … 10Qit 12Rfr` (56 — fits the 60-column floor), and `1 … 10 12` (23). `hit_slots` emits slot 12 and `keybar_command(12)` returns `RereadPanel(active)`. Drawing an empty `11` slot would waste five columns and invite the question of what F11 does; it is left out.

**D8 — `refresh_interval` is a top-level integer key.** Parsed next to `panel_split`; accepted values are non-negative integers (seconds). `0` disables. A non-integer or negative value produces the standard line-numbered warning and the default 60 applies, matching how `splash`/`animations` report uninterpretable values. No upper clamp: a user who wants a 10-minute interval can have it.

## Risks / Trade-offs

- [Enumeration cost on huge or slow directories] → A probe reads the whole directory once per interval per panel — the same I/O as a non-streaming listing — on a worker thread; the UI thread does a `u64` compare. While the worker runs the loop wakes on its 33 ms cadence as it does for any worker (existing behavior). One outstanding probe per panel (D3); the interval is configurable and can be turned off.
- [A re-read is briefly visible] → The changed panel shows `Reading… N` while it streams, as Ctrl+R does. Acceptable: it only happens when the directory really changed.
- [Digest collision] → A 64-bit hash of the entry tuples could in theory collide, hiding a change until the next real difference. Negligible for a convenience refresh.
- [Clock semantics] → The probe timer runs on the reducer's monotonic clock; a laptop sleep makes one probe fall due on wake. Harmless.
- [Terminal F12 interception] → Windows Terminal, conhost, and common Linux terminals pass F12 through; some IDE-hosted terminals may bind it. Ctrl+R remains, so nothing is lost.
- [Key-bar snapshots] → Every `matrix_keybar_{w}` and panel snapshot that includes the bar changes; they are regenerated as part of the work, which is the cost of any bar change.

## Open Questions

None blocking. A later change could surface a one-shot mini-status note ("Refreshed") after an automatic re-read; this proposal keeps it silent, as NC did.
