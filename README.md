# APRSWolf

Desktop APRS messaging for macOS, Windows, and Linux, on the air through any TNC that speaks KISS and AGW over TCP, or over the internet through APRS-IS with no radio at all.

![APRSWolf main window](docs/images/aprswolf-readme.png)

APRS messaging shouldn't mean squinting at a terminal or wrestling a
client from another decade. APRSWolf puts your station on one clean
screen. Point it at a TNC such as Dire Wolf or Graywolf over the
network and you're on RF. Without a radio, connect to APRS-IS and you're
on the internet backbone with the same window and the same messages.

Type a callsign and a message and hit Send. APRSWolf numbers it,
queues it, and keeps retrying with backoff until the other station acks
or the message expires, so you can watch delivery instead of guessing.
Incoming traffic sorts itself into filters for direct messages,
bulletins, weather, Mic-E, telemetry and more, and a click on any
message starts a reply. Everything you send and receive is kept in a
local history you can search, and the app icon shows a count of unread
messages addressed to you. Send a position beacon with one click, or
turn on beaconing at connect and on a timer in Preferences.

Leave the radio in the shack and run it from the couch: set a Remote
Token in Preferences, click **Enable Remote** in the main window, and
open the address it shows (port 18420 by default) from your phone on
the local network. **Open Remote UI** opens the same page in your
system browser.

## Download

Grab the latest version from the
[Releases page](https://github.com/WT1W-Actual/aprswolf/releases/latest).
No build tools needed.

| Your computer | Download this |
|---------------|---------------|
| Mac, Intel | `APRSWolf_1.1.0_x64.dmg` |
| Mac, Apple Silicon (M1/M2/M3/M4) | `APRSWolf_1.1.0_aarch64.dmg` |
| Windows | `APRSWolf_1.1.0_x64-setup.exe` (or the `.msi`) |
| Linux, Debian or Ubuntu (Intel/AMD 64-bit) | `APRSWolf_1.1.0_amd64.deb` |
| Linux, Debian or Ubuntu (ARM64, e.g. Raspberry Pi OS 64-bit) | `APRSWolf_1.1.0_arm64.deb` |
| Linux, Fedora or openSUSE (Intel/AMD 64-bit) | `APRSWolf-1.1.0-1.amd64.rpm` |
| Linux, Fedora or openSUSE (ARM64) | `APRSWolf-1.1.0-1.arm64.rpm` |
| Linux, any distro (Intel/AMD 64-bit) | `APRSWolf_1.1.0_amd64.AppImage` |
| Linux, any distro (ARM64) | `APRSWolf_1.1.0_arm64.AppImage` |

**Which ARM build do I need?** The `aarch64` DMG is for **Apple Silicon Macs**
(M1 and later). The `arm64` Linux packages are for **ARM64 Linux**, such as a
64-bit Raspberry Pi. They are not interchangeable - pick the one that matches
your operating system, not just the CPU family.

The macOS and Windows builds aren't code-signed yet, so the first launch
needs one extra click:

- **macOS:** open the `.dmg` and drag APRSWolf to Applications. If macOS
  says it can't verify the app, open System Settings → Privacy &
  Security, scroll down, and click **Open Anyway**. If it says the app is
  "damaged" (v1.0.2 and earlier), install the latest release or run
  `xattr -cr /Applications/APRSWolf.app` once.
- **Windows:** if SmartScreen says it protected your PC, click
  **More info**, then **Run anyway**.
- **Linux AppImage:** make it executable first
  (`chmod +x APRSWolf_1.1.0_amd64.AppImage`), then run it.

### Getting started

You'll need an amateur radio callsign. Open Preferences with the gear
(⚙︎) at the top right of the main window, enter your callsign, and save.
Then pick one way onto the network from the bar along the bottom of the
main window. Only one can be connected at a time.

- **No radio needed:** click **Connect APRS-IS**. It uses
  `rotate.aprs2.net:14580` unless you pick another server in
  Preferences, and APRSWolf derives the passcode from your callsign. A
  passcode of `-1` connects receive-only.
- **On the air:** APRSWolf doesn't include a TNC. Run one that offers
  KISS and AGW over TCP, such as
  [Dire Wolf](https://github.com/wb2osz/direwolf) or Graywolf, with
  your radio. Enter its host and KISS port (8001 by default) under TNC
  in Preferences, then click **Connect TNC**. APRSWolf sends over the
  KISS port and receives over the AGW port, one below it (8000 by
  default), so the TNC needs both turned on.

A green dot beside Station, TNC, APRS-IS and Remote in that bottom bar
means it's on; red means off.

To send your first message, type a callsign in **Message To**, type the
text in **Message**, and click **Send**. It shows up under Sent Messages
as queued, then sent, and turns acked when the other station replies
with an ack.

The [End User Guide](docs/END-USER-GUIDE.md) walks through the rest.

### Troubleshooting

- No replies from bots:
  Verify "Message To" is the bot callsign and path is valid for your RF area.
- Can't connect to the TNC:
  Make sure the TNC is running on the host you entered and that your network allows TCP traffic
  to its KISS and AGW ports. In Dire Wolf these are `KISSPORT` and `AGWPORT` in `direwolf.conf`.
- Connected to the TNC but nothing is received:
  APRSWolf receives on the TNC's AGW port, one below the KISS port (8000 when KISS is 8001).
  Check that the TNC has AGW turned on at that port.
- Can't connect to APRS-IS, or connected but can't send:
  Check the server and port in Preferences, and that the passcode matches your base callsign.
  A passcode of `-1` only receives.
- No unread count on the app icon on Linux:
  KDE Plasma and the Ubuntu dock show it; stock GNOME doesn't display app badges.
  On Windows the taskbar icon shows a red dot instead of a number.

## Documentation

- Documentation index: see [docs/README.md](docs/README.md)
- End users: see [docs/END-USER-GUIDE.md](docs/END-USER-GUIDE.md)
- Developers: see [docs/DEVELOPER-GUIDE.md](docs/DEVELOPER-GUIDE.md) (architecture, code patterns, audit history); build and test steps are below
- In-app help reference: see [docs/IN-APP-HELP.md](docs/IN-APP-HELP.md)
- Release notes template: see [docs/RELEASE-NOTES-TEMPLATE.md](docs/RELEASE-NOTES-TEMPLATE.md)
- Release index: see [docs/releases/README.md](docs/releases/README.md)
- Current release notes: see [docs/releases/1.1.0.md](docs/releases/1.1.0.md)

## What It Does

- Connects to a TNC such as Dire Wolf or Graywolf over TCP (KISS to send, AGW to receive), or to APRS-IS over the internet.
- Sends APRS messages with message numbers, then retries with backoff until the other station acks, the message fails, or it expires.
- Shows Sent and Received side by side, with each sent message's delivery status (queued, sent, acked, failed, expired) kept current.
- Filters received traffic by type: To Me, Bulletins, Queries, Messages, Weather, Position, Status, Objects, Items, Mic-E, Telemetry, Other.
- Starts a reply when you click a received message.
- Keeps sent and received messages in a local database across restarts, and searches all of it from one search field.
- Stores a packet heard twice (over RF and APRS-IS, or through two digipeaters) only once.
- Lets you hide or delete messages one at a time, as a selection, or everything a list shows.
- Exports your message history to a CSV file for a spreadsheet, and imports one back.
- Shows an unread count for messages addressed to you on the To Me filter, and on the app icon while APRSWolf is in the background.
- Shows UTC and local time in the header.
- Opens a separate Messages window with sent and received in one timeline, and a Traffic Log window with every raw packet and event.
- Derives the APRS-IS passcode from your callsign.
- Serves a browser remote control page for your phone or another computer.
- Includes a Help window with setup and troubleshooting topics, an About window with links to the project and release notes, and native `Preferences...` and `Help > APRSWolf Help` menu items.

## Using the Main Window

From top to bottom:

- **Header:** version, connection status, UTC and local time (`15:08Z 08:08 PDT`), **Traffic Log ↗**, and the gear (⚙︎) for Preferences. Escape closes Preferences.
- **Send row:** **Message To** is the station you're messaging, and **Message** is the text, up to 67 characters. Click **Send**.
- **Sent Messages:** what you've sent, newest first, each with its delivery status, plus **Clear all** and the search field.
- **Received Messages:** incoming traffic with the filter chips and **Clear all**. Click a message to reply. A red count on **To Me** shows messages addressed to you that arrived while APRSWolf was in the background; it clears when you come back. **Pop Out ↗** moves the list to the separate Messages window, which shows sent and received together.
- **Bottom bar:** Station (with **Beacon**), TNC, APRS-IS, and Remote Web Control, each with its on/off dot and address. Hover a title for what it does. TNC and APRS-IS can't both be connected.

Sent messages have a purple background and received ones green, in both windows. A message split into parts by the sender is joined back into one when the parts arrive close together.

## Message History

Sent and received messages are stored in a SQLite database, `messages.db`, in the app data folder:

- macOS: `~/Library/Application Support/com.jperry.aprswolf/`
- Linux: `~/.local/share/com.jperry.aprswolf/`
- Windows: `%APPDATA%\com.jperry.aprswolf\`

What is kept:

- Sent messages, and received messages, queries, and bulletins, are kept until you delete them.
- Positions, weather, telemetry, status, objects, items, Mic-E, and acks are removed after 7 days by default. Change this under Preferences > Message History (1 day, 7 days, 30 days, or keep everything). The trim runs at most once an hour while packets arrive.
- A sent message still waiting for its ack when the app closes is marked expired at the next launch, since nothing will retry it.
- A packet heard again within 30 seconds (over RF and APRS-IS, or through a second digipeater) is stored once. The Traffic Log still shows every copy, tagged `(duplicate)`.

Each list loads the newest 200 messages. In the separate Messages window (**Pop Out ↗**), **Load older messages** at the bottom fetches the next 200.

To search, use the field in the Sent Messages header. It matches callsigns and message text across the whole history and filters both panes as you type. The Messages window has its own search field.

On first launch after upgrading, the old `received_messages.json` is imported once and renamed to `received_messages.json.migrated`.

### Clearing messages

- Hover a message and click **×** to clear it.
- **Clear all**, beside each pane's title, clears the messages that pane shows: up to the newest 200, after any filter chips. Older history isn't touched.
- To clear several at once, hold a message for half a second. A checkbox appears on every message; check the ones you want, then click **×** on any checked message. **Done** or Escape leaves selection.

Each opens a dialog with a **Delete from database** box, unchecked by default. Left unchecked, the messages leave the list but stay in the database, and search still finds them marked **HIDDEN**. Click **↺** beside a hidden result to put it back in its list. Checked, they are deleted and no search finds them.

### Exporting messages

**File > Export Messages...** (Cmd+Shift+E on macOS, Ctrl+Shift+E elsewhere) opens the Export Messages window, then **Export...** saves a CSV file that opens in Numbers, Excel, or LibreOffice. The suggested name, such as `aprswolf-messages-2026-10-12-143005.csv`, ends with the date and time so several exports in a day don't collide.

With nothing changed, the file holds sent messages and received messages, queries, and bulletins, oldest first, hidden ones included. The window can narrow it:

- **Sent and received**, **Received only**, or **Sent only**.
- **Only messages new since the last new-only export.** New means added to this computer's history since then, so messages imported from elsewhere still count. The first new-only export takes everything, and a second one straight after it is empty. Only a new-only export with no other filter moves the mark, so messages a narrower one left out still reach the next. Each message goes out as it stood then; a later ack or the rest of a multipart message doesn't send it again.
- **Limit to a date range**, From and To in local time, both days included.
- **Callsigns**, matched against sender or addressee. A callsign without an SSID matches all of its SSIDs.
- **Include positions, weather, and other short-lived packets**, off by default. Turning it on warns that those packets are removed after the retention period (7 days by default), so only the ones still stored are exported.

The columns are `uuid`, `timestamp` (ISO 8601, UTC), `direction` (sent or received), `from`, `to`, `body`, `aprs_type`, `path`, `message_number`, `status` (sent messages' delivery status), and `hidden`. Text that starts with `=`, `+`, `-`, or `@` gets a leading apostrophe so a spreadsheet shows it as typed rather than running it as a formula.

### Importing messages

**File > Import Messages...** (Cmd+Shift+I on macOS, Ctrl+Shift+I elsewhere) reads a CSV export back into your history, from this computer or another one, including a file you edited by hand. Messages land in the lists at their own time.

- Columns are matched by name in any order. `direction`, `from`, `body`, and `timestamp` are required; the rest are optional and unknown columns are ignored.
- A message whose `uuid` is already in your history is skipped, so importing the same file twice changes nothing. A row with an empty `uuid` gets a new one, unless the same message (direction, sender, addressee, text, and time) is already there.
- The apostrophe that export puts in front of formula-like text is removed.
- Every row is checked before anything is stored: callsigns, a known `aprs_type`, a time from 1990 to now (a time without a zone is read as UTC), a valid `uuid`, a message number of up to five letters or digits, and a status of acked, failed, or expired for sent messages. If any row fails, nothing is imported and the dialog lists the rows and what's wrong.
- Positions, weather, and other short-lived packets older than the retention period (Preferences > Message History) bring up a warning with their count before anything is stored. **Cancel** leaves your history as it was.

## APRS And AX.25 Behavior

Outbound message behavior is APRS-compliant:

- AX.25 destination is always APRS.
- UI "Message To" field is the APRS addressee (callsign being messaged), not the AX.25 destination.
- Info payload format is APRS message format with optional message number suffix.
- Message numbers are appended as two digits (00 to 99, wrapping) for ACK tracking.

In plain terms:

- Source field = your station callsign/SSID.
- Message To field = station you want to message (example: WXYO).
- Digipeater Path field = WIDE path list.

## Building from source

Only needed if you want to build APRSWolf yourself; to just run it, use
[Download](#download) above. Do this setup once per machine, before
installing or building anything.

### What you need

- **Node.js** (LTS)
- **Rust**, installed through rustup
- **Tauri's system libraries** for your OS, listed below

To run what you build, you need the same callsign and TNC or APRS-IS
connection as in [Getting started](#getting-started).

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
targets added above are never seen, and the build fails with
`can't find crate for std` for `x86_64-apple-darwin`. If you already
installed it, `brew uninstall rust` fixes this.

The `release:macos` script checks for both targets and stops with a
"Missing Rust targets" message if one is missing.

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
npm run release:macos             # macOS: Intel and Apple Silicon .dmg
npm run release:linux             # Linux: .deb, .rpm and .AppImage
npm run release:windows           # Windows (PowerShell): .msi and setup .exe
```

The macOS DMGs are written to
`src-tauri/target/<arch>-apple-darwin/release/bundle/dmg/`
(`npm run release:macos:open` opens the Apple Silicon folder). Linux builds are
collected in `dist-linux/` and Windows builds in `dist-windows/`.

## Scripts

- npm run dev: Vite frontend only.
- npm run build: Type-check and frontend build.
- npm run build -- --mode production: Example for passing Vite args.
- npm run test: Frontend tests (Vitest).
- npm run play: Tauri dev app (frontend + Rust backend).
- npm run tauri dev: Direct Tauri dev run.
- npm run release:macos: Build Intel and Apple Silicon DMGs.
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
