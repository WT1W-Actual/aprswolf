import { describe, expect, it } from "vitest";
import { isAckMessageBody, matchesStationAddress } from "./helpers";
import { type ReceivedItem } from "../receivedBus";

const item = (over: Partial<ReceivedItem>): ReceivedItem => ({
  id: 1,
  timestamp: 0,
  from: "N0CALL",
  body: "",
  ...over,
});

describe("isAckMessageBody", () => {
  it("matches spec-shaped acks (1-5 alnum)", () => {
    expect(isAckMessageBody("ack1")).toBe(true);
    expect(isAckMessageBody("ack42")).toBe(true);
    expect(isAckMessageBody("ACKAB1")).toBe(true);
    expect(isAckMessageBody(" ack12 ")).toBe(true);
  });

  it("rejects non-acks and over-length ids", () => {
    expect(isAckMessageBody("ack")).toBe(false);
    expect(isAckMessageBody("acknowledge me")).toBe(false);
    expect(isAckMessageBody("acknowledged")).toBe(false);
    expect(isAckMessageBody("ack123456")).toBe(false);
    expect(isAckMessageBody("ack-42")).toBe(false);
    expect(isAckMessageBody("hello")).toBe(false);
  });
});

describe("matchesStationAddress", () => {
  it("matches the addressee field exactly or by base callsign", () => {
    expect(matchesStationAddress(item({ to: "WT1W-15" }), "WT1W-15", false)).toBe(true);
    expect(matchesStationAddress(item({ to: "WT1W" }), "WT1W-15", false)).toBe(false);
    expect(matchesStationAddress(item({ to: "WT1W" }), "WT1W-15", true)).toBe(true);
  });

  it("matches callsigns in the body only on non-alphanumeric boundaries", () => {
    expect(matchesStationAddress(item({ body: "hi W1AW!" }), "W1AW", false)).toBe(true);
    expect(matchesStationAddress(item({ body: "from W1AWZ earlier" }), "W1AW", false)).toBe(false);
    expect(matchesStationAddress(item({ body: "CW1AW" }), "W1AW", false)).toBe(false);
    expect(matchesStationAddress(item({ body: "w1aw qsl?" }), "W1AW", false)).toBe(true);
  });

  it("honors allowBaseCallsign for body matches", () => {
    expect(matchesStationAddress(item({ body: "msg for WT1W" }), "WT1W-15", false)).toBe(false);
    expect(matchesStationAddress(item({ body: "msg for WT1W" }), "WT1W-15", true)).toBe(true);
  });
});
