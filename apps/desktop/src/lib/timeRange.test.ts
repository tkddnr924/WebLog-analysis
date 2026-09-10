import { describe, expect, it } from "vitest";
import { emptyRange, parseRange, withRange } from "./timeRange";
import { emptyFilter } from "../types";

describe("parseRange", () => {
  it("treats an empty box as no bound", () => {
    expect(parseRange(emptyRange)).toEqual({ from: null, to: null });
  });

  it("reads 한국 시간 input as UTC micros", () => {
    expect(parseRange({ from: "2024-01-01T01:00", to: "2024-01-02" })).toEqual({
      from: 1_704_038_400_000_000,
      to: 1_704_121_200_000_000,
    });
  });

  it("rejects garbage and a backwards range", () => {
    expect(typeof parseRange({ from: "yesterday", to: "" })).toBe("string");
    expect(typeof parseRange({ from: "2024-01-02", to: "2024-01-01" })).toBe("string");
  });
});

describe("withRange", () => {
  it("puts the range on top of the rule condition", () => {
    const rule = { ...emptyFilter(), expr: { kind: "cond" as const, field: "status" as const, op: "eq" as const, value: "500" }, active_only: true };
    const f = withRange(rule, { from: 10, to: 20 });
    expect(f.time_from_micros).toBe(10);
    expect(f.time_to_micros).toBe(20);
    expect(f.expr).toEqual(rule.expr);
    expect(f.active_only).toBe(true);
  });

  it("replaces a range the rule carried", () => {
    const f = withRange({ ...emptyFilter(), time_from_micros: 1, time_to_micros: 2 }, { from: null, to: 9 });
    expect(f.time_from_micros).toBeNull();
    expect(f.time_to_micros).toBe(9);
  });
});
