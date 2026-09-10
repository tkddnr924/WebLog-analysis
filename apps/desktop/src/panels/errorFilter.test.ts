import { describe, expect, it } from "vitest";
import { composeErrorFilter, type ErrorForm } from "./errorFilter";
import { openScope } from "../lib/scope";
import { emptyFilter, type FilterExpr, type LogFilter } from "../types";

const form = (over: Partial<ErrorForm> = {}): ErrorForm => ({ q: "", ...over });

const search = (q: string): FilterExpr => ({
  kind: "or",
  items: [
    { kind: "cond", field: "message", op: "icontains", value: q },
    { kind: "cond", field: "client_ip", op: "eq", value: q },
  ],
});

describe("composeErrorFilter", () => {
  it("always pins the error kind and keeps the rule's own conditions", () => {
    const rule: FilterExpr = { kind: "cond", field: "level", op: "regex", value: "(?i)crit" };
    const base: LogFilter = { ...emptyFilter(), active_only: true, bookmarked_only: true, expr: rule };
    const f = composeErrorFilter(base, form(), openScope);
    expect(f.log_kind).toBe("error");
    expect(f.active_only).toBe(true);
    expect(f.bookmarked_only).toBe(true);
    expect(f.expr).toEqual(rule);
  });

  it("matches the search text against the message or the client ip", () => {
    expect(composeErrorFilter(emptyFilter(), form({ q: "  No such file  " }), openScope).expr).toEqual(search("No such file"));
    // 주소를 넣어도 같은 식이므로 IPv4·IPv6 구분 없이 클라이언트로 찾는다.
    expect(composeErrorFilter(emptyFilter(), form({ q: "2001:db8::1" }), openScope).expr).toEqual(search("2001:db8::1"));
  });

  it("ands the search with the rule condition", () => {
    const rule: FilterExpr = { kind: "cond", field: "level", op: "eq", value: "error" };
    const f = composeErrorFilter({ ...emptyFilter(), expr: rule }, form({ q: "refused" }), openScope);
    expect(f.expr).toEqual({ kind: "and", items: [rule, search("refused")] });
  });

  it("keeps the sidebar range above the rule's own range", () => {
    const base: LogFilter = { ...emptyFilter(), time_from_micros: 111, time_to_micros: 222 };
    const f = composeErrorFilter(base, form({ q: "refused" }), { from: 1_704_038_400_000_000, to: null, ips: [] });
    expect(f.time_from_micros).toBe(1_704_038_400_000_000);
    expect(f.time_to_micros).toBeNull();
  });
});
