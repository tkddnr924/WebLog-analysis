import { describe, expect, it } from "vitest";
import { errorFields, levelClass } from "./errorRows";
import type { LogRow } from "../types";

const row = (extra_json: string | null, client_ip: string | null = null): LogRow => ({
  source_id: 1,
  line_number: 7,
  timestamp_utc: 1,
  client_ip,
  method: null,
  request_target: null,
  status: null,
  bytes_sent: null,
  bookmarked: false,
  extra_json,
});

describe("errorFields", () => {
  it("reads level and message from an nginx error row", () => {
    const f = errorFields(row('{"level":"error","pid_tid":"1234#0","connection":"*5","message":"open() failed (2: No such file)","server":"example.com"}'));
    expect(f.level).toBe("error");
    expect(f.message).toBe("open() failed (2: No such file)");
    expect(f.client).toBe("");
  });

  it("uses the apache client key and prefixes the error code", () => {
    const f = errorFields(row('{"level":"core:error","client":"172.17.0.1:5678","error_code":"AH00128","message":"File does not exist"}'));
    expect(f.level).toBe("core:error");
    expect(f.client).toBe("172.17.0.1:5678");
    expect(f.message).toBe("AH00128 File does not exist");
  });

  it("prefers the standard client_ip column over the extra client key", () => {
    expect(errorFields(row('{"client":"172.17.0.1:5678"}', "10.0.0.9")).client).toBe("10.0.0.9");
  });

  it("returns empty values for missing or broken json", () => {
    const empty = { level: "", client: "", message: "" };
    expect(errorFields(row(null))).toEqual(empty);
    expect(errorFields(row("{oops"))).toEqual(empty);
    expect(errorFields(row("[1,2]"))).toEqual(empty);
  });

  it("summarizes the remaining keys when there is no message", () => {
    const f = errorFields(row('{"level":"error","client":"10.0.0.9","source":"upstream","error_code":"502"}'));
    expect(f.message).toBe("source=upstream, error_code=502");
  });
});

describe("levelClass", () => {
  it("separates severe levels, warnings and the rest", () => {
    expect(levelClass("error")).toBe("s5");
    expect(levelClass("core:error")).toBe("s5");
    expect(levelClass("emerg")).toBe("s5");
    expect(levelClass("warn")).toBe("s4");
    expect(levelClass("notice")).toBe("s0");
    expect(levelClass("")).toBe("s0");
  });
});
