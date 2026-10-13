# APRSWolf Developer Guide

> If you're reading this after a 6-month gap: start here, not in the source code.
> This document is the handoff from the last person who deeply understood this codebase.
> Read it before you change anything.

## Table of Contents

1. [Quick Start](#quick-start)
2. [Project Architecture](#project-architecture)
3. [Codebase Map](#codebase-map)
4. [Key Design Decisions & Patterns](#key-design-decisions--patterns)
5. [Ponytail Audit - What Was Fixed & Why](#ponytail-audit--what-was-fixed--why)
6. [Common Pitfalls (Don't Repeat These)](#common-pitfalls-dont-repeat-these)
7. [Mac App Store Gotchas](#mac-app-store-gotchas)
8. [Cross-Platform Notes](#cross-platform-notes)
9. [Build & Release](#build--release)

---

## Quick Start

```bash
# Dependencies
npm install
source ~/.cargo/env   # if cargo is missing in a fresh shell

# Dev mode (Tauri + Vite, hot-reloads both frontend and Rust)
npm run play

# Just the frontend (faster iteration on UI)
npm run dev

# Full type check + compile
npm run build

# Tests
npm run test          # frontend (Vitest)
cd src-tauri && cargo test    # backend (Rust)
cd src-tauri && cargo check   # backend compile check only

# Release (macOS, Linux, Windows)
npm run release:macos
```

## Project Architecture

APRSWolf is a **Tauri v2 + React (TypeScript)** desktop app - a native app bundle with a Rust backend and a web-based frontend rendered by the system WebView (WKWebView on macOS, WebView2 on Windows, WebKitGTK on Linux).

```
┌─────────────────────────────────────────────────┐
│                  System WebView                  │
│  ┌───────────────────────────────────────────┐  │
│  │         React (TypeScript) Frontend       │  │
│  │  src/App.tsx          src/*Window.tsx     │  │
│  │  src/lib/types.ts     src/lib/helpers.ts  │  │
│  │  src/lib/useTheme.ts  src/logBus.ts       │  │
│  └──────────────────────┬────────────────────┘  │
│                         │ Tauri IPC (invoke)     │
│  ┌──────────────────────▼────────────────────┐  │
│  │           Rust Backend (Tauri)            │  │
│  │  src-tauri/src/lib.rs  (entry + commands) │  │
│  │  src-tauri/src/runtime.rs  (app state)    │  │
│  │  src-tauri/src/tnc.rs, agw.rs, kiss.rs    │  │
│  │  src-tauri/src/aprsis.rs, ax25.rs         │  │
│  │  src-tauri/src/settings.rs                │  │
│  │  src-tauri/src/message_store.rs (SQLite)  │  │
│  │  src-tauri/src/webserver.rs               │  │
│  └──────────────────────────────────────────┘  │
└─────────────────────────────────────────────────┘
```

### Communication Model

- **Frontend → Backend**: Tauri `invoke("command_name", { ...args })` calls. Each command is a `#[tauri::command]` function in `lib.rs` or `runtime.rs`. Payloads are JSON-serialized structs.
- **Backend → Frontend**: Tauri events via `app_handle.emit("event_name", payload)`. The frontend subscribes with `listen("event_name", callback)`.
- **Frontend ↔ Frontend**: Cross-window communication uses a lightweight event bus (`src/logBus.ts`, `src/receivedBus.ts`) - a simple typed subscriber pattern, not Redux/Zustand.

### The Two Sides Have Different Conventions

| Concern | Frontend (TypeScript) | Backend (Rust) |
|---------|----------------------|-----------------|
| Settings field names | `snake_case` (Tauri IPC serializes to Rust structs) | `snake_case` |
| Internal variable names | `camelCase` in helper params | `snake_case` |
| Source of truth for display | Frontend (formatting happens here) | Raw data, unformatted |
| Connection state | Display-only (read from events) | Authoritative (owns TNC/APRS-IS connections) |

## Codebase Map

### Frontend (`src/`)

| File | Purpose | ~LOC | Notes |
|------|---------|------|-------|
| `App.tsx` | Main application component - message composer, sent pane, connection controls, settings panel | 1,500 | The god file. Consider splitting if it grows much more. |
| `ReceivedWindow.tsx` | Popped-out received messages window (filter + reply) | - | Opened from App.tsx's button |
| `PreferencesWindow.tsx` | macOS-style preferences (settings, theme, sound, beacon, message history) | - | Save uses `buildSettingsPayload()` |
| `HelpWindow.tsx` | In-app help browser with sidebar navigation & search | - | Content in `helpContent.ts` |
| `LogWindow.tsx` | Traffic log panel | - | Uses `useTheme()` (one-shot) |
| `ClearDialog.tsx` | Hide/delete confirmation dialog with the `Delete from database` box | - | Shared by both windows |
| `HeaderClock.tsx` | UTC + local time in the header row | - | Redraws each minute |
| `MessageRowActions.tsx` | Per-row `×` clear and `↺` unhide controls | - | Hover and selection states |
| `MessageSelection.tsx` | Long-press multi-select UI for message lists | - | Checkbox per row |
| `UnreadDot.tsx` | Red unread count badge on the `To Me` filter | - | Driven by `useToMeUnread()` |
| `main.tsx` | React entry point, window creation | - | Thin wrapper |

### Frontend Libraries (`src/lib/`)

| File | Purpose | Notes |
|------|---------|-------|
| `types.ts` | Shared type definitions and constants | **Single source of truth** for `AppSettingsPayload`, `ThemeMode`, `ReceivedFilter`, `FILTER_OPTIONS`. Imported by multiple windows - never duplicate these types. |
| `helpers.ts` | Utility functions | `buildSettingsPayload()`, `deriveAprsIsPasscode()`, input normalisers, `nextId()`, message helpers |
| `useTheme.ts` | Shared theme hook | Two modes: **reactive** (pass `themeMode` prop) or **one-shot** (loads from backend). Includes `prefers-color-scheme` listener. |
| `messageHistory.ts` | Message history types and helpers | Row shapes, paging, and search glue shared by both windows |
| `appBadge.ts` | App icon unread badge | macOS/Linux dock badge via Tauri; Windows red-dot taskbar overlay |
| `useSelection.ts` | Long-press multi-select state | Backs `MessageSelection.tsx` |
| `useToMeUnread.ts` | Unread `To Me` count | Feeds `UnreadDot.tsx` and the app icon badge |
| `useWindowFocused.ts` | Window focus tracking | Clears the unread count when a window gains focus |

**Why these libraries exist:** The Ponytail audit found `AppSettingsPayload` defined identically in 3 files, and the same settings payload constructed inline in 3 different places. Extracting them eliminated drift and cut ~50 lines of duplicated code.

### Frontend Event Buses

| File | Purpose |
|------|---------|
| `logBus.ts` | Typed pub/sub for log/message events between components |
| `receivedBus.ts` | Typed pub/sub for received message events |

These are *window-local* buses, not cross-window. Cross-window state is managed through Tauri events (Rust backend broadcasts to all windows).

### Backend (`src-tauri/src/`)

| File | Purpose | ~LOC |
|------|---------|------|
| `main.rs` | Tauri app entry point | Small |
| `lib.rs` | Command handlers + `tauri::Builder` setup | Medium |
| `runtime.rs` | Application state (`RuntimeState`), shared runtime data | ~300 |
| `tnc.rs` | KISS TNC connection management | - |
| `agw.rs` | AGW TCP protocol handler | - |
| `kiss.rs` | KISS framing encode/decode | - |
| `aprsis.rs` | APRS-IS connection over TCP | - |
| `ax25.rs` | AX.25 frame parsing and construction | - |
| `settings.rs` | Settings load/save (JSON file in app data dir) | - |
| `message_store.rs` | Message history store (SQLite `messages.db`): UUIDs, retention, full-text search, hide/delete, legacy import | - |
| `webserver.rs` | Embedded web server for remote control interface | - |
| `main.rs` | `fn main()` - starts the Tauri app | 5 lines |

### Remote Control (`src-tauri/`)

The remote web control is **not a React app** - it's a standalone HTML file served by the embedded Rust web server:

| File | Purpose |
|------|---------|
| `src-tauri/remote-control.html` | Phone-friendly dashboard for remote monitoring | ~400 LOC |

This was extracted from a 1,600-line inline string inside `webserver.rs` - it used to be embedded as a Rust string literal. Extracting it made the HTML editable without recompiling Rust, and let us trim 75% of dead code.

## Key Design Decisions & Patterns

### 1. Settings Payload Construction

**Always use `buildSettingsPayload()`** from `src/lib/helpers.ts`. Never construct the payload inline.

```typescript
import { buildSettingsPayload } from "./lib/helpers";

const payload = buildSettingsPayload({
  source: "user",
  path: "/dev/ttyUSB0",
  host: "localhost",
  port: "8001",
  // ... all settings as camelCase
});

await invoke("save_settings", { settings: payload });
```

The builder function:
- Accepts clean **camelCase** params
- Returns **snake_case** payload (matching Rust struct fields)
- Applies defaults via `??` nullish coalescing
- Is the **single source of truth** for field name mapping

**Why:** Three different components (auto-save in App.tsx, manual save in App.tsx, PreferencesWindow) each built the same payload with different conventions. One had defaults, one didn't. They drifted. This centralizes it.

### 2. Shared Types

**All cross-component types live in `src/lib/types.ts`.** If you're defining a type that's imported from more than one file, it goes here.

Current exports:
- `ThemeMode` - `"system" | "light" | "dark"`
- `AppSettingsPayload` - the full backend settings struct
- `ReceivedFilter` - union type of all filter categories
- `FILTER_OPTIONS` - display-friendly array of filter choices

**Never** re-declare `AppSettingsPayload` or `ReceivedFilter` in a component file. Import them:
```typescript
import { type AppSettingsPayload, type ReceivedFilter, FILTER_OPTIONS } from "./lib/types";
```

**Why:** When `PreferencesWindow.tsx`, `ReceivedWindow.tsx`, and `App.tsx` each defined the same type verbatim, adding a field meant updating 3 files. They inevitably drifted. Now it's one file.

### 3. Theme Hook

Use `useTheme()` from `src/lib/useTheme.ts` instead of inline `useEffect` for theme management.

**Two modes:**

```typescript
// One-shot (loads from backend on mount) - for secondary windows
useTheme();

// Reactive (prop-driven, re-applies on change) - for preferences window
useTheme(themeMode);
```

The hook handles:
- Applying `data-theme` attribute to `<html>`
- Listening to `prefers-color-scheme` media query when mode is `"system"`
- Cleaning up listeners on unmount
- Loading saved theme from backend in one-shot mode

**Why:** Three secondary windows had the same `useEffect` for theme loading. The hook eliminated ~30 lines of duplicated effect code and made all windows consistent.

### 4. Remote Control HTML

The remote web control (`remote-control.html`) is a **standalone HTML file**, not a React component. It's served by the Rust web server as a static page.

**Rules for editing it:**
- It's served raw - no bundler, no TypeScript, no JSX. Write plain ES6+ JS and CSS.
- It talks to the Rust backend via `fetch()` calls to `http://localhost:{port}/api/...`
- Keep it thin - it's a monitoring dashboard, not a full APRS client.
- If it needs significant new functionality, consider a separate React build pipeline for it (but that's a lot of complexity for what should remain a simple dashboard).

### 5. Console Logging

Debug logging uses a `pushLog()` function that routes to both the UI event bus and the developer console:

```typescript
function pushLog(kind: LogItem["kind"], text: string) {
  publishUiLog(kind, text);           // always - drives the TUI log panel
  if (import.meta.env.DEV) {
    console.log(`[${kind.toUpperCase()}] ${text}`);  // dev only
  }
}
```

The `console.log` is gated behind `import.meta.env.DEV` - Vite tree-shakes it in production builds. **Do not add bare `console.log()` calls outside this guard.**

### 6. Monotonic IDs

Use `nextId()` from `helpers.ts` for any ID that needs to be unique within a session. It's a simple incrementing counter - not UUIDs, not timestamps. Works because Tauri windows share a process.

### 7. APRS-IS Passcode Derivation

Built into `helpers.ts` as `deriveAprsIsPasscode(callsign)` - this is the standard APRS-IS passcode algorithm. It's duplicated on the Rust side too; keep both in sync if the algorithm changes (unlikely - it's a published standard).

### 8. Message History Store (SQLite)

Message history lives in `src-tauri/src/message_store.rs`, backed by `messages.db` in the app data folder. The **backend is the only writer**: the TNC and APRS-IS receive paths store each packet, and the send path records each sent message and its delivery status. The frontend reads pages and runs searches through commands; it never writes the database directly.

Key rules:

- **Stable UUIDs.** Every message gets a UUID assigned once and kept for its lifetime. The frontend keys rows by that UUID, not by a session counter. Sent messages sent this session keep the backend queue number in its own field for delivery updates, but their row id is a window id like every other row.
- **Schema migrations by `user_version`.** The schema is at version 3 (v1: received table; v2: sent table; v3: full-text index on sent messages + `hidden` flag on both tables). Bump `user_version` and migrate in the store when you add a column or table.
- **Retention.** Messages, queries, and bulletins are kept until deleted. Positions, weather, telemetry, status, objects, items, Mic-E, and acks are trimmed after 7 days by default (Preferences > Message History: 1/7/30 days or keep everything). The trim runs at most once an hour while packets arrive.
- **Duplicate suppression.** A packet heard twice within 30 seconds (RF + APRS-IS, or two digipeaters) is fingerprinted on sender, addressee, body, and message number and stored once. The duplicate's event carries the first copy's UUID and sequence with `duplicate` set, so the lists skip it and the Traffic Log tags it `(duplicate)`.
- **Hide vs. delete.** The `hidden` flag keeps a cleared message in the database so search still finds it (marked `HIDDEN`); a real delete removes the row. Lists and the web remote leave hidden rows out, and a new multipart piece never joins a hidden row.
- **Legacy import.** On first launch the old `received_messages.json` is imported once and renamed `received_messages.json.migrated`.

**Don't re-introduce:** Don't write to `messages.db` from the frontend, and don't key message rows by a session counter. Both were the source of the stale-duplicate bug fixed in v1.1.0.

## Ponytail Audit - What Was Fixed & Why

This audit was run on 2026-06-17 by the Ponytail plugin. The findings below are the permanent record so nobody re-introduces these patterns.

### Finding 1: Large embedded HTML resource in Rust source

**What:** 1,610-line HTML/JS/CSS app embedded as a `r##"..."##` string literal in `webserver.rs`.

**Fix:** Extracted to `src-tauri/remote-control.html`, trimmed to ~400 lines. The extracted file is loaded at runtime with `include_str!()`.

**Why:** An inline 1,600-line string makes the Rust file unreadable, breaks syntax highlighting, and requires a Rust recompile for any HTML change.

**Don't re-introduce:** Never embed more than ~20 lines of HTML/JS in Rust. If the remote control needs significant new features, it gets added to the HTML file.

### Finding 2: Duplicated `format_display_version` in Rust + TypeScript

**What:** The same display version formatting logic existed in both `webserver.rs` (Rust) and `App.tsx` (TypeScript), doing identical transformations on raw version strings.

**Fix:** Removed the Rust copy. The web server sends raw version data; both the desktop app and remote HTML page format it client-side.

**Why:** Two sources of truth drift. The frontend renders the data, so the formatting belongs there.

**Don't re-introduce:** The Rust backend sends raw values. All display formatting belongs in TypeScript. If you add a new API endpoint to the web server, return raw data, not formatted strings.

### Finding 3: Duplicated theme `useEffect` across three windows

**What:** `HelpWindow.tsx`, `LogWindow.tsx`, and `ReceivedWindow.tsx` each had an identical `useEffect` for loading theme settings and applying the `data-theme` attribute. `PreferencesWindow.tsx` had a reactive variant that also watched for theme changes.

**Fix:** Extracted to `src/lib/useTheme.ts` with two modes (one-shot and reactive).

**Why:** The boilerplate was identical in 4 files. The hook centralizes the `prefers-color-scheme` listener logic and makes it impossible for a new window to get the theme wrong.

**Don't re-introduce:** Any new window that needs theme support should call `useTheme()`. Period.

### Finding 4: Inline settings payload construction in 3 places

**What:** Auto-save in `App.tsx`, manual save in `App.tsx`, and `PreferencesWindow.tsx` each built the `{ settings: {...} }` payload inline, with different default values and field naming conventions.

**Fix:** Created `buildSettingsPayload()` in `src/lib/helpers.ts` - the single function that maps camelCase params to snake_case payload with consistent defaults.

**See:** [Key Decision #1](#1-settings-payload-construction)

### Finding 5: Duplicated type definitions in 3 files

**What:** `AppSettingsPayload` defined identically in `App.tsx`, `PreferencesWindow.tsx`, and `ReceivedWindow.tsx`. Same for `ReceivedFilter`/`FILTER_OPTIONS`.

**Fix:** Consolidated to `src/lib/types.ts`.

**See:** [Key Decision #2](#2-shared-types)

### Finding 6: Production `console.log` in pushLog

**What:** `pushLog()` called `console.log()` on every log call, even in production builds, cluttering terminal output.

**Fix:** Wrapped in `if (import.meta.env.DEV)`.

**See:** [Key Decision #5](#5-console-logging)

## Common Pitfalls (Don't Repeat These)

### TypeScript / React

- **Don't define the same type twice.** If a type is used in more than one file, it goes in `src/lib/types.ts`. The Ponytail audit found `AppSettingsPayload` defined verbatim in 3 files - it had already drifted once.
- **Don't build settings payloads inline.** Use `buildSettingsPayload()`. There were 3 different inline versions with different defaults and field names. One had `true` as the auto-beacon default, another had `1500` as the interval. They were inconsistent.
- **Don't add bare `console.log`.** Wrap it in `pushLog()` or gate with `import.meta.env.DEV`. Production builds should have zero terminal noise.
- **Don't inline theme effects.** Use `useTheme()` from `src/lib/useTheme.ts`. Four files had the same pattern before the audit.
- **Don't stack patches on the same file without re-reading.** If you're applying multiple `patch` calls to the same file, re-read between each one. The fuzzy matcher can misalign if surrounding context shifted.

### Rust / Tauri

- **Don't embed HTML strings.** `r##"..."##` is not a file format. Anything over 20 lines goes in a separate file loaded with `include_str!()`.
- **Don't format data in the backend.** The Rust side returns raw values. Formatting belongs in the frontend. If the same data goes to the web server, the HTML page formats it client-side with JS.
- **Don't duplicate window size config.** Tauri applies `tauri.conf.json` size settings automatically. Adding a `set_size()` call in `setup()` with the same values is dead code. Only override sizes in Rust when you need dynamic sizing.
- **The Rust side uses `Mutex<RuntimeStateInner>` pattern** (single mutex for the whole state struct), not per-field `Arc<Mutex<T>>`. If you add a new field to `RuntimeState`, add it as a plain field to the inner struct and access it through the single lock. This is by design - simpler API, lower overhead, and fields are nearly always read together anyway.

### Cross-Window

- **Tauri v2 IPC `invoke` with no args:** pass `{}` explicitly as the args parameter. Passing nothing or `undefined` may fail silently or return stale data.
- **Cross-window state:** Don't try to sync a React state store across windows. Use Tauri events from the backend - the backend is the authoritative state owner, and it broadcasts to all windows.

## Mac App Store Gotchas

- **Code signing is mandatory.** The macOS DMG build produces an unsigned bundle. You need an Apple Developer ID certificate and must run `codesign` on the `.app` before packaging.
- **Notarization required.** After signing, submit to Apple for notarization before distribution.
- **Sandbox entitlements.** The Mac App Store requires sandboxing. APRSWolf's TNC access (network connections to Dire Wolf) and file I/O (settings persistence) will need explicit entitlements. Map these carefully - the networking entitlements for TNC TCP access must allow arbitrary TCP connections to localhost devices.
- **Tauri v2 + Mac App Store** has known quirks. Test the sandboxed build thoroughly before submission. Key areas: file access (settings), network access (TNC, APRS-IS), and webview behavior under sandbox.

## Cross-Platform Notes

APRSWolf now ships for macOS, Linux, and Windows. The notes below capture the
platform-specific behavior to keep in mind.

### Windows

- **PTY differences:** No UNIX pseudoterminals. KISS TCP connection logic is portable; TCPServer bind behavior is verified on Windows.
- **Tauri v2 Windows support** is mature. The main effort is UI layout testing - React components are portable, but WKWebView vs WebView2 render differently.
- **Directory paths:** Rust's `dirs` crate handles this; settings file locations are verified on Windows.
- **Packaging:** MSI (WiX) + NSIS `.exe`, built by the `release` workflow on `windows-latest`.
- **System alert sound:** `runtime.rs` plays the macOS `osascript beep` only on macOS; on Windows/Linux the sound call is a no-op.

### Linux

- **No built-in webview on most distros.** Tauri uses WebKitGTK. Users may need to install it: `sudo apt install libwebkit2gtk-4.1-dev`.
- **No App Store distribution.** Packages are `.deb`, `.rpm`, and `.AppImage`.
- **Dire Wolf is native on Linux** - TNC mode works best here.
- **Test on both X11 and Wayland.** Tauri v2 handles Wayland, but window positioning and sizes can differ.
- **Build baseline:** Ubuntu 22.04 is used in CI to keep the glibc floor low.

### General Cross-Platform

- **The frontend (React/TypeScript)** is the portable part. All business logic for display, formatting, and UI lives here and works identically on all platforms.
- **The backend (Rust/Tauri)** handles platform-specific I/O (TNC connections, file storage, system tray). Platform-specific code is isolated in `src-tauri/src/` - only `main.rs` and `lib.rs` need platform-conditional compilation.
- **`remote-control.html`** works on any platform with a browser - it's served by the embedded web server and is fully platform-agnostic.

## Build & Release

See the [project README](../README.md) for quick-start commands. Full release process:

```bash
# 1. Bump package.json version, src-tauri/Cargo.toml, and
#    src-tauri/tauri.conf.json; refresh the package and Cargo lockfiles.
# 2. Write release notes (copy RELEASE-NOTES-TEMPLATE.md → docs/releases/X.Y.Z.md)
#    and add the new version to docs/releases/README.md
# 3. Commit, then push a version tag:
git tag v1.1.0 && git push origin v1.1.0
# 4. The `release` GitHub Actions workflow builds all platforms and
#    attaches every artifact to a single GitHub Release.
```

Local builds per platform:

```bash
npm run release:macos             # macOS Intel (x64) + Apple Silicon (aarch64) DMGs
npm run release:linux             # Linux .deb + .rpm + .AppImage (on Linux); or release:linux:no-tests
npm run release:windows           # Windows MSI + NSIS .exe (on Windows); or release:windows:no-tests
```

macOS and Windows artifacts are not Developer ID signed or notarized; do that before public
distribution by adding signing secrets to the `release` workflow. The macOS app is ad-hoc
signed (`bundle.macOS.signingIdentity: "-"`); do not remove this, because the default
linker-only signature on Apple Silicon makes Gatekeeper report the app as "damaged".
After building, check with `codesign --verify --deep --strict APRSWolf.app`.

### Artifact naming and ARM targets

The macOS and Linux ARM artifacts target different operating systems and are not
interchangeable:

- macOS Apple Silicon uses the `aarch64` label (`APRSWolf_X.Y.Z_aarch64.dmg`).
- Linux ARM64 uses the `arm64` label (`APRSWolf_X.Y.Z_arm64.deb`, `.rpm`, `.AppImage`),
  for ARM64 Linux such as a 64-bit Raspberry Pi.

There is no macOS universal DMG; Intel and Apple Silicon are shipped as separate
DMGs.

## Version History

| Version | Date | Key Changes |
|---------|------|-------------|
| 1.1.0 | 2026-10-12 | Message history in SQLite (`message_store.rs`), full-text search, hide/delete/select, unread `To Me` + app icon badge, reworked main window (send bar on top, compact status bar, header clock), unified Messages pop-out. |
| 1.0.4 | 2026-10-12 | Release filenames use the full `major.minor.patch` version; the in-app badge shows the full version. |
| 1.0.3 | 2026-10-12 | Fix "damaged" error on Apple Silicon: `tauri.conf.json` sets `bundle.macOS.signingIdentity` to `-` so the whole app is ad-hoc signed with sealed resources. |
| 1.0.2 | 2026-10-12 | Preferences now provide working standard connection defaults; Open Remote UI launches the system browser. |
| 1.0.1 | 2026-10-11 | Bug fixes for message delivery, malformed input, remote access, and duplicate Traffic Log lines. |
| 1.0 | 2026-05-21 | First stable release. Consolidated RX/Mic-E improvements, pop-out received window, in-app help, remote web control parity. |
| (post-1.0) | 2026-06-17 | Code quality audit - extracted remote HTML, deduplicated types/hooks/helpers, dev logging guard. No user-facing changes. |

---

*Last updated: 2026-10-12 - v1.1.0 message history, search, clearing, and main-window rework integrated.*
*Next reader: you. This doc is your insurance against repeating the same bugs twice.*
