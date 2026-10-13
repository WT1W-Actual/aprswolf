import { describe, expect, it } from "vitest";
import { buildExportFilter, exportFileName } from "./ExportWindow";

const blank = {
  direction: "both" as const,
  newOnly: false,
  fromDate: "",
  toDate: "",
  callsigns: "",
  includeShortLived: false,
};

describe("buildExportFilter", () => {
  it("leaves every limit off for a blank form", () => {
    expect(buildExportFilter(blank)).toEqual({
      direction: "both",
      newOnly: false,
      fromMs: null,
      toMs: null,
      callsigns: [],
      includeShortLived: false,
    });
  });

  it("turns local dates into an inclusive day range", () => {
    const filter = buildExportFilter({ ...blank, fromDate: "2026-10-01", toDate: "2026-10-12" });
    expect(filter.fromMs).toBe(new Date(2026, 9, 1).getTime());
    expect(filter.toMs).toBe(new Date(2026, 9, 13).getTime());
  });

  it("splits callsigns on commas and spaces and uppercases them", () => {
    expect(buildExportFilter({ ...blank, callsigns: " k6test, n6test-7  w6test-1," }).callsigns).toEqual([
      "K6TEST",
      "N6TEST-7",
      "W6TEST-1",
    ]);
  });
});

describe("exportFileName", () => {
  it("ends with the local date and hhmmss", () => {
    expect(exportFileName(new Date(2026, 9, 12, 9, 5, 7))).toBe("aprswolf-messages-2026-10-12-090507.csv");
  });
});
