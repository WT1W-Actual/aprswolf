import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { makeUiLogPayload, subscribeUiLog, type LogKind, type UiLogPayload } from "./logBus";
import { nextId, hasNormalizedMessagePayload, isAckMessageBody, type MessagePayload } from "./lib/helpers";
import { type AppSettingsPayload } from "./lib/types";
import { useTheme } from "./lib/useTheme";

type LogItem = {
  id: number;
  kind: LogKind;
  text: string;
  time: string;
};

type StatusPayload = {
  state: string;
  detail?: string | null;
};

type DeliveryPayload = {
  id: number;
  status: string;
  attempts: number;
  detail?: string | null;
};

const MAX_LOG_ENTRIES = 2000;

function LogWindow() {
  const [logs, setLogs] = useState<LogItem[]>([]);
  const logsRef = useRef<LogItem[]>([]);
  const bottomRef = useRef<HTMLDivElement>(null);
  const [autoScroll, setAutoScroll] = useState(true);
  const [showAckMessages, setShowAckMessages] = useState(false);
  // Ref mirror so the Tauri listeners below can read the flag without
  // re-subscribing every time it toggles (which dropped events in the gap).
  const showAckRef = useRef(false);
  useEffect(() => {
    showAckRef.current = showAckMessages;
  }, [showAckMessages]);

  // Start from the saved preference (main window and Received window do too).
  useEffect(() => {
    (async () => {
      try {
        const settings = await invoke<AppSettingsPayload>("load_settings");
        if (settings?.show_ack_messages != null) {
          setShowAckMessages(Boolean(settings.show_ack_messages));
        }
      } catch {
        // Browser preview — keep default.
      }
    })();
  }, []);

  useTheme();

  function pushLogItem(payload: UiLogPayload) {
    const item: LogItem = {
      id: nextId(),
      kind: payload.kind,
      text: payload.text,
      time: payload.time,
    };
    logsRef.current = [item, ...logsRef.current].slice(0, MAX_LOG_ENTRIES);
    setLogs([...logsRef.current]);
  }

  function pushLog(kind: LogKind, text: string) {
    pushLogItem(makeUiLogPayload(kind, text));
  }

  // Auto-scroll to bottom (newest entry) when new logs arrive
  useEffect(() => {
    if (autoScroll && bottomRef.current) {
      bottomRef.current.scrollIntoView({ behavior: "auto" });
    }
  }, [logs, autoScroll]);

  useEffect(() => {
    let active = true;
    const unsubscribeUiLog = subscribeUiLog((payload) => {
      if (!active) return;
      pushLogItem(payload);
    });

    const setup = async () => {
      const unlistens: Array<() => void> = [];
      // Unlisten everything registered so far if the effect is torn down
      // while a listen() is still in flight (StrictMode double-mount).
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
        const { state, detail } = event.payload;
        if (state === "tx_debug" || state === "rx_debug") {
          return; // suppress packet-level debug noise
        } else if (state === "tx_error") {
          pushLog("error", `[TX ERROR] ${detail}`);
        } else if (state === "rx_raw") {
          pushLog("status", `[RX RAW] ${detail}`);
        } else if (state === "rx_parsed") {
          pushLog("status", `[RX PARSED] ${detail}`);
        } else if (state === "rx_parse_error") {
          pushLog("error", `[RX PARSE ERROR] ${detail}`);
        } else {
          const extra = detail ? ` (${detail})` : "";
          pushLog("status", `tnc: ${state}${extra}`);
        }
      });
      if (!track(unlistenStatus)) return () => {};

      const unlistenMessage = await listen<MessagePayload>("tnc_message", (event) => {
        if (!active) return;
        if (!hasNormalizedMessagePayload(event.payload)) {
          pushLog("error", "[RX ERROR] malformed receive payload dropped");
          return;
        }

        const {
          source,
          destination,
          path,
          bytes,
          payload_hex,
          text,
          protocol_source,
          protocol_destination,
          body,
          summary_source,
          summary_destination,
        } = event.payload;
        const pathStr = path?.length ? ` via ${path.join(",")}` : "";
        if (!showAckRef.current && isAckMessageBody(body)) {
          return;
        }
        const summary =
          (summary_source ?? protocol_source ?? source) && (summary_destination ?? protocol_destination ?? destination)
            ? `${summary_source ?? protocol_source ?? source} > ${summary_destination ?? protocol_destination ?? destination}${pathStr}: ${body}`
            : text;
        pushLog("rx", `RX ${bytes}B ${summary} [${payload_hex}]`);
      });
      if (!track(unlistenMessage)) return () => {};

      const unlistenDelivery = await listen<DeliveryPayload>("tnc_delivery", (event) => {
        if (!active) return;
        const { id, status, attempts, detail } = event.payload;
        const extra = detail ? ` (${detail})` : "";
        const kind =
          status === "failed" || status === "expired" ? "error" : "status";
        pushLog(kind, `delivery #${id} ${status} attempts=${attempts}${extra}`);
      });
      if (!track(unlistenDelivery)) return () => {};

      const unlistenAprsIs = await listen<StatusPayload>("aprsis_status", (event) => {
        if (!active) return;
        const { state, detail } = event.payload;
        if (state === "aprsis_debug") return; // skip keepalive noise
        const extra = detail ? ` (${detail})` : "";
        const kind = state === "aprsis_error" ? "error" : "status";
        pushLog(kind, `aprsis: ${state}${extra}`);
      });
      if (!track(unlistenAprsIs)) return () => {};

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
      unsubscribeUiLog();
      cleanup?.();
    };
  }, []);

  function clearLog() {
    logsRef.current = [];
    setLogs([]);
  }

  async function closeWindow() {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().destroy();
      return;
    } catch {
      window.close();
    }
  }

  async function saveLog() {
    if (logs.length === 0) return;

    const lines = [...logs]
      .reverse()
      .map((l) => `[${l.time}] [${l.kind.toUpperCase().padEnd(6)}] ${l.text}`)
      .join("\n");
    const fileName = `aprswolf-log-${new Date()
      .toISOString()
      .slice(0, 19)
      .replace(/:/g, "-")}.txt`;

    try {
      const [{ save }, { writeTextFile }] = await Promise.all([
        import("@tauri-apps/plugin-dialog"),
        import("@tauri-apps/plugin-fs"),
      ]);

      const filePath = await save({
        defaultPath: fileName,
        filters: [{ name: "Text", extensions: ["txt"] }],
      });

      if (!filePath) {
        pushLog("status", "save cancelled");
        return;
      }

      await writeTextFile(filePath, lines);
      pushLog("status", `saved traffic log to ${filePath}`);
      return;
    } catch {
      const blob = new Blob([lines], { type: "text/plain;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = fileName;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
      pushLog("status", `downloaded traffic log as ${fileName}`);
    }
  }

  // Detect when the user scrolls up (disable auto-scroll)
  function handleScroll(e: React.UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    const atBottom = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
    setAutoScroll(atBottom);
  }

  const kindLabel: Record<LogItem["kind"], string> = {
    status: "STAT",
    rx: "RX",
    tx: "TX",
    error: "ERR",
  };

  return (
    <div className="log-window">
      <div className="log-window-toolbar">
        <span className="log-window-title">Traffic Log</span>
        <span className="log-window-count">
          {logs.length} {logs.length === 1 ? "entry" : "entries"}
        </span>
        <div className="log-window-controls">
          <label className="log-autoscroll-label">
            <input
              type="checkbox"
              checked={showAckMessages}
              onChange={(e) => setShowAckMessages(e.currentTarget.checked)}
            />
            Show ACKs
          </label>
          <label className="log-autoscroll-label">
            <input
              type="checkbox"
              checked={autoScroll}
              onChange={(e) => setAutoScroll(e.currentTarget.checked)}
            />
            Auto-scroll
          </label>
          <button
            type="button"
            className="log-btn-secondary"
            onClick={clearLog}
            disabled={logs.length === 0}
          >
            Clear
          </button>
          <button
            type="button"
            className="log-btn-primary"
            onClick={saveLog}
            disabled={logs.length === 0}
          >
            Save…
          </button>
          <button
            type="button"
            className="log-btn-secondary"
            onClick={closeWindow}
          >
            Close Window
          </button>
        </div>
      </div>

      <div className="log-entries" onScroll={handleScroll}>
        {logs.length === 0 ? (
          <p className="log-empty">Waiting for events…</p>
        ) : (
          // Render newest-first (logsRef stores newest at index 0)
          [...logs].reverse().map((item) => (
            <p key={item.id} className={`log-entry log-${item.kind}`}>
              <span className="log-entry-time">{item.time}</span>
              <span className={`log-entry-kind log-kind-${item.kind}`}>
                {kindLabel[item.kind]}
              </span>
              <span className="log-entry-text">{item.text}</span>
            </p>
          ))
        )}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}

export default LogWindow;
