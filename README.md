# APRSWolf

Tauri + React desktop app for APRS messaging over KISS/AGW TCP (Dire Wolf compatible) and APRS-IS.

## Documentation

- Documentation index: see [docs/README.md](docs/README.md)
- End users: see [docs/END-USER-GUIDE.md](docs/END-USER-GUIDE.md)
- Developers: see [docs/DEVELOPER-GUIDE.md](docs/DEVELOPER-GUIDE.md) (architecture, code patterns, audit history)
- In-app help reference: see [docs/IN-APP-HELP.md](docs/IN-APP-HELP.md)
- Release notes template: see [docs/RELEASE-NOTES-TEMPLATE.md](docs/RELEASE-NOTES-TEMPLATE.md)
- Release index: see [docs/releases/README.md](docs/releases/README.md)
- Current release notes: see [docs/releases/1.0.md](docs/releases/1.0.md)
- Developers: use the build and test notes below

## Releases

Typical publish flow for the universal macOS release artifact:

1. Build the universal macOS app and DMG with a clean rebuild of both architectures:
  `npm run release:macos:universal`
2. If you also need separate per-architecture DMGs:
  `npm run release:macos`

Recommended end-user artifact:

- Universal macOS DMG: `src-tauri/target/universal-apple-darwin/release/bundle/dmg/APRSWolf_1.0_universal.dmg`

The universal packaging script now performs a clean rebuild of both macOS architecture targets before merging them, so the DMG does not reuse a stale `.app` bundle.

When publishing a new release:

1. Copy [docs/RELEASE-NOTES-TEMPLATE.md](docs/RELEASE-NOTES-TEMPLATE.md)
2. Save it as `docs/releases/X.Y.Z.md`
3. Fill in the current version and shipped changes
4. Add the new version to [docs/releases/README.md](docs/releases/README.md)
5. Publish the universal DMG as the primary download

Signing and notarization note:

- If you distribute the universal `.app`, sign it after the `lipo` merge step used by the universal packaging script.

## What It Does

- Connects to a Dire Wolf TNC via KISS TCP with separate TX/RX status indicators.
- Connects to APRS-IS for internet-based APRS messaging (mutually exclusive with TNC).
- Sends APRS message frames with ACK numbering.
- Persists outbound messages to a durable queue.
- Retries queued and failed messages with backoff until acked, failed, or expired.
- Shows compact Sent and Received panes that resize dynamically with the window.
- Supports click-to-reply from Received messages.
- Includes a dedicated Help window with guided setup and troubleshooting topics.
- Exposes native macOS `Preferences...` and `Help > APRSWolf Help` menu items.
- Supports a Received filter (To Me Only).
- Supports optional Traffic Log panel (off by default).
- Auto-derives APRS-IS passcode from the source callsign.

## APRS And AX.25 Behavior

Outbound message behavior is APRS-compliant:

- AX.25 destination is always APRS.
- UI "Message To" field is the APRS addressee (callsign being messaged), not the AX.25 destination.
- Info payload format is APRS message format with optional message number suffix.
- Message numbers are appended as two-digit values for ACK tracking.

In plain terms:

- Source field = your station callsign/SSID.
- Message To field = station you want to message (example: WXYO).
- Digipeater Path field = WIDE path list.

## UI Notes

- APRSWolf logo is centered at the top of the interface.
- Station settings (callsign, digi path), TNC, and APRS-IS controls are combined in a
  single compact row separated by vertical dividers.
- TNC and APRS-IS are mutually exclusive — connecting one disables the other.
- Input fields are fixed-width to avoid wasted space (callsigns, ports, hosts).
- Sent and Received message panes resize dynamically to fill available window space.
- Message and Traffic Log panels stretch both horizontally and vertically with the window.
- Received text wraps instead of forcing horizontal growth.
- Basic multipart stitching is applied for near-sequential fragments from the same sender.
- Traffic Log can be shown/hidden via a centered toggle button.
- Help opens in its own macOS-style window with sidebar navigation and search.

## Prerequisites

- **Dire Wolf** (optional, for TNC mode) — APRSWolf does not include a software TNC. You can use Dire Wolf as a KISS TCP TNC. Dire Wolf must be configured with a KISS TCP port (default 8001) that APRSWolf can connect to. See [Dire Wolf documentation](https://github.com/wb2osz/direwolf) for setup details.
- **APRS-IS Server** (optional, for APRS-IS mode) — You can connect to any APRS-IS server (default `rotate.aprs2.net:14580`) for internet-based APRS messaging.
- **Valid APRS Callsign** — Required for both TNC and APRS-IS modes.
- **APRS-IS Passcode** — Automatically derived from your callsign for APRS-IS connections. Use `-1` for receive-only mode.
- Node.js (LTS recommended)
- Rust toolchain (`cargo` and `rustc`)
- Tauri platform requirements for macOS

## Install

```bash
npm install
```

## Run

Recommended:

```bash
npm run play
```

Equivalent direct command:

```bash
npm run tauri dev
```

If cargo is missing in a fresh shell:

```bash
source ~/.cargo/env
```

## Scripts

- npm run dev: Vite frontend only.
- npm run build: Type-check and frontend build.
- npm run build -- --mode production: Example for passing Vite args.
- npm run test: Frontend tests (Vitest).
- npm run play: Tauri dev app (frontend + Rust backend).
- npm run tauri dev: Direct Tauri dev run.
- npm run release:macos: Build Intel and Apple Silicon DMGs.
- npm run release:macos:universal: Build a universal macOS app and DMG from the two arch bundles.
- npm run release:macos:all: Build both per-arch DMGs and the universal macOS DMG.

## Test

Frontend:

```bash
npm run test
```

Backend tests:

```bash
cd src-tauri
cargo test
```

Backend compile check:

```bash
cd src-tauri
cargo check
```

## Troubleshooting

- No replies from bots:
  Verify "Message To" is the bot callsign and path is valid for your RF area.
- App layout jumps or grows:
  Traffic Log should stay off by default; if enabled, it scrolls internally.
- Connection succeeds but no decode events:
  Verify the RX socket endpoint and that Dire Wolf is configured to allow AGW TCP connections.
- Can't connect to Dire Wolf:
  Ensure Dire Wolf is running on the target host and that your network allows TCP traffic to
  the configured AGW/KISS port. On the Dire Wolf host, check `AGWPORT` and `KISSPORT` settings
  in `direwolf.conf`.
