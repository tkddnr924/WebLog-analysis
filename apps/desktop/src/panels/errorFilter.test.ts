import { describe, expect, it } from "vitest";
import { composeErrorFilter, type ErrorForm } from "./errorFilter";
import { emptyFilter, type FilterExpr, type LogFilter } from "../types";

const form = (over: Partial<ErrorForm> = {}): ErrorForm => ({ from: "", to: "", q: "", ...over });

/** 오류 문자열이 오면 테스트를 실패시킨다. */
const ok = (f: ErrorForm, base: LogFilter = emptyFilter()): LogFilter => {
  const r = composeErrorFilter(base, f);
  if (typeof r === "string") throw new Error(r);
  return r;
};

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
    const f = ok(form(), { ...emptyFilter(), active_only: true, bookmarked_only: true, expr: rule });
    expect(f.log_kind).toBe("error");
    expect(f.active_only).toBe(true);
    expect(f.bookmarked_only).toBe(true);
    expect(f.expr).toEqual(rule);
  });

  it("matches the search text against the message or the client ip", () => {
    expect(ok(form({ q: "  No such file  " })).expr).toEqual(search("No such file"));
    // 주소를 넣어도 같은 식이므로 IPv4·IPv6 구분 없이 클라이언트로 찾는다.
    expect(ok(form({ q: "2001:db8::1" })).expr).toEqual(search("2001:db8::1"));
  });

  it("ands the search with the rule condition", () => {
    const rule: FilterExpr = { kind: "cond", field: "level", op: "eq", value: "error" };
    expect(ok(form({ q: "refused" }), { ...emptyFilter(), expr: rule }).expr).toEqual({ kind: "and", items: [rule, search("refused")] });
  });

  it("parses the time range and rejects garbage", () => {
    const f = ok(form({ from: "2024-01-01T01:00" }));
    expect(f.time_from_micros).toBe(1_704_038_400_000_000);
    expect(f.time_to_micros).toBeNull();
    expect(typeof composeErrorFilter(emptyFilter(), form({ to: "yesterday" }))).toBe("string");
  });
});
