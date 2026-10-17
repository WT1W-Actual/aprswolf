export type HelpSection = {
  id: string;
  title: string;
  summary: string;
  blocks: Array<
    | { type: "paragraph"; text: string }
    | { type: "list"; title?: string; items: string[] }
    | { type: "steps"; title?: string; items: string[] }
    | { type: "callout"; tone: "info" | "warning"; title: string; text: string }
  >;
};

export const HELP_SECTIONS: HelpSection[] = [
  {
    id: "overview",
    title: "Overview",
    summary: "What APRSWolf does and how the app is organized.",
    blocks: [
      {
        type: "paragraph",
        text: "APRSWolf is a desktop APRS messaging app for macOS, Windows, and Linux. Open Preferences first to save your station callsign, transport details, beacon coordinates, and remote-control defaults. Then use the main window for live connections, messaging, and traffic review. Only one transport can be active at a time.",
      },
      {
        type: "list",
        title: "Main areas of the app",
        items: [
          "Preferences for your station callsign, digi path, theme, sound, beacon coordinates, APRS-IS settings, and remote web options.",
          "Live TNC connection controls for the host and port saved in Preferences.",
          "Live APRS-IS controls using the saved server, port, passcode, and receive filter.",
          "Message composer for direct APRS messages.",
          "Sent and Received panes for day-to-day traffic review.",
          "Traffic Log, the Messages pop-out, Preferences, and Help as separate windows for focused tasks.",
        ],
      },
    ],
  },
  {
    id: "quick-start",
    title: "Quick Start",
    summary: "Send your first message with the fewest possible steps.",
    blocks: [
      {
        type: "steps",
        title: "Fastest path",
        items: [
          "Choose Preferences from the app menu (macOS: APRSWolf > Preferences) and enter your callsign.",
          "Add at least one transport: TNC host/port or APRS-IS server/port/passcode.",
          "Click Save to store your setup.",
          "Choose one transport: Connect TNC for RF or Connect APRS-IS for internet APRS.",
          "Enter the destination station in Message To.",
          "Type the message body.",
          "Click Send and watch the activity strip or Traffic Log for queue and receive status.",
        ],
      },
      {
        type: "callout",
        tone: "info",
        title: "ACKs",
        text: "Leave Show ACKs off for normal operation. Turn it on only when you need to inspect message acknowledgements or troubleshoot packet flow.",
      },
    ],
  },
  {
    id: "preferences",
    title: "Preferences",
    summary: "Use the Preferences window for first-run setup and saved defaults.",
    blocks: [
      {
        type: "paragraph",
        text: "Open Preferences from the APRSWolf menu or with the gear at the top right of the main window; Escape closes it. This is where a new user should enter the static setup the app reuses later.",
      },
      {
        type: "list",
        title: "What belongs in Preferences",
        items: [
          "Station callsign and RF digi path.",
          "Theme, message sound, and ACK visibility behavior.",
          "TNC host and KISS port for any TNC that offers KISS and AGW over TCP, such as Dire Wolf or Graywolf.",
          "APRS-IS server, port, passcode, and receive filter.",
          "Beacon latitude, longitude, and symbol.",
          "Remote Web Control port and launch behavior.",
          "Standard defaults include TNC 127.0.0.1:8001 with path WIDE1-1,WIDE2-1, APRS-IS rotate.aprs2.net:14580, and Remote Web Control port 18420.",
        ],
      },
      {
        type: "callout",
        tone: "info",
        title: "App menu",
        text: "Use Preferences for saved setup and Help > APRSWolf Help for the built-in guide. On macOS these live in the APRSWolf menu bar; on Windows and Linux they are in the app menu. The main window stays focused on live operating tasks.",
      },
    ],
  },
  {
    id: "station",
    title: "Station Setup",
    summary: "Configure your callsign, path, theme, sound, and optional beacon coordinates.",
    blocks: [
      {
        type: "list",
        items: [
          "Callsign: your APRS callsign, including SSID if you use one.",
          "Digi Path: used for RF/TNC sends only. APRS-IS sends ignore the RF path.",
          "Theme: System, Light, or Dark.",
          "Play Sound: system alert sound for direct inbound messages addressed to you (macOS only; a no-op on Windows and Linux).",
          "Message Font Size: slider to resize sent and received message text (10–32px).",
          "Latitude, Longitude, Symbol: used by the Beacon button and by automatic beacons.",
        ],
      },
      {
        type: "callout",
        tone: "warning",
        title: "Auto-beacon behavior",
        text: "Automatic beacons are off by default. Check Beacon on connect to send one each time a connection is made, and set Auto-beacon every to send one on a timer. Both need beacon coordinates and a symbol.",
      },
    ],
  },
  {
    id: "tnc",
    title: "TNC Mode",
    summary: "Use any TNC that offers KISS and AGW over TCP, such as Dire Wolf or Graywolf, for RF messaging.",
    blocks: [
      {
        type: "steps",
        items: [
          "Enter the host running your TNC.",
          "Enter its KISS TCP port (8001 by default). APRSWolf sends over KISS and receives over the AGW port one below it (8000 by default).",
          "Click Connect TNC.",
          "Wait for the dot beside TNC in the bottom bar to turn green.",
        ],
      },
      {
        type: "list",
        title: "Useful checks",
        items: [
          "Make sure the TNC is already running, with both KISS and AGW turned on.",
          "Confirm the port matches the TNC's KISS port (KISSPORT in Dire Wolf), and that its AGW port is one below (AGWPORT).",
          "Use a path such as WIDE1-1,WIDE2-1 only when transmitting over RF.",
        ],
      },
    ],
  },
  {
    id: "aprsis",
    title: "APRS-IS Mode",
    summary: "Connect directly to APRS-IS for internet APRS messaging.",
    blocks: [
      {
        type: "list",
        items: [
          "Server: usually rotate.aprs2.net.",
          "Port: usually 14580.",
          "Pass: derived automatically from your callsign in most cases.",
          "Filter: optional server-side receive filter. Leave it blank by default unless you want APRSWolf to send a filter command.",
          "Examples: m/50 for a radius filter, r/42.36/-71.06/25 for a fixed latitude/longitude radius, and t/WT1W for traffic to one callsign.",
        ],
      },
      {
        type: "callout",
        tone: "info",
        title: "Passcode note",
        text: "APRS-IS passcodes are based on the base callsign, not the SSID. A callsign ending in -9 or -15 uses the same passcode as the base call.",
      },
    ],
  },
  {
    id: "receive-filters",
    title: "APRS-IS Receive Filters",
    summary: "Limit inbound APRS-IS traffic by distance, callsign, or packet type.",
    blocks: [
      {
        type: "paragraph",
        text: "Receive filters tell APRS-IS which packets to deliver before they reach APRSWolf. They are for reducing traffic, focusing on a local area, or tracking specific callsigns and packet categories.",
      },
      {
        type: "list",
        title: "Common parameters",
        items: [
          "m/<miles>: packets within a radius of your login position, such as m/50.",
          "r/<lat>/<lon>/<miles>: packets within a radius of a fixed point, such as r/42.36/-71.06/25.",
          "t/<callsign>: packets for a specific callsign, such as t/WT1W.",
          "Leave the field blank if you want APRSWolf to receive an unfiltered APRS-IS feed.",
        ],
      },
      {
        type: "list",
        title: "Examples",
        items: [
          "m/25 - receive packets within 25 miles of your login position.",
          "r/42.36/-71.06/25 - receive packets within 25 miles of Boston.",
          "t/W1AW - receive packets for a specific callsign.",
          "m/50 t/WT1W - combine a distance filter with a callsign filter.",
        ],
      },
      {
        type: "callout",
        tone: "info",
        title: "Default behavior",
        text: "APRSWolf leaves the receive filter empty by default. Enter a filter only when you want to narrow APRS-IS traffic.",
      },
    ],
  },
  {
    id: "sending",
    title: "Sending Messages",
    summary: "How message addressing, queuing, and replies work.",
    blocks: [
      {
        type: "list",
        items: [
          "Message To is the APRS addressee, not the AX.25 destination field.",
          "The app queues outbound messages and tracks ACK delivery automatically.",
          "Click any received message to open a pre-filled reply dialog.",
          "Send Reply uses your current transport and the saved digi path if you are on RF.",
        ],
      },
    ],
  },
  {
    id: "receiving",
    title: "Receiving Traffic",
    summary: "Use filters, search, the Messages window, and the traffic log to manage APRS traffic.",
    blocks: [
      {
        type: "list",
        items: [
          "Use filter chips to narrow traffic by category such as Messages, Queries, Weather, Telemetry, or To Me.",
          "Messages shows all APRS message frames. To Me only shows traffic addressed to your station.",
          "A red count on To Me, and on the app icon, shows messages addressed to you that arrived while APRSWolf was in the background. You can turn the app icon count off in Preferences.",
          "Promiscuous Base-Call is a main-window toggle that also treats messages for your base callsign as yours.",
          "The search field in the Sent Messages header searches sent and received messages across the whole history and filters both panes.",
          "Right-click an expired sent message and choose Resend to queue it again under its original message number.",
          "Pop Out opens the Messages window: sent and received in one timeline, with its own search and reply behavior.",
          "A packet heard twice, over RF and APRS-IS or through two digipeaters, is listed once.",
          "Multipart stitching is applied for near-sequential fragments from the same station.",
          "Mic-E packets display with a badge and decoded body when available.",
        ],
      },
      {
        type: "callout",
        tone: "info",
        title: "Clearing messages",
        text: "Hover a message and click × to clear it, or use Clear all beside a pane title to clear what that pane shows. Hold a message for half a second to select several, then click × on any selected one. Leave Delete from database unchecked to hide the messages but keep them findable by search, where ↺ puts one back; check it to delete them.",
      },
      {
        type: "callout",
        tone: "info",
        title: "Exporting messages",
        text: "File > Export Messages... (Cmd+Shift+E on macOS, Ctrl+Shift+E on Windows and Linux) opens the Export Messages window. By default it saves sent and received messages, queries, and bulletins to a CSV file, oldest first, hidden ones included. It can narrow the file to sent or received, messages new since the last new-only export, a date range, or some callsigns, and can add positions, weather, and other short-lived packets. Times in the file are in UTC.",
      },
      {
        type: "callout",
        tone: "info",
        title: "Importing messages",
        text: "File > Import Messages... (Cmd+Shift+I on macOS, Ctrl+Shift+I on Windows and Linux) reads an export back in, even one edited by hand. Messages already in your history are skipped, so a second import of the same file changes nothing. A copy that's further along, such as an acked message or a multipart message with its later parts, updates the one you have. Every row is checked first; if any is wrong, nothing is imported and the dialog lists what to fix. Old positions and other short-lived packets bring up a warning before they're stored.",
      },
      {
        type: "callout",
        tone: "info",
        title: "Received Message Filters",
        text: "The filter bar appears above Received Messages. All returns the full list. To Me follows your station callsign, Messages shows every APRS message frame, and Promiscuous Base-Call expands To Me to include your base call without the SSID.",
      },
      {
        type: "callout",
        tone: "info",
        title: "Traffic Log",
        text: "Use Traffic Log when you need a full scrolling history of TX, RX, delivery, and error events. The main activity banner is only a brief summary.",
      },
    ],
  },
  {
    id: "remote",
    title: "Remote Web Control",
    summary: "Drive the app from a browser while the desktop window stays visible but locked.",
    blocks: [
      {
        type: "steps",
        items: [
          "Choose a Web Port in the Remote Web Control section.",
          "Click Enable Remote.",
          "Open the shown local URL in a browser or use Open Remote UI, which launches your system browser in the desktop app.",
          "Disable Remote from the desktop app when you want to restore desktop controls.",
        ],
      },
      {
        type: "callout",
        tone: "warning",
        title: "Desktop lock",
        text: "While remote control is enabled, desktop controls remain visible but are intentionally locked to avoid conflicting commands between the desktop and browser session.",
      },
    ],
  },
  {
    id: "help",
    title: "Help Window",
    summary: "Find the built-in user guide from the app menu or the main window.",
    blocks: [
      {
        type: "list",
        items: [
          "Choose Help > APRSWolf Help from the app menu for the standard app help entry point (the macOS menu bar on Mac, the app menu on Windows and Linux).",
          "Use the search field to jump to setup, transport, beacon, remote-control, or troubleshooting topics.",
          "Use the topic list to move between overview, quick start, preferences, transport setup, and troubleshooting pages.",
          "Keep the Help window open while you work; it does not interrupt the live connection window.",
          "Press Escape to close Help, Preferences, or the About window.",
          "Shortcuts, with Ctrl in place of Cmd on Windows and Linux: Preferences (Cmd+,), Help (Cmd+Shift+/), Export Messages (Cmd+Shift+E), Import Messages (Cmd+Shift+I).",
          "APRSWolf > About APRSWolf shows the version and credits, with links to the project, the release notes, and this Help.",
        ],
      },
    ],
  },
  {
    id: "troubleshooting",
    title: "Troubleshooting",
    summary: "Common issues and the fastest checks to make.",
    blocks: [
      {
        type: "list",
        title: "If something is not working",
        items: [
          "No response from a station or bot: verify Message To, transport, and RF path for your area.",
          "Cannot connect to TNC: confirm the TNC is running and the host/port are correct.",
          "Cannot connect to APRS-IS: check server, port, base callsign, and passcode.",
          "Too much traffic: tighten the APRS-IS filter instead of running unfiltered.",
          "No alert sound: only direct messages addressed to your station trigger the sound.",
        ],
      },
    ],
  },
];