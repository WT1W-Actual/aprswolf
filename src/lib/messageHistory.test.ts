import { describe, expect, it } from "vitest";
import { mergeTimeline } from "./messageHistory";
import { type ReceivedItem, type SentItem } from "../receivedBus";

const rx = (id: number, timestamp: number): ReceivedItem => ({ id, timestamp, from: "N6TEST-7", body: `rx ${id}` });
const tx = (id: number, timestamp: number): SentItem => ({
  id,
  timestamp,
  destination: "N6TEST-7",
  body: `tx ${id}`,
  status: "acked",
});

const order = (entries: ReturnType<typeof mergeTimeline>) =>
  entries.map(({ direction, item }) => `${direction === "sent" ? "tx" : "rx"}${item.id}`);

describe("mergeTimeline", () => {
  it("interleaves received and sent newest first", () => {
    const merged = mergeTimeline([rx(1, 100), rx(2, 300)], [tx(3, 200), tx(4, 400)], { received: false, sent: false });
    expect(order(merged)).toEqual(["tx4", "rx2", "tx3", "rx1"]);
  });

  it("stops at the oldest loaded message of a side with more to load", () => {
    // Received has older pages unloaded below 300, so sent at 200 and 100
    // cannot be placed yet: an unloaded received message may sit between.
    const merged = mergeTimeline([rx(1, 300), rx(2, 500)], [tx(3, 100), tx(4, 200), tx(5, 400)], {
      received: true,
      sent: false,
    });
    expect(order(merged)).toEqual(["rx2", "tx5", "rx1"]);
  });

  it("uses the later cutoff when both sides have more", () => {
    const merged = mergeTimeline([rx(1, 300)], [tx(2, 200)], { received: true, sent: true });
    expect(order(merged)).toEqual(["rx1"]);
  });
});
