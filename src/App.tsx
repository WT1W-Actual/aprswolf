import { FormEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import "./App.css";
import aprsWolfLogo from "./assets/APRSWolf.png";
import { publishUiLog, subscribeUiLog, type LogKind, type UiLogPayload } from "./logBus";
import {
  buildSettingsPayload,
  deriveAprsIsPasscode,
  hasNormalizedMessagePayload,
  isAckMessageBody,
  matchesStationAddress,
  nextId,
  normalizeAprsIsFilterInput,
  normalizeOutboundCallsignInput,
  normalizeOutboundPathInput,
  type MessagePayload,
} from "./lib/helpers";
import { type AppSettingsPayload, type ReceivedFilter, type ThemeMode, FILTER_OPTIONS } from "./lib/types";
import {
  isReceivedWindowOpen,
  publishReceivedSnapshot,
  type SentItem,
  subscribePopoutFocus,
  subscribeReceivedWindowState,
  type ReceivedItem,
} from "./receivedBus";
import { useToMeUnread } from "./lib/useToMeUnread";
import { setAppBadge } from "./lib/appBadge";
import { useWindowFocused } from "./lib/useWindowFocused";
import UnreadDot from "./UnreadDot";
import HeaderClock from "./HeaderClock";
import { formatReceivedTime, useMessageSearch } from "./lib/messageHistory";
import packageJson from "../package.json";

type StatusPayload = {
  state: string;
  detail?: string | null;
};

type DeliveryPayload = {
  id: number;
  uuid?: string;
  status: "queued" | "sent" | "acked" | "failed" | "expired";
  attempts: number;
  detail?: string | null;
};

type WebServerStatusPayload = {
  enabled: boolean;
  port?: number | null;
  url?: string | null;
};

type LogItem = {
  id: number;
  kind: "status" | "rx" | "tx" | "error";
  text: string;
};

type ActivityNotice = {
  kind: LogKind;
  text: string;
};

// Matches the backend's PAGE_SIZE: windows hold one page of the message
// history in memory; older messages stay in the database.
const MAX_RECEIVED_ITEMS = 200;
// Shown as the tooltip on the window's title; it never changes, so it no
// longer takes a panel of its own.
const APP_TAGLINE =
  "Control Center: operate APRSWolf from one place. Use this window for live connections, messaging, beacons, and traffic review. Open Preferences for saved station, transport, and remote-control settings.";

function App() {
  const isTauriRuntime = Boolean((globalThis as { isTauri?: boolean }).isTauri);
  const docsPreviewMode = new URLSearchParams(window.location.search).get("docs") === "1";
  const uiLocked = !isTauriRuntime && !docsPreviewMode;
  const [appVersion, setAppVersion] = useState(`v${packageJson.displayVersion ?? formatDisplayVersion(packageJson.version)}`);

  const [host, setHost] = useState("");
  const [port, setPort] = useState("");
  const [source, setSource] = useState("");
  const [path, setPath] = useState("");
  const [destination, setDestination] = useState("");
  const [message, setMessage] = useState("");
  const [txStatus, setTxStatus] = useState<StatusPayload>({ state: "disconnected" });
  const [rxStatus, setRxStatus] = useState<StatusPayload>({ state: "disconnected" });
  const [isConnected, setIsConnected] = useState(false);
  const [aprsisHost, setAprsisHost] = useState<string>("");
  const [aprsisPort, setAprsisPort] = useState("");
  const [aprsisPasscode, setAprsisPasscode] = useState("");
  const [aprsisFilter, setAprsisFilter] = useState("");
  const [beaconLat, setBeaconLat] = useState("");
  const [beaconLon, setBeaconLon] = useState("");
  const [beaconSymbol, setBeaconSymbol] = useState("");
  const [manualBeaconCooldownSeconds, setManualBeaconCooldownSeconds] = useState(0);
  const [webServerPort, setWebServerPort] = useState("");
  const [webServerAutoStart, setWebServerAutoStart] = useState(false);
  const [webServerEnabled, setWebServerEnabled] = useState(false);
  const [webServerUrl, setWebServerUrl] = useState<string | null>(null);
  const [aprsisConnected, setAprsisConnected] = useState(false);
  const [sentItems, setSentItems] = useState<SentItem[]>([]);
  const [receivedItems, setReceivedItems] = useState<ReceivedItem[]>([]);
  const [activeReceivedFilters, setActiveReceivedFilters] = useState<ReceivedFilter[]>(["all"]);
  const [showAckMessages, setShowAckMessages] = useState(false);
  const [autoBeaconIntervalSecs, setAutoBeaconIntervalSecs] = useState(0);
  const [autoBeaconOnConnect, setAutoBeaconOnConnect] = useState(false);
  const [playMessageSound, setPlayMessageSound] = useState(true);
  const [showAppBadge, setShowAppBadge] = useState(true);
  const [promiscuousBaseCallsign, setPromiscuousBaseCallsign] = useState(false);
  const [themeMode, setThemeMode] = useState<ThemeMode>("system");
  const [msgFontSize, setMsgFontSize] = useState(15);

  const [replyOpen, setReplyOpen] = useState(false);
  const [replyTo, setReplyTo] = useState<string>("");
  const [replyMessage, setReplyMessage] = useState("");
  const [replyContext, setReplyContext] = useState("");
  const [activityNotice, setActivityNotice] = useState<ActivityNotice | null>(null);
  const [receivedWindowOpen, setReceivedWindowOpen] = useState(() => isReceivedWindowOpen());

  const settingsLoaded = useRef(false);
  const skipNextSettingsSave = useRef(false);
  const activityTimerRef = useRef<number | null>(null);

  function formatDisplayVersion(version: string): string {
    if (/^\d+\.\d+\.0$/.test(version)) {
      return version.replace(/\.0$/, "");
    }

    const prereleaseMatch = version.match(/^(\d+)\.(\d+)\.0-([0-9A-Za-z][0-9A-Za-z.]*)$/);
    if (prereleaseMatch) {
      const [, major, minor, prerelease] = prereleaseMatch;
      return `${major}.${minor}${prerelease.replace(/\./g, "")}`;
    }

    return version;
  }

  const applyWebServerStatus = useCallback((status: WebServerStatusPayload) => {
    setWebServerEnabled(status.enabled);
    if (status.port) {
      setWebServerPort(String(status.port));
    }
    setWebServerUrl(status.url ?? null);
  }, []);

  const applySettings = useCallback((s: AppSettingsPayload) => {
    const normalizedSource = normalizeOutboundCallsignInput(s.source ?? "");
    setSource(normalizedSource);
    setPath(normalizeOutboundPathInput(s.path ?? ""));
    setHost(s.host ?? "");
    setPort(s.port ?? "");
    setAprsisHost(s.aprsis_host ?? "");
    setAprsisPort(s.aprsis_port ?? "");
    setAprsisPasscode(s.aprsis_passcode ?? (normalizedSource ? deriveAprsIsPasscode(normalizedSource) : ""));
    setAprsisFilter(s.aprsis_filter ?? "");
    setBeaconLat(s.beacon_lat ?? "");
    setBeaconLon(s.beacon_lon ?? "");
    setBeaconSymbol(s.beacon_symbol ?? "");
    setWebServerPort(s.web_server_port ?? "");
    setWebServerAutoStart(Boolean(s.web_server_auto_start));
    setShowAckMessages(Boolean(s.show_ack_messages));
    setAutoBeaconIntervalSecs(typeof s.auto_beacon_interval_secs === "number" ? s.auto_beacon_interval_secs : 0);
    setAutoBeaconOnConnect(s.auto_beacon_on_connect ?? false);
    setPlayMessageSound(s.play_message_sound ?? true);
    setShowAppBadge(s.show_app_badge ?? true);
    setPromiscuousBaseCallsign(Boolean(s.promiscuous_base_callsign));
    if (s.theme_mode === "light" || s.theme_mode === "dark" || s.theme_mode === "system") {
      setThemeMode(s.theme_mode);
    }
    if (typeof s.msg_font_size === "number" && s.msg_font_size >= 10 && s.msg_font_size <= 32) {
      setMsgFontSize(s.msg_font_size);
    }
  }, []);

  useEffect(() => {
    if (!settingsLoaded.current) return;
    // The initial applySettings() at startup marks one save to skip so we
    // don't immediately write back the exact settings we just read.
    if (skipNextSettingsSave.current) {
      skipNextSettingsSave.current = false;
      return;
    }
    const timer = window.setTimeout(() => {
      invoke("save_settings", {
        settings: buildSettingsPayload({
          source,
          path,
          host,
          port,
          aprsisHost,
          aprsisPort,
          aprsisPasscode,
          aprsisFilter,
          beaconLat,
          beaconLon,
          beaconSymbol,
          webServerPort,
          webServerAutoStart,
          autoBeaconOnConnect,
          autoBeaconIntervalSecs,
          showAckMessages,
          playMessageSound,
          promiscuousBaseCallsign,
          themeMode,
          msgFontSize,
        }),
      }).catch(() => {/* ignore */});
    }, 200);
    return () => window.clearTimeout(timer);
  }, [
    aprsisFilter,
    aprsisHost,
    aprsisPasscode,
    aprsisPort,
    autoBeaconOnConnect,
    autoBeaconIntervalSecs,
    beaconLat,
    beaconLon,
    beaconSymbol,
    host,
    path,
    playMessageSound,
    port,
    promiscuousBaseCallsign,
    showAckMessages,
    source,
    themeMode,
    msgFontSize,
    webServerAutoStart,
    webServerPort,
  ]);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");

    const applyTheme = () => {
      const resolved = themeMode === "system"
        ? (media.matches ? "dark" : "light")
        : themeMode;
      document.documentElement.setAttribute("data-theme", resolved);
    };

    applyTheme();
    if (themeMode !== "system") {
      return;
    }

    const onChange = () => applyTheme();
    media.addEventListener("change", onChange);
    return () => media.removeEventListener("change", onChange);
  }, [themeMode]);

  // Apply message font size as a CSS custom property
  useEffect(() => {
    document.documentElement.style.setProperty("--msg-font-size", `${msgFontSize}px`);
  }, [msgFontSize]);

  useEffect(() => {
    if (!isTauriRuntime) {
      return;
    }

    let active = true;

    (async () => {
      try {
        const { getVersion } = await import("@tauri-apps/api/app");
        const version = await getVersion();
        if (active && version) {
          setAppVersion(`v${formatDisplayVersion(version)}`);
        }
      } catch {
        // Keep the package version fallback in browser preview or test mode.
      }
    })();

    return () => {
      active = false;
    };
  }, [isTauriRuntime]);

  useEffect(() => {
    const unsubscribe = subscribeUiLog((payload: UiLogPayload) => {
      setActivityNotice({ kind: payload.kind, text: payload.text });
      if (activityTimerRef.current !== null) {
        window.clearTimeout(activityTimerRef.current);
      }
      activityTimerRef.current = window.setTimeout(() => {
        setActivityNotice(null);
        activityTimerRef.current = null;
      }, 4500);
    });

    return () => {
      unsubscribe();
      if (activityTimerRef.current !== null) {
        window.clearTimeout(activityTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    return subscribeReceivedWindowState((open) => {
      setReceivedWindowOpen(open);
    });
  }, []);

  // Load persisted settings on startup; clear any leftover queue and received messages
  useEffect(() => {
    (async () => {
      try {
        const s = await invoke<AppSettingsPayload>("load_settings");
        // Mark the save effect to skip the write-back of what we just read,
        // and mark settings as loaded before applying so the skip lands on
        // exactly the applySettings-triggered effect run.
        skipNextSettingsSave.current = true;
        settingsLoaded.current = true;
        applySettings(s);
      } catch {/* use defaults */}
      // Expire old queue entries so each session starts with an empty queue.
      // Also clear the pop-out window state key — if the app crashed while the
      // pop-out was open, the key would be left set and the embedded pane would
      // remain permanently disabled.
      try { window.localStorage.removeItem("aprswolf-received-window-open"); } catch {/* ignore */}
      setReceivedWindowOpen(false);
      // Awaited so the sent history below already shows what this expired.
      await invoke("clear_queue").catch(() => {/* ignore */});
      // Sent messages persist with their last delivery status.
      try {
        const sent = await invoke<SentItem[]>("load_sent_messages");
        if (Array.isArray(sent) && sent.length > 0) {
          setSentItems((prev) => {
            const seen = new Set(prev.map((item) => item.uuid).filter(Boolean));
            const older = sent
              .filter((item) => !item.uuid || !seen.has(item.uuid))
              .map((item) => ({ ...item, id: nextId() }));
            return [...prev, ...older];
          });
        }
      } catch {/* no sent history */}
      // Received messages persist in the message history; show the newest
      // page behind anything that arrived while this was loading.
      try {
        const stored = await invoke<ReceivedItem[]>("load_received_messages");
        if (Array.isArray(stored) && stored.length > 0) {
          setReceivedItems((prev) => {
            const seen = new Set(prev.map((item) => item.uuid).filter(Boolean));
            const older = stored
              .filter((item) => !item.uuid || !seen.has(item.uuid))
              .map((item) => ({ ...item, id: nextId() }));
            return [...prev, ...older].slice(0, MAX_RECEIVED_ITEMS);
          });
        }
      } catch {/* no history yet */}
      if (isTauriRuntime) {
        try {
          const status = await invoke<WebServerStatusPayload>("get_web_server_status");
          applyWebServerStatus(status);
        } catch {/* ignore */}
        try {
          const remaining = await invoke<number>("get_manual_beacon_cooldown_remaining");
          if (Number.isFinite(remaining) && remaining > 0) {
            setManualBeaconCooldownSeconds(Math.floor(remaining));
          }
        } catch {/* ignore */}
      }
      settingsLoaded.current = true;
    })();
  }, [applyWebServerStatus, applySettings, isTauriRuntime]);

  useEffect(() => {
    if (manualBeaconCooldownSeconds <= 0) return;
    const timer = window.setInterval(() => {
      setManualBeaconCooldownSeconds((current) => (current > 0 ? current - 1 : 0));
    }, 1000);
    return () => window.clearInterval(timer);
  }, [manualBeaconCooldownSeconds]);

  useEffect(() => {
    publishReceivedSnapshot(receivedItems);
  }, [receivedItems]);

  const unreadToMe = useToMeUnread(receivedItems, source, promiscuousBaseCallsign);

  // One search box, in the Sent header, filters both panes: while it has
  // text, each lists matches from the whole stored history instead of its
  // live items.
  const [searchQuery, setSearchQuery] = useState("");
  const search = useMessageSearch(searchQuery);
  const sentSearch = useMessageSearch<SentItem>(searchQuery, "search_sent_messages");
  const shownSent = sentSearch.results ?? sentItems;
  const shownReceived = search.results ?? receivedItems;

  // Focus marks messages read (see useToMeUnread), but the badge also hides
  // the moment either window takes focus, before the read marker catches up.
  const mainFocused = useWindowFocused();
  const [popoutFocused, setPopoutFocused] = useState(false);
  useEffect(() => subscribePopoutFocus(setPopoutFocused), []);
  const appFocused = mainFocused || popoutFocused;

  useEffect(() => {
    const count = showAppBadge && !appFocused ? unreadToMe : 0;
    // Moving focus between our own windows blurs one before the other
    // reports focus; wait a beat so the badge doesn't flash.
    const timer = window.setTimeout(() => { void setAppBadge(count); }, 150);
    return () => window.clearTimeout(timer);
  }, [showAppBadge, appFocused, unreadToMe]);

  const connected = isConnected;
  const canSend = isConnected || aprsisConnected;
  const tncConnecting = txStatus.state.endsWith("_connecting") || rxStatus.state.endsWith("_connecting");
  const tncSessionActive = isConnected || tncConnecting;
  const tncBlockedByAprsis = aprsisConnected;
  const aprsisBlockedByTnc = isConnected;
  const controlsLocked = uiLocked || webServerEnabled;

  function resetTncSessionState() {
    const disconnected: StatusPayload = { state: "disconnected" };
    setIsConnected(false);
    setTxStatus(disconnected);
    setRxStatus(disconnected);
  }

  function pushLog(kind: LogItem["kind"], text: string) {
    publishUiLog(kind, text);
    if (import.meta.env.DEV) {
      console.log(`[${kind.toUpperCase()}] ${text}`);
    }
  }

  // For lines built from backend events. The Traffic Log window renders
  // those events from its own listeners, so the flag keeps it from showing
  // each one a second time; the activity notice here still gets them.
  function pushEventLog(kind: LogItem["kind"], text: string) {
    publishUiLog(kind, text, { fromBackendEvent: true });
    if (import.meta.env.DEV) {
      console.log(`[${kind.toUpperCase()}] ${text}`);
    }
  }

  function shouldMergeMultipart(previous: string, incoming: string): boolean {
    const prev = previous.trim();
    const next = incoming.trim();
    if (!prev || !next) return false;
    if (/^ack\d+$/i.test(prev) || /^ack\d+$/i.test(next)) return false;
    if (next.startsWith("mm,")) return true;
    if (next.startsWith("...") || next.startsWith("and ")) return true;
    if (prev.endsWith(",") || prev.endsWith("...")) return true;
    if (prev.endsWith("{EW") || prev.endsWith("{EX")) return true;
    return false;
  }

  useEffect(() => {
    let active = true;

    const setup = async () => {
      const unlistens: Array<() => void> = [];
      // If the effect is torn down while a listen() is still in flight
      // (e.g. StrictMode double-mount), unlisten everything registered so
      // far instead of leaking listeners.
      const track = (unlisten: () => void): boolean => {
        unlistens.push(unlisten);
        if (!active) {
          unlistens.forEach((fn) => fn());
          return false;
        }
        return true;
      };

      const unlistenStatus = await listen<StatusPayload>("tnc_status", (event) => {
        if (!active) return;
        
        // Track TX and RX states separately
        if (event.payload.state.startsWith("tx_")) {
          setTxStatus(event.payload);
        } else if (event.payload.state.startsWith("rx_")) {
          setRxStatus(event.payload);
        } else if (event.payload.state === "disconnected") {
          setTxStatus(event.payload);
          setRxStatus(event.payload);
        }

        if (event.payload.state === "tx_rx_loop_start") {
          setIsConnected(true);
        }
        if (event.payload.state === "disconnected" || event.payload.state === "tx_error" || event.payload.state === "rx_error") {
          setIsConnected(false);
        }
        
        // Handle debug events with special tags
        if (event.payload.state === "tx_debug" || event.payload.state === "rx_debug") {
          return; // suppress packet-level debug noise
        } else if (event.payload.state === "tx_error") {
          pushEventLog("error", `[TX ERROR] ${event.payload.detail}`);
        } else if (event.payload.state === "rx_raw") {
          pushEventLog("status", `[RX RAW] ${event.payload.detail}`);
        } else if (event.payload.state === "rx_parsed") {
          pushEventLog("status", `[RX PARSED] ${event.payload.detail}`);
        } else if (event.payload.state === "rx_parse_error") {
          pushEventLog("error", `[RX PARSE ERROR] ${event.payload.detail}`);
        } else {
          const detail = event.payload.detail ? ` (${event.payload.detail})` : "";
          pushEventLog("status", `status: ${event.payload.state}${detail}`);
        }
      });
      if (!track(unlistenStatus)) return () => {};

      const unlistenMessage = await listen<MessagePayload>("tnc_message", (event) => {
        if (!active) return;
        if (!hasNormalizedMessagePayload(event.payload)) {
          pushEventLog("error", "[RX ERROR] malformed receive payload dropped");
          return;
        }

        const summarySource = event.payload.summary_source;
        const summaryDestination = event.payload.summary_destination ?? event.payload.protocol_destination ?? event.payload.destination;
        const summaryPath = event.payload.path;
        const path = summaryPath?.length ? ` via ${summaryPath.join(",")}` : "";
        const normalizedBody = event.payload.body;
        const frameType = event.payload.aprsType;
        const bulletin = event.payload.isBulletin;
        const addressee = event.payload.to ?? undefined;

        // ACKs are always stored; the render-time filter (filteredReceived)
        // decides whether they're visible, so toggling "show ACKs" never
        // loses messages.
        const summary = summarySource && summaryDestination
          ? `${summarySource} > ${summaryDestination}${path}: ${normalizedBody}`
          : event.payload.text;
        const dup = event.payload.duplicate ? " (duplicate)" : "";
        pushEventLog("rx", `RX ${event.payload.bytes}B ${summary}${dup} [${event.payload.payload_hex}]`);
        if (event.payload.duplicate) return;
        setReceivedItems((prev) => {
          const now = Date.now();
          const from = event.payload.from;
          const latest = prev[0];

          if (
            latest &&
            latest.from === from &&
            latest.to === addressee &&
            now - latest.timestamp < 30_000 &&
            shouldMergeMultipart(latest.body, normalizedBody)
          ) {
            return [
              {
                ...latest,
                timestamp: now,
                body: `${latest.body} ${normalizedBody}`,
                aprsType: frameType,
                isBulletin: bulletin,
                micEDecoded: latest.micEDecoded || event.payload.micEDecoded,
              },
              ...prev.slice(1),
            ].slice(0, MAX_RECEIVED_ITEMS);
          }

          return [
            {
              id: nextId(),
              uuid: event.payload.uuid,
              seq: event.payload.seq,
              timestamp: now,
              from,
              to: addressee,
              body: normalizedBody,
              aprsType: frameType,
              isBulletin: bulletin,
              micEDecoded: event.payload.micEDecoded,
            },
            ...prev,
          ].slice(0, MAX_RECEIVED_ITEMS);
        });

      });
      if (!track(unlistenMessage)) return () => {};

      const unlistenDelivery = await listen<DeliveryPayload>("tnc_delivery", (event) => {
        if (!active) return;
        const detail = event.payload.detail ? ` (${event.payload.detail})` : "";
        const kind = event.payload.status === "failed" || event.payload.status === "expired" ? "error" : "status";
        pushEventLog(
          kind,
          `delivery #${event.payload.id} ${event.payload.status} attempts=${event.payload.attempts}${detail}`,
        );
        // Match on the history UUID, or on the queue number for a message
        // the store never recorded.
        const { uuid, id, status } = event.payload;
        setSentItems((prev) =>
          prev.map((item) =>
            (uuid ? item.uuid === uuid : item.queueId === id) ? { ...item, status } : item,
          ),
        );
      });
      if (!track(unlistenDelivery)) return () => {};

      const unlistenAprsIs = await listen<StatusPayload>("aprsis_status", (event) => {
        if (!active) return;
        if (event.payload.state === "aprsis_connected") {
          setAprsisConnected(true);
        } else if (event.payload.state === "aprsis_disconnected") {
          setAprsisConnected(false);
        }
        const detail = event.payload.detail ? ` (${event.payload.detail})` : "";
        const kind = event.payload.state === "aprsis_error" ? "error" : "status";
        if (event.payload.state !== "aprsis_debug") {
          pushEventLog(kind, `aprsis: ${event.payload.state}${detail}`);
        }
      });
      if (!track(unlistenAprsIs)) return () => {};

      const unlistenWebServer = await listen<WebServerStatusPayload>("web_server_status", (event) => {
        if (!active) return;
        applyWebServerStatus(event.payload);
      });
      if (!track(unlistenWebServer)) return () => {};

      const unlistenSettings = await listen<AppSettingsPayload>("settings_updated", (event) => {
        if (!active) return;
        applySettings(event.payload);
      });
      if (!track(unlistenSettings)) return () => {};

      return () => {
        unlistens.forEach((fn) => fn());
      };
    };

    let cleanup: (() => void) | undefined;
    setup().then((fn) => {
      cleanup = fn;
    });

    return () => {
      active = false;
      cleanup?.();
    };
  }, [applyWebServerStatus, applySettings]);

  async function connect() {
    const portNumber = Number(port);
    if (!Number.isInteger(portNumber) || portNumber <= 0 || portNumber > 65535) {
      pushLog("error", `connect failed: invalid port \"${port}\"`);
      return;
    }

    try {
      const sourceCallsign = source.trim().toUpperCase();
      await invoke("connect_tnc", {
        callsign: sourceCallsign,
        host: host.trim(),
        port: portNumber,
      });
    } catch (error) {
      setIsConnected(false);
      pushLog("error", `connect failed: ${String(error)}`);
    }
  }

  async function disconnect() {
    try {
      await invoke("disconnect_tnc");
      resetTncSessionState();
      setManualBeaconCooldownSeconds(0);
    } catch (error) {
      const text = String(error);
      if (text.toLowerCase().includes("no active tnc connection")) {
        resetTncSessionState();
        setManualBeaconCooldownSeconds(0);
        pushLog("status", "already disconnected");
        return;
      }
      pushLog("error", `disconnect failed: ${text}`);
    }
  }

  async function connectAprsIs() {
    const portNumber = Number(aprsisPort);
    if (!Number.isInteger(portNumber) || portNumber <= 0 || portNumber > 65535) {
      pushLog("error", `APRS-IS connect failed: invalid port "${aprsisPort}"`);
      return;
    }

    try {
      const sourceCallsign = source.trim().toUpperCase();
      const normalizedFilter = normalizeAprsIsFilterInput(aprsisFilter);
      if (normalizedFilter !== aprsisFilter) {
        setAprsisFilter(normalizedFilter);
      }
      await invoke("connect_aprsis", {
        host: aprsisHost.trim(),
        port: portNumber,
        callsign: sourceCallsign,
        passcode: aprsisPasscode.trim(),
        filter: normalizedFilter,
      });
    } catch (error) {
      pushLog("error", `APRS-IS connect failed: ${String(error)}`);
    }
  }

  async function disconnectAprsIs() {
    try {
      await invoke("disconnect_aprsis");
      setManualBeaconCooldownSeconds(0);
    } catch (error) {
      pushLog("error", `APRS-IS disconnect failed: ${String(error)}`);
    }
  }

  async function sendBeacon() {
    if (manualBeaconCooldownSeconds > 0) {
      pushLog("status", `Beacon cooldown active: ${manualBeaconCooldownSeconds}s remaining`);
      return;
    }

    const lat = parseFloat(beaconLat);
    const lon = parseFloat(beaconLon);
    if (isNaN(lat) || isNaN(lon)) {
      pushLog("error", "Beacon failed: invalid latitude or longitude");
      return;
    }
    if (lat < -90 || lat > 90 || lon < -180 || lon > 180) {
      pushLog("error", "Beacon failed: latitude/longitude out of range (±90 / ±180)");
      return;
    }

    const transport = connected ? "TNC" : aprsisConnected ? "APRS-IS" : "none";
    pushLog("status", `Beacon requested via ${transport}: ${lat.toFixed(4)},${lon.toFixed(4)} sym=${beaconSymbol}`);

    try {
      await invoke("send_beacon", {
        source: source.trim().toUpperCase(),
        path: path.trim().toUpperCase(),
        lat,
        lon,
        symbol: beaconSymbol,
        comment: "",
      });
      pushLog("tx", `Beacon sent via ${transport}: ${lat.toFixed(4)},${lon.toFixed(4)} sym=${beaconSymbol}`);
      setManualBeaconCooldownSeconds(180);
    } catch (error) {
      const text = String(error);
      const cooldown = text.match(/Wait\s+(\d+)\s+second/i);
      if (cooldown?.[1]) {
        setManualBeaconCooldownSeconds(Number(cooldown[1]));
      }
      pushLog("error", `beacon failed: ${text}`);
    }
  }

  const beaconCooldownLabel = useMemo(() => {
    const remaining = manualBeaconCooldownSeconds;
    if (remaining <= 0) return "";
    const minutes = Math.floor(remaining / 60);
    const seconds = remaining % 60;
    return `Reset in ${minutes}:${String(seconds).padStart(2, "0")}`;
  }, [manualBeaconCooldownSeconds]);

  async function send(e: FormEvent) {
    e.preventDefault();

    const destinationCallsign = destination.trim().toUpperCase();
    if (!destinationCallsign || !message.trim()) {
      pushLog("error", "send failed: destination and message are required");
      return;
    }

    try {
      const sourceCallsign = source.trim().toUpperCase();
      const digiPath = path.trim().toUpperCase();
      const result = await invoke<{ id: number; uuid?: string; status: string; bytes: number; payload_hex: string }>("send_message", {
        source: sourceCallsign,
        destination: destinationCallsign,
        path: digiPath,
        message,
      });
      pushLog(
        "tx",
        `TX queued #${result.id} ${result.bytes}B to ${destinationCallsign} [${result.payload_hex}] status=${result.status}`,
      );
      setSentItems((prev) => [
        {
          id: nextId(),
          queueId: result.id,
          uuid: result.uuid,
          timestamp: Date.now(),
          destination: destinationCallsign,
          body: message,
          status: result.status as SentItem["status"],
        },
        ...prev,
      ]);
      setMessage("");
    } catch (error) {
      pushLog("error", `send failed: ${String(error)}`);
    }
  }

  function openReplyDialog(item: ReceivedItem) {
    const preview = item.body.replace(/\s+/g, " ").trim().slice(0, 60);
    setReplyTo(normalizeOutboundCallsignInput(item.from));
    setReplyMessage(preview ? `RE: ${preview}` : "");
    setReplyContext(item.body);
    setReplyOpen(true);
  }

  function closeReplyDialog() {
    setReplyOpen(false);
    setReplyTo("");
    setReplyMessage("");
    setReplyContext("");
  }

  async function sendReply(e: FormEvent) {
    e.preventDefault();

    const destinationCallsign = replyTo.trim().toUpperCase();
    if (!destinationCallsign || !replyMessage.trim()) {
      pushLog("error", "send failed: destination and message are required");
      return;
    }

    try {
      const sourceCallsign = source.trim().toUpperCase();
      const digiPath = path.trim().toUpperCase();
      const result = await invoke<{ id: number; uuid?: string; status: string; bytes: number; payload_hex: string }>("send_message", {
        source: sourceCallsign,
        destination: destinationCallsign,
        path: digiPath,
        message: replyMessage,
      });

      pushLog(
        "tx",
        `TX queued #${result.id} ${result.bytes}B to ${destinationCallsign} [${result.payload_hex}] status=${result.status}`,
      );
      setSentItems((prev) => [
        {
          id: nextId(),
          queueId: result.id,
          uuid: result.uuid,
          timestamp: Date.now(),
          destination: destinationCallsign,
          body: replyMessage,
          status: result.status as SentItem["status"],
        },
        ...prev,
      ]);
      closeReplyDialog();
    } catch (error) {
      pushLog("error", `send failed: ${String(error)}`);
    }
  }

  async function clearQueue() {
    try {
      const count = await invoke<number>("clear_queue");
      pushLog("status", `queue cleared: ${count} message(s) expired`);
    } catch (error) {
      pushLog("error", `clear queue failed: ${String(error)}`);
    }
  }

  async function dumpQueue() {
    try {
      const snapshot = await invoke<Array<{ id: number; status: string; attempts: number; source: string; destination: string; body: string; ax25_hex: string; next_retry_at_ms: number | null }>>("inspect_queue");
      if (snapshot.length === 0) {
        pushLog("status", "queue is empty");
      } else {
        pushLog("status", `=== Queue Snapshot (${snapshot.length} message(s)) ===`);
        snapshot.forEach((msg) => {
          pushLog(
            "status",
            `[${msg.id}] status=${msg.status} attempts=${msg.attempts} src=${msg.source} dst=${msg.destination} body="${msg.body}" ax25_len=${msg.ax25_hex.length / 2}B`
          );
        });
      }
    } catch (error) {
      pushLog("error", `dump queue failed: ${String(error)}`);
    }
  }

  async function openReceivedWindow() {
    try {
      const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
      const existing = await WebviewWindow.getByLabel("received-messages");
      if (existing) {
        await existing.show();
        await existing.setFocus();
        setReceivedWindowOpen(true);
        return;
      }
      const win = new WebviewWindow("received-messages", {
        url: "/#received",
        title: "APRSWolf — Received Messages",
        width: 680,
        height: 560,
        minWidth: 460,
        minHeight: 300,
        resizable: true,
      });
      win.once("tauri://error", (e) => {
        pushLog("error", `received messages window failed: ${String(e)}`);
        window.open("/#received", "received-messages", "width=680,height=560");
      });
    } catch {
      window.open("/#received", "received-messages", "width=680,height=560");
    }
  }

  async function openPreferences() {
    try {
      await invoke("open_preferences");
    } catch (e) {
      pushLog("error", `preferences window failed: ${String(e)}`);
    }
  }

  async function openLogWindow() {
    try {
      const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
      const existing = await WebviewWindow.getByLabel("traffic-log");
      if (existing) {
        await existing.show();
        await existing.setFocus();
        return;
      }
      const win = new WebviewWindow("traffic-log", {
        url: "/#log",
        title: "APRSWolf — Traffic Log",
        width: 760,
        height: 520,
        minWidth: 520,
        minHeight: 320,
      });
      win.once("tauri://error", (e) => {
        pushLog("error", `traffic log window failed: ${String(e)}`);
        // fallback: open in current tab
        window.open("/#log", "traffic-log", "width=760,height=520");
      });
    } catch {
      // web mode fallback
      window.open("/#log", "traffic-log", "width=760,height=520");
    }
  }

  async function enableWebServer() {
    const portNumber = Number(webServerPort);
    if (!Number.isInteger(portNumber) || portNumber <= 0 || portNumber > 65535) {
      pushLog("error", `web server failed: invalid port \"${webServerPort}\"`);
      return;
    }

    try {
      await invoke("save_settings", {
        settings: buildSettingsPayload({
          source,
          path,
          host,
          port,
          aprsisHost,
          aprsisPort,
          aprsisPasscode,
          aprsisFilter,
          beaconLat,
          beaconLon,
          beaconSymbol,
          webServerPort,
          webServerAutoStart,
          autoBeaconOnConnect,
          autoBeaconIntervalSecs,
          showAckMessages,
          playMessageSound,
          promiscuousBaseCallsign,
          themeMode,
          msgFontSize,
        }),
      });

      const status = await invoke<WebServerStatusPayload>("enable_web_server", { port: portNumber });
      applyWebServerStatus(status);
      pushLog("status", `remote control enabled on ${status.url ?? `port ${portNumber}`}`);
    } catch (error) {
      pushLog("error", `web server failed: ${String(error)}`);
    }
  }

  async function disableWebServer() {
    try {
      await invoke("disable_web_server");
      applyWebServerStatus({ enabled: false, port: null, url: null });
      pushLog("status", "remote control disabled");
    } catch (error) {
      pushLog("error", `web server disable failed: ${String(error)}`);
    }
  }

  async function openRemoteWebUi() {
    if (!webServerUrl) return;
    // The desktop webview ignores window.open with _blank, so hand the URL
    // to the system browser instead. Browser preview keeps window.open.
    if (isTauriRuntime) {
      try {
        await openUrl(webServerUrl);
      } catch (error) {
        pushLog("error", `open remote UI failed: ${String(error)}`);
      }
      return;
    }
    window.open(webServerUrl, "_blank", "noopener,noreferrer");
  }

  const filteredReceived = useMemo(() => {
    const ackFiltered = showAckMessages
      ? shownReceived
      : shownReceived.filter((item) => !isAckMessageBody(item.body));

    if (activeReceivedFilters.length === 0 || activeReceivedFilters.includes("all")) {
      return ackFiltered;
    }

    return ackFiltered.filter((item) => {
      const kind = (item.aprsType ?? "other") as ReceivedFilter;
      return activeReceivedFilters.some((filter) => {
        if (filter === "to_me") return matchesStationAddress(item, source, promiscuousBaseCallsign);
        if (filter === "bulletin") return item.isBulletin === true;
        return kind === filter;
      });
    });
  }, [activeReceivedFilters, promiscuousBaseCallsign, shownReceived, showAckMessages, source]);

  function toggleReceivedFilter(key: ReceivedFilter) {
    setActiveReceivedFilters((current) => {
      if (key === "all") {
        return ["all"];
      }

      const next = new Set(current.filter((k) => k !== "all"));
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }

      return next.size > 0 ? Array.from(next) : ["all"];
    });
  }

  const filterCounts = useMemo(() => {
    const counts: Record<ReceivedFilter, number> = {
      all: shownReceived.length,
      to_me: 0,
      bulletin: 0,
      qry: 0,
      msg: 0,
      wx: 0,
      pos: 0,
      status: 0,
      obj: 0,
      item: 0,
      "mic-e": 0,
      tlm: 0,
      other: 0,
    };

    for (const item of shownReceived) {
      const kind = (item.aprsType ?? "other") as ReceivedFilter;
      if (kind in counts && kind !== "all" && kind !== "to_me" && kind !== "bulletin") {
        counts[kind] += 1;
      } else if (!(kind in counts)) {
        counts.other += 1;
      }

      if (item.isBulletin) {
        counts.bulletin += 1;
      }

      if (matchesStationAddress(item, source, promiscuousBaseCallsign)) {
        counts.to_me += 1;
      }
    }

    return counts;
  }, [promiscuousBaseCallsign, shownReceived, source]);

  const beaconConfigured = Boolean(beaconLat.trim() && beaconLon.trim() && beaconSymbol.trim());
  const tncConfigured = Boolean(source.trim() && host.trim() && port.trim());
  const aprsisConfigured = Boolean(source.trim() && aprsisHost.trim() && aprsisPort.trim() && aprsisPasscode.trim());
  const remoteSummary = webServerPort.trim()
    ? `port ${webServerPort.trim()} · auto-start ${webServerAutoStart ? "on" : "off"}`
    : "no port set";
  const needsPreferencesSetup = !source.trim() || (!tncConfigured && !aprsisConfigured);
  const dashboardStatus = aprsisConnected
    ? "APRS-IS session is active."
    : connected
      ? "TNC session is active."
      : needsPreferencesSetup
        ? "Finish setup in Preferences before connecting."
        : "Choose a transport and connect when ready.";
  return (
    <main className="app">
      <header className="app-header">
        <img src={aprsWolfLogo} alt="APRSWolf logo" className="app-logo" />
        <h1 title={APP_TAGLINE}>APRSWolf</h1>
        <span className="app-version" aria-label={`Application version ${appVersion}`}>
          {appVersion}
        </span>
        <p className="header-status" aria-live="polite">{dashboardStatus}</p>
        <HeaderClock />
        <button type="button" className="log-window-btn" onClick={openLogWindow}>Traffic Log ↗</button>
        <button
          type="button"
          className="settings-btn"
          onClick={openPreferences}
          title="Preferences"
          aria-label="Open Preferences"
          disabled={!isTauriRuntime}
        >
          ⚙︎
        </button>
      </header>

      {uiLocked ? (
        <section className="panel runtime-warning" aria-live="polite">
          Browser preview mode detected (port 1420). This page is not the desktop runtime,
          so live TNC/APRS-IS state is not synchronized. Use the APRSWolf desktop window to
          control connections.
        </section>
      ) : null}

      {needsPreferencesSetup ? (
        <div className="dashboard-onboarding" aria-live="polite">
          <strong>First run:</strong> Open Preferences to configure your station callsign and at least one connection path before trying to connect.
        </div>
      ) : null}

      <fieldset className="runtime-fieldset" disabled={controlsLocked}>
      <section className="panel compose-panel">
        <form className="compose-bar" onSubmit={send}>
          <label className="compose-to">
            Message To
            <input
              value={destination}
              onChange={(e) => setDestination(normalizeOutboundCallsignInput(e.currentTarget.value))}
              placeholder="WXYO"
            />
          </label>
          <label className="compose-message">
            Message
            <input
              value={message}
              onChange={(e) => setMessage(e.currentTarget.value)}
              placeholder="Test message"
              maxLength={67}
            />
          </label>
          <button className="send" type="submit" disabled={!canSend}>
            Send
          </button>
        </form>
        {!canSend ? <p className="compose-help">Connect TNC or APRS-IS to enable message sending.</p> : null}
      </section>

      {activityNotice ? (
        <div className={`activity-strip activity-${activityNotice.kind}`} aria-live="polite">
          <span className="activity-dot" />
          <span className="activity-text">{activityNotice.text}</span>
        </div>
      ) : null}

      <section className="panel messages-panel">
        <div className="msg-pane">
          <div className="msg-pane-header">
            <h2>Sent Messages</h2>
            <input
              type="search"
              className="msg-pane-search"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.currentTarget.value)}
              placeholder="Search all messages"
              aria-label="Search message history"
              title="Searches sent and received messages across the whole history"
            />
          </div>
          <div className="msg-list">
            {shownSent.length === 0 ? (
              <p className="empty">{sentSearch.results ? "No sent messages match your search." : "No sent messages."}</p>
            ) : null}
            {shownSent.map((item) => (
              <div key={item.uuid ?? item.id} className={`msg-item sent-item status-${item.status}`}>
                <div className="msg-item-top-row">
                  <span className="msg-time">{formatReceivedTime(item.timestamp)}</span>
                  <span className="msg-addr">▶ {item.destination}</span>
                  {item.hidden ? (
                    <span className="msg-hidden-badge" title="Cleared from the list; kept in the database">hidden</span>
                  ) : null}
                  <span className={`msg-badge ${item.status}`}>{item.status}</span>
                </div>
                <div className="msg-body">{item.body}</div>
              </div>
            ))}
            {sentSearch.results && sentSearch.hasMore ? (
              <button type="button" className="secondary received-window-load-older" onClick={() => void sentSearch.loadMore()}>
                Load older matches
              </button>
            ) : null}
          </div>
        </div>
        <div className="msg-pane">
          <div className="msg-pane-header">
            <h2>Received Messages</h2>
            <button type="button" className="log-window-btn" onClick={openReceivedWindow}>
              {receivedWindowOpen ? "Focus Pop-Out ↗" : "Pop Out ↗"}
            </button>
          </div>
          {receivedWindowOpen ? (
            <div className="msg-popout-disabled" aria-live="polite">
              <p className="msg-popout-disabled-copy">
                Received messages are active in the popped-out window. This embedded pane is disabled until that window closes.
              </p>
              <button type="button" className="log-window-btn" onClick={openReceivedWindow}>
                Bring Pop-Out Forward
              </button>
            </div>
          ) : (
            <>
              <div className="rx-filter-meta">
                <label className="inline-toggle rx-filter-toggle">
                  <input
                    type="checkbox"
                    checked={promiscuousBaseCallsign}
                    onChange={(event) => setPromiscuousBaseCallsign(event.currentTarget.checked)}
                  />
                  Promiscuous Base-Call
                </label>
              </div>
              <div
                className="rx-filter-bar"
                role="group"
                aria-label="Received message filters"
                title={'Select one or more filters. "Messages" shows all APRS message frames. "To Me" only shows traffic addressed to your station.'}
              >
                {FILTER_OPTIONS.map(({ key, label }) => (
                  <button
                    key={key}
                    type="button"
                    className={`rx-filter-btn ${activeReceivedFilters.includes(key) ? "active" : ""}`}
                    onClick={() => toggleReceivedFilter(key)}
                    aria-pressed={activeReceivedFilters.includes(key)}
                  >
                    {label} <span className="rx-filter-count">{filterCounts[key]}</span>
                    {key === "to_me" ? <UnreadDot count={unreadToMe} /> : null}
                  </button>
                ))}
              </div>
              <div className="msg-list">
                {filteredReceived.length === 0 ? (
                  <p className="empty">
                    {search.results
                      ? "No messages match your search."
                      : activeReceivedFilters.includes("to_me") && activeReceivedFilters.length === 1
                        ? "No messages addressed to you."
                        : "No received messages for this filter."}
                  </p>
                ) : null}
                {filteredReceived.map((item) => (
                  <button
                    key={item.uuid ?? item.id}
                    type="button"
                    className="msg-item recv-item recv-clickable"
                    onClick={() => openReplyDialog(item)}
                  >
                    <div className="msg-item-top-row">
                      <span className="msg-time">{formatReceivedTime(item.timestamp)}</span>
                      <span className="msg-addr">◀ {item.from}</span>
                      {item.aprsType && item.aprsType !== "other" ? <span className="msg-type-badge">{item.aprsType}</span> : null}
                      {item.micEDecoded ? <span className="mic-e-inline-badge">Mic-E</span> : null}
                    </div>
                    <div className="msg-body">{item.body}</div>
                  </button>
                ))}
                {search.results && search.hasMore ? (
                  <button type="button" className="secondary received-window-load-older" onClick={() => void search.loadMore()}>
                    Load older matches
                  </button>
                ) : null}
              </div>
            </>
          )}
        </div>
      </section>

      {replyOpen ? (
        <div className="reply-overlay" role="dialog" aria-modal="true" aria-label="Reply to message">
          <div className="reply-dialog">
            <h2>Reply</h2>
            <p className="reply-context">Original: {replyContext}</p>
            <form className="grid" onSubmit={sendReply}>
              <label>
                Reply To
                <input
                  value={replyTo}
                  onChange={(e) => setReplyTo(normalizeOutboundCallsignInput(e.currentTarget.value))}
                  placeholder="Callsign"
                />
              </label>
              <label>
                Message
                <input
                  value={replyMessage}
                  onChange={(e) => setReplyMessage(e.currentTarget.value)}
                  placeholder="Reply text"
                  maxLength={67}
                />
              </label>
              <div className="reply-actions">
                <button type="submit" disabled={!canSend}>Send Reply</button>
                <button type="button" className="secondary" onClick={closeReplyDialog}>Cancel</button>
              </div>
            </form>
          </div>
        </div>
      ) : null}
      </fieldset>

      <section className="panel control-box" aria-label="Connections and remote control">
        <div className="config-group">
          <div className="config-head">
            <h2 className="section-title">
              <StatusDot on={isConnected || aprsisConnected} label={isConnected || aprsisConnected ? "Station on the air" : "Station off the air"} />
              Station
            </h2>
            <span className="config-meta">{source.trim() || "no callsign set"}</span>
          </div>
          <div className="config-buttons">
            <button type="button" onClick={sendBeacon} disabled={controlsLocked || !canSend || !beaconConfigured || manualBeaconCooldownSeconds > 0}>
              Beacon
            </button>
            {manualBeaconCooldownSeconds > 0 ? (
              <span className="beacon-cooldown" aria-live="polite">{beaconCooldownLabel}</span>
            ) : null}
          </div>
        </div>
        <div className="config-sep" />
        <div className="config-group">
          <div className="config-head">
            <h2 className="section-title" title={TNC_TIP}>
              <StatusDot on={isConnected} label={isConnected ? "TNC connected" : "TNC disconnected"} />
              TNC (Dire Wolf)
            </h2>
            <span className="config-meta">{host.trim() && port.trim() ? `${host.trim()}:${port.trim()}` : "not set"}</span>
          </div>
          <div className="config-buttons">
            <button type="button" onClick={connect} disabled={controlsLocked || !tncConfigured || tncSessionActive || tncBlockedByAprsis}>Connect TNC</button>
            <button type="button" onClick={disconnect} disabled={controlsLocked || !tncSessionActive}>Disconnect</button>
            <button type="button" onClick={clearQueue} disabled={controlsLocked}>Clear Q</button>
            <button type="button" onClick={dumpQueue} disabled={controlsLocked}>Dump Q</button>
          </div>
        </div>
        <div className="config-sep" />
        <div className="config-group">
          <div className="config-head">
            <h2 className="section-title" title={APRSIS_TIP}>
              <StatusDot on={aprsisConnected} label={aprsisConnected ? "APRS-IS connected" : "APRS-IS disconnected"} />
              APRS-IS
            </h2>
            <span className="config-meta">{aprsisHost.trim() && aprsisPort.trim() ? `${aprsisHost.trim()}:${aprsisPort.trim()}` : "not set"}</span>
          </div>
          <div className="config-buttons">
            <button type="button" onClick={connectAprsIs} disabled={controlsLocked || !aprsisConfigured || aprsisConnected || aprsisBlockedByTnc}>Connect APRS-IS</button>
            <button type="button" onClick={disconnectAprsIs} disabled={controlsLocked || !aprsisConnected}>Disconnect</button>
          </div>
        </div>
        <div className="config-sep" />
        <div
          className="config-group remote-group"
          title="Runs a control page you can open in a browser. While it is on, the desktop controls stay visible but locked until you disable remote control here or from the browser page."
        >
          <div className="config-head">
            <h2 className="section-title">
              <StatusDot on={webServerEnabled} label={webServerEnabled ? "Remote control on" : "Remote control off"} />
              Remote Web Control
            </h2>
            {webServerEnabled && webServerUrl ? (
              <a className="config-meta" href={webServerUrl} onClick={(e) => { e.preventDefault(); openRemoteWebUi(); }}>{webServerUrl}</a>
            ) : (
              <span className="config-meta">{remoteSummary}</span>
            )}
          </div>
          <div className="config-buttons">
            <button type="button" onClick={enableWebServer} disabled={uiLocked || webServerEnabled || !webServerPort.trim()}>Enable Remote</button>
            <button type="button" className="secondary" onClick={disableWebServer} disabled={uiLocked || !webServerEnabled}>Disable Remote</button>
            <button type="button" className="secondary" onClick={openRemoteWebUi} disabled={!webServerEnabled || !webServerUrl}>Open Remote UI</button>
          </div>
          {webServerEnabled ? <p className="web-server-lock-note">Desktop controls are locked while remote control is enabled.</p> : null}
        </div>
      </section>
    </main>
  );
}

const TNC_TIP =
  "TNC and APRS-IS are mutually exclusive: connect one at a time. TNC sends go out over RF and use the WIDE digi path.";
const APRSIS_TIP =
  "TNC and APRS-IS are mutually exclusive: connect one at a time. Direct APRS-IS sends ignore the RF digi path; WIDE paths are only used on TNC/RF sends.";

function StatusDot({ on, label }: { on: boolean; label: string }) {
  // Hidden from the accessible name so headings read as their titles; the
  // hover tooltip carries the state.
  return <span className={`status-dot ${on ? "on" : "off"}`} aria-hidden="true" title={label} />;
}

export default App;
