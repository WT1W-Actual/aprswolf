export type LogKind = "status" | "rx" | "tx" | "error";

export type UiLogPayload = {
  kind: LogKind;
  text: string;
  time: string;
  // Set when the line was built from a backend event (tnc_status,
  // tnc_message, tnc_delivery, aprsis_status). The Traffic Log window
  // listens to those events itself, so it skips these to avoid duplicates.
  fromBackendEvent?: boolean;
};

const CHANNEL_NAME = "aprswolf-traffic-log";

// One long-lived publish channel instead of create+close per log line.
let publishChannel: BroadcastChannel | null = null;

function getPublishChannel(): BroadcastChannel | null {
  if (publishChannel) return publishChannel;
  try {
    publishChannel = new BroadcastChannel(CHANNEL_NAME);
    return publishChannel;
  } catch {
    return null;
  }
}

function createTimestamp(): string {
  return new Date().toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function makeUiLogPayload(kind: LogKind, text: string): UiLogPayload {
  return {
    kind,
    text,
    time: createTimestamp(),
  };
}

export function publishUiLog(
  kind: LogKind,
  text: string,
  options: { fromBackendEvent?: boolean } = {},
): UiLogPayload {
  const payload: UiLogPayload = { ...makeUiLogPayload(kind, text), ...options };
  try {
    getPublishChannel()?.postMessage(payload);
  } catch {
    // BroadcastChannel may be unavailable in some runtimes; the caller still gets the payload.
  }
  return payload;
}

export function subscribeUiLog(listener: (payload: UiLogPayload) => void): () => void {
  try {
    const channel = new BroadcastChannel(CHANNEL_NAME);
    channel.onmessage = (event: MessageEvent<UiLogPayload>) => {
      listener(event.data);
    };
    return () => {
      channel.close();
    };
  } catch {
    return () => {};
  }
}