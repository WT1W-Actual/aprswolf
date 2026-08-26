# APRSWolf — Review & Maintenance Plan

Living tracking document. Update the checkboxes and the "Current status" section
as work lands. Created from the 2026-07-21 full-codebase review
(efficiencies, logic errors, APRS standards compliance).

Related docs: [DEVELOPER-GUIDE.md](DEVELOPER-GUIDE.md) (prior Ponytail audit 2026-06-17),
[APRS12c.pdf](APRS12c.pdf) (APRS 1.2 protocol spec, the normative reference).

Legend: `[ ]` pending · `[~]` in progress · `[x]` done

---

## Current status

- Baseline captured: 2026-07-21
- Phase 0 DONE 2026-07-21: build restored.
- Phase 1 DONE 2026-07-21 (except 1.9, deferred — needs live Dire Wolf capture). Note: 1.2 was retracted on re-verification (no panic possible — see Phase 1 notes).
- Phase 2 DONE 2026-07-21: `cargo test` 69/69, `npm run test` 29/29, `tsc --noEmit` clean, `cargo check --release` clean.
- Phase 3 DONE 2026-07-21: shared-token auth (user decision) + merge-based settings saves. Found & fixed a bigger issue en route: the remote-control page was mostly broken (route/payload mismatch with the backend). 3.2 retracted per user (passcode is public). `cargo test` 72/72, `npm run test` 29/29, tsc clean.
- Phase 4 DONE 2026-07-21: parser probe matrices, position edge tests, APRS-IS line cap, `npm run verify` gate + CI workflow. Backend now 76/76.
- Phase 5 DONE 2026-07-21: all frontend findings fixed or consciously deferred. `npm run verify`: 35/35 frontend, 76/76 backend, tsc, clippy.
- 0.4 DONE 2026-07-21: clippy backlog cleared — `cargo clippy` runs silent. Remaining open items: 1.9 live Dire Wolf capture and the manual checklist below.
- Prior audit on record: Ponytail audit 2026-06-17 (see DEVELOPER-GUIDE.md) — the compile break was introduced after it.

## Manual verification checklist (for Jim — needs a running app)

- [ ] **End-to-end web remote auth** (from Phase 3, ~5 min):
  1. `npm run play`, Preferences > Remote Web Control: leave token EMPTY, enable remote.
  2. From the same Mac: open the remote URL — should work (loopback exempt).
  3. From phone/another machine on the LAN: open the URL — page loads but
     Refresh/actions should fail with "token not configured" (403).
  4. Set a token in Preferences, save. Retry from the other device: actions
     fail 401 until the token is entered in the Remote Token field; then
     Refresh, TNC connect, and Send Message work.
  5. Confirm the message actually transmits (watch the desktop Traffic Log).
- [ ] **AGW 'T'-frame live capture** (item 1.9): connect to real Dire Wolf,
  send a message, capture the monitor frame in the Traffic Log, confirm the
  `":Fm "` parsing in `process_agw_frame` matches, then add a unit test with
  the captured bytes.

---

## Phase 0 — Restore the build (gate) — DONE 2026-07-21

- [x] **0.1 Fix `aprsis.rs:334` compile error (E0061).**
  Converted the third-party-unwrap branch call to the `NormalizeFieldsInput`
  struct form with `..Default::default()` for the four `existing_*` fields.
- [x] **0.2 Remove unused import warning** — `Manager` in `runtime.rs:7`.
- [x] **0.3 Verify:** `cargo test` 64/64 pass, `npm run test` 29/29 pass,
  `npx tsc --noEmit` clean.
- [x] **0.4 Clippy backlog — CLEARED 2026-07-21, `cargo clippy` now runs
  silent.** Deleted dead `build_command_frame` (`agw.rs:85`), derived
  `Default` for `AgwBinaryDecoder`, `(-90.0..=90.0).contains()` in
  `validate_position`, `Some("")` pattern instead of the redundant guard in
  `split_reply_ack_id` (semantics preserved — empty reply-ack still maps to
  `None`), `io::Error::other()` x2, `div_ceil()` x2 (also dropped the no-op
  `.max(0)` on u64), and `#[allow(clippy::too_many_arguments)]` on
  `send_beacon_for_app` (params map 1:1 to UI fields; a wrapper struct is
  churn). Full `npm run verify` green after the sweep.

## Phase 1 — Logic errors & APRS standards compliance — DONE 2026-07-21
(except 1.9, deferred)

Normative reference: docs/APRS12c.pdf (APRS 1.2), AX.25 v2.2, KISS, AGWPE.

Verified after all changes: `cargo test` 69/69 (5 new), `npm run test` 29/29,
`tsc --noEmit` clean, clippy shows only the pre-existing item-0.4 backlog.

- [x] **1.1 Position minutes rollover.** `ax25.rs build_aprs_position_info`
  now rounds to centi-minutes with carry into degrees and clamps at 90/180;
  42.999999° -> `4300.00N` (was `4260.00N`). Covered by
  `build_aprs_position_info_never_emits_60_minutes` and
  `..._normal_case_unchanged` tests.
- [x] **1.2 RETRACTED — no panic.** Re-verified during implementation: both
  `parse_aprs_message` and the settings.rs helpers check that byte 9 is
  `:` (single-byte ASCII) before slicing at 9/10, so those offsets are
  always valid char boundaries. Multi-byte input can't panic. Locked in
  with regression test `parse_aprs_message_non_ascii_does_not_panic`.
- [x] **1.3 Third-party (`}`) unwrap on the RF/AGW RX path.**
  `tnc.rs process_agw_frame` now unwraps via `parse_tnc2_protocol_view`
  and runs message/ACK logic on the inner packet, mirroring the APRS-IS
  path. ACKs and reply-acks are attributed to the inner source.
- [x] **1.4 RF auto-ACK digi path.** Auto-ACK frames now use the configured
  digipeater path from settings (was always empty/direct).
- [x] **1.5 APRS-IS `# logresp` handling.** The RX loop surfaces
  `aprsis_verified` / `aprsis_unverified` status events ("unverified"
  checked first — it contains "verified"). Note: TX is not gated on
  verification; the status is surfaced for the UI to warn. Gating TX is a
  product decision — revisit if wanted.
- [x] **1.6 Outbound msgid validation.** `build_aprs_message_info` only
  appends `{id` when it matches spec form: 1-5 alnum, or reply-ack forms
  `MM}` / `MM}AA`. Invalid ids fall back to no-msgid instead of emitting
  a non-compliant packet. Tests cover valid/rejected/reply-ack forms.
- [x] **1.7 `mark_acked` matches the ACK sender.** New `from: &str`
  parameter; a queued message is only acked when the sender matches its
  destination (base-callsign compare). All 5 call sites updated
  (3 in tnc.rs, 2 in aprsis.rs). Unknown/empty sender -> no ack recorded
  (safe direction).
- [x] **1.8 AGW/RX port setting.** New `agw_port` field in AppSettings
  (default empty = keep the KISS-port-minus-1 convention). `connect_tnc`
  honors the override when set to a valid port. CAVEAT: there is no
  Preferences UI field for it yet, and the frontend's settings payloads
  don't include `agw_port` — a frontend settings save will reset it to ""
  (same class as finding 5.1). Add the UI field with Phase 5.
- [~] **1.9 AGW 'T'-frame monitor format — DEFERRED, needs live capture.**
  The `":Fm "` / `" To "` header parsing in `process_agw_frame` is an
  assumption about Dire Wolf's monitor text. Requires a live Dire Wolf
  session to confirm; capture one real 'T' frame, then add a unit test
  with the captured bytes. Do NOT guess the format.

## Phase 2 — Efficiency — DONE 2026-07-21

Verified: `cargo test` 69/69, `cargo check --release` clean, clippy unchanged
(11 pre-existing item-0.4 warnings), `npm run test` 29/29, `tsc --noEmit` clean.

- [x] **2.1 Received-messages in-memory store + debounced flush.**
  `settings.rs` now keeps the received list in memory (read-through cache,
  200-entry cap) and appends in O(1). A single long-lived worker (tokio
  Notify + 5s debounce) coalesces bursts into at most one disk write per
  window; notifies arriving during a flush leave a permit so no append is
  ever left unflushed. Trade-off documented in code: up to ~5s of received
  messages can be lost on an ungraceful quit.
- [x] **2.2 Settings cache.** `load_settings` is now a read-through in-memory
  cache invalidated only by `save_settings` (cache updated after successful
  disk write). The 2-3 settings reads per received packet are now memory
  hits. Note: only this app writes settings.json, so no external-edit
  staleness concern.
- [x] **2.3 `rx_raw` / `rx_buffered` gated on `cfg!(debug_assertions)`.**
  Dev runs (`npm run play`) keep the raw hex dumps; release builds no longer
  flood the webview bridge. Parsed/error status events are unchanged.
- [x] **2.4 Web server `/api/state` cost — solved by 2.1 + 2.2, no code
  change needed.** Both `load_received_messages` and `load_settings` are
  now memory reads; a polling remote client no longer causes disk I/O.
- [x] **2.5 Double `settings_updated` emit removed.** `webserver.rs`
  `save_settings_handler` no longer re-emits what `save_settings` already
  emits.

## Phase 3 — Security (web remote control) — DONE 2026-07-21

Decision (user, 2026-07-21): shared Bearer token. APRS-IS passcode is NOT
treated as a secret (publicly derivable from the callsign) — no masking.

Verified: `cargo test` 72/72 (3 new auth tests), `npm run test` 29/29,
`tsc --noEmit` clean, clippy unchanged. Remaining manual check: end-to-end
against a live server via `npm run play` (auth middleware needs a real
AppHandle, so it's covered by unit tests on `authorize` + compile only).

- [x] **3.0 NEW FINDING (fixed): remote page ↔ backend contract mismatch.**
  While adding auth, found the remote-control page was mostly broken:
  7 of 10 endpoints it calls didn't exist in the router
  (`/api/tnc/connect` vs `/api/connect-tnc` etc.), all action payloads had
  the wrong shapes (missing callsign/source/path fields), `/api/state`
  field names didn't match AppSettings (`callsign` vs `source`...), and
  `api()` crashed on 204 No Content responses. Fixed by aligning the
  backend to the page's cleaner REST-ish routes and deriving station
  fields (callsign, path, beacon lat/lon/symbol) from settings server-side;
  fixed the page's state field names and made `api()` 204-tolerant.
- [x] **3.1 Shared-token auth on all /api routes.** Axum middleware with
  `Authorization: Bearer <token>`, constant-time compare, ConnectInfo for
  client IP. Rule: token empty -> loopback only (remote callers get 403
  with a "configure the token" message); token set -> everyone (including
  localhost) needs it. Token managed in Preferences > Remote Web Control;
  remote page has a token field persisted to localStorage.
- [x] **3.2 RETRACTED (per user):** the APRS-IS passcode is a publicly
  derivable function of the callsign, not a credential. `/api/state`
  continues to return full settings; no masking.
- [x] **3.3 BONUS — settings saves are now merge-based.** `save_settings`
  overlays the provided keys onto current settings instead of replacing
  the whole object, so a client that omits a field can no longer reset it
  to defaults. This kills the entire settings-clobbering class backend-side
  (frontend findings 5.1/5.2, the `agw_port` caveat from 1.8, and the new
  `web_server_token`). The Phase 5 frontend cleanups remain worthwhile but
  are no longer correctness-critical.

## Phase 4 — Hardening & test coverage — DONE 2026-07-21

Verified: `npm run verify` passes (frontend 29/29, backend 76/76, tsc clean,
clippy 11 pre-existing warnings, exit 0).

- [x] **4.1 Parser probe matrix.** New tests in `ax25.rs`:
  `parse_aprs_message_probe_matrix` (6 reject shapes + 7 accept shapes incl.
  braces-in-text, empty body, over-long msgid, 500-char body),
  `parse_tnc2_protocol_view_probe_matrix`, `decode_mic_e_probe_matrix`
  (short info, wrong type byte, ambiguity flags).
- [x] **4.2 Position encoding edges.** `build_aprs_position_info_edge_cases`:
  equator/prime-meridian, tiny negatives (hemisphere correctness),
  near-pole/antimeridian carry clamping, exact ±90/±180. (One test
  expectation was itself wrong during development — lat +89.999999 is N,
  not S; fixed and now passing.)
- [x] **4.3 APRS-IS line read capped at 8192 bytes** via
  `(&mut line_reader).take(MAX).read_line()` in `aprsis.rs`. Note: tokio's
  `AsyncReadExt` has no `by_ref()` (that's the futures crate) — `&mut R`
  works because BufReader is Unpin. An over-long line is truncated and its
  remainder parses (and is discarded) as a separate line, so framing
  recovers.
- [x] **4.4 Verify gate.** `npm run verify` = frontend tests + backend
  tests + `tsc --noEmit` + `cargo clippy`. CI workflow at
  `.github/workflows/verify.yml` runs the same on every push/PR (this
  folder isn't a git repo yet — the workflow is ready for when it is).

## Phase 5 — Frontend findings (parallel audit) — DONE 2026-07-21

Frontend sub-audit baseline was green (29/29, tsc clean); two of its
"CRITICAL" event-contract flags were false positives (`tnc_delivery`,
`web_server_status` ARE emitted — verified `tnc.rs:444`, `webserver.rs:119`).
Verified after all changes: `npm run verify` — 35/35 frontend tests
(incl. 5 new helpers tests + 1 rewritten), backend 76/76, tsc, clippy.

- [x] **5.1 Settings clobbering via `settings_updated` payloads.** App now
  tracks `autoBeaconOnConnect` in state (applied from settings), and both
  `buildSettingsPayload` call sites pass it plus `msgFontSize`. (The Phase 3
  merge-save is the second layer of defense for any field the UI forgets.)
- [x] **5.2 Stale `receivedWindowOpen` localStorage init.** Startup now
  clears the state along with the key (crash recovery actually works).
  The old test enshrined the bug — replaced with two tests: stale-key
  recovery, and live BroadcastChannel-driven disable.
- [x] **5.3 APRS-IS port validated** like `connect()` (integer, 1-65535).
- [x] **5.4 `receivedItems` capped at 200** (matches backend
  `MAX_RECEIVED_MESSAGES`), bounding the O(n) snapshot publishes.
- [x] **5.5 Listener lifecycle.** All Tauri listener setups (App, LogWindow,
  ReceivedWindow) now track registered unlisteners and tear down any that
  resolve after the effect died (StrictMode/double-mount leak). The
  `showAckMessages` resubscribe churn is gone — LogWindow uses a ref;
  App no longer needs the flag in the handler at all (see 5.8).
- [x] **5.6 `maxLength={67}` on all compose/reply inputs** (App main, App
  reply dialog, ReceivedWindow reply).
- [x] **5.7 `isAckMessageBody` tightened to `/^ack[A-Za-z0-9]{1,5}$/i`;
  `matchesStationAddress` body fallback now uses non-alnum boundaries
  (no more "W1AW" matching "W1AWZ"); LogWindow loads `show_ack_messages`
  from settings on mount like the other windows. Unit tests added
  (`src/lib/helpers.test.ts`).
- [x] **5.8 ACKs no longer dropped at ingest** — they're always stored and
  only filtered at render, so toggling the filter can't lose them.
- [x] **5.9 Passcode auto-derive no longer clobbers manual entries** —
  only derives while the field is empty or still matches a derived value.
- [x] **5.10 Empty destination/message blocked in the submit handlers**
  (Enter path), in App send/sendReply and ReceivedWindow sendReply.
- [x] **5.11 Minor batch (done):** module-level BroadcastChannel for both
  buses (no create/close churn per publish); LogWindow scroll uses
  `behavior:"auto"`; startup no longer writes settings back immediately
  (skip-once ref); beacon lat/lon range check.
  **Deferred by design:** strict client-side callsign regex (backend
  validates — YAGNI), multipart-merge heuristic tuning and msgid dupe
  suppression (behavior changes that need product thought, not bug fixes).

---

## How to use this file

1. Work phases in order; Phase 0 gates everything.
2. When starting an item: mark `[~]`, note the date.
3. When done: mark `[x]`, note commit/date, and run the full verify trio
   (`cargo test`, `npm run test`, `tsc --noEmit`).
4. New findings go at the bottom of the relevant phase — don't delete
   completed items; they're the record.
