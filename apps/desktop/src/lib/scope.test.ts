import { describe, expect, it } from "vitest";
import { applyScope, normalizeIpPattern, openScope } from "./scope";
import { emptyFilter, type FilterExpr, type LogFilter } from "../types";

const rule: FilterExpr = { kind: "cond", field: "status", op: "eq", value: "500" };

describe("normalizeIpPattern", () => {
  it("takes an address or a prefix with a trailing star", () => {
    expect(normalizeIpPattern(" 10.0.0.1 ")).toBe("10.0.0.1");
    expect(normalizeIpPattern("1.1.*")).toBe("1.1.*");
    expect(normalizeIpPattern("2001:db8::*")).toBe("2001:db8::*");
  });

  it("rejects an empty, star-only or malformed pattern", () => {
    for (const bad of ["", "  ", "*", "1.*.3", "10.0.0.1;drop", "1.1.**"]) {
      expect(normalizeIpPattern(bad)).toBeNull();
    }
  });
});

describe("applyScope", () => {
  it("keeps the filter as is when nothing is set", () => {
    const f: LogFilter = { ...emptyFilter(), expr: rule };
    expect(applyScope(f, openScope)).toEqual({ ...f, time_from_micros: null, time_to_micros: null });
  });

  it("puts the range and the whitelist above the rule", () => {
    const f = applyScope({ ...emptyFilter(), expr: rule }, { from: 10, to: 20, ips: ["10.0.0.1", "1.1.*"] });
    expect(f.time_from_micros).toBe(10);
    expect(f.time_to_micros).toBe(20);
    expect(f.expr).toEqual({
      kind: "and",
      items: [
        rule,
        {
          kind: "not",
          item: {
            kind: "or",
            items: [
              { kind: "cond", field: "client_ip", op: "eq", value: "10.0.0.1" },
              { kind: "cond", field: "client_ip", op: "starts_with", value: "1.1." },
            ],
          },
        },
      ],
    });
  });

  it("excludes the whitelist even when the rule has no condition", () => {
    const f = applyScope(emptyFilter(), { from: null, to: null, ips: ["203.0.113.7"] });
    expect(f.expr).toEqual({ kind: "not", item: { kind: "cond", field: "client_ip", op: "eq", value: "203.0.113.7" } });
  });
});
