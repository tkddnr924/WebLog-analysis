import { describe, expect, it } from "vitest";
import { formatBytes, formatTime, parseTimeInput, statusClass } from "./format";
import { appendPage, emptyCache } from "./pages";
import type { LogPage, LogRow } from "../types";

describe("format", () => {
  it("formats bytes with binary units", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KiB");
    expect(formatBytes(150 * 1024 * 1024)).toBe("150 MiB");
  });

  it("formats utc micros without local timezone conversion", () => {
    expect(formatTime(971_211_336_000_000)).toBe("2000-10-11 05:55:36");
    expect(formatTime(null)).toBe("(시간 미확정)");
  });

  it("parses utc input and rejects garbage", () => {
    expect(parseTimeInput("")).toBeNull();
    expect(parseTimeInput("2024-01-01T01:00")).toBe(1_704_038_400_000_000);
    expect(formatTime(parseTimeInput("2024-01-01T01:00")!)).toBe("2024-01-01 01:00:00");
    expect(parseTimeInput("yesterday")).toBeUndefined();
  });

  it("classifies status codes", () => {
    expect(statusClass(200)).toBe("s2");
    expect(statusClass(404)).toBe("s4");
    expect(statusClass(null)).toBe("s0");
  });
});

const row = (line: number): LogRow => ({
  source_id: 1,
  line_number: line,
  timestamp_utc: line,
  client_ip: "10.0.0.1",
  method: "GET",
  request_target: "/x",
  status: 200,
  bytes_sent: 1,
});

const page = (from: number, n: number, more: boolean): LogPage => ({
  rows: Array.from({ length: n }, (_, i) => row(from + i)),
  next_cursor: more
    ? { filter_hash: "h", max_batch_id: 1, segment: "timed", last_ts: from + n - 1, last_source_id: 1, last_line: from + n - 1 }
    : null,
  approx_bytes: n * 100,
});

describe("page cache", () => {
  it("ignores responses from an older request", () => {
    const cache = emptyCache(2);
    const same = appendPage(cache, 1, page(1, 5, true), 10_000);
    expect(same).toBe(cache);
  });

  it("appends pages and drops oldest rows beyond the byte cap", () => {
    let cache = emptyCache(1);
    cache = appendPage(cache, 1, page(1, 10, true), 1_500);
    cache = appendPage(cache, 1, page(11, 10, false), 1_500);
    expect(cache.rows.length).toBe(15);
    expect(cache.rows[0].line_number).toBe(6);
    expect(cache.droppedRows).toBe(5);
    expect(cache.exhausted).toBe(true);
  });

  it("never drops rows from the page just received", () => {
    let cache = emptyCache(1);
    cache = appendPage(cache, 1, page(1, 10, true), 100);
    expect(cache.rows.length).toBe(10);
    cache = appendPage(cache, 1, page(11, 10, true), 100);
    expect(cache.rows.map((r) => r.line_number)).toEqual([11, 12, 13, 14, 15, 16, 17, 18, 19, 20]);
  });
});
