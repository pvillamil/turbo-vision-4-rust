# TO-DO — Code Review Action Items

From the 2026-07-02 review of `turbo-vision-4-rust` v1.3.1 vs the kloczek/tvision C++ reference. Work top to bottom.

## P0 — Critical (broken or unsafe)

### UTF-8 byte/char index mixing (panics on non-ASCII input)
- [x] `input_line.rs` — cursor/selection/paste now char-indexed, byte offsets computed at the string-op boundary
- [x] `editor.rs` — search rewritten in char space (also fixes wrap + whole-word `continue` bug)
- [x] `memo.rs` — `get_selection` uses char-based skip/take
- [x] `paramtext.rs`, `terminal_widget.rs` — truncate by chars
- [x] (bonus) clipboard: OS pasteboard access serialized + skipped under `cfg(test)` — fixes pre-existing flaky SIGSEGV in parallel tests
- [x] (bonus) global HistoryManager tests serialized via test lock — fixes test-order flake

### Unfinished features that look done
- [x] Radio buttons now mutually exclusive via `CM_RADIO_SELECTED` broadcast with group id in new `Event.info` field
- [x] Clusters (checkbox/radio) now respond to mouse clicks
- [x] ColorDialog reads selections back via shared `Rc<RefCell<u8>>` values on the selectors
- [x] History wired end-to-end: button click → `CM_SHOW_HISTORY` → popup opened by Dialog/Application; `CM_RECORD_HISTORY` broadcast on dialog OK records linked InputLine data; selection copied back via `CM_HISTORY_SELECTED`; HistoryWindow viewer coordinates fixed
- [x] SortedListBox incremental type-to-search implemented (typing extends prefix, Backspace shrinks, navigation resets); prefix compare also made UTF-8-safe
- [x] Menu item hot keys dispatched while the bar is closed via recursive `Menu::find_hotkey` (Borland findHotKey), gated on the global command set

### Editor correctness
- [x] Enter / line-join deletes now recorded as `InsertText`/`DeleteText` with `\n`; `DeleteText` replay handles embedded newlines; overwrite mode is a single Compound undo step; regression tests added
- [x] `replace_all` infinite-loops when replacement contains pattern — fixed by removing search wrap-around (Borland-faithful); regression test added

### SSH / network
- [x] SSH auth is now default-deny: `SshAuthPolicy` with `auth_password_fn`/`auth_publickey_fn` callbacks and explicit `allow_anonymous()` opt-in (example updated)
- [x] `poll_event` honors its timeout (sleep-poll, near-idle CPU); backend errors now stop the app via `poll_event_or_quit` instead of spinning on dead sessions
- [x] SSH resize broadcasts `CM_REDRAW`; `handle_redraw` queries `terminal.backend_size()` instead of crossterm
- [x] Input parser: pending-sequence buffer capped at 64 bytes with resync; partial X10 mouse waits for all 6 bytes; regression tests added

## P1 — High (visible divergence from C++)

- [ ] Enter fires static default button, not focused one — `cmGrabDefault`/`cmReleaseDefault` unported (`dialog.rs:277`, `button.rs`)
- [ ] Disabled commands still fire: status-line keyboard path (`status_line.rs:256`), MenuBox Enter/mouse (`menu_box.rs:326-402`)
- [ ] Mouse clicks outside any child delivered to focused child — `group.rs:645-669`; Frame close-button MouseDown doesn't clear event (`frame.rs:232`)
- [ ] No per-child `growMode` — resize stretches every child (`group.rs:435-458`)
- [ ] `SF_ACTIVE` never propagated — inactive windows draw as active (`frame.rs:52`, `window.rs:668`)
- [ ] Non-modal close skips `valid(cmClose)` by default (`window.rs:643`)
- [ ] Modal loop tracks view by index, not identity (`application.rs:311`); `valid(endState)` re-entry missing (`group.rs:284` TODO)
- [ ] Command IDs diverge from Borland (`CM_QUIT=24` vs `cmQuit=1` etc.) — fix or document and drop "100% parity" claim
- [ ] Picture validator mask chars wrong vs Borland; auto-fill never wired — `picture_validator.rs:75`, `input_line.rs:388`
- [ ] File dialog: typed path with `/` navigates instead of returning file (`file_dialog.rs:499-516`); wildcards are substring, not glob (`file_dialog.rs:673`)
- [ ] Menu dropdown hit-testing hardcodes width 20 — `menu_bar.rs:527/574/787`
- [ ] `CP_BLUE_DIALOG` maps into wrong palette region; app-palette bytes 15/23/31 zeroed; `CP_CLUSTER` 4th entry off — `palette.rs`

## P2 — Medium (feature gaps / semantic drift)

- [ ] Editor: no word movement/deletion, no Ins overwrite toggle, no persistent blocks; tabs→spaces; overwrite pushes two undo entries
- [ ] Memo consumes Tab instead of moving dialog focus — `memo.rs:715`
- [ ] FileEditor: no `.bak` backup; save normalizes CRLF→LF and drops trailing newline
- [ ] Indicator shows `col x row` vs Borland `row:col` — `indicator.rs:66`
- [ ] Missing: `putEvent` slot, Alt+1-9 window select, window numbers, `cmZoom`/`cmResize` dispatch, zoom icon, keyboard move/resize, status-line `update()` on idle, `StatusDef` switching
- [ ] Status line sees events last instead of first; app hard-codes Alt+X/F1/F12 — `application.rs:476-522`
- [ ] Focus chain ignores `SF_DISABLED`/`SF_VISIBLE`; broadcasts stop at first consumer; `Group::remove` of focused child leaves nothing focused
- [ ] Desktop cascade/tile geometry differs from Borland; skip `tileError` and `sfVisible` checks — `desktop.rs:283-384`
- [ ] Scroller `set_limit` semantics off by a page; no scrollbar auto-repeat; full thumb when nothing to scroll
- [ ] `CommandSet::enable_range(n, n)` silent no-op — `command_set.rs:206/244`; `init_command_set` disables only CM_CLOSE
- [ ] Quit during modal returns `CM_CANCEL` instead of `cmQuit` — `application.rs:345`
- [ ] Frame: title not centered; close fires on bare MouseUp without press tracking
- [ ] InputLine: no select-all-on-focus, no shift-selection or Ins mode; `max_length` off-by-one
- [ ] `set_esc_timeout` is a silent no-op — `terminal/mod.rs:318`
- [ ] History/clipboard global mutexes `.lock().unwrap()` — poison cascades

## Deliberate deviations — document in README instead of fixing

Vec<String> buffer, multi-step undo/redo, CUA keybindings, markdown help format, "commands < 1000 close dialog", double-ESC cancel (inconsistent with single-ESC menus), 65k disableable commands, per-list history caps, adaptive msgbox layout, degenerate `Rect::contains`, draw buffer always overwriting attributes.
