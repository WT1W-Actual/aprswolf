import { type ReceivedItem } from "../receivedBus";

// ── Monotonic ID counter ─────────────────────────────────────
let _nextId = 1;
export function nextId(): number {
  return _nextId++;
}

// ── APRS-IS passcode ────────────────────────────────────────

/**
 * Derive the APRS-IS passcode for a given callsign.
 * This is the standard algorithm used by APRS-IS servers.
 */
export function deriveAprsIsPasscode(callsign: string): string {
  const call = callsign.trim().toUpperCase().split("-")[0];
  if (!call) return "";
  let hash = 0x73e2;
  for (let i = 0; i < call.length; i += 2) {
    hash ^= call.charCodeAt(i) << 8;
    if (i + 1 < call.length) {
      hash ^= call.charCodeAt(i + 1);
    }
  }
  return String(hash & 0x7fff);
}

// ── Input normalisers ───────────────────────────────────────

export function normalizeOutboundCallsignInput(value: string): string {
  return value.toUpperCase();
}

export function normalizeOutboundPathInput(value: string): string {
  return value.toUpperCase();
}

export function normalizeAprsIsFilterInput(value: string): string {
  let trimmed = value.trim();
  if (trimmed.toLowerCase().startsWith("#filter")) {
    trimmed = trimmed.slice(7).trimStart();
  } else if (trimmed.toLowerCase().startsWith("filter")) {
    trimmed = trimmed.slice(6).trimStart();
  }
  return trimmed.replace(/[\r\n]+/g, "").trim();
}

// ── Message body helpers ────────────────────────────────────

export function isAckMessageBody(text: string): boolean {
  // APRS msgid is 1-5 alphanumeric chars — don't match "acknowledge...".
  return /^ack[A-Za-z0-9]{1,5}$/i.test(text.trim());
}

export function isAckOrRejectBody(text: string): boolean {
  return /^(ack|rej)[A-Za-z0-9]{1,5}$/i.test(text.trim());
}

// ── Message payload types ───────────────────────────────────

export type MessagePayload = {
  text: string;
  payload_hex: string;
  bytes: number;
  source: string | null;
  destination: string | null;
  path: string[] | null;
  info_text: string | null;
  protocol_source: string | null;
  protocol_destination: string | null;
  protocol_path: string[] | null;
  protocol_info_text: string | null;
  from: string;
  to?: string | null;
  body: string;
  aprsType: string;
  isBulletin: boolean;
  micEDecoded: boolean;
  summary_source: string;
  summary_destination?: string | null;
  mic_e_text: string | null;
  // Set by the backend once the packet is in the message history.
  uuid?: string;
  seq?: number;
};

export function hasNormalizedMessagePayload(
  payload: unknown,
): payload is MessagePayload {
  if (!payload || typeof payload !== "object") return false;
  const candidate = payload as Partial<MessagePayload>;
  return (
    typeof candidate.text === "string" &&
    typeof candidate.payload_hex === "string" &&
    typeof candidate.bytes === "number" &&
    typeof candidate.from === "string" &&
    typeof candidate.body === "string" &&
    typeof candidate.aprsType === "string" &&
    typeof candidate.isBulletin === "boolean" &&
    typeof candidate.micEDecoded === "boolean" &&
    typeof candidate.summary_source === "string"
  );
}

// ── Callsign / address matching ─────────────────────────────

export function normalizeCall(input: string): string {
  return input.trim().toUpperCase();
}

export function matchesStationAddress(
  item: ReceivedItem,
  station: string,
  allowBaseCallsign: boolean,
): boolean {
  const myCall = normalizeCall(station);
  if (!myCall) return false;
  const myBase = myCall.split("-")[0];
  const to = item.to ? normalizeCall(item.to) : "";
  if (to) {
    const toBase = to.split("-")[0];
    return to === myCall || (allowBaseCallsign && toBase === myBase);
  }
  const bodyUpper = item.body.toUpperCase();
  // Word-boundary matching so "W1AW" doesn't match "W1AWZ" or callsigns
  // embedded inside longer words.
  const escapeRe = (s: string) => s.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  if (new RegExp(`(?<![A-Z0-9])${escapeRe(myCall)}(?![A-Z0-9])`).test(bodyUpper)) {
    return true;
  }
  return (
    allowBaseCallsign &&
    new RegExp(`(?<![A-Z0-9])${escapeRe(myBase)}(?![A-Z0-9])`).test(bodyUpper)
  );
}

// To Me messages that arrived after the read marker, leaving out acks and
// rejects, which answer our own sends rather than ask for attention.
export function countUnreadToMe(
  items: ReceivedItem[],
  station: string,
  allowBaseCallsign: boolean,
  readAt: number,
): number {
  let count = 0;
  for (const item of items) {
    if (item.timestamp <= readAt) continue;
    if (isAckOrRejectBody(item.body)) continue;
    if (matchesStationAddress(item, station, allowBaseCallsign)) count += 1;
  }
  return count;
}

// ── Settings payload builder ─────────────────────────────────

export type SettingsParams = {
  source: string;
  path: string;
  host: string;
  port: string;
  aprsisHost: string;
  aprsisPort: string;
  aprsisPasscode: string;
  aprsisFilter: string;
  beaconLat: string;
  beaconLon: string;
  beaconSymbol: string;
  webServerPort: string;
  webServerAutoStart: boolean;
  webServerToken?: string;
  autoBeaconOnConnect?: boolean;
  autoBeaconIntervalSecs?: number;
  showAckMessages?: boolean;
  playMessageSound?: boolean;
  promiscuousBaseCallsign?: boolean;
  themeMode?: "system" | "light" | "dark";
  msgFontSize?: number;
  shortLivedRetentionDays?: number;
  showAppBadge?: boolean;
};

/**
 * Build a settings payload object suitable for `invoke("save_settings", { settings: ... })`.
 * Converts camelCase params to snake_case keys and applies defaults for optional fields.
 */
export function buildSettingsPayload(params: SettingsParams): Record<string, unknown> {
  return {
    source: params.source,
    path: params.path,
    host: params.host,
    port: params.port,
    aprsis_host: params.aprsisHost,
    aprsis_port: params.aprsisPort,
    aprsis_passcode: params.aprsisPasscode,
    aprsis_filter: params.aprsisFilter,
    beacon_lat: params.beaconLat,
    beacon_lon: params.beaconLon,
    beacon_symbol: params.beaconSymbol,
    web_server_port: params.webServerPort,
    web_server_auto_start: params.webServerAutoStart,
    web_server_token: params.webServerToken,
    auto_beacon_on_connect: params.autoBeaconOnConnect ?? false,
    auto_beacon_interval_secs: params.autoBeaconIntervalSecs ?? 0,
    show_ack_messages: params.showAckMessages ?? false,
    play_message_sound: params.playMessageSound ?? true,
    promiscuous_base_callsign: params.promiscuousBaseCallsign ?? false,
    theme_mode: params.themeMode ?? "system",
    msg_font_size: params.msgFontSize ?? 15,
    // Sent only when the caller sets it, so a save from a window that
    // doesn't show the retention choice can't reset it (the backend merges).
    ...(params.shortLivedRetentionDays !== undefined
      ? { short_lived_retention_days: params.shortLivedRetentionDays }
      : {}),
    ...(params.showAppBadge !== undefined ? { show_app_badge: params.showAppBadge } : {}),
  };
}
