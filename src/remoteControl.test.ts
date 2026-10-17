// @ts-expect-error jsdom ships no type declarations and the project carries no @types/jsdom.
import { JSDOM } from "jsdom";
import { afterEach, describe, expect, it } from "vitest";
import html from "../src-tauri/remote-control.html?raw";

// The remote page is a standalone HTML file the web server serves as-is, so
// it runs here in its own jsdom window against a stubbed /api.

type PageWindow = Window & { refresh: () => Promise<void>; Event: typeof Event };

function settings(overrides: Record<string, unknown> = {}) {
  return {
    source: "K6ABC",
    path: "WIDE1-1",
    beacon_lat: "38.5",
    beacon_lon: "-121.4",
    beacon_symbol: "/-",
    host: "127.0.0.1",
    port: 8001,
    aprsis_host: "rotate.aprs2.net",
    aprsis_port: 14580,
    aprsis_passcode: "12345",
    aprsis_filter: "m/50",
    show_ack_messages: false,
    promiscuous_base_callsign: false,
    ...overrides,
  };
}

function snapshot(n: number, overrides: Record<string, unknown> = {}) {
  return {
    app_version: "1.1.0",
    settings: settings(overrides),
    runtime: { tnc_connected: false, aprsis_connected: true },
    web_server: { port: 8765 },
    manual_beacon_cooldown_remaining_sec: 0,
    sent: [],
    received: Array.from({ length: n }, (_, i) => ({
      from: `W${i}XYZ`,
      to: "",
      body: `Packet ${i}`,
      aprsType: "msg",
      timestamp: 1_760_000_000_000 + i,
    })),
  };
}

let dom: { window: PageWindow } | null = null;

function load() {
  const page = { state: snapshot(1), saved: null as unknown };
  dom = new JSDOM(html, {
    runScripts: "dangerously",
    url: "http://localhost:8765/",
    beforeParse(window: { fetch: unknown }) {
      window.fetch = async (path: string, opts: { method: string; body?: string }) => {
        if (path === "/api/save-settings") page.saved = JSON.parse(opts.body ?? "{}");
        const body = opts.method === "GET" ? page.state : {};
        return { status: 200, ok: true, json: async () => JSON.parse(JSON.stringify(body)) };
      };
    },
  });
  const window = dom!.window;
  const field = (id: string) => window.document.getElementById(id) as HTMLInputElement;
  const type = (id: string, value: string) => {
    field(id).value = value;
    field(id).dispatchEvent(new window.Event("input"));
  };
  return { page, window, field, type };
}

const settle = () => new Promise((r) => setTimeout(r, 20));

afterEach(() => {
  dom?.window.close();
  dom = null;
});

describe("remote page settings", () => {
  it("keeps unsaved edits while snapshots change, and follows the desktop elsewhere", async () => {
    const { page, window, field, type } = load();
    await settle();
    expect(field("path").value).toBe("WIDE1-1");

    type("path", "WIDE2-2");
    type("aprsisFilter", "r/38.5/-121.4/50");
    for (let n = 2; n <= 5; n++) {
      page.state = snapshot(n, { host: `10.0.0.${n}` });
      await window.refresh();
      await settle();
    }

    expect(field("path").value).toBe("WIDE2-2");
    expect(field("aprsisFilter").value).toBe("r/38.5/-121.4/50");
    expect(field("host").value).toBe("10.0.0.5");
  });

  it("keeps a passcode derived from a callsign change", async () => {
    const { page, window, field, type } = load();
    await settle();

    type("source", "N0CALL");
    field("source").dispatchEvent(new window.Event("change"));
    const derived = field("aprsisPasscode").value;
    expect(derived).not.toBe("12345");

    page.state = snapshot(3);
    await window.refresh();
    await settle();
    expect(field("source").value).toBe("N0CALL");
    expect(field("aprsisPasscode").value).toBe(derived);
  });

  it("lets snapshots fill the fields again after Save", async () => {
    const { page, window, field, type } = load();
    await settle();

    type("path", "WIDE2-2");
    field("saveSettingsBtn").click();
    await settle();
    expect((page.saved as { path: string }).path).toBe("WIDE2-2");

    page.state = snapshot(4, { path: "WIDE1-1,WIDE2-1" });
    await window.refresh();
    await settle();
    expect(field("path").value).toBe("WIDE1-1,WIDE2-1");
  });

  it("keeps an edit typed while a save is in flight", async () => {
    const { page, window, field, type } = load();
    await settle();

    type("path", "WIDE2-2");
    field("saveSettingsBtn").click();
    type("path", "WIDE2-1");
    await settle();
    expect((page.saved as { path: string }).path).toBe("WIDE2-2");

    page.state = snapshot(4, { path: "WIDE2-2" });
    await window.refresh();
    await settle();
    expect(field("path").value).toBe("WIDE2-1");
  });
});
