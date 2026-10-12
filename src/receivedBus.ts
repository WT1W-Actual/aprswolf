export type ReceivedItem = {
  id: number;
  // Stable identity and position in the message history; absent only when
  // the store could not record the packet.
  uuid?: string;
  seq?: number;
  timestamp: number;
  from: string;
  to?: string;
  body: string;
  aprsType?: string;
  isBulletin?: boolean;
  micEDecoded?: boolean;
  text?: string;
  source?: string;
  destination?: string;
  path?: string[];
  info_text?: string;
  mic_e_text?: string;
  protocol_source?: string;
  protocol_destination?: string;
  protocol_path?: string[];
  protocol_info_text?: string;
};

type ReceivedSyncEvent =
  | {
      type: "snapshot";
      items: ReceivedItem[];
    }
  | {
      type: "window-state";
      open: boolean;
    };

const CHANNEL_NAME = "aprswolf-received-sync";
const WINDOW_STATE_KEY = "aprswolf-received-window-open";

// One long-lived publish channel instead of create+close per message —
// bursts on a busy feed no longer churn channel objects. (BroadcastChannel
// never delivers to the publishing object itself, so sharing is safe.)
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

function postEvent(payload: ReceivedSyncEvent): void {
  try {
    getPublishChannel()?.postMessage(payload);
  } catch {
    // BroadcastChannel is optional in some runtimes.
  }
}

function subscribe(listener: (payload: ReceivedSyncEvent) => void): () => void {
  try {
    const channel = new BroadcastChannel(CHANNEL_NAME);
    channel.onmessage = (event: MessageEvent<ReceivedSyncEvent>) => {
      listener(event.data);
    };
    return () => {
      channel.close();
    };
  } catch {
    return () => {};
  }
}

export function publishReceivedSnapshot(items: ReceivedItem[]): void {
  postEvent({ type: "snapshot", items });
}

export function subscribeReceivedSnapshots(listener: (items: ReceivedItem[]) => void): () => void {
  return subscribe((payload) => {
    if (payload.type === "snapshot") {
      listener(payload.items);
    }
  });
}

export function publishReceivedWindowState(open: boolean): void {
  try {
    if (open) {
      window.localStorage.setItem(WINDOW_STATE_KEY, "1");
    } else {
      window.localStorage.removeItem(WINDOW_STATE_KEY);
    }
  } catch {
    // Ignore storage issues and still broadcast the state.
  }
  postEvent({ type: "window-state", open });
}

export function isReceivedWindowOpen(): boolean {
  try {
    return window.localStorage.getItem(WINDOW_STATE_KEY) === "1";
  } catch {
    return false;
  }
}

export function subscribeReceivedWindowState(listener: (open: boolean) => void): () => void {
  return subscribe((payload) => {
    if (payload.type === "window-state") {
      listener(payload.open);
    }
  });
}