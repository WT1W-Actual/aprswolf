import { FormEvent, useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import {
  buildSettingsPayload,
  deriveAprsIsPasscode,
  normalizeAprsIsFilterInput,
  normalizeOutboundCallsignInput,
  normalizeOutboundPathInput,
} from "./lib/helpers";
import { type AppSettingsPayload } from "./lib/types";

const EMPTY_SETTINGS: AppSettingsPayload = {
  source: "",
  path: "",
  host: "",
  port: "",
  aprsis_host: "",
  aprsis_port: "",
  aprsis_passcode: "",
  aprsis_filter: "",
  beacon_lat: "",
  beacon_lon: "",
  beacon_symbol: "",
  web_server_port: "",
  web_server_auto_start: false,
  web_server_token: "",
  auto_beacon_on_connect: true,
  auto_beacon_interval_secs: 1500,
  show_ack_messages: false,
  play_message_sound: true,
  promiscuous_base_callsign: false,
  theme_mode: "system",
  msg_font_size: 15,
};

function PreferencesWindow() {
  const [settings, setSettings] = useState<AppSettingsPayload>(EMPTY_SETTINGS);
  const [saveState, setSaveState] = useState<"idle" | "saved" | "error">("idle");
  const [saveMessage, setSaveMessage] = useState("");
  // Tracks the last passcode this window derived itself, so callsign edits
  // only auto-derive when the user hasn't typed a manual passcode.
  const lastDerivedPasscode = useRef("");

  useEffect(() => {
    let mounted = true;

    (async () => {
      try {
        const loaded = await invoke<AppSettingsPayload>("load_settings");
        if (!mounted) return;
        setSettings({
          ...EMPTY_SETTINGS,
          ...loaded,
          theme_mode:
            loaded?.theme_mode === "light" || loaded?.theme_mode === "dark" || loaded?.theme_mode === "system"
              ? loaded.theme_mode
              : "system",
        });
      } catch {
        if (!mounted) return;
        setSettings(EMPTY_SETTINGS);
      }
    })();

    return () => {
      mounted = false;
    };
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");

    const applyTheme = () => {
      const mode = settings.theme_mode ?? "system";
      const resolved = mode === "system" ? (media.matches ? "dark" : "light") : mode;
      document.documentElement.setAttribute("data-theme", resolved);
    };

    const handleChange = () => applyTheme();

    applyTheme();
    media.addEventListener("change", handleChange);
    return () => media.removeEventListener("change", handleChange);
  }, [settings.theme_mode]);

  function updateSetting<Key extends keyof AppSettingsPayload>(key: Key, value: AppSettingsPayload[Key]) {
    setSettings((current) => ({ ...current, [key]: value }));
    setSaveState("idle");
    setSaveMessage("");
  }

  function handleSourceChange(value: string) {
    const normalized = normalizeOutboundCallsignInput(value);
    setSettings((current) => {
      // Only auto-derive while the passcode is empty or still matches what
      // we derived earlier — never overwrite a manually entered passcode.
      const untouched =
        !current.aprsis_passcode ||
        current.aprsis_passcode === lastDerivedPasscode.current ||
        current.aprsis_passcode === deriveAprsIsPasscode(current.source);
      if (!untouched) {
        return { ...current, source: normalized };
      }
      const derived = deriveAprsIsPasscode(normalized);
      lastDerivedPasscode.current = derived;
      return { ...current, source: normalized, aprsis_passcode: derived };
    });
    setSaveState("idle");
    setSaveMessage("");
  }

  function validateSettings(): string | null {
    const s = settings;

    // Callsign is required.
    if (!s.source.trim()) {
      return "Callsign is required.";
    }

    // Port validation helper.
    function validPort(v: string, name: string): string | null {
      if (!v.trim()) return null; // empty is OK (will use default)
      const n = Number(v);
      if (!Number.isInteger(n) || n < 1 || n > 65535) {
        return `${name} must be a number between 1 and 65535.`;
      }
      return null;
    }

    const portErr = validPort(s.port, "TNC Port");
    if (portErr) return portErr;

    const aprsisPortErr = validPort(s.aprsis_port, "APRS-IS Port");
    if (aprsisPortErr) return aprsisPortErr;

    const webPortErr = validPort(s.web_server_port, "Web Port");
    if (webPortErr) return webPortErr;

    // Beacon coordinate validation.
    if (s.beacon_lat.trim()) {
      const lat = Number(s.beacon_lat);
      if (isNaN(lat) || lat < -90 || lat > 90) {
        return "Beacon latitude must be between -90 and 90.";
      }
    }
    if (s.beacon_lon.trim()) {
      const lon = Number(s.beacon_lon);
      if (isNaN(lon) || lon < -180 || lon > 180) {
        return "Beacon longitude must be between -180 and 180.";
      }
    }

    return null; // all good
  }

  async function savePreferences(event?: FormEvent) {
    event?.preventDefault();

    const validationError = validateSettings();
    if (validationError) {
      setSaveState("error");
      setSaveMessage(validationError);
      return;
    }

    const s = settings;
    const payload = buildSettingsPayload({
      source: normalizeOutboundCallsignInput(s.source),
      path: normalizeOutboundPathInput(s.path),
      host: s.host,
      port: s.port,
      aprsisHost: s.aprsis_host,
      aprsisPort: s.aprsis_port,
      aprsisPasscode: s.aprsis_passcode,
      aprsisFilter: normalizeAprsIsFilterInput(s.aprsis_filter),
      beaconLat: s.beacon_lat,
      beaconLon: s.beacon_lon,
      beaconSymbol: s.beacon_symbol,
      webServerPort: s.web_server_port,
      webServerAutoStart: s.web_server_auto_start,
      webServerToken: s.web_server_token,
      autoBeaconOnConnect: s.auto_beacon_on_connect,
      autoBeaconIntervalSecs: s.auto_beacon_interval_secs,
      showAckMessages: s.show_ack_messages,
      playMessageSound: s.play_message_sound,
      promiscuousBaseCallsign: s.promiscuous_base_callsign,
      themeMode: s.theme_mode,
      msgFontSize: s.msg_font_size,
    });

    try {
      await invoke("save_settings", { settings: payload });
      await closeWindow();
    } catch (error) {
      setSaveState("error");
      setSaveMessage(`Save failed: ${String(error)}`);
    }
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

  return (
    <div className="preferences-window">
      <div className="preferences-window-toolbar">
        <div>
          <div className="help-window-title">Preferences</div>
          <div className="help-window-subtitle">Saved setup and behavior settings</div>
        </div>
        <div className="preferences-window-actions">
          <button type="button" className="secondary" onClick={closeWindow}>Close Window</button>
          <button type="submit" form="preferences-form">Save</button>
        </div>
      </div>

      <form id="preferences-form" className="preferences-window-body" onSubmit={savePreferences}>
        <section className="panel preferences-panel">
          <h2 className="section-title">Station</h2>
          <div className="preferences-grid preferences-grid-three">
            <label>
              Callsign
              <input
                value={settings.source}
                onChange={(event) => handleSourceChange(event.currentTarget.value)}
                placeholder="W1AW-9"
                className="input-call"
              />
            </label>
            <label>
              Digi Path
              <input
                value={settings.path}
                onChange={(event) => updateSetting("path", normalizeOutboundPathInput(event.currentTarget.value))}
                placeholder="WIDE1-1,WIDE2-1"
                className="input-path"
              />
            </label>
            <label>
              Theme
              <select
                value={settings.theme_mode ?? "system"}
                onChange={(event) => updateSetting("theme_mode", event.currentTarget.value as "system" | "light" | "dark")}
                className="input-theme"
              >
                <option value="system">System</option>
                <option value="light">Light</option>
                <option value="dark">Dark</option>
              </select>
            </label>
            <label className="font-size-label">
              Message Font Size
              <div className="font-size-control">
                <input
                  type="range"
                  min="10"
                  max="32"
                  value={settings.msg_font_size ?? 15}
                  onChange={(event) => updateSetting("msg_font_size", Number(event.currentTarget.value))}
                />
                <span className="font-size-value">{settings.msg_font_size ?? 15}px</span>
              </div>
            </label>
          </div>
          <div className="preferences-toggle-row">
            <label className="inline-toggle">
              <input
                type="checkbox"
                checked={settings.play_message_sound ?? true}
                onChange={(event) => updateSetting("play_message_sound", event.currentTarget.checked)}
              />
              Play sound for direct inbound messages
            </label>
            <label className="inline-toggle">
              <input
                type="checkbox"
                checked={Boolean(settings.show_ack_messages)}
                onChange={(event) => updateSetting("show_ack_messages", event.currentTarget.checked)}
              />
              Show ACK messages in received views
            </label>
          </div>
        </section>

        <section className="panel preferences-panel">
          <h2 className="section-title">TNC (Dire Wolf)</h2>
          <div className="preferences-grid preferences-grid-two">
            <label>
              Host
              <input
                value={settings.host}
                onChange={(event) => updateSetting("host", event.currentTarget.value)}
                placeholder="127.0.0.1"
                className="input-host"
              />
            </label>
            <label>
              Port
              <input
                value={settings.port}
                onChange={(event) => updateSetting("port", event.currentTarget.value)}
                placeholder="8001"
                className="input-port"
              />
            </label>
          </div>
        </section>

        <section className="panel preferences-panel">
          <h2 className="section-title">APRS-IS</h2>
          <div className="preferences-grid preferences-grid-four">
            <label>
              Server
              <input
                value={settings.aprsis_host}
                onChange={(event) => updateSetting("aprsis_host", event.currentTarget.value)}
                placeholder="rotate.aprs2.net"
                className="input-host"
              />
            </label>
            <label>
              Port
              <input
                value={settings.aprsis_port}
                onChange={(event) => updateSetting("aprsis_port", event.currentTarget.value)}
                placeholder="14580"
                className="input-port"
              />
            </label>
            <label>
              Pass
              <input
                type="password"
                value={settings.aprsis_passcode}
                onChange={(event) => updateSetting("aprsis_passcode", event.currentTarget.value)}
                placeholder="-1"
                autoComplete="off"
                className="input-pass"
              />
            </label>
            <label>
              Filter
              <input
                value={settings.aprsis_filter}
                onChange={(event) => updateSetting("aprsis_filter", event.currentTarget.value)}
                onBlur={(event) => updateSetting("aprsis_filter", normalizeAprsIsFilterInput(event.currentTarget.value))}
                placeholder="Optional APRS-IS filter"
                className="input-filter"
              />
            </label>
          </div>
          <p className="aprsis-filter-note">Optional. Enter only filter terms, not #filter. Examples: m/50, r/42.36/-71.06/25, t/WT1W.</p>
        </section>

        <section className="panel preferences-panel">
          <h2 className="section-title">Beacon</h2>
          <div className="preferences-grid preferences-grid-three">
            <label>
              Latitude
              <input
                value={settings.beacon_lat}
                onChange={(event) => updateSetting("beacon_lat", event.currentTarget.value)}
                placeholder="37.7583"
                className="input-coord"
              />
            </label>
            <label>
              Longitude
              <input
                value={settings.beacon_lon}
                onChange={(event) => updateSetting("beacon_lon", event.currentTarget.value)}
                placeholder="-122.3908"
                className="input-coord"
              />
            </label>
            <label>
              Symbol
              <input
                value={settings.beacon_symbol}
                onChange={(event) => updateSetting("beacon_symbol", event.currentTarget.value)}
                placeholder="/-"
                className="input-symbol"
                maxLength={2}
              />
            </label>
          </div>
          <div className="preferences-toggle-row">
            <label className="inline-toggle">
              <input
                type="checkbox"
                checked={settings.auto_beacon_on_connect ?? true}
                onChange={(event) => updateSetting("auto_beacon_on_connect", event.currentTarget.checked)}
              />
              Beacon on connect
            </label>
            <label className="inline-toggle">
              Auto-beacon every
              <select
                value={settings.auto_beacon_interval_secs ?? 1500}
                onChange={(event) => updateSetting("auto_beacon_interval_secs", Number(event.currentTarget.value))}
                className="input-interval"
              >
                <option value={0}>Off</option>
                <option value={600}>10 min</option>
                <option value={900}>15 min</option>
                <option value={1200}>20 min</option>
                <option value={1500}>25 min</option>
                <option value={1800}>30 min</option>
                <option value={2700}>45 min</option>
                <option value={3600}>60 min</option>
              </select>
            </label>
          </div>
        </section>

        <section className="panel preferences-panel">
          <h2 className="section-title">Remote Web Control</h2>
          <div className="preferences-grid preferences-grid-two">
            <label>
              Web Port
              <input
                value={settings.web_server_port}
                onChange={(event) => updateSetting("web_server_port", event.currentTarget.value)}
                placeholder="18420"
                className="input-port"
              />
            </label>
            <label>
              Remote Token
              <input
                type="password"
                value={settings.web_server_token ?? ""}
                onChange={(event) => updateSetting("web_server_token", event.currentTarget.value)}
                placeholder="Empty = localhost only"
                autoComplete="off"
              />
            </label>
            <label className="inline-toggle preferences-inline-toggle">
              <input
                type="checkbox"
                checked={settings.web_server_auto_start}
                onChange={(event) => updateSetting("web_server_auto_start", event.currentTarget.checked)}
              />
              Enable remote on launch
            </label>
          </div>
        </section>

        <div className={`preferences-save-status preferences-save-${saveState}`} aria-live="polite">
          {saveMessage || "Changes here update the saved configuration used by the main app window."}
        </div>
      </form>
    </div>
  );
}

export default PreferencesWindow;