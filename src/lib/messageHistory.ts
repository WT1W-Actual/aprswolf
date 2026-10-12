import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { publishUiLog } from "../logBus";
import { nextId } from "./helpers";
import { type ReceivedItem } from "../receivedBus";

// Rows per page from the message history; matches the backend's PAGE_SIZE.
export const PAGE_SIZE = 200;

/** Keep the first copy of each stored message, in order. */
export function dedupeByUuid(items: ReceivedItem[]): ReceivedItem[] {
  const seen = new Set<string>();
  return items.filter((item) => {
    if (!item.uuid) return true;
    if (seen.has(item.uuid)) return false;
    seen.add(item.uuid);
    return true;
  });
}

/** Smallest stored sequence among `items`: the cursor for the next older page. */
export function oldestSeq(items: ReceivedItem[]): number | undefined {
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

export function withFreshIds(items: ReceivedItem[]): ReceivedItem[] {
  return items.map((item) => ({ ...item, id: nextId() }));
}

export type MessageSearch = {
  query: string;
  setQuery: (query: string) => void;
  /** Matches across the whole stored history, or null while the box is empty. */
  results: ReceivedItem[] | null;
  hasMore: boolean;
  loadMore: () => Promise<void>;
};

/** Full-history search behind a search box, shared by the main window and the pop-out. */
export function useMessageSearch(): MessageSearch {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ReceivedItem[] | null>(null);
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
        const hits = await invoke<ReceivedItem[]>("search_received_messages", { query: trimmed });
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
  }, [query]);

  async function loadMore() {
    if (!results) return;
    try {
      const hits = await invoke<ReceivedItem[]>("search_received_messages", {
        query: query.trim(),
        beforeSeq: oldestSeq(results),
      });
      setResults((prev) => dedupeByUuid([...(prev ?? []), ...withFreshIds(hits)]));
      setHasMore(hits.length >= PAGE_SIZE);
    } catch (error) {
      publishUiLog("error", `search failed: ${String(error)}`);
    }
  }

  return { query, setQuery, results, hasMore, loadMore };
}
