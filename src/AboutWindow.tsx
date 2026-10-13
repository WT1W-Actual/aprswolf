import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";
import aprsWolfLogo from "./assets/APRSWolf.png";
import packageJson from "../package.json";
import { type AppSettingsPayload } from "./lib/types";

const REPO_URL = "https://github.com/WT1W-Actual/aprswolf";

async function closeWindow() {
  try {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().destroy();
  } catch {
    window.close();
  }
}

async function openLink(url: string) {
  try {
    const { openUrl } = await import("@tauri-apps/plugin-opener");
    await openUrl(url);
  } catch {
    window.open(url, "_blank", "noopener");
  }
  await closeWindow();
}

async function openHelp() {
  await invoke("open_help").catch(() => {});
  await closeWindow();
}

function AboutWindow() {
  const [version, setVersion] = useState(packageJson.version);
  const [themeMode, setThemeMode] = useState<AppSettingsPayload["theme_mode"]>("system");

  useEffect(() => {
    (async () => {
      try {
        const { getVersion } = await import("@tauri-apps/api/app");
        setVersion(await getVersion());
      } catch {/* keep the package version */}
      try {
        const settings = await invoke<Partial<AppSettingsPayload>>("load_settings");
        if (settings?.theme_mode) setThemeMode(settings.theme_mode);
      } catch {/* system theme */}
    })();
  }, []);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    const applyTheme = () => {
      const resolved = themeMode === "system" ? (media.matches ? "dark" : "light") : themeMode;
      document.documentElement.setAttribute("data-theme", resolved ?? "light");
    };
    applyTheme();
    media.addEventListener("change", applyTheme);
    return () => media.removeEventListener("change", applyTheme);
  }, [themeMode]);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && !event.defaultPrevented) void closeWindow();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <div className="about-window">
      <img className="about-logo" src={aprsWolfLogo} alt="" />
      <h1 className="about-name">APRSWolf</h1>
      <div className="about-version">v{version}</div>

      <p className="about-tagline">APRS messaging on one clean screen, over RF or the internet.</p>

      <p className="about-text">
        Use a TNC that speaks KISS and AGW over TCP, such as Dire Wolf or Graywolf, or skip the radio and go straight
        to APRS-IS.
      </p>

      <ul className="about-points">
        <li>Watch every message get numbered, retried, and acked.</li>
        <li>Search, export, and import your whole history.</li>
        <li>Run the station from your phone.</li>
      </ul>

      <div className="about-platforms">macOS · Windows · Linux</div>

      <nav className="about-links" aria-label="Links">
        <button type="button" className="link-button" onClick={() => void openLink(REPO_URL)}>
          GitHub
        </button>
        <span aria-hidden="true">·</span>
        <button type="button" className="link-button" onClick={() => void openLink(`${REPO_URL}/releases/tag/v${version}`)}>
          Release notes
        </button>
        <span aria-hidden="true">·</span>
        <button type="button" className="link-button" onClick={() => void openHelp()}>
          Help
        </button>
      </nav>

      <div className="about-credits">
        <div className="about-credits-title">Credits</div>
        <div>Jim Perry, WT1W</div>
        <div>and the APRSWolf contributors</div>
      </div>

      <div className="about-signoff">73 de the APRSWolf team</div>
    </div>
  );
}

export default AboutWindow;
