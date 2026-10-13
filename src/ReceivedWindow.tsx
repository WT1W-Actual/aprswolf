import { FormEvent, useEffect, useMemo, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { publishUiLog } from "./logBus";
import {
  isAckMessageBody,
  matchesStationAddress,
  normalizeOutboundCallsignInput,
} from "./lib/helpers";
import { useTheme } from "./lib/useTheme";
import { useToMeUnread } from "./lib/useToMeUnread";
import {
  PAGE_SIZE,
  clearMessages,
  dedupeByUuid,
  dropCleared,
  formatReceivedTime,
  insertRestored,
  markCleared,
  markRestored,
  mergeTimeline,
  restoreMessages,
  type MessageGroups,
  type TimelineEntry,
  oldestSeq,
  useMessageSearch,
  withFreshIds,
} from "./lib/messageHistory";
import { useWindowFocused } from "./lib/useWindowFocused";
import UnreadDot from "./UnreadDot";
import ClearDialog, { type ClearRequest } from "./ClearDialog";
import MessageRowActions from "./MessageRowActions";
import { rowClass, SelectBox, SelectionHeader } from "./MessageSelection";
import { useSelection } from "./lib/useSelection";
import {
  publishPopoutFocus,
  publishReceivedWindowState,
  subscribeMessagesCleared,
  subscribeMessagesRestored,
  subscribeReceivedSnapshots,
  subscribeSentSnapshots,
  type MessagesCleared,
  type MessagesRestored,
  type ReceivedItem,
  type SentItem,
} from "./receivedBus";
import { FILTER_OPTIONS, type AppSettingsPayload, type ReceivedFilter } from "./lib/types";

// ── Component ────────────────────────────────────────────────

// The pop-out "Messages" window: received and sent messages in one timeline.
function ReceivedWindow() {
  const [receivedItems, setReceivedItems] = useState<ReceivedItem[]>([]);
  // Pages fetched from the message history beyond the newest page, which
  // the main window keeps current through snapshots.
  const [olderItems, setOlderItems] = useState<ReceivedItem[]>([]);
  const [hasOlder, setHasOlder] = useState(false);
  // Sent messages: the main window shares its list, which is kept current
  // with delivery status; older pages come from the history.
  const [sentItems, setSentItems] = useState<SentItem[]>([]);
  const [olderSent, setOlderSent] = useState<SentItem[]>([]);
  const [hasOlderSent, setHasOlderSent] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const search = useMessageSearch(searchQuery);
  const searchResults = search.results;
  const sentSearch = useMessageSearch<SentItem>(searchQuery, "search_sent_messages");
  const [activeReceivedFilters, setActiveReceivedFilters] = useState<ReceivedFilter[]>(["all"]);
  const [showAckMessages, setShowAckMessages] = useState(false);
  const [promiscuousBaseCallsign, setPromiscuousBaseCallsign] = useState(false);
  const [source, setSource] = useState("");
  const [path, setPath] = useState("");
  const [replyOpen, setReplyOpen] = useState(false);
  const [replyTo, setReplyTo] = useState("");
  const [replyMessage, setReplyMessage] = useState("");
  const [replyContext, setReplyContext] = useState("");

  useEffect(() => {
    const handleCloseState = () => publishReceivedWindowState(false);
    let tauriCleanup: (() => void) | undefined;

    publishReceivedWindowState(true);
    window.addEventListener("beforeunload", handleCloseState);
    window.addEventListener("pagehide", handleCloseState);

    (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const currentWindow = getCurrentWindow();
        tauriCleanup = await currentWindow.onCloseRequested(() => {
          handleCloseState();
        });
      } catch {
        // Browser preview mode.
      }
    })();

    return () => {
      window.removeEventListener("beforeunload", handleCloseState);
      window.removeEventListener("pagehide", handleCloseState);
      tauriCleanup?.();
      handleCloseState();
    };
  }, []);

  useTheme();

  // Load persisted settings on startup
  useEffect(() => {
    let mounted = true;
    (async () => {
      try {
        const settings = await invoke<AppSettingsPayload>("load_settings");
        if (settings?.show_ack_messages != null) {
          setShowAckMessages(Boolean(settings.show_ack_messages));
        }
        if (settings?.promiscuous_base_callsign != null) {
          setPromiscuousBaseCallsign(Boolean(settings.promiscuous_base_callsign));
        }
        if (settings?.source) {
          setSource(settings.source.trim().toUpperCase());
        }
        if (settings?.path) {
          setPath(settings.path.trim().toUpperCase());
        }
        if (typeof settings?.msg_font_size === "number" && settings.msg_font_size >= 10 && settings.msg_font_size <= 32) {
          document.documentElement.style.setProperty("--msg-font-size", `${settings.msg_font_size}px`);
        }
      } catch {
        // Browser preview — use defaults
      }
      if (!mounted) return;
    })();
    return () => {
      mounted = false;
    };
  }, []);

  // Load persisted messages on startup
  useEffect(() => {
    (async () => {
      try {
        const msgs = await invoke<ReceivedItem[]>("load_received_messages");
        if (msgs.length > 0) setReceivedItems(withFreshIds(msgs));
        setHasOlder(msgs.length >= PAGE_SIZE);
      } catch {/* no persisted messages */}
      try {
        const sent = await invoke<SentItem[]>("load_sent_messages");
        if (sent.length > 0) setSentItems((prev) => (prev.length > 0 ? prev : withFreshIds(sent)));
        setHasOlderSent(sent.length >= PAGE_SIZE);
      } catch {/* no sent history */}
    })();
  }, []);

  useEffect(() => {
    return subscribeReceivedSnapshots((items) => {
      setReceivedItems(items);
    });
  }, []);

  useEffect(() => {
    return subscribeSentSnapshots((items) => {
      setSentItems(items);
    });
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;

    (async () => {
      try {
        const registered = await (await import("@tauri-apps/api/event")).listen<AppSettingsPayload>("settings_updated", (event) => {
          if (!active) return;
          const settings = event.payload;
          if (settings.show_ack_messages != null) {
            setShowAckMessages(Boolean(settings.show_ack_messages));
          }
          if (settings.promiscuous_base_callsign != null) {
            setPromiscuousBaseCallsign(Boolean(settings.promiscuous_base_callsign));
          }
          setSource((settings.source ?? "").trim().toUpperCase());
          setPath((settings.path ?? "").trim().toUpperCase());
          if (typeof settings.msg_font_size === "number" && settings.msg_font_size >= 10 && settings.msg_font_size <= 32) {
            document.documentElement.style.setProperty("--msg-font-size", `${settings.msg_font_size}px`);
          }
        });
        // Unmounted while listen() was in flight — don't leak the listener.
        if (!active) {
          registered();
          return;
        }
        unlisten = registered;
      } catch {
        // Browser preview mode.
      }
    })();

    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  async function closeWindow() {
    try {
      const { getCurrentWindow } = await import("@tauri-apps/api/window");
      publishReceivedWindowState(false);
      await getCurrentWindow().destroy();
      return;
    } catch {
      publishReceivedWindowState(false);
      window.close();
    }
  }

  function openReplyDialog(item: ReceivedItem) {
    const preview = item.body.replace(/\s+/g, " ").trim().slice(0, 60);
    setReplyTo(normalizeOutboundCallsignInput(item.from));
    setReplyMessage(preview ? `RE: ${preview}` : "");
    setReplyContext(item.body);
    setReplyOpen(true);
  }

  function closeReplyDialog() {
    setReplyOpen(false);
    setReplyTo("");
    setReplyMessage("");
    setReplyContext("");
  }

  async function sendReply(e: FormEvent) {
    e.preventDefault();

    const destinationCallsign = replyTo.trim().toUpperCase();
    if (!destinationCallsign || !replyMessage.trim()) {
      publishUiLog("error", "send failed: destination and message are required");
      return;
    }

    try {
      const sourceCallsign = source.trim().toUpperCase();
      const digiPath = path.trim().toUpperCase();
      const result = await invoke<{ id: number; status: string; bytes: number; payload_hex: string }>("send_message", {
        source: sourceCallsign,
        destination: destinationCallsign,
        path: digiPath,
        message: replyMessage,
      });

      publishUiLog(
        "tx",
        `TX queued #${result.id} ${result.bytes}B to ${destinationCallsign} [${result.payload_hex}] status=${result.status}`,
      );
      closeReplyDialog();
    } catch (error) {
      publishUiLog("error", `send failed: ${String(error)}`);
    }
  }

  const historyItems = useMemo(
    () => dedupeByUuid([...receivedItems, ...olderItems]),
    [olderItems, receivedItems],
  );
  const shownItems = searchResults ?? historyItems;
  const sentHistory = useMemo(
    () => dedupeByUuid([...sentItems, ...olderSent]),
    [olderSent, sentItems],
  );
  const shownSent = sentSearch.results ?? sentHistory;
  const receivedHasMore = searchResults ? search.hasMore : hasOlder;
  const sentHasMore = sentSearch.results ? sentSearch.hasMore : hasOlderSent;

  async function loadOlder() {
    try {
      if (searchResults || sentSearch.results) {
        await Promise.all([
          search.hasMore ? search.loadMore() : null,
          sentSearch.hasMore ? sentSearch.loadMore() : null,
        ]);
        return;
      }
      if (hasOlder) {
        const page = await invoke<ReceivedItem[]>("load_received_messages", {
          beforeSeq: oldestSeq(historyItems),
        });
        setOlderItems((prev) => [...prev, ...withFreshIds(page)]);
        setHasOlder(page.length >= PAGE_SIZE);
      }
      if (hasOlderSent) {
        const page = await invoke<SentItem[]>("load_sent_messages", {
          beforeSeq: oldestSeq(sentHistory),
        });
        setOlderSent((prev) => [...prev, ...withFreshIds(page)]);
        setHasOlderSent(page.length >= PAGE_SIZE);
      }
    } catch (error) {
      publishUiLog("error", `loading older messages failed: ${String(error)}`);
    }
  }

  const canLoadMore = receivedHasMore || sentHasMore;

  // Clearing and restoring messages, from this window or the main one.
  const [clearRequest, setClearRequest] = useState<ClearRequest | null>(null);
  const selection = useSelection(clearRequest !== null);
  const applyClearedRef = useRef<(cleared: MessagesCleared) => void>(() => {});
  applyClearedRef.current = (cleared) => {
    if (cleared.list === "sent") {
      setSentItems((prev) => dropCleared(prev, cleared));
      setOlderSent((prev) => dropCleared(prev, cleared));
      sentSearch.update((prev) => markCleared(prev, cleared));
    } else {
      setReceivedItems((prev) => dropCleared(prev, cleared));
      setOlderItems((prev) => dropCleared(prev, cleared));
      search.update((prev) => markCleared(prev, cleared));
    }
  };
  const applyRestoredRef = useRef<(restored: MessagesRestored) => void>(() => {});
  applyRestoredRef.current = (restored) => {
    const uuids = restored.items.map((item) => item.uuid).filter((uuid): uuid is string => Boolean(uuid));
    if (restored.list === "sent") {
      setSentItems((prev) => insertRestored(prev, restored.items));
      sentSearch.update((prev) => markRestored(prev, uuids));
    } else {
      setReceivedItems((prev) => insertRestored(prev, restored.items));
      search.update((prev) => markRestored(prev, uuids));
    }
  };
  useEffect(() => subscribeMessagesCleared((cleared) => applyClearedRef.current(cleared)), []);
  useEffect(() => subscribeMessagesRestored((restored) => applyRestoredRef.current(restored)), []);

  // Selection keys carry the direction, since this list holds both.
  const entryKey = (entry: TimelineEntry) => `${entry.direction}:${entry.item.uuid ?? ""}`;
  const groupsOf = (entries: TimelineEntry[]): MessageGroups => ({
    sent: entries.flatMap((entry) => (entry.direction === "sent" ? [entry.item] : [])),
    received: entries.flatMap((entry) => (entry.direction === "received" ? [entry.item] : [])),
  });

  // The rows an x or restore control acts on: every selected row when this
  // row is selected, otherwise the row alone.
  function targetsFor(entry: TimelineEntry): TimelineEntry[] {
    if (selection.active && selection.isSelected(entryKey(entry))) {
      return timeline.filter((other) => other.item.uuid && selection.isSelected(entryKey(other)));
    }
    return [entry];
  }

  function askClear(entries: TimelineEntry[], what: string) {
    if (entries.length === 0) return;
    setClearRequest({
      what,
      alreadyHidden: entries.every((entry) => entry.item.hidden),
      run: async (deleted) => {
        try {
          (await clearMessages(groupsOf(entries), deleted)).forEach((cleared) => applyClearedRef.current(cleared));
          selection.exit();
        } catch (error) {
          publishUiLog("error", `clear failed: ${String(error)}`);
        }
      },
    });
  }

  function askClearEntry(entry: TimelineEntry) {
    const targets = targetsFor(entry);
    askClear(targets, targets.length === 1 ? "this message" : `these ${targets.length} messages`);
  }

  // Clear all takes exactly what the list shows now, sent and received,
  // never older history.
  function askClearAll() {
    const shown = timeline.filter((entry) => !entry.item.hidden);
    askClear(shown, `the ${shown.length} message${shown.length === 1 ? "" : "s"} shown`);
  }

  async function restoreEntry(entry: TimelineEntry) {
    try {
      const hidden = targetsFor(entry).filter((target) => target.item.hidden);
      (await restoreMessages(groupsOf(hidden))).forEach((restored) => applyRestoredRef.current(restored));
      selection.exit();
    } catch (error) {
      publishUiLog("error", `restore failed: ${String(error)}`);
    }
  }


  // Counted from the live items the main window shares, so both windows show
  // the same number; focusing this window marks it read in both.
  const unreadToMe = useToMeUnread(receivedItems, source, promiscuousBaseCallsign);

  const focused = useWindowFocused();
  useEffect(() => {
    publishPopoutFocus(focused);
  }, [focused]);
  useEffect(() => () => publishPopoutFocus(false), []);

  function toggleFilter(key: ReceivedFilter) {
    setActiveReceivedFilters((current) => {
      if (key === "all") return ["all"];
      const next = new Set(current.filter((k) => k !== "all"));
      if (next.has(key)) {
        next.delete(key);
      } else {
        next.add(key);
      }
      return next.size > 0 ? Array.from(next) : ["all"];
    });
  }

  // The chips filter received messages. Sent messages are conversation, so
  // they show under All and Messages.
  const showAll = activeReceivedFilters.length === 0 || activeReceivedFilters.includes("all");
  const showSent = showAll || activeReceivedFilters.includes("msg");

  const timeline = useMemo(() => {
    const passes = (item: ReceivedItem) => {
      if (!showAckMessages && isAckMessageBody(item.body)) return false;
      if (showAll) return true;
      const kind = (item.aprsType ?? "other") as ReceivedFilter;
      return activeReceivedFilters.some((filter) => {
        if (filter === "to_me") {
          return matchesStationAddress(item, source, promiscuousBaseCallsign);
        }
        if (filter === "bulletin") return item.isBulletin === true;
        return kind === filter;
      });
    };
    // Merged before filtering, so paging stops at the same point whatever
    // the chips hide.
    return mergeTimeline(shownItems, shownSent, { received: receivedHasMore, sent: sentHasMore }).filter((entry) =>
      entry.direction === "sent" ? showSent : passes(entry.item),
    );
  }, [
    activeReceivedFilters,
    promiscuousBaseCallsign,
    receivedHasMore,
    sentHasMore,
    showAckMessages,
    showAll,
    showSent,
    shownItems,
    shownSent,
    source,
  ]);

  const filterCounts = useMemo(() => {
    const counts: Record<ReceivedFilter, number> = {
      all: shownItems.length + shownSent.length,
      to_me: 0,
      bulletin: 0,
      qry: 0,
      msg: 0,
      wx: 0,
      pos: 0,
      status: 0,
      obj: 0,
      item: 0,
      "mic-e": 0,
      tlm: 0,
      other: 0,
    };

    for (const item of shownItems) {
      const kind = (item.aprsType ?? "other") as ReceivedFilter;
      if (kind in counts && kind !== "all" && kind !== "to_me" && kind !== "bulletin") {
        counts[kind] += 1;
      } else if (!(kind in counts)) {
        counts.other += 1;
      }
      if (item.isBulletin) counts.bulletin += 1;
      if (matchesStationAddress(item, source, promiscuousBaseCallsign)) counts.to_me += 1;
    }

    counts.msg += shownSent.length;
    return counts;
  }, [promiscuousBaseCallsign, shownItems, shownSent, source]);

  const canSend = source.trim().length > 0 && replyTo.trim().length > 0 && replyMessage.trim().length > 0;

  return (
    <div className="received-window">
      <div className="received-window-toolbar">
        <span className="received-window-title">Messages</span>
        <span className="received-window-count">
          {timeline.length} / {shownItems.length + shownSent.length}
        </span>
        <label className="received-window-toggle">
          <input
            type="checkbox"
            checked={showAckMessages}
            onChange={(e) => setShowAckMessages(e.currentTarget.checked)}
          />
          Show ACKs
        </label>
        <SelectionHeader selection={selection} onClearAll={askClearAll} />
        <button type="button" className="received-window-close" onClick={closeWindow}>
          Close Window
        </button>
      </div>

      <div className="received-window-search-row">
        <input
          type="search"
          className="received-window-search"
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.currentTarget.value)}
          placeholder="Search callsigns and message text"
          aria-label="Search message history"
        />
        {searchResults || sentSearch.results ? (
          <span className="received-window-search-note">Searching all history</span>
        ) : null}
      </div>

      <div className="rx-filter-bar received-window-filters" role="group" aria-label="Message filters">
        {FILTER_OPTIONS.map(({ key, label }) => (
          <button
            key={key}
            type="button"
            className={`rx-filter-btn ${activeReceivedFilters.includes(key) ? "active" : ""}`}
            onClick={() => toggleFilter(key)}
            aria-pressed={activeReceivedFilters.includes(key)}
          >
            {label} <span className="rx-filter-count">{filterCounts[key]}</span>
            {key === "to_me" ? <UnreadDot count={unreadToMe} /> : null}
          </button>
        ))}
      </div>

      <div className="received-window-list">
        {timeline.length === 0 ? (
          <p className="received-window-empty">
            {searchResults || sentSearch.results
              ? "No messages match your search."
              : activeReceivedFilters.includes("to_me") && activeReceivedFilters.length === 1
              ? "No messages addressed to you."
              : shownItems.length + shownSent.length === 0
                ? "Waiting for messages…"
                : "No messages for this filter."}
          </p>
        ) : (
          timeline.map((entry) => {
            const { direction, item } = entry;
            const key = entryKey(entry);
            const actions = item.uuid ? (
              <MessageRowActions
                hidden={item.hidden}
                onClear={() => askClearEntry(entry)}
                onRestore={() => void restoreEntry(entry)}
              />
            ) : null;
            return direction === "sent" ? (
              <div key={`sent-${item.uuid ?? item.id}`} className={rowClass(selection, item.hidden)}>
                <SelectBox selection={selection} uuid={item.uuid ? key : undefined} />
                <div
                  className={`msg-item sent-item status-${item.status} received-window-msg-item`}
                  {...(item.uuid ? selection.pressHandlers(key) : {})}
                  onClick={() => {
                    if (item.uuid) selection.handleClick(key);
                  }}
                >
                  <div className="msg-item-top-row">
                    <span className="msg-time">{formatReceivedTime(item.timestamp)}</span>
                    <span className="msg-addr">▶ {item.destination}</span>
                    <span className={`msg-badge ${item.status}`}>{item.status}</span>
                  </div>
                  <div className="msg-body">{item.body}</div>
                </div>
                {actions}
              </div>
            ) : (
              <div key={`received-${item.uuid ?? item.id}`} className={rowClass(selection, item.hidden)}>
                <SelectBox selection={selection} uuid={item.uuid ? key : undefined} />
                <button
                  type="button"
                  className="msg-item recv-item recv-clickable received-window-msg-item"
                  {...(item.uuid ? selection.pressHandlers(key) : {})}
                  onClick={() => {
                    if (item.uuid && selection.handleClick(key)) return;
                    openReplyDialog(item);
                  }}
                  title={selection.active ? undefined : `Reply to ${item.from}`}
                >
                  <div className="msg-item-top-row">
                    <span className="msg-time">{formatReceivedTime(item.timestamp)}</span>
                    <span className="msg-addr">◀ {item.from}</span>
                    {item.to ? <span className="msg-addr">▶ {item.to}</span> : null}
                    {item.aprsType && item.aprsType !== "other" ? (
                      <span className="msg-type-badge">{item.aprsType}</span>
                    ) : null}
                    {item.micEDecoded ? <span className="mic-e-inline-badge">Mic-E</span> : null}
                  </div>
                  <div className="msg-body">{item.body}</div>
                </button>
                {actions}
              </div>
            );
          })
        )}
        {canLoadMore ? (
          <button type="button" className="secondary received-window-load-older" onClick={loadOlder}>
            Load older messages
          </button>
        ) : null}
      </div>

      <ClearDialog request={clearRequest} onClose={() => setClearRequest(null)} />

      {replyOpen ? (
        <div className="reply-overlay" role="dialog" aria-modal="true" aria-label="Reply to message">
          <div className="reply-dialog">
            <h2>Reply</h2>
            <p className="reply-context">Original: {replyContext}</p>
            <form className="grid" onSubmit={sendReply}>
              <label>
                Reply To
                <input
                  value={replyTo}
                  onChange={(e) => setReplyTo(normalizeOutboundCallsignInput(e.currentTarget.value))}
                  placeholder="Callsign"
                />
              </label>
              <label>
                Message
                <input
                  value={replyMessage}
                  onChange={(e) => setReplyMessage(e.currentTarget.value)}
                  placeholder="Reply text"
                  maxLength={67}
                />
              </label>
              <div className="reply-actions">
                <button type="submit" disabled={!canSend}>Send Reply</button>
                <button type="button" className="secondary" onClick={closeReplyDialog}>Cancel</button>
              </div>
            </form>
          </div>
        </div>
      ) : null}
    </div>
  );
}

export default ReceivedWindow;
