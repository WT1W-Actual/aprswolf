import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import App from "./App";
import { formatHeaderClock } from "./HeaderClock";
import HelpWindow from "./HelpWindow";
import LogWindow from "./LogWindow";
import PreferencesWindow from "./PreferencesWindow";
import ReceivedWindow from "./ReceivedWindow";
import { publishUiLog } from "./logBus";
import packageJson from "../package.json";

const invokeMock = vi.fn() as any;
const destroyWindowMock = vi.fn();
const setBadgeCountMock = vi.fn();
const listenMock = vi.fn() as any;
const getVersionMock = vi.fn() as any;
const openUrlMock = vi.fn() as any;

let localStorageStore: Record<string, string> = {};

const DEFAULT_SETTINGS = {
  source: "WT1W-15",
  path: "WIDE1-1,WIDE2-1",
  host: "172.16.2.201",
  port: "8001",
  aprsis_host: "rotate.aprs2.net",
  aprsis_port: "14580",
  aprsis_passcode: "12345",
  aprsis_filter: "",
  beacon_lat: "",
  beacon_lon: "",
  beacon_symbol: "",
  web_server_port: "18420",
  web_server_auto_start: false,
  auto_beacon_on_connect: true,
  show_ack_messages: false,
  play_message_sound: true,
  theme_mode: "system",
};

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (cmd: string, payload?: unknown) => invokeMock(cmd, payload),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: (e: unknown) => void) => listenMock(event, handler),
}));

vi.mock("@tauri-apps/api/app", () => ({
  getVersion: () => getVersionMock(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (url: string) => openUrlMock(url),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ destroy: destroyWindowMock, setBadgeCount: setBadgeCountMock }),
}));

beforeEach(() => {
  try { cleanup(); } catch { /* previous test may have torn down the document */ }
  localStorageStore = {};
  (globalThis as any).isTauri = true;
  Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
    configurable: true,
    value: vi.fn(),
  });
  Object.defineProperty(window, "localStorage", {
    configurable: true,
    value: {
      getItem: vi.fn((key: string) => localStorageStore[key] ?? null),
      setItem: vi.fn((key: string, value: string) => {
        localStorageStore[key] = value;
      }),
      removeItem: vi.fn((key: string) => {
        delete localStorageStore[key];
      }),
      clear: vi.fn(() => {
        localStorageStore = {};
      }),
    },
  });
  Object.defineProperty(window, "matchMedia", {
    configurable: true,
    writable: true,
    value: vi.fn().mockImplementation(() => ({
      matches: false,
      media: "(prefers-color-scheme: dark)",
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    })),
  });
  invokeMock.mockReset();
  listenMock.mockReset();
  getVersionMock.mockReset();
  getVersionMock.mockResolvedValue(packageJson.version);
  invokeMock.mockImplementation((cmd: string) => {
    if (cmd === "load_settings") {
      return Promise.resolve(DEFAULT_SETTINGS);
    }
    if (cmd === "load_received_messages") {
      return Promise.resolve([]);
    }
    if (cmd === "get_web_server_status") {
      return Promise.resolve({ enabled: false, port: 18420, url: null });
    }
    if (cmd === "get_manual_beacon_cooldown_remaining") {
      return Promise.resolve(0);
    }
    return Promise.resolve(undefined);
  });
  listenMock.mockImplementation(async () => () => {});
});

afterEach(() => {
  (document.hasFocus as any).mockRestore?.();
  delete (globalThis as any).isTauri;
  window.localStorage.clear();
  vi.restoreAllMocks();
  try { cleanup(); } catch { /* document may already be torn down */ }
});

describe("App", () => {
  it("shows blank setup summaries when no saved preferences exist yet", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") {
        return Promise.resolve({
          ...DEFAULT_SETTINGS,
          source: "",
          path: "",
          host: "",
          port: "",
          aprsis_host: "",
          aprsis_port: "",
          aprsis_passcode: "",
          aprsis_filter: "",
          web_server_port: "",
        });
      }
      if (cmd === "load_received_messages") {
        return Promise.resolve([]);
      }
      if (cmd === "get_web_server_status") {
        return Promise.resolve({ enabled: false, port: 18420, url: null });
      }
      if (cmd === "get_manual_beacon_cooldown_remaining") {
        return Promise.resolve(0);
      }
      return Promise.resolve(undefined);
    });

    render(<App />);

    expect(await screen.findByText(/connect tnc or aprs-is to enable message sending/i)).toBeInTheDocument();
    expect(screen.getAllByText((_, element) => element?.textContent?.includes("Open Preferences to configure your station callsign") ?? false).length).toBeGreaterThan(0);
    expect(screen.getByRole("button", { name: "Connect TNC" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Connect APRS-IS" })).toBeDisabled();
  });

  it("renders initial disconnected state and disabled send", async () => {
    render(<App />);

    expect(await screen.findByRole("heading", { name: "TNC (Dire Wolf)" })).toHaveAttribute(
      "title",
      expect.stringMatching(/mutually exclusive/i),
    );
    const expectedVersion = packageJson.version;
    expect(screen.getByLabelText(new RegExp(`application version v${expectedVersion}`, "i"))).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Connect TNC" })).toBeEnabled();
  });

  it("shows the full runtime version in the badge", async () => {
    getVersionMock.mockResolvedValue("1.0.0");

    render(<App />);

    expect(await screen.findByLabelText(/application version v1\.0\.0/i)).toBeInTheDocument();
  });

  it("shows APRS-IS filter and path guidance", async () => {
    render(<App />);

    expect(await screen.findByRole("heading", { name: "APRS-IS" })).toHaveAttribute(
      "title",
      expect.stringMatching(/direct aprs-is sends ignore the rf digi path; wide paths are only used on tnc\/rf sends/i),
    );
  });

  it("recovers from a stale pop-out state key left by a crash", async () => {
    // A leftover key from a crashed session must NOT disable the embedded
    // pane for the whole new session — startup clears it as crash residue.
    window.localStorage.setItem("aprswolf-received-window-open", "1");

    render(<App />);

    await waitFor(() => {
      expect(
        screen.queryByText(/received messages are active in the popped-out window/i),
      ).not.toBeInTheDocument();
    });
  });

  it("disables the embedded received pane while the pop-out window is active", async () => {
    render(<App />);

    const { publishReceivedWindowState } = await import("./receivedBus");
    publishReceivedWindowState(true);

    expect(
      await screen.findByText(/received messages are active in the popped-out window/i),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /bring pop-out forward/i })).toBeInTheDocument();
  });

  it("opens Preferences from the header gear button", async () => {
    render(<App />);
    fireEvent.click(await screen.findByRole("button", { name: "Open Preferences" }));

    expect(invokeMock).toHaveBeenCalledWith("open_preferences", undefined);
  });

  it("invokes connect_tnc when Connect is clicked", async () => {
    render(<App />);
    await screen.findByRole("heading", { name: "TNC (Dire Wolf)" });
    fireEvent.click(screen.getByRole("button", { name: "Connect TNC" }));

    expect(invokeMock).toHaveBeenCalledWith("connect_tnc", {
      callsign: "WT1W-15",
      host: "172.16.2.201",
      port: 8001,
    });
  });

  it("allows disconnect while a TNC connection attempt is in progress", async () => {
    render(<App />);

    const statusCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_status");
    expect(statusCall?.[1]).toBeTypeOf("function");

    const statusHandler = statusCall[1] as (event: unknown) => void;
    statusHandler({ payload: { state: "tx_connecting", detail: "172.16.2.201:8001" } });

    const tncHeading = screen.getByRole("heading", { name: "TNC (Dire Wolf)" });
    const tncSection = tncHeading.closest(".config-group") as HTMLElement;

    await waitFor(() => {
      expect(within(tncSection).getByRole("button", { name: "Connect TNC" })).toBeDisabled();
      expect(within(tncSection).getByRole("button", { name: "Disconnect" })).toBeEnabled();
    });
  });

  it("sends a message and clears input after successful queue", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") {
        return Promise.resolve(DEFAULT_SETTINGS);
      }
      if (cmd === "send_message") {
        return Promise.resolve({
          id: 7,
          status: "queued",
          bytes: 12,
          payload_hex: "48656C6C6F",
        });
      }
      if (cmd === "load_received_messages") {
        return Promise.resolve([]);
      }
      if (cmd === "get_web_server_status") {
        return Promise.resolve({ enabled: false, port: 18420, url: null });
      }
      if (cmd === "get_manual_beacon_cooldown_remaining") {
        return Promise.resolve(0);
      }
      return Promise.resolve(undefined);
    });

    render(<App />);
    await screen.findByRole("heading", { name: "TNC (Dire Wolf)" });

    const statusCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_status");
    if (statusCall?.[1]) {
      const statusHandler = statusCall[1] as (event: unknown) => void;
      statusHandler({ payload: { state: "tx_rx_loop_start", detail: null } });
    }

    fireEvent.click(screen.getByRole("button", { name: "Connect TNC" }));

    const destinationInput = screen.getByPlaceholderText("WXYO") as HTMLInputElement;
    fireEvent.change(destinationInput, { target: { value: "APRS" } });

    const messageInput = screen.getByPlaceholderText("Test message") as HTMLInputElement;
    fireEvent.change(messageInput, { target: { value: "Hello world" } });
    fireEvent.click(screen.getByRole("button", { name: "Send" }));

    expect(invokeMock).toHaveBeenCalledWith("connect_tnc", {
      callsign: "WT1W-15",
      host: "172.16.2.201",
      port: 8001,
    });
    expect(invokeMock).toHaveBeenCalledWith("send_message", {
      source: "WT1W-15",
      destination: "APRS",
      path: "WIDE1-1,WIDE2-1",
      message: "Hello world",
    });
    await waitFor(() => {
      expect(messageInput.value).toBe("");
    });
    await waitFor(() => {
      expect(screen.getByText(/TX queued #7 12B to APRS/)).toBeInTheDocument();
    });
  });

  it("renders decoded AX.25 receive messages in the log", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "WT1W > APRS via WIDE1-1,WIDE2-1: hello",
        payload_hex: "AABBCC",
        bytes: 24,
        source: "WT1W",
        destination: "APRS",
        path: ["WIDE1-1", "WIDE2-1"],
        from: "WT1W",
        body: "hello",
        aprsType: "other",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "WT1W",
        summary_destination: "APRS",
      },
    });

    expect(
      await screen.findByText(/RX 24B WT1W > APRS via WIDE1-1,WIDE2-1: hello \[AABBCC\]/),
    ).toBeInTheDocument();
  });

  it("strips APRS message ids from displayed received message bodies", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "WXBOT>APZWLF::WT1W-15  :Mitylene AL. This Afternoon,Mostly Cloudy High 70{ET",
        payload_hex: "AABBCCDD",
        bytes: 73,
        source: "WXBOT",
        destination: "APZWLF",
        path: [],
        from: "WXBOT",
        to: "WT1W-15",
        body: "Mitylene AL. This Afternoon,Mostly Cloudy High 70",
        aprsType: "msg",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "WXBOT",
        summary_destination: "APZWLF",
      },
    });

    expect(
      await screen.findByText("Mitylene AL. This Afternoon,Mostly Cloudy High 70"),
    ).toBeInTheDocument();
    expect(screen.queryByText(/\{ET$/)).not.toBeInTheDocument();
  });

  it("lists a packet heard twice once and marks the copy in the log", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    const packet = {
      text: "N6TEST-7>APRS::WT1W-15  :Heard twice{12",
      payload_hex: "AABB",
      bytes: 40,
      source: "N6TEST-7",
      destination: "APRS",
      path: [],
      from: "N6TEST-7",
      to: "WT1W-15",
      body: "Heard twice",
      aprsType: "msg",
      isBulletin: false,
      micEDecoded: false,
      summary_source: "N6TEST-7",
      summary_destination: "APRS",
      uuid: "first-copy",
      seq: 41,
    };
    messageHandler?.({ payload: packet });
    messageHandler?.({ payload: { ...packet, path: ["TCPIP"], duplicate: true } });

    expect(await screen.findByText(/RX 40B .*Heard twice \(duplicate\) \[AABB\]/)).toBeInTheDocument();
    expect(screen.getAllByText("Heard twice")).toHaveLength(1);
  });

  it("classifies telemetry metadata messages as telemetry", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "N1YOQ-1>APMI0A::N1YOQ-1  :PARM.Volt,None,None,None,None",
        payload_hex: "AABBCCDD",
        bytes: 58,
        source: "N1YOQ-1",
        destination: "APMI0A",
        path: ["WIDE1-1"],
        from: "N1YOQ-1",
        to: "N1YOQ-1",
        body: "PARM.Volt,None,None,None,None",
        aprsType: "tlm",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "N1YOQ-1",
        summary_destination: "APMI0A",
      },
    });

    expect(await screen.findByText("PARM.Volt,None,None,None,None")).toBeInTheDocument();
    expect(screen.getByText("tlm")).toBeInTheDocument();
  });

  it("classifies position-based APRS weather reports as weather", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "W1TG2>APU25N:@091842z4256.20N/07049.42W_310/004g015t081r000p033P002h54b10001",
        payload_hex: "AABBCCDDEE",
        bytes: 74,
        source: "W1TG2",
        destination: "APU25N",
        path: [],
        from: "W1TG2",
        body: "@091842z4256.20N/07049.42W_310/004g015t081r000p033P002h54b10001",
        aprsType: "wx",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "W1TG2",
        summary_destination: "APU25N",
      },
    });

    expect(
      (await screen.findAllByText(/@091842z4256\.20N\/07049\.42W_310\/004g015t081r000p033P002h54b10001/)).length,
    ).toBeGreaterThan(0);
    expect(screen.getByText("wx")).toBeInTheDocument();
  });

  it("classifies APRS status reports as status", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "W1ABC>APZWLF:>IO91SX /R linked system online",
        payload_hex: "AA11BB22",
        bytes: 31,
        source: "W1ABC",
        destination: "APZWLF",
        path: [],
        from: "W1ABC",
        body: ">IO91SX /R linked system online",
        aprsType: "status",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "W1ABC",
        summary_destination: "APZWLF",
      },
    });

    expect(await screen.findByText(">IO91SX /R linked system online")).toBeInTheDocument();
    expect(screen.getByText("status")).toBeInTheDocument();
  });

  it("classifies APRS item reports as items", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "W1ABC>APZWLF:)CHECKPT!4903.50N/07201.75W-Test marker",
        payload_hex: "CC33DD44",
        bytes: 41,
        source: "W1ABC",
        destination: "APZWLF",
        path: [],
        from: "W1ABC",
        body: ")CHECKPT!4903.50N/07201.75W-Test marker",
        aprsType: "item",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "W1ABC",
        summary_destination: "APZWLF",
      },
    });

    expect(await screen.findByText(")CHECKPT!4903.50N/07201.75W-Test marker")).toBeInTheDocument();
    expect(screen.getByText("item")).toBeInTheDocument();
  });

  it("classifies APRS queries separately from messages", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "WT1W-15>APZWLF:?WX?",
        payload_hex: "AABB",
        bytes: 4,
        source: "WT1W-15",
        destination: "APZWLF",
        path: [],
        from: "WT1W-15",
        body: "?WX?",
        aprsType: "qry",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "WT1W-15",
        summary_destination: "APZWLF",
      },
    });

    messageHandler?.({
      payload: {
        text: "WT1W-15>APZWLF::N0CALL   :?APRSP",
        payload_hex: "CCDD",
        bytes: 16,
        source: "WT1W-15",
        destination: "APZWLF",
        path: [],
        from: "WT1W-15",
        to: "N0CALL",
        body: "?APRSP",
        aprsType: "qry",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "WT1W-15",
        summary_destination: "APZWLF",
      },
    });

    expect(await screen.findByText("?WX?")).toBeInTheDocument();
    expect(await screen.findByText("?APRSP")).toBeInTheDocument();
    expect(screen.getAllByText("qry")).toHaveLength(2);
  });

  it("classifies third-party wrapped APRS queries by the inner payload", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "WZ0C-4>APN20H:}WHO-IS>APJIW4,TCPIP,WZ0C-4*::WT1W-15  :?APRSP",
        payload_hex: "EEFF",
        bytes: 28,
        source: "WZ0C-4",
        destination: "APN20H",
        path: ["WIDE1-1"],
        from: "WHO-IS",
        to: "WT1W-15",
        body: "?APRSP",
        aprsType: "qry",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "WHO-IS",
        summary_destination: "APJIW4",
      },
    });

    expect(await screen.findByText("?APRSP")).toBeInTheDocument();
    expect(screen.getByText("qry")).toBeInTheDocument();
    expect(
      await screen.findByText(/RX 28B WHO-IS > APJIW4 via WIDE1-1: \?APRSP \[EEFF\]/),
    ).toBeInTheDocument();
  });

  it("suppresses third-party wrapped ACKs when ACK display is disabled", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "WB2OSZ-5>APDW17:}WHO-IS>APJIW4,TCPIP,WB2OSZ-5*::WB2OSZ-7 :ack0",
        payload_hex: "1122",
        bytes: 20,
        source: "WB2OSZ-5",
        destination: "APDW17",
        path: ["WIDE1-1"],
        from: "WHO-IS",
        to: "WB2OSZ-7",
        body: "ack0",
        aprsType: "msg",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "WHO-IS",
        summary_destination: "APJIW4",
      },
    });

    await waitFor(() => {
      expect(screen.queryByText("ack0")).not.toBeInTheDocument();
    });
  });

  it("drops malformed live receive payloads and logs an RX error", async () => {
    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "broken-packet",
        payload_hex: "DEAD",
        bytes: 4,
        source: "BROKEN",
      },
    });

    expect(await screen.findByText("[RX ERROR] malformed receive payload dropped")).toBeInTheDocument();
    expect(screen.queryByText(/RX 4B/)).not.toBeInTheDocument();
    expect(screen.queryByText("broken-packet")).not.toBeInTheDocument();
  });

  it("treats same-base different-SSID messages as To Me when promiscuous mode is enabled", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") {
        return Promise.resolve({
          ...DEFAULT_SETTINGS,
          source: "WT1W-15",
          promiscuous_base_callsign: false,
        });
      }
      if (cmd === "load_received_messages") {
        return Promise.resolve([]);
      }
      if (cmd === "get_web_server_status") {
        return Promise.resolve({ enabled: false, port: 18420, url: null });
      }
      if (cmd === "get_manual_beacon_cooldown_remaining") {
        return Promise.resolve(0);
      }
      return Promise.resolve(undefined);
    });

    render(<App />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "N0CALL-1>APZWLF::WT1W-9   :Hello same base{01",
        payload_hex: "AABBCCDD",
        bytes: 32,
        source: "N0CALL-1",
        destination: "APZWLF",
        path: [],
        from: "N0CALL-1",
        to: "WT1W-9",
        body: "Hello same base",
        aprsType: "msg",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "N0CALL-1",
        summary_destination: "APZWLF",
      },
    });

    expect(await screen.findByText("Hello same base")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /to me/i }));
    await waitFor(() => {
      expect(screen.queryByText("Hello same base")).not.toBeInTheDocument();
    });

    fireEvent.click(screen.getByLabelText(/promiscuous base-call/i));

    expect(await screen.findByText("Hello same base")).toBeInTheDocument();
  });

  async function renderWithMessageHandler(settings: Record<string, unknown> = {}) {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") return Promise.resolve({ ...DEFAULT_SETTINGS, ...settings });
      if (cmd === "load_received_messages") return Promise.resolve([]);
      if (cmd === "get_web_server_status") return Promise.resolve({ enabled: false, port: 18420, url: null });
      if (cmd === "get_manual_beacon_cooldown_remaining") return Promise.resolve(0);
      return Promise.resolve(undefined);
    });
    render(<App />);
    let handler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      handler = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message")?.[1];
      expect(handler).toBeTruthy();
    });
    return (from: string, to: string | undefined, body: string, aprsType = "msg") =>
      handler?.({
        payload: {
          text: `${from}>APZWLF::${to ?? ""}:${body}`,
          payload_hex: "AABBCCDD",
          bytes: 32,
          source: from,
          destination: "APZWLF",
          path: [],
          from,
          to,
          body,
          aprsType,
          isBulletin: false,
          micEDecoded: false,
          summary_source: from,
          summary_destination: "APZWLF",
        },
      });
  }

  it("marks To Me messages unread on the chip and app icon while the app is in the background", async () => {
    const hasFocus = vi.spyOn(document, "hasFocus").mockReturnValue(false);
    localStorageStore["aprswolf-to-me-read-at"] = "1";
    setBadgeCountMock.mockClear();
    const emit = await renderWithMessageHandler();

    emit("N0CALL-1", "WT1W-15", "Are you on the net tonight");
    emit("N0CALL-1", "WT1W-15", "ack12");
    emit("N0CALL-2", undefined, "!3830.00N/12130.00W>Mobile", "pos");

    expect(await screen.findByTitle("1 unread")).toBeInTheDocument();
    await waitFor(() => expect(setBadgeCountMock).toHaveBeenLastCalledWith(1));

    // Bringing the app forward marks it read: chip dot and icon badge clear.
    hasFocus.mockReturnValue(true);
    fireEvent.focus(window);
    await waitFor(() => expect(setBadgeCountMock).toHaveBeenLastCalledWith(undefined));
    await waitFor(() => expect(screen.queryByTitle(/unread/)).not.toBeInTheDocument());
  });

  it("shows no unread dot or icon badge for messages that arrive while the app has focus", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(true);
    localStorageStore["aprswolf-to-me-read-at"] = "1";
    setBadgeCountMock.mockClear();
    const emit = await renderWithMessageHandler();

    emit("N0CALL-1", "WT1W-15", "Are you on the net tonight");

    expect(await screen.findByText("Are you on the net tonight")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(screen.queryByTitle(/unread/)).not.toBeInTheDocument();
    expect(setBadgeCountMock).not.toHaveBeenCalledWith(1);
  });

  it("keeps the app badge off when the setting is disabled", async () => {
    vi.spyOn(document, "hasFocus").mockReturnValue(false);
    localStorageStore["aprswolf-to-me-read-at"] = "1";
    setBadgeCountMock.mockClear();
    const emit = await renderWithMessageHandler({ show_app_badge: false });

    emit("N0CALL-1", "WT1W-15", "Are you on the net tonight");

    expect(await screen.findByTitle("1 unread")).toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 250));
    expect(setBadgeCountMock).not.toHaveBeenCalledWith(1);
  });

  it("searches full message history from the main window's received pane", async () => {
    const old = Date.UTC(2026, 7, 27, 18, 0, 0);
    invokeMock.mockImplementation((cmd: string, payload?: any) => {
      if (cmd === "load_settings") return Promise.resolve(DEFAULT_SETTINGS);
      if (cmd === "load_received_messages") return Promise.resolve([]);
      if (cmd === "get_web_server_status") return Promise.resolve({ enabled: false, port: 18420, url: null });
      if (cmd === "get_manual_beacon_cooldown_remaining") return Promise.resolve(0);
      if (cmd === "search_received_messages" && payload?.query === "potluck") {
        return Promise.resolve([
          { id: 1, uuid: "u-1", seq: 7, timestamp: old, from: "W6TEST-1", to: "WT1W-15", body: "Bring a dish to the potluck", aprsType: "msg" },
        ]);
      }
      return Promise.resolve(undefined);
    });
    render(<App />);

    fireEvent.change(await screen.findByLabelText("Search message history"), { target: { value: "potluck" } });

    expect(await screen.findByText("Bring a dish to the potluck")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("search_received_messages", { query: "potluck" });
    expect(screen.getByText(new Date(old).toLocaleString())).toBeInTheDocument();
  });

  it("shows UTC and local time in the header row", async () => {
    render(<App />);
    const clock = await screen.findByTitle("UTC and local time");
    expect(clock.closest("header")).toBeInTheDocument();
    expect(clock.textContent).toMatch(/^\d{2}:\d{2}Z \d{2}:\d{2} \S+$/);
  });

  it("formats the header clock as 24-hour UTC and local hh:mm", () => {
    const when = new Date(Date.UTC(2026, 9, 12, 15, 8, 30));
    const [utc] = formatHeaderClock(when).split(" ");
    expect(utc).toBe("15:08Z");
  });

  it("filters both panes from the one search field by Sent Messages", async () => {
    invokeMock.mockImplementation((cmd: string, payload?: any) => {
      if (cmd === "load_settings") return Promise.resolve(DEFAULT_SETTINGS);
      if (cmd === "load_received_messages") return Promise.resolve([]);
      if (cmd === "get_web_server_status") return Promise.resolve({ enabled: false, port: 18420, url: null });
      if (cmd === "get_manual_beacon_cooldown_remaining") return Promise.resolve(0);
      if (cmd === "search_received_messages" && payload?.query === "net") {
        return Promise.resolve([
          { uuid: "r-1", seq: 9, timestamp: Date.now(), from: "N6TEST-7", to: "WT1W-15", body: "Are you on the net", aprsType: "msg" },
        ]);
      }
      if (cmd === "search_sent_messages" && payload?.query === "net") {
        return Promise.resolve([
          { uuid: "s-1", seq: 3, timestamp: Date.now(), destination: "N6TEST-7", body: "See you on the net", status: "acked", hidden: true },
        ]);
      }
      return Promise.resolve(undefined);
    });
    render(<App />);

    const field = await screen.findByLabelText("Search message history");
    expect(field.closest(".msg-pane")?.querySelector("h2")?.textContent).toBe("Sent Messages");
    expect(screen.getAllByRole("searchbox")).toHaveLength(1);

    fireEvent.change(field, { target: { value: "net" } });

    const sentRow = (await screen.findByText("See you on the net")).closest(".msg-row") as HTMLElement;
    expect(within(sentRow).getByText("hidden")).toBeInTheDocument();
    expect(await screen.findByText("Are you on the net")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("search_sent_messages", { query: "net" });
    expect(invokeMock).toHaveBeenCalledWith("search_received_messages", { query: "net" });
  });

  it("hides one sent message with the row's x, keeping it findable by search", async () => {
    invokeMock.mockImplementation((cmd: string, payload?: any) => {
      if (cmd === "load_settings") return Promise.resolve(DEFAULT_SETTINGS);
      if (cmd === "load_received_messages") return Promise.resolve([]);
      if (cmd === "load_sent_messages") {
        return Promise.resolve([
          { uuid: "s-1", seq: 2, timestamp: Date.now(), destination: "N6TEST-7", body: "Clear me", status: "acked" },
          { uuid: "s-2", seq: 1, timestamp: Date.now() - 1_000, destination: "K6TEST-9", body: "Keep me", status: "acked" },
        ]);
      }
      if (cmd === "get_web_server_status") return Promise.resolve({ enabled: false, port: 18420, url: null });
      if (cmd === "get_manual_beacon_cooldown_remaining") return Promise.resolve(0);
      if (cmd === "clear_messages") return Promise.resolve(payload?.uuids?.length ?? 0);
      if (cmd === "search_received_messages") return Promise.resolve([]);
      if (cmd === "search_sent_messages") {
        return Promise.resolve([
          { uuid: "s-1", seq: 2, timestamp: Date.now(), destination: "N6TEST-7", body: "Clear me", status: "acked", hidden: true },
        ]);
      }
      return Promise.resolve(undefined);
    });
    render(<App />);

    const row = (await screen.findByText("Clear me")).closest(".msg-row") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Clear this message" }));

    const dialog = await screen.findByRole("dialog", { name: "Clear messages" });
    expect(within(dialog).getByRole("heading")).toHaveTextContent("Clear this message?");
    expect(within(dialog).getByLabelText("Delete from database")).not.toBeChecked();
    fireEvent.click(within(dialog).getByRole("button", { name: "Clear" }));

    await waitFor(() => expect(screen.queryByText("Clear me")).not.toBeInTheDocument());
    expect(invokeMock).toHaveBeenCalledWith("clear_messages", { list: "sent", uuids: ["s-1"], delete: false });
    expect(screen.getByText("Keep me")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Search message history"), { target: { value: "clear" } });
    const hit = (await screen.findByText("Clear me")).closest(".msg-row") as HTMLElement;
    expect(within(hit).getByText("hidden")).toBeInTheDocument();
  });

  it("deletes only the received messages shown with Clear all and the database box checked", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") return Promise.resolve(DEFAULT_SETTINGS);
      if (cmd === "load_sent_messages") return Promise.resolve([]);
      if (cmd === "load_received_messages") {
        return Promise.resolve([
          { uuid: "r-1", seq: 2, timestamp: Date.now(), from: "N6TEST-7", to: "WT1W-15", body: "First to go", aprsType: "msg" },
          { uuid: "r-2", seq: 1, timestamp: Date.now() - 1_000, from: "K6TEST-9", to: "WT1W-15", body: "Second to go", aprsType: "msg" },
        ]);
      }
      if (cmd === "clear_messages") return Promise.resolve(2);
      if (cmd === "get_web_server_status") return Promise.resolve({ enabled: false, port: 18420, url: null });
      if (cmd === "get_manual_beacon_cooldown_remaining") return Promise.resolve(0);
      return Promise.resolve(undefined);
    });
    render(<App />);

    await screen.findByText("First to go");
    const header = screen.getByRole("heading", { name: "Received Messages" }).parentElement as HTMLElement;
    fireEvent.click(within(header).getByRole("button", { name: "Clear all" }));

    const dialog = await screen.findByRole("dialog", { name: "Clear messages" });
    expect(within(dialog).getByRole("heading")).toHaveTextContent("Clear the 2 received messages shown?");
    fireEvent.click(within(dialog).getByLabelText("Delete from database"));
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(screen.queryByText("First to go")).not.toBeInTheDocument());
    expect(screen.queryByText("Second to go")).not.toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("clear_messages", { list: "received", uuids: ["r-1", "r-2"], delete: true });
  });

  it("selects messages with a long press and clears the selection from one x", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") return Promise.resolve(DEFAULT_SETTINGS);
      if (cmd === "load_received_messages") return Promise.resolve([]);
      if (cmd === "load_sent_messages") {
        return Promise.resolve([
          { uuid: "s-1", seq: 3, timestamp: Date.now(), destination: "N6TEST-7", body: "Pick one", status: "acked" },
          { uuid: "s-2", seq: 2, timestamp: Date.now() - 1_000, destination: "N6TEST-7", body: "Skip me", status: "acked" },
          { uuid: "s-3", seq: 1, timestamp: Date.now() - 2_000, destination: "K6TEST-9", body: "Pick two", status: "acked" },
        ]);
      }
      if (cmd === "get_web_server_status") return Promise.resolve({ enabled: false, port: 18420, url: null });
      if (cmd === "get_manual_beacon_cooldown_remaining") return Promise.resolve(0);
      if (cmd === "clear_messages") return Promise.resolve(2);
      return Promise.resolve(undefined);
    });
    render(<App />);

    const first = (await screen.findByText("Pick one")).closest(".msg-item") as HTMLElement;
    fireEvent.pointerDown(first, { button: 0 });
    await new Promise((resolve) => setTimeout(resolve, 600));
    fireEvent.pointerUp(first);
    fireEvent.click(first);

    const boxes = await screen.findAllByRole("checkbox", { name: "Select message" });
    expect(boxes).toHaveLength(3);
    expect(boxes[0]).toBeChecked();
    expect(screen.getByText("1 selected")).toBeInTheDocument();

    fireEvent.click(screen.getByText("Pick two").closest(".msg-item") as HTMLElement);
    expect(screen.getByText("2 selected")).toBeInTheDocument();

    const row = screen.getByText("Pick two").closest(".msg-row") as HTMLElement;
    fireEvent.click(within(row).getByRole("button", { name: "Clear this message" }));
    const dialog = await screen.findByRole("dialog", { name: "Clear messages" });
    expect(within(dialog).getByRole("heading")).toHaveTextContent("Clear these 2 sent messages?");
    fireEvent.click(within(dialog).getByRole("button", { name: "Clear" }));

    await waitFor(() => expect(screen.queryByText("Pick one")).not.toBeInTheDocument());
    expect(screen.queryByText("Pick two")).not.toBeInTheDocument();
    expect(screen.getByText("Skip me")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("clear_messages", { list: "sent", uuids: ["s-1", "s-3"], delete: false });
    expect(screen.queryByRole("checkbox", { name: "Select message" })).not.toBeInTheDocument();
  });

  it("unhides a hidden search result and puts it back in its list", async () => {
    const now = Date.now();
    invokeMock.mockImplementation((cmd: string, payload?: any) => {
      if (cmd === "load_settings") return Promise.resolve(DEFAULT_SETTINGS);
      if (cmd === "load_received_messages") return Promise.resolve([]);
      if (cmd === "load_sent_messages") {
        return Promise.resolve([{ uuid: "s-2", seq: 1, timestamp: now - 5_000, destination: "N6TEST-7", body: "Still listed", status: "acked" }]);
      }
      if (cmd === "get_web_server_status") return Promise.resolve({ enabled: false, port: 18420, url: null });
      if (cmd === "get_manual_beacon_cooldown_remaining") return Promise.resolve(0);
      if (cmd === "search_received_messages") return Promise.resolve([]);
      if (cmd === "search_sent_messages" && payload?.query === "back") {
        return Promise.resolve([{ uuid: "s-1", seq: 2, timestamp: now, destination: "N6TEST-7", body: "Bring me back", status: "acked", hidden: true }]);
      }
      if (cmd === "restore_messages") return Promise.resolve(1);
      return Promise.resolve(undefined);
    });
    render(<App />);

    const field = await screen.findByLabelText("Search message history");
    fireEvent.change(field, { target: { value: "back" } });
    const hit = (await screen.findByText("Bring me back")).closest(".msg-row") as HTMLElement;
    fireEvent.click(within(hit).getByRole("button", { name: "Unhide this message" }));

    await waitFor(() => expect(within(hit).queryByText("hidden")).not.toBeInTheDocument());
    expect(invokeMock).toHaveBeenCalledWith("restore_messages", { list: "sent", uuids: ["s-1"] });

    fireEvent.change(field, { target: { value: "" } });
    expect(await screen.findByText("Bring me back")).toBeInTheDocument();
    expect(screen.getByText("Still listed")).toBeInTheDocument();
  });

  it("lists sent messages from earlier sessions and keeps their status current", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") return Promise.resolve(DEFAULT_SETTINGS);
      if (cmd === "load_received_messages") return Promise.resolve([]);
      if (cmd === "load_sent_messages") {
        return Promise.resolve([
          { uuid: "s-1", seq: 4, timestamp: Date.now(), destination: "N6TEST-7", body: "See you on the net", status: "sent" },
        ]);
      }
      if (cmd === "get_web_server_status") return Promise.resolve({ enabled: false, port: 18420, url: null });
      if (cmd === "get_manual_beacon_cooldown_remaining") return Promise.resolve(0);
      return Promise.resolve(undefined);
    });
    render(<App />);

    const row = (await screen.findByText("See you on the net")).closest(".msg-item") as HTMLElement;
    expect(within(row).getByText("sent")).toBeInTheDocument();

    let deliveryHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      deliveryHandler = listenMock.mock.calls.find((call: any) => call[0] === "tnc_delivery")?.[1];
      expect(deliveryHandler).toBeTruthy();
    });
    deliveryHandler?.({ payload: { id: 1, uuid: "s-1", status: "acked", attempts: 1, detail: "ack received" } });

    await waitFor(() => expect(within(row).getByText("acked")).toBeInTheDocument());
  });

  it("does not invoke connect_tnc when TNC port is invalid", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") {
        return Promise.resolve({
          ...DEFAULT_SETTINGS,
          port: "abc",
        });
      }
      if (cmd === "load_received_messages") {
        return Promise.resolve([]);
      }
      if (cmd === "get_web_server_status") {
        return Promise.resolve({ enabled: false, port: 18420, url: null });
      }
      if (cmd === "get_manual_beacon_cooldown_remaining") {
        return Promise.resolve(0);
      }
      return Promise.resolve(undefined);
    });

    render(<App />);

    const tncHeadings = await screen.findAllByRole("heading", { name: "TNC (Dire Wolf)" });
    const tncHeading = tncHeadings[tncHeadings.length - 1];
    const tncSection = tncHeading?.closest(".config-group");
    expect(tncSection).toBeTruthy();

    fireEvent.click(within(tncSection as HTMLElement).getByRole("button", { name: "Connect TNC" }));

    expect(invokeMock).not.toHaveBeenCalledWith(
      "connect_tnc",
      expect.objectContaining({ callsign: "WT1W-15", host: "172.16.2.201" }),
    );
    expect(within(tncSection as HTMLElement).getByRole("button", { name: "Connect TNC" })).toBeEnabled();
  });

  it("keeps received messages across restarts instead of clearing them", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_received_messages") {
        return Promise.resolve([
          { uuid: "u-1", seq: 1, timestamp: Date.now() - 60_000, from: "N0CALL-1", to: "WT1W-15", body: "Stored before restart", aprsType: "msg" },
        ]);
      }
      if (cmd === "get_web_server_status") {
        return Promise.resolve({ enabled: false, port: null, url: null });
      }
      if (cmd === "get_manual_beacon_cooldown_remaining") {
        return Promise.resolve(0);
      }
      return Promise.resolve(undefined);
    });

    render(<App />);

    expect(await screen.findByText("Stored before restart")).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("save_received_messages", expect.anything());
  });

  it("searches the full message history from the received messages window", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") {
        return Promise.resolve({ source: "WT1W-15", show_ack_messages: false, theme_mode: "system" });
      }
      if (cmd === "load_received_messages") {
        return Promise.resolve([
          { uuid: "u-new", seq: 900, timestamp: Date.now(), from: "W1AW", to: "WT1W-15", body: "Newest note", aprsType: "msg" },
        ]);
      }
      if (cmd === "search_received_messages") {
        return Promise.resolve([
          { uuid: "u-old", seq: 12, timestamp: Date.now() - 86_400_000 * 40, from: "KM4ACK-7", to: "WT1W-15", body: "Net tonight at 8", aprsType: "msg" },
        ]);
      }
      return Promise.resolve(undefined);
    });

    render(<ReceivedWindow />);
    expect(await screen.findByText("Newest note")).toBeInTheDocument();

    fireEvent.change(screen.getByRole("searchbox", { name: "Search message history" }), {
      target: { value: "km4ack" },
    });

    expect(await screen.findByText("Net tonight at 8")).toBeInTheDocument();
    expect(screen.queryByText("Newest note")).not.toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("search_received_messages", { query: "km4ack" });

    fireEvent.change(screen.getByRole("searchbox", { name: "Search message history" }), {
      target: { value: "" },
    });
    expect(await screen.findByText("Newest note")).toBeInTheDocument();
  });

  it("loads older pages of message history on request", async () => {
    const newestPage = Array.from({ length: 200 }, (_, i) => ({
      uuid: `u-${1000 - i}`,
      seq: 1000 - i,
      timestamp: Date.now() - i * 1000,
      from: "W1AW",
      to: "WT1W-15",
      body: `Page one note ${1000 - i}`,
      aprsType: "msg",
    }));
    invokeMock.mockImplementation((cmd: string, payload?: { beforeSeq?: number }) => {
      if (cmd === "load_settings") {
        return Promise.resolve({ source: "WT1W-15", show_ack_messages: false, theme_mode: "system" });
      }
      if (cmd === "load_received_messages") {
        if (payload?.beforeSeq === 801) {
          return Promise.resolve([
            { uuid: "u-5", seq: 5, timestamp: Date.now() - 86_400_000 * 3, from: "W1AW", to: "WT1W-15", body: "From last week", aprsType: "msg" },
          ]);
        }
        return Promise.resolve(newestPage);
      }
      return Promise.resolve(undefined);
    });

    render(<ReceivedWindow />);
    expect(await screen.findByText("Page one note 1000")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: "Load older messages" }));

    expect(await screen.findByText("From last week")).toBeInTheDocument();
    expect(invokeMock).toHaveBeenCalledWith("load_received_messages", { beforeSeq: 801 });
    expect(screen.queryByRole("button", { name: "Load older messages" })).not.toBeInTheDocument();
  });

  it("lists sent and received together in the pop-out Messages window", async () => {
    const now = Date.now();
    invokeMock.mockImplementation((cmd: string, payload?: any) => {
      if (cmd === "load_settings") return Promise.resolve({ source: "WT1W-15", show_ack_messages: false, theme_mode: "system" });
      if (cmd === "load_received_messages") {
        return Promise.resolve([
          { uuid: "r-2", seq: 2, timestamp: now - 1_000, from: "N6TEST-7", to: "WT1W-15", body: "Copy, see you there", aprsType: "msg" },
          { uuid: "r-1", seq: 1, timestamp: now - 3_000, from: "WX6TEST", body: "_10111213c220s004g010t060", aprsType: "wx" },
        ]);
      }
      if (cmd === "load_sent_messages") {
        return Promise.resolve([
          { uuid: "s-1", seq: 1, timestamp: now - 2_000, destination: "N6TEST-7", body: "On the net tonight?", status: "acked" },
        ]);
      }
      if (cmd === "search_received_messages" && payload?.query === "net") return Promise.resolve([]);
      if (cmd === "search_sent_messages" && payload?.query === "net") {
        return Promise.resolve([
          { uuid: "s-1", seq: 1, timestamp: now - 2_000, destination: "N6TEST-7", body: "On the net tonight?", status: "acked" },
        ]);
      }
      return Promise.resolve(undefined);
    });

    const { container } = render(<ReceivedWindow />);

    expect(await screen.findByText("On the net tonight?")).toBeInTheDocument();
    expect(screen.getByText("Messages", { selector: ".received-window-title" })).toBeInTheDocument();
    const bodies = () => Array.from(container.querySelectorAll(".received-window-list .msg-body")).map((el) => el.textContent);
    await waitFor(() => {
      expect(bodies()).toEqual(["Copy, see you there", "On the net tonight?", "_10111213c220s004g010t060"]);
    });
    const sentRow = screen.getByText("On the net tonight?").closest(".msg-item") as HTMLElement;
    expect(within(sentRow).getByText("acked")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Weather/ }));
    expect(bodies()).toEqual(["_10111213c220s004g010t060"]);

    fireEvent.click(screen.getByRole("button", { name: /^All/ }));
    fireEvent.change(screen.getByLabelText("Search message history"), { target: { value: "net" } });
    await waitFor(() => {
      expect(bodies()).toEqual(["On the net tonight?"]);
    });
    expect(invokeMock).toHaveBeenCalledWith("search_sent_messages", { query: "net" });
  });

  it("opens a reply dialog from the popped-out received messages window and sends a reply", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") {
        return Promise.resolve({
          source: "WT1W-15",
          path: "WIDE1-1,WIDE2-1",
          show_ack_messages: false,
          theme_mode: "system",
        });
      }
      if (cmd === "load_received_messages") {
        return Promise.resolve([
          {
            id: 1,
            timestamp: Date.now(),
            from: "N0CALL-1",
            to: "WT1W-15",
            body: "Hello from the pop-out",
            aprsType: "msg",
          },
        ]);
      }
      if (cmd === "send_message") {
        return Promise.resolve({
          id: 12,
          status: "queued",
          bytes: 24,
          payload_hex: "00112233",
        });
      }
      return Promise.resolve(undefined);
    });

    render(<ReceivedWindow />);

    const messageBody = await screen.findByText("Hello from the pop-out");
    fireEvent.click(messageBody.closest("button") as HTMLButtonElement);

    expect(await screen.findByRole("dialog", { name: /reply to message/i })).toBeInTheDocument();

    const replyToInput = screen.getByPlaceholderText("Callsign") as HTMLInputElement;
    expect(replyToInput.value).toBe("N0CALL-1");

    const replyMessageInput = screen.getByPlaceholderText("Reply text") as HTMLInputElement;
    expect(replyMessageInput.value).toBe("RE: Hello from the pop-out");

    fireEvent.change(replyMessageInput, { target: { value: "Roger that" } });
    fireEvent.click(screen.getByRole("button", { name: "Send Reply" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("send_message", {
        source: "WT1W-15",
        destination: "N0CALL-1",
        path: "WIDE1-1,WIDE2-1",
        message: "Roger that",
      });
    });
  });

  it("uses backend-normalized persisted packets on ReceivedWindow startup", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") {
        return Promise.resolve({
          source: "WT1W-15",
          path: "WIDE1-1,WIDE2-1",
          show_ack_messages: false,
          theme_mode: "system",
        });
      }
      if (cmd === "load_received_messages") {
        return Promise.resolve([
          {
            id: 7,
            timestamp: Date.now(),
            from: "WHO-IS",
            to: "WT1W-15",
            body: "?APRSP",
            aprsType: "qry",
            isBulletin: false,
            micEDecoded: false,
          },
        ]);
      }
      if (cmd === "send_message") {
        return Promise.resolve({
          id: 18,
          status: "queued",
          bytes: 20,
          payload_hex: "445566",
        });
      }
      return Promise.resolve(undefined);
    });

    render(<ReceivedWindow />);

    const body = await screen.findByText("?APRSP");
    expect(screen.getByText("qry")).toBeInTheDocument();

    fireEvent.click(body.closest("button") as HTMLButtonElement);

    const replyToInput = await screen.findByPlaceholderText("Callsign") as HTMLInputElement;
    expect(replyToInput.value).toBe("WHO-IS");
  });

  it("locks controls in browser preview mode", async () => {
    (globalThis as any).isTauri = false;
    render(<App />);

    expect(await screen.findByText(/Browser preview mode detected/)).toBeInTheDocument();
    const tncButtons = screen.getAllByRole("button", { name: "Connect TNC" });
    const aprsisButtons = screen.getAllByRole("button", { name: "Connect APRS-IS" });
    expect(tncButtons[tncButtons.length - 1]).toBeDisabled();
    expect(aprsisButtons[aprsisButtons.length - 1]).toBeDisabled();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("locks desktop controls when remote web control is enabled", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_received_messages") {
        return Promise.resolve([]);
      }
      if (cmd === "get_web_server_status") {
        return Promise.resolve({ enabled: true, port: 18420, url: "http://127.0.0.1:18420" });
      }
      if (cmd === "get_manual_beacon_cooldown_remaining") {
        return Promise.resolve(0);
      }
      return Promise.resolve(undefined);
    });

    render(<App />);

    expect(await screen.findByText(/Desktop controls are locked while remote control is enabled/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect TNC" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Connect APRS-IS" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Disable Remote" })).toBeEnabled();
  });

  it("opens the remote UI in the system browser", async () => {
    openUrlMock.mockReset();
    openUrlMock.mockResolvedValue(undefined);
    const windowOpenSpy = vi.spyOn(window, "open").mockImplementation(() => null);
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_received_messages") {
        return Promise.resolve([]);
      }
      if (cmd === "get_web_server_status") {
        return Promise.resolve({ enabled: true, port: 18420, url: "http://127.0.0.1:18420" });
      }
      if (cmd === "get_manual_beacon_cooldown_remaining") {
        return Promise.resolve(0);
      }
      return Promise.resolve(undefined);
    });

    render(<App />);

    fireEvent.click(await screen.findByRole("button", { name: "Open Remote UI" }));

    await waitFor(() => expect(openUrlMock).toHaveBeenCalledWith("http://127.0.0.1:18420"));
    expect(windowOpenSpy).not.toHaveBeenCalledWith("http://127.0.0.1:18420", "_blank", expect.anything());
    windowOpenSpy.mockRestore();
  });
});

describe("PreferencesWindow", () => {
  it("closes the window when Escape is pressed", async () => {
    destroyWindowMock.mockClear();
    render(<PreferencesWindow />);
    await screen.findByRole("heading", { name: "Station" });

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => expect(destroyWindowMock).toHaveBeenCalled());
  });

  it("loads blank first-run fields and saves normalized preferences", async () => {
    invokeMock.mockImplementation((cmd: string, payload?: any) => {
      if (cmd === "load_settings") {
        return Promise.resolve({
          ...DEFAULT_SETTINGS,
          source: "",
          path: "",
          host: "",
          port: "",
          aprsis_host: "",
          aprsis_port: "",
          aprsis_passcode: "",
          aprsis_filter: "",
          beacon_symbol: "",
          web_server_port: "",
        });
      }
      if (cmd === "save_settings") {
        return Promise.resolve(payload?.settings);
      }
      return Promise.resolve(undefined);
    });

    render(<PreferencesWindow />);

    const callsignInput = await screen.findByPlaceholderText("W1AW-9") as HTMLInputElement;
    expect(callsignInput.value).toBe("");

    fireEvent.change(callsignInput, { target: { value: "ab1cde-7" } });
    expect(callsignInput.value).toBe("AB1CDE-7");
    // The guidance stays on screen once the field has text.
    expect(screen.getByText("Your callsign with SSID, e.g. W1AW-9")).toBeInTheDocument();

    const pathInput = screen.getByPlaceholderText("WIDE1-1,WIDE2-1") as HTMLInputElement;
    fireEvent.change(pathInput, { target: { value: "wide1-1,wide2-2" } });
    expect(pathInput.value).toBe("WIDE1-1,WIDE2-2");

    const hostInput = screen.getByPlaceholderText("127.0.0.1") as HTMLInputElement;
    fireEvent.change(hostInput, { target: { value: "localhost" } });

    const filterInput = screen.getByPlaceholderText("Optional APRS-IS filter") as HTMLInputElement;
    fireEvent.change(filterInput, { target: { value: "#filter m/50" } });
    fireEvent.blur(filterInput);
    expect(filterInput.value).toBe("m/50");

    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => {
      expect(invokeMock).toHaveBeenCalledWith("save_settings", {
        settings: expect.objectContaining({
          source: "AB1CDE-7",
          path: "WIDE1-1,WIDE2-2",
          host: "localhost",
          aprsis_filter: "m/50",
        }),
      });
    });
  });

  it("refuses to save when callsign is empty", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "load_settings") {
        return Promise.resolve({
          ...DEFAULT_SETTINGS,
          source: "",
          path: "",
          host: "",
          port: "",
          aprsis_host: "",
          aprsis_port: "",
          aprsis_passcode: "",
          aprsis_filter: "",
          beacon_symbol: "",
          web_server_port: "",
        });
      }
      return Promise.resolve(undefined);
    });

    render(<PreferencesWindow />);

    await screen.findByPlaceholderText("W1AW-9");
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByText("Callsign is required.")).toBeInTheDocument();
    expect(invokeMock).not.toHaveBeenCalledWith("save_settings", expect.anything());
  });
});

describe("LogWindow", () => {
  it("summarizes third-party wrapped APRS payloads using the inner body", async () => {
    render(<LogWindow />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "WZ0C-4>APN20H:}WHO-IS>APJIW4,TCPIP,WZ0C-4*::WT1W-15  :?APRSP",
        payload_hex: "EEFF",
        bytes: 28,
        source: "WZ0C-4",
        destination: "APN20H",
        path: ["WIDE1-1"],
        from: "WHO-IS",
        to: "WT1W-15",
        body: "?APRSP",
        aprsType: "qry",
        isBulletin: false,
        micEDecoded: false,
        summary_source: "WHO-IS",
        summary_destination: "APJIW4",
      },
    });

    expect(
      await screen.findByText(/RX 28B WHO-IS > APJIW4 via WIDE1-1: \?APRSP \[EEFF\]/),
    ).toBeInTheDocument();
  });

  it("drops malformed live receive payloads and logs an RX error", async () => {
    render(<LogWindow />);

    let messageHandler: ((event: unknown) => void) | undefined;
    await waitFor(() => {
      const messageCall = listenMock.mock.calls.find((call: any) => call[0] === "tnc_message");
      expect(messageCall?.[1]).toBeTruthy();
      messageHandler = messageCall?.[1] as (event: unknown) => void;
    });

    messageHandler?.({
      payload: {
        text: "broken-packet",
        payload_hex: "DEAD",
        bytes: 4,
        source: "BROKEN",
      },
    });

    expect(await screen.findByText("[RX ERROR] malformed receive payload dropped")).toBeInTheDocument();
    expect(screen.queryByText(/RX 4B/)).not.toBeInTheDocument();
    expect(screen.queryByText("broken-packet")).not.toBeInTheDocument();
  });

  it("shows main-window log lines once and skips copies of backend events", async () => {
    render(<LogWindow />);

    // The log window renders backend events from its own listeners, so the
    // main window's republished copy of one must not appear a second time.
    publishUiLog("status", "aprsis: aprsis_verified (copy)", { fromBackendEvent: true });
    publishUiLog("tx", "Beacon sent via APRS-IS: 0.0000,0.0000 sym=/-");

    expect(await screen.findByText("Beacon sent via APRS-IS: 0.0000,0.0000 sym=/-")).toBeInTheDocument();
    expect(screen.queryByText("aprsis: aprsis_verified (copy)")).not.toBeInTheDocument();
  });
});

describe("HelpWindow", () => {
  it("renders the help overview and lets users switch topics", async () => {
    render(<HelpWindow />);

    expect(await screen.findByText("APRSWolf User Guide")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Overview" })).toBeInTheDocument();
    expect(screen.getByText(/APRSWolf is a desktop APRS messaging app for macOS, Windows, and Linux/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Remote Web Control/i }));

    expect(await screen.findByRole("heading", { name: "Remote Web Control" })).toBeInTheDocument();
    expect(screen.getAllByText(/browser while the desktop window stays visible but locked/i).length).toBeGreaterThan(0);
  });

  it("filters help topics by search text", async () => {
    render(<HelpWindow />);

    const search = await screen.findByLabelText("Search help");
    fireEvent.change(search, { target: { value: "beacon" } });

    expect(screen.getByRole("button", { name: /Station Setup/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /TNC Mode/i })).not.toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /Station Setup/i }));
    expect(await screen.findByText(/auto-beacon behavior/i)).toBeInTheDocument();
  });
});
