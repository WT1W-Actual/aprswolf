import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { publishUiLog } from "../logBus";
import { nextId } from "./helpers";
import { type ReceivedItem } from "../receivedBus";

// Rows per page from the message history; matches the backend's PAGE_SIZE.
export const PAGE_SIZE = 200;

/** Any message row from the history: received or sent. */
type HistoryItem = { id: number; uuid?: string; seq?: number };

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

/** Smallest stored sequence among `items`: the cursor for the next older page. */
export function oldestSeq(items: HistoryItem[]): number | undefined {
  let oldest: number | undefined;
  for (const item of items) {
    if (typeof item.seq === "number" && (oldest === undefined || item.seq < oldest)) {
      oldest = item.seq;
    }
  }
  return oldest;
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
        beforeSeq: oldestSeq(results),
      });
      setResults((prev) => dedupeByUuid([...(prev ?? []), ...withFreshIds(hits)]));
      setHasMore(hits.length >= PAGE_SIZE);
    } catch (error) {
      publishUiLog("error", `search failed: ${String(error)}`);
    }
  }

  return { results, hasMore, loadMore };
}

