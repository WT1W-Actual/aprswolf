# APRSWolf

Tauri + React desktop app for APRS messaging over KISS/AGW TCP (Dire Wolf compatible) and APRS-IS.

![APRSWolf main window](docs/images/aprswolf-readme.png)

APRS messaging shouldn't mean squinting at a terminal or wrestling a
client from another decade. APRSWolf puts your station on one clean
screen. Point it at Dire Wolf or Graywolf over the network and you're
on RF; switch to APRS-IS and you're on the internet backbone with the
same window and the same messages.

Type a callsign and a message and hit Send. APRSWolf numbers it,
queues it, and keeps retrying with backoff until the other station acks
or the message expires, so you can watch delivery instead of guessing.
Incoming traffic sorts itself into filters for direct messages,
bulletins, weather, Mic-E, telemetry and more, and a click on any
message starts a reply. Position beacons go out on connect and on a
timer.

Leave the radio in the shack and run it from the couch: the built-in
remote control serves a browser page you can open from your phone on
the local network once you set a remote token. It runs on macOS, Linux
and Windows.

## Download

Grab the latest version from the
[Releases page](https://github.com/WT1W-Actual/aprswolf/releases/latest).
No build tools needed.

| Your computer | Download this |
|---------------|---------------|
| Mac, any (Intel or Apple Silicon) | `APRSWolf_1.0_universal.dmg` |
| Mac, Intel only | `APRSWolf_1.0_x64.dmg` |
| Mac, Apple Silicon only | `APRSWolf_1.0_arm64.dmg` |
| Windows | `APRSWolf_1.0_x64-setup.exe` (or the `.msi`) |
| Linux, Debian or Ubuntu | `APRSWolf_1.0_amd64.deb` (`arm64` for a 64-bit Raspberry Pi) |
| Linux, Fedora or openSUSE | `APRSWolf-1.0-1.amd64.rpm` (`arm64` for ARM) |
| Linux, any distro | `APRSWolf_1.0_amd64.AppImage` (`arm64` for ARM) |

The macOS and Windows builds aren't code-signed yet, so the first launch
needs one extra click:

- **macOS:** open the `.dmg` and drag APRSWolf to Applications. If macOS
  says it can't verify the app, open System Settings → Privacy &
  Security, scroll down, and click **Open Anyway**.
- **Windows:** if SmartScreen says it protected your PC, click
  **More info**, then **Run anyway**.
- **Linux AppImage:** make it executable first
  (`chmod +x APRSWolf_1.0_amd64.AppImage`), then run it.

### Getting started

You'll need an amateur radio callsign. Then pick one way onto the
network:

- **No radio needed:** choose APRS-IS, enter your callsign, and connect.
  APRSWolf fills in the passcode from your callsign.
- **On the air:** run [Dire Wolf](https://github.com/wb2osz/direwolf) or
  Graywolf with your radio, then point APRSWolf at its host and KISS port
  (8001 by default).

The [End User Guide](docs/END-USER-GUIDE.md) walks through the rest.

### Troubleshooting

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

## Documentation

- Documentation index: see [docs/README.md](docs/README.md)
- End users: see [docs/END-USER-GUIDE.md](docs/END-USER-GUIDE.md)
- Developers: see [docs/DEVELOPER-GUIDE.md](docs/DEVELOPER-GUIDE.md) (architecture, code patterns, audit history)
- In-app help reference: see [docs/IN-APP-HELP.md](docs/IN-APP-HELP.md)
- Release notes template: see [docs/RELEASE-NOTES-TEMPLATE.md](docs/RELEASE-NOTES-TEMPLATE.md)
- Release index: see [docs/releases/README.md](docs/releases/README.md)
- Current release notes: see [docs/releases/1.0.md](docs/releases/1.0.md)
- Developers: use the build and test notes below

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
- TNC and APRS-IS are mutually exclusive - connecting one disables the other.
- Input fields are fixed-width to avoid wasted space (callsigns, ports, hosts).
- Sent and Received message panes resize dynamically to fill available window space.
- Message and Traffic Log panels stretch both horizontally and vertically with the window.
- Received text wraps instead of forcing horizontal growth.
- Basic multipart stitching is applied for near-sequential fragments from the same sender.
- Traffic Log can be shown/hidden via a centered toggle button.
- Help opens in its own macOS-style window with sidebar navigation and search.

## Building from source

Only needed if you want to build APRSWolf yourself; to just run it, use
[Download](#download) above. Do this setup once per machine, before
installing or building anything.

### What you need

- **Node.js** (LTS)
- **Rust**, installed through rustup
- **Tauri's system libraries** for your OS, listed below
- **Dire Wolf** (optional, for TNC mode). APRSWolf does not include a software TNC. You can use Dire Wolf as a KISS TCP TNC. Dire Wolf must be configured with a KISS TCP port (default 8001) that APRSWolf can connect to. See [Dire Wolf documentation](https://github.com/wb2osz/direwolf) for setup details.
- **APRS-IS Server** (optional, for APRS-IS mode). You can connect to any APRS-IS server (default `rotate.aprs2.net:14580`) for internet-based APRS messaging.
- **Valid APRS Callsign**, required for both TNC and APRS-IS modes.
- **APRS-IS Passcode**, derived automatically from your callsign for APRS-IS connections. Use `-1` for receive-only mode.

The upstream Tauri guide covers other distros and edge cases:
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

### macOS

Install the Xcode Command Line Tools (`xcode-select --install`) and
Node.js LTS, then:

```bash
brew install rustup                       # Rust's toolchain manager
```

Homebrew installs rustup "keg-only", which means it stays off the PATH
until you add it. Run `echo $SHELL` to see which shell you use, then
add the line for that shell:

```bash
# zsh, the macOS default since Catalina
echo 'export PATH="$(brew --prefix rustup)/bin:$HOME/.cargo/bin:$PATH"' >> ~/.zshrc

# bash (Terminal starts bash as a login shell, which reads ~/.bash_profile)
echo 'export PATH="$(brew --prefix rustup)/bin:$HOME/.cargo/bin:$PATH"' >> ~/.bash_profile

# fish (saved once, applies to every new fish session)
fish -c 'fish_add_path (brew --prefix rustup)/bin ~/.cargo/bin'
```

Open a new terminal window so the change applies, then:

```bash
rustup default stable                     # install the stable toolchain
rustup target add x86_64-apple-darwin aarch64-apple-darwin   # Intel and Apple Silicon targets
which cargo rustc                         # both should be under .../rustup/bin
```

If you'd rather not use Homebrew, the installer from
[rustup.rs](https://rustup.rs) works too and adds Rust to the PATH for
bash, zsh and fish itself.

Don't also run `brew install rust`. That formula is a separate Rust with
its own `cargo` and `rustc`, built for your Mac's own chip only. If it
comes first on the PATH, the build uses it instead of rustup, the
targets added above are never seen, and the universal build fails with
`can't find crate for std` for `x86_64-apple-darwin`. If you already
installed it, `brew uninstall rust` fixes this.

The `release:macos` and `release:macos:universal` scripts check for both
targets and stop with a "Missing Rust targets" message if one is
missing.

### Linux (Debian, Ubuntu, Raspberry Pi OS)

```bash
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev rpm xdg-utils
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
. "$HOME/.cargo/env"                      # this shell only (fish: source ~/.cargo/env.fish)
rustup default stable
```

The rustup installer adds Rust to the PATH for future bash, zsh and
fish sessions. The `.cargo/env` line covers the shell you ran it in.

These are the packages the release workflow installs. `rpm` is only
needed for the `.rpm` bundle and `xdg-utils` only for the `.AppImage`.
Install Node.js LTS from [nodejs.org](https://nodejs.org) or NodeSource,
since distro packages are often older than LTS. On Fedora and other
non-apt distros, use the package list from the Tauri guide linked above.
`npm run release:linux` installs the apt packages and rustup itself if
they're missing.

### Windows

1. Install [Microsoft C++ Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
   and select the "Desktop development with C++" workload.
2. WebView2 ships with Windows 10 and 11. On an older system, install the
   Evergreen runtime from [Microsoft](https://developer.microsoft.com/microsoft-edge/webview2/).
3. Install Rust with `rustup-init.exe` from [rustup.rs](https://rustup.rs),
   then in a new PowerShell window:

   ```powershell
   rustup default stable-msvc
   rustup target add x86_64-pc-windows-msvc
   ```

4. Install Node.js LTS from [nodejs.org](https://nodejs.org).

Build from PowerShell with `npm run release:windows`.

### Optional on any OS: a `tauri` command

`npm run tauri ...` uses the Tauri CLI that `npm install` puts in
`node_modules`, so nothing else is needed. If you want to call Tauri
directly, `cargo install tauri-cli` adds `cargo tauri`. It creates only
`cargo-tauri`, not a bare `tauri`, so add a link if you want one:

```bash
ln -s ~/.cargo/bin/cargo-tauri ~/.cargo/bin/tauri   # macOS and Linux
```

### Get the code

```bash
git clone https://github.com/WT1W-Actual/aprswolf.git
cd aprswolf
```

Git comes with the Xcode Command Line Tools on macOS. On Linux, install
it with `sudo apt-get install git`; on Windows, use
[Git for Windows](https://git-scm.com/download/win). Every command
below runs from this `aprswolf` folder.

### Install

```bash
npm install
```

Run this in the repo folder before any `npm run` command. It installs
TypeScript, Vite and the Tauri CLI that the build scripts call. If a
build stops with `tsc: command not found`, this step was skipped, and
running it fixes the error without adding anything to `package.json`.

## Run

Recommended:

```bash
npm run play
```

Equivalent direct command:

```bash
npm run tauri dev
```

If cargo is missing in a fresh shell, load it for that session:

```bash
source ~/.cargo/env          # bash and zsh
source ~/.cargo/env.fish     # fish
```

On a Mac that installed rustup through Homebrew, recheck the PATH line
for your shell under [macOS](#macos) instead.

`npm run play` starts a development session with live reload. It
doesn't produce an app you can install.

## Build an installer

Each command builds the installers for the OS it runs on, the same
files the [Download](#download) table lists:

```bash
npm run release:macos:universal   # macOS: universal .dmg
npm run release:linux             # Linux: .deb, .rpm and .AppImage
npm run release:windows           # Windows (PowerShell): .msi and setup .exe
```

The macOS `.dmg` is written to
`src-tauri/target/universal-apple-darwin/release/bundle/dmg/`
(`npm run release:macos:open` opens that folder). Linux builds are
collected in `dist-linux/` and Windows builds in `dist-windows/`.
`npm run release:macos` builds separate Intel and Apple Silicon DMGs
instead.

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
- npm run release:linux: Build Linux `.deb` + `.rpm` + `.AppImage` (run on Linux).
- npm run release:linux:no-tests: Same, skipping tests.
- npm run release:windows: Build Windows MSI + NSIS `.exe` (run on Windows).

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

## For maintainers

Release steps are in the [Developer Guide](docs/DEVELOPER-GUIDE.md#build--release).
