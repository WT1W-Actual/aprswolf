import { FormEvent, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import { type AppSettingsPayload } from "./lib/types";

type Direction = "both" | "received" | "sent";

export type ExportFilter = {
  direction: Direction;
  newOnly: boolean;
  fromMs: number | null;
  toMs: number | null;
  callsigns: string[];
  includeShortLived: boolean;
};

/** Local midnight at the start of a `YYYY-MM-DD` date input value. */
function startOfLocalDay(value: string): number | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  return new Date(Number(match[1]), Number(match[2]) - 1, Number(match[3])).getTime();
}

export function buildExportFilter(form: {
  direction: Direction;
  newOnly: boolean;
  fromDate: string;
  toDate: string;
  callsigns: string;
  includeShortLived: boolean;
}): ExportFilter {
  const toStart = startOfLocalDay(form.toDate);
  return {
    direction: form.direction,
    newOnly: form.newOnly,
    fromMs: startOfLocalDay(form.fromDate),
    // The To date is inclusive: stop at the start of the day after it.
    toMs: toStart === null ? null : new Date(toStart).setDate(new Date(toStart).getDate() + 1),
    callsigns: form.callsigns
      .split(/[\s,]+/)
      .map((call) => call.trim().toUpperCase())
      .filter(Boolean),
    includeShortLived: form.includeShortLived,
  };
}

const pad = (n: number) => String(n).padStart(2, "0");

function localDateStamp(date: Date): string {
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}

/** `aprswolf-messages-YYYY-MM-DD-hhmmss.csv` in local time, so several
 * exports on one day don't collide. */
export function exportFileName(now: Date): string {
  const time = `${pad(now.getHours())}${pad(now.getMinutes())}${pad(now.getSeconds())}`;
  return `aprswolf-messages-${localDateStamp(now)}-${time}.csv`;
}

function shortLivedWarning(retentionDays: number): string {
  const kinds = "Positions, weather, telemetry, status, objects, items, Mic-E, and acks";
  if (retentionDays === 0) {
    return `${kinds} are kept until you clear them (Preferences > Message History), so the export can be large.`;
  }
  const span = retentionDays === 1 ? "1 day" : `${retentionDays} days`;
  return `${kinds} are removed after ${span} (Preferences > Message History), so the export holds only the ones still stored.`;
}

function ExportWindow() {
  const [direction, setDirection] = useState<Direction>("both");
  const [newOnly, setNewOnly] = useState(false);
  const [limitDates, setLimitDates] = useState(false);
  const [fromDate, setFromDate] = useState("");
  const [toDate, setToDate] = useState("");
  const [callsigns, setCallsigns] = useState("");
  const [includeShortLived, setIncludeShortLived] = useState(false);
  const [lastExport, setLastExport] = useState<number | null>(null);
  const [settings, setSettings] = useState<Partial<AppSettingsPayload>>({});
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    invoke<Partial<AppSettingsPayload>>("load_settings").then(setSettings).catch(() => {});
    invoke<number | null>("last_new_only_export").then(setLastExport).catch(() => {});
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const applyTheme = () => {
      const mode = settings.theme_mode ?? "system";
      const resolved = mode === "system" ? (media.matches ? "dark" : "light") : mode;
      document.documentElement.setAttribute("data-theme", resolved);
    };
    applyTheme();
    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [settings.theme_mode]);

  function toggleDates(checked: boolean) {
    setLimitDates(checked);
    if (checked && !fromDate && !toDate) {
      const today = new Date();
      const weekAgo = new Date(today.getFullYear(), today.getMonth(), today.getDate() - 7);
      setFromDate(localDateStamp(weekAgo));
      setToDate(localDateStamp(today));
    }
  }

  async function toggleShortLived(checked: boolean) {
    if (!checked) {
      setIncludeShortLived(false);
      return;
    }
    const text = shortLivedWarning(settings.short_lived_retention_days ?? 7);
    try {
      const { ask } = await import("@tauri-apps/plugin-dialog");
      const ok = await ask(text, {
        title: "Include short-lived packets?",
        kind: "warning",
        okLabel: "Include",
        cancelLabel: "Cancel",
      });
      setIncludeShortLived(ok);
    } catch {
      setIncludeShortLived(window.confirm(text));
    }
  }

  async function closeWindow() {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      await getCurrentWindow().destroy();
    } catch {
      window.close();
    }
  }

  async function runExport(event: FormEvent) {
    event.preventDefault();
    if (limitDates && fromDate && toDate && fromDate > toDate) {
      setStatus("The From date is after the To date.");
      return;
    }
    setBusy(true);
    setStatus("");
    try {
      const { save, message } = await import("@tauri-apps/plugin-dialog");
      const path = await save({
        title: "Export Messages",
        defaultPath: exportFileName(new Date()),
        filters: [{ name: "CSV", extensions: ["csv"] }],
      });
      if (!path) return;
      const filter = buildExportFilter({
        direction,
        newOnly,
        fromDate: limitDates ? fromDate : "",
        toDate: limitDates ? toDate : "",
        callsigns,
        includeShortLived,
      });
      const count = await invoke<number>("export_messages", { path, filter });
      await message(`Exported ${count} message${count === 1 ? "" : "s"} to ${path}.`, {
        title: "Export Messages",
        kind: "info",
      });
      await closeWindow();
    } catch (err) {
      setStatus(`Export failed: ${String(err)}`);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="preferences-window export-window">
      <div className="preferences-window-toolbar">
        <div>
          <div className="help-window-title">Export Messages</div>
          <div className="help-window-subtitle">Save message history as a CSV file</div>
        </div>
        <div className="preferences-window-actions">
          <button type="button" className="secondary" onClick={closeWindow}>
            Cancel
          </button>
          <button type="submit" form="export-form" disabled={busy}>
            Export...
          </button>
        </div>
      </div>

      <form id="export-form" className="export-window-body" onSubmit={runExport}>
        <section className="panel preferences-panel">
          <h2 className="section-title">Messages</h2>
          <div className="export-choice-row" role="radiogroup" aria-label="Messages">
            {(
              [
                ["both", "Sent and received"],
                ["received", "Received only"],
                ["sent", "Sent only"],
              ] as const
            ).map(([value, label]) => (
              <label key={value} className="inline-toggle">
                <input
                  type="radio"
                  name="direction"
                  value={value}
                  checked={direction === value}
                  onChange={() => setDirection(value)}
                />
                {label}
              </label>
            ))}
          </div>
          <label className="inline-toggle">
            <input type="checkbox" checked={newOnly} onChange={(event) => setNewOnly(event.currentTarget.checked)} />
            Only messages new since the last new-only export
          </label>
          <span className="field-hint">
            {lastExport === null
              ? "No new-only export yet, so the first one includes everything."
              : `Last new-only export: ${new Date(lastExport).toLocaleString()}`}
          </span>
          {newOnly && (direction !== "both" || (limitDates && (fromDate || toDate)) || callsigns.trim() !== "") ? (
            <span className="field-hint">
              With other filters on, this export won't count as the last new-only export.
            </span>
          ) : null}
        </section>

        <section className="panel preferences-panel">
          <h2 className="section-title">Dates</h2>
          <label className="inline-toggle">
            <input type="checkbox" checked={limitDates} onChange={(event) => toggleDates(event.currentTarget.checked)} />
            Limit to a date range
          </label>
          {limitDates ? (
            <>
              <div className="preferences-grid preferences-grid-two">
                <label>
                  From
                  <input type="date" value={fromDate} onChange={(event) => setFromDate(event.currentTarget.value)} />
                </label>
                <label>
                  To
                  <input type="date" value={toDate} onChange={(event) => setToDate(event.currentTarget.value)} />
                </label>
              </div>
              <span className="field-hint">Both days are included, in local time.</span>
            </>
          ) : (
            <span className="field-hint">All dates.</span>
          )}
        </section>

        <section className="panel preferences-panel">
          <h2 className="section-title">Callsigns</h2>
          <label>
            Sender or addressee
            <input
              value={callsigns}
              onChange={(event) => setCallsigns(event.currentTarget.value)}
              placeholder="K6TEST, N6TEST-7"
              autoCapitalize="characters"
              spellCheck={false}
            />
          </label>
          <span className="field-hint">Separate with commas. A callsign without an SSID matches all of its SSIDs.</span>
        </section>

        <section className="panel preferences-panel">
          <h2 className="section-title">Packet types</h2>
          <label className="inline-toggle">
            <input
              type="checkbox"
              checked={includeShortLived}
              onChange={(event) => void toggleShortLived(event.currentTarget.checked)}
            />
            Include positions, weather, and other short-lived packets
          </label>
          <span className="field-hint">Off by default. Messages, queries, and bulletins are always included.</span>
        </section>

        {status ? (
          <div className="preferences-save-status preferences-save-error" aria-live="polite">
            {status}
          </div>
        ) : null}
      </form>
    </div>
  );
}

export default ExportWindow;
