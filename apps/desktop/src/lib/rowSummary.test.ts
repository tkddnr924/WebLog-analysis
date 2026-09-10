import { describe, expect, it } from "vitest";
import { rowSummary } from "./rowSummary";
import type { LogRow } from "../types";

const row = (over: Partial<LogRow> = {}): LogRow => ({
  source_id: 1,
  line_number: 1,
  timestamp_utc: 0,
  client_ip: null,
  method: null,
  request_target: null,
  status: null,
  bytes_sent: null,
  bookmarked: true,
  extra_json: null,
  ...over,
});

describe("rowSummary", () => {
  it("reads an access row as request, status and client", () => {
    expect(rowSummary(row({ method: "GET", request_target: "/admin", status: 404, client_ip: "10.0.0.1" }))).toBe("GET /admin · 404 · 10.0.0.1");
  });

  it("reads an error row as level and message", () => {
    const r = row({ extra_json: JSON.stringify({ level: "crit", message: 'open() "/x" failed' }), client_ip: "10.0.2.7" });
    expect(rowSummary(r)).toBe('crit · open() "/x" failed · 10.0.2.7');
  });

  it("skips the parts a row does not have", () => {
    expect(rowSummary(row({ request_target: "/", status: 200 }))).toBe("/ · 200");
    expect(rowSummary(row({ extra_json: JSON.stringify({ message: "restarted" }) }))).toBe("restarted");
  });

  it("says the row is empty rather than showing nothing", () => {
    expect(rowSummary(row())).toBe("(내용 없음)");
  });
});
