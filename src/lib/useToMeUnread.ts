import { useEffect, useMemo, useState } from "react";
import { countUnreadToMe } from "./helpers";
import { useWindowFocused } from "./useWindowFocused";
import { getToMeReadAt, markToMeRead, subscribeToMeRead, type ReceivedItem } from "../receivedBus";

/**
 * Count of unread To Me messages, shared by the main window and the pop-out.
 *
 * Unread means it arrived while APRSWolf was in the background. Whichever of
 * its windows has focus marks everything read, so the chip dot and the app
 * icon badge only ever show while you're working in another app.
 */
export function useToMeUnread(
  items: ReceivedItem[],
  station: string,
  allowBaseCallsign: boolean,
): number {
  const [readAt, setReadAt] = useState(() => getToMeReadAt());
  const focused = useWindowFocused();

  useEffect(() => subscribeToMeRead(setReadAt), []);

  const unread = useMemo(
    () => countUnreadToMe(items, station, allowBaseCallsign, readAt),
    [items, station, allowBaseCallsign, readAt],
  );

  useEffect(() => {
    if (!focused || unread === 0) return;
    const now = Date.now();
    setReadAt(now);
    markToMeRead(now);
  }, [focused, unread]);

  return unread;
}
