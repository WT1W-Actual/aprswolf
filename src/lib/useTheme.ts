import { useEffect } from "react";

export type ThemeMode = "system" | "light" | "dark";

/**
 * Apply the theme to `document.documentElement` via `data-theme` attribute.
 *
 * Two modes:
 * 1. **Reactive** — pass a `themeMode` prop; re-applies on change (PreferencesWindow).
 * 2. **One-shot** — omit `themeMode`; loads from backend `load_settings` on mount
 *    (HelpWindow, LogWindow, ReceivedWindow).
 *
 * In both modes, when the resolved mode is `"system"`, a `change` listener on the
 * `prefers-color-scheme` media query keeps the attribute in sync.
 */
export function useTheme(themeMode?: ThemeMode): void {
  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    let currentMode: ThemeMode = themeMode ?? "system";

    const applyTheme = () => {
      const resolved =
        currentMode === "system"
          ? (media.matches ? "dark" : "light")
          : currentMode;
      document.documentElement.setAttribute("data-theme", resolved);
    };

    const handleChange = () => applyTheme();

    // ── Reactive mode (caller owns state) ──────────────────
    if (themeMode !== undefined) {
      applyTheme();
      if (currentMode === "system") {
        media.addEventListener("change", handleChange);
      }
      return () => media.removeEventListener("change", handleChange);
    }

    // ── One-shot mode (load from backend) ──────────────────
    let mounted = true;

    (async () => {
      const { invoke } = await import("@tauri-apps/api/core");
      try {
        const settings = await invoke<{ theme_mode?: ThemeMode }>("load_settings");
        const mode = settings?.theme_mode;
        if (mode === "light" || mode === "dark" || mode === "system") {
          currentMode = mode;
        }
      } catch {
        // Browser preview or non-desktop runtime
      }
      if (!mounted) return;
      applyTheme();
      if (currentMode === "system") {
        media.addEventListener("change", handleChange);
      }
    })();

    return () => {
      mounted = false;
      media.removeEventListener("change", handleChange);
    };
  }, [themeMode]);
}
