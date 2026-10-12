import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { publishUiLog } from "../logBus";
import { nextId } from "./helpers";
import {
  publishMessagesCleared,
  publishMessagesRestored,
  type MessagesCleared,
  type MessagesRestored,
  type ReceivedItem,
  type SentItem,
} from "../receivedBus";

// Rows per page from the message history; matches the backend's PAGE_SIZE.
export const PAGE_SIZE = 200;

/** Any message row from the history: received or sent. */
type HistoryItem = { id: number; uuid?: string; seq?: number; hidden?: boolean; timestamp: number };

/** Keep the first copy of each stored message, in order. */
export function dedupeByUuid<T extends HistoryItem>(items: T[]): T[] {
  const seen = new Set<string>();
  return items.filter((item) => {
    if (!item.uuid) return true;
    if (seen.has(item.uuid)) return false;
    seen.add(item.uuid);
    return true;
  });
}

/**
 * Where the next older page starts: the time and sequence of the last
 * stored message in `items` when they run newest first by message time.
 */
export function olderPageCursor(items: HistoryItem[]): { beforeTime?: number; beforeSeq?: number } {
  let last: HistoryItem | undefined;
  for (const item of items) {
    if (typeof item.seq !== "number") continue;
    if (
      !last ||
      item.timestamp < last.timestamp ||
      (item.timestamp === last.timestamp && item.seq < (last.seq as number))
    ) {
      last = item;
    }
  }
  return last ? { beforeTime: last.timestamp, beforeSeq: last.seq } : {};
}

/** Time alone for today's messages; date and time once history spans days. */
export function formatReceivedTime(timestamp: number): string {
  const when = new Date(timestamp);
  const sameDay = when.toDateString() === new Date().toDateString();
  return sameDay ? when.toLocaleTimeString() : when.toLocaleString();
}

export function withFreshIds<T extends HistoryItem>(items: T[]): T[] {
  return items.map((item) => ({ ...item, id: nextId() }));
}

export type MessageSearch<T extends HistoryItem = ReceivedItem> = {
  /** Matches across the whole stored history, or null while the box is empty. */
  results: T[] | null;
  hasMore: boolean;
  loadMore: () => Promise<void>;
  /** Change the current results in place, as when messages are cleared. */
  update: (change: (items: T[]) => T[]) => void;
};

/**
 * Full-history search for what is typed in a search box. The caller owns the
 * query, so one box can drive a received and a sent search together.
 */
export function useMessageSearch<T extends HistoryItem = ReceivedItem>(
  query: string,
  command: "search_received_messages" | "search_sent_messages" = "search_received_messages",
): MessageSearch<T> {
  const [results, setResults] = useState<T[] | null>(null);
  const [hasMore, setHasMore] = useState(false);

  // Search once typing pauses.
  useEffect(() => {
    const trimmed = query.trim();
    if (!trimmed) {
      setResults(null);
      setHasMore(false);
      return;
    }
    let current = true;
    const timer = window.setTimeout(async () => {
      try {
        const hits = await invoke<T[]>(command, { query: trimmed });
        if (!current) return;
        setResults(withFreshIds(hits));
        setHasMore(hits.length >= PAGE_SIZE);
      } catch (error) {
        if (!current) return;
        setResults([]);
        setHasMore(false);
        publishUiLog("error", `search failed: ${String(error)}`);
      }
    }, 250);
    return () => {
      current = false;
      window.clearTimeout(timer);
    };
  }, [query, command]);

  async function loadMore() {
    if (!results) return;
    try {
      const hits = await invoke<T[]>(command, {
        query: query.trim(),
        ...olderPageCursor(results),
      });
      setResults((prev) => dedupeByUuid([...(prev ?? []), ...withFreshIds(hits)]));
      setHasMore(hits.length >= PAGE_SIZE);
    } catch (error) {
      publishUiLog("error", `search failed: ${String(error)}`);
    }
  }

  function update(change: (items: T[]) => T[]) {
    setResults((prev) => (prev ? change(prev) : prev));
  }

  return { results, hasMore, loadMore, update };
}

export type TimelineEntry =
  | { direction: "received"; item: ReceivedItem }
  | { direction: "sent"; item: SentItem };

/**
 * Received and sent messages in one list, newest first. Each side is loaded
 * a page at a time, so while a side has older messages still unloaded the
 * list stops at that side's oldest loaded message: anything older from the
 * other side would otherwise appear before messages that belong ahead of it.
 */
export function mergeTimeline(
  received: ReceivedItem[],
  sent: SentItem[],
  more: { received: boolean; sent: boolean },
): TimelineEntry[] {
  const oldest = (items: { timestamp: number }[]) =>
    items.reduce((min, item) => Math.min(min, item.timestamp), Infinity);
  let cutoff = -Infinity;
  if (more.received && received.length > 0) cutoff = Math.max(cutoff, oldest(received));
  if (more.sent && sent.length > 0) cutoff = Math.max(cutoff, oldest(sent));
  const entries: TimelineEntry[] = [
    ...received.map((item) => ({ direction: "received" as const, item })),
    ...sent.map((item) => ({ direction: "sent" as const, item })),
  ];
  return entries
    .filter((entry) => entry.item.timestamp >= cutoff)
    .sort((a, b) => b.item.timestamp - a.item.timestamp);
}

function isCleared(item: HistoryItem, cleared: MessagesCleared): boolean {
  return Boolean(item.uuid && cleared.uuids.includes(item.uuid));
}

/** A list without the cleared messages. */
export function dropCleared<T extends HistoryItem>(items: T[], cleared: MessagesCleared): T[] {
  return items.filter((item) => !isCleared(item, cleared));
}

/**
 * Search results after a clear: deleted messages go, hidden ones stay and
 * are marked hidden, since search still finds them.
 */
export function markCleared<T extends HistoryItem>(items: T[], cleared: MessagesCleared): T[] {
  if (cleared.deleted) return dropCleared(items, cleared);
  return items.map((item) => (isCleared(item, cleared) ? { ...item, hidden: true } : item));
}

/**
 * A list with restored messages back in place, newest first. One older than
 * everything loaded belongs to a page not loaded yet, so it waits there.
 */
export function insertRestored<T extends HistoryItem & { timestamp: number }>(items: T[], restored: T[]): T[] {
  const present = new Set(items.map((item) => item.uuid).filter(Boolean));
  const oldest = items.reduce((min, item) => Math.min(min, item.timestamp), Infinity);
  const back = restored
    .filter((item) => !item.uuid || !present.has(item.uuid))
    .filter((item) => items.length === 0 || item.timestamp >= oldest)
    .map((item) => ({ ...item, hidden: false, id: nextId() }));
  if (back.length === 0) return items;
  return [...items, ...back].sort((a, b) => b.timestamp - a.timestamp);
}

/** Search results with restored messages no longer marked hidden. */
export function markRestored<T extends HistoryItem>(items: T[], uuids: string[]): T[] {
  return items.map((item) => (item.uuid && uuids.includes(item.uuid) ? { ...item, hidden: false } : item));
}

/** Messages to clear or restore, grouped by the list they belong to. */
export type MessageGroups = { sent: SentItem[]; received: ReceivedItem[] };

const uuidsOf = (items: HistoryItem[]) => items.map((item) => item.uuid).filter((uuid): uuid is string => Boolean(uuid));

/** Clear messages in the store, then tell every window. Returns what changed. */
export async function clearMessages(groups: MessageGroups, deleted: boolean): Promise<MessagesCleared[]> {
  const done: MessagesCleared[] = [];
  for (const list of ["sent", "received"] as const) {
    const uuids = uuidsOf(groups[list]);
    if (uuids.length === 0) continue;
    await invoke("clear_messages", { list, uuids, delete: deleted });
    const cleared = { list, uuids, deleted };
    publishMessagesCleared(cleared);
    done.push(cleared);
  }
  return done;
}

/** Unhide messages in the store, then tell every window. */
export async function restoreMessages(groups: MessageGroups): Promise<MessagesRestored[]> {
  const done: MessagesRestored[] = [];
  if (groups.sent.length > 0) {
    await invoke("restore_messages", { list: "sent", uuids: uuidsOf(groups.sent) });
    done.push({ list: "sent", items: groups.sent });
  }
  if (groups.received.length > 0) {
    await invoke("restore_messages", { list: "received", uuids: uuidsOf(groups.received) });
    done.push({ list: "received", items: groups.received });
  }
  done.forEach(publishMessagesRestored);
  return done;
}
