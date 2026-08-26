// ── Settings ────────────────────────────────────────────────

export type ThemeMode = "system" | "light" | "dark";

export type AppSettingsPayload = {
  source: string;
  path: string;
  host: string;
  port: string;
  aprsis_host: string;
  aprsis_port: string;
  aprsis_passcode: string;
  aprsis_filter: string;
  beacon_lat: string;
  beacon_lon: string;
  beacon_symbol: string;
  web_server_port: string;
  web_server_auto_start: boolean;
  web_server_token?: string;
  auto_beacon_on_connect?: boolean;
  auto_beacon_interval_secs?: number;
  show_ack_messages?: boolean;
  play_message_sound?: boolean;
  promiscuous_base_callsign?: boolean;
  theme_mode?: ThemeMode;
  msg_font_size?: number;
};

// ── Received / filter types ─────────────────────────────────

export type ReceivedFilter =
  | "all"
  | "to_me"
  | "bulletin"
  | "qry"
  | "msg"
  | "wx"
  | "pos"
  | "status"
  | "obj"
  | "item"
  | "mic-e"
  | "tlm"
  | "other";

export const FILTER_OPTIONS: Array<{ key: ReceivedFilter; label: string }> = [
  { key: "all", label: "All" },
  { key: "to_me", label: "To Me" },
  { key: "bulletin", label: "Bulletins" },
  { key: "qry", label: "Queries" },
  { key: "msg", label: "Messages" },
  { key: "wx", label: "Weather" },
  { key: "pos", label: "Position" },
  { key: "status", label: "Status" },
  { key: "obj", label: "Objects" },
  { key: "item", label: "Items" },
  { key: "mic-e", label: "Mic-E" },
  { key: "tlm", label: "Telemetry" },
  { key: "other", label: "Other" },
];
