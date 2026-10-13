export type ReceivedItem = {
  id: number;
  // Stable identity and position in the message history; absent only when
  // the store could not record the packet.
  uuid?: string;
  seq?: number;
  // Set on search results cleared from the list but kept in the database.
  hidden?: boolean;
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

export type SentItem = {
  // Display id from this window's counter, like received items.
  id: number;
  // Key of the row in the sent history; delivery updates match on it.
  uuid?: string;
  // The backend queue's number for a message sent this session.
  queueId?: number;
  timestamp: number;
  destination: string;
  body: string;
  status: "queued" | "sent" | "acked" | "failed" | "expired";
  // Set on rows loaded from the history: the paging cursor, and whether the
  // message was cleared from the list but kept in the database.
  seq?: number;
  hidden?: boolean;
};

/** Messages cleared from a list: hidden, or deleted from the database. */
export type MessagesCleared = {
  list: "sent" | "received";
  uuids: string[];
  deleted: boolean;
};

/** Hidden messages put back in their list. */
export type MessagesRestored =
  | { list: "sent"; items: SentItem[] }
  | { list: "received"; items: ReceivedItem[] };

type ReceivedSyncEvent =
  | {
      type: "snapshot";
      items: ReceivedItem[];
    }
  | {
      type: "sent-snapshot";
      items: SentItem[];
    }
  | ({ type: "messages-cleared" } & MessagesCleared)
  | { type: "messages-restored"; restored: MessagesRestored }
  | {
      type: "window-state";
      open: boolean;
    }
  | {
      type: "to-me-read";
      at: number;
    }
  | {
      type: "popout-focus";
      focused: boolean;
    };

const CHANNEL_NAME = "aprswolf-received-sync";
const WINDOW_STATE_KEY = "aprswolf-received-window-open";
const TO_ME_READ_KEY = "aprswolf-to-me-read-at";

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

// The main window owns the sent list and its delivery status; the pop-out
// shows it beside received messages.
export function publishSentSnapshot(items: SentItem[]): void {
  postEvent({ type: "sent-snapshot", items });
}

export function subscribeSentSnapshots(listener: (items: SentItem[]) => void): () => void {
  return subscribe((payload) => {
    if (payload.type === "sent-snapshot") {
      listener(payload.items);
    }
  });
}

// Every window drops cleared messages from what it shows. The channel never
// delivers to the sender, so the window that cleared applies it itself too.
export function publishMessagesCleared(cleared: MessagesCleared): void {
  postEvent({ type: "messages-cleared", ...cleared });
}

export function subscribeMessagesCleared(listener: (cleared: MessagesCleared) => void): () => void {
  return subscribe((payload) => {
    if (payload.type === "messages-cleared") {
      const { list, uuids, deleted } = payload;
      listener({ list, uuids, deleted });
    }
  });
}

export function publishMessagesRestored(restored: MessagesRestored): void {
  postEvent({ type: "messages-restored", restored });
}

export function subscribeMessagesRestored(listener: (restored: MessagesRestored) => void): () => void {
  return subscribe((payload) => {
    if (payload.type === "messages-restored") {
      listener(payload.restored);
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

// ── Unread To Me marker ──────────────────────────────────────
// To Me messages newer than this timestamp are unread. Both windows read the
// same marker from localStorage and hear changes over the channel, so viewing
// To Me in either window clears the count in both. It survives a restart, so
// messages that arrived unread before quitting stay unread.

export function getToMeReadAt(): number {
  try {
    const stored = Number(window.localStorage.getItem(TO_ME_READ_KEY));
    if (Number.isFinite(stored) && stored > 0) return stored;
  } catch {
    // Fall through to "now" so stored history is not all unread.
  }
  const now = Date.now();
  try {
    window.localStorage.setItem(TO_ME_READ_KEY, String(now));
  } catch {
    // Ignore storage issues.
  }
  return now;
}

export function markToMeRead(at: number = Date.now()): void {
  try {
    window.localStorage.setItem(TO_ME_READ_KEY, String(at));
  } catch {
    // Ignore storage issues and still broadcast.
  }
  postEvent({ type: "to-me-read", at });
}

export function subscribeToMeRead(listener: (at: number) => void): () => void {
  return subscribe((payload) => {
    if (payload.type === "to-me-read") {
      listener(payload.at);
    }
  });
}

// The pop-out reports its focus so the main window knows when the app as a
// whole is in front.
export function publishPopoutFocus(focused: boolean): void {
  postEvent({ type: "popout-focus", focused });
}

export function subscribePopoutFocus(listener: (focused: boolean) => void): () => void {
  return subscribe((payload) => {
    if (payload.type === "popout-focus") {
      listener(payload.focused);
    }
  });
}
