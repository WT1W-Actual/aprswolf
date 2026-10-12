# APRSWolf In-App Help

This document mirrors the Help window available from inside APRSWolf.

![APRSWolf Help Window](images/aprswolf-help-window.png)

## Help Topics

### Overview

- APRSWolf is a desktop APRS messaging app for macOS, Windows, and Linux.
- It supports RF messaging through a Dire Wolf compatible TNC and internet messaging through APRS-IS.
- Only one transport can be active at a time.

### Quick Start

1. Open `Preferences` with the gear (⚙︎) at the top right of the main window, or from the app menu (macOS: `APRSWolf > Preferences`).
2. Enter your callsign and at least one transport.
3. Click `Save`.
4. Connect either TNC or APRS-IS from the main window.
5. Enter the destination in `Message To`.
6. Type a message.
7. Click `Send`.

### Preferences

- `Preferences` is the first stop for a new user.
- Save your callsign, digi path, theme, sound, ACK visibility, beacon coordinates, TNC settings, APRS-IS settings, and remote web defaults there.
- Standard connection defaults include TNC `127.0.0.1:8001` with path `WIDE1-1,WIDE2-1`, APRS-IS `rotate.aprs2.net:14580`, and Remote Web Control port `18420`.
- Use the main window for live operating after setup is saved.

### Station Setup

- `Callsign` is your APRS callsign.
- `Digi Path` is only used for RF/TNC sends.
- `Theme` controls System, Light, or Dark appearance.
- `Play Sound` uses the system alert sound for direct inbound messages (macOS only; a no-op on Windows and Linux).
- `Message Font Size` resizes sent and received message text (range 10–32px, default 15px).
- `Latitude`, `Longitude`, and `Symbol` are used for beaconing.

### TNC Mode

- Use `Connect TNC` when APRSWolf should talk to Dire Wolf over TCP.
- Confirm the host and port match your Dire Wolf configuration.
- TX and RX status show whether the connection is active.

### APRS-IS Mode

- Use `Connect APRS-IS` for internet APRS messaging.
- The app normally derives the passcode from your callsign.
- The APRS-IS filter is optional; APRSWolf does not force a default filter.
- Example filters: `m/50`, `r/42.36/-71.06/25`, `t/WT1W`.

### Sending Messages

- `Message To` is the APRS addressee.
- APRSWolf manages APRS message numbering and ACK tracking automatically.
- Click any received message to open a reply dialog.

### Receiving Traffic

- Use filter chips to limit traffic by category.
- `Messages` shows all APRS message frames; `To Me` only shows traffic addressed to your station.
- Turn on `Promiscuous Base-Call` in the main window if `To Me` should also include your base callsign without the SSID.
- A red count on `To Me`, and on the app icon, shows messages addressed to you that arrived while APRSWolf was in the background. The app icon count can be turned off in Preferences.
- The search field in the Sent Messages header searches sent and received messages across the whole history and filters both panes.
- Use `Pop Out` to open the Messages window: sent and received in one timeline, with its own search.
- A packet heard twice, over RF and APRS-IS or through two digipeaters, is listed once.
- Hover a message and click `×` to clear it, or use `Clear all` beside a pane title to clear what that pane shows. Hold a message for half a second to select several, then click `×` on any selected one. Leave `Delete from database` unchecked to hide them but keep them findable by search, where `↺` puts one back; check it to delete them.
- `File > Export Messages...` saves sent and received messages, queries, and bulletins to a CSV file for a spreadsheet, hidden ones included. Its window can narrow the file to sent or received, new since the last new-only export, a date range, or some callsigns, and can add short-lived packets.
- `File > Import Messages...` reads an export back in, even one edited by hand. Messages already in your history are skipped, and a file with any bad row is refused with the rows listed.
- Leave `Show ACKs` off for everyday use.
- Use `Traffic Log` when you need a full event history.

### Remote Web Control

- Enable the browser-based control page from the Remote Web Control panel.
- Click `Open Remote UI` to open the page in the system browser; the displayed URL can also be opened on another device.
- While remote control is enabled, desktop controls stay visible but locked.
- Disable Remote to restore normal desktop interaction.
- With no remote token configured, the server is limited to this computer.

### Help Menu

- Use `Help > APRSWolf Help` from the app menu for the built-in guide.
- The Help window includes search, topic navigation, setup guidance, and troubleshooting notes.

### Troubleshooting

- No replies: verify destination, transport, and RF path.
- TNC connection issues: confirm Dire Wolf is running and the TCP port is correct.
- APRS-IS issues: verify the server, base callsign, and passcode.
- Too much receive traffic: tighten the APRS-IS filter.

## Recommendation

Use the Help window for quick answers inside the app.
Use [END-USER-GUIDE.md](END-USER-GUIDE.md) for the full step-by-step user guide.