import { describe, expect, it } from "vitest";
import { baseName, compareNatural, joinPath, relativeTo } from "./paths";

describe("paths", () => {
  it("joins with the root's separator", () => {
    expect(joinPath("/var/log/nginx", "weblog.duckdb")).toBe("/var/log/nginx/weblog.duckdb");
    expect(joinPath("C:\\inetpub\\logs", "weblog.duckdb")).toBe("C:\\inetpub\\logs\\weblog.duckdb");
    expect(joinPath("C:\\inetpub\\logs\\", "weblog.duckdb")).toBe("C:\\inetpub\\logs\\weblog.duckdb");
    expect(joinPath("", "weblog.duckdb")).toBe("weblog.duckdb");
  });

  it("strips the root prefix for display", () => {
    expect(relativeTo("/var/log/nginx", "/var/log/nginx/access.log")).toBe("access.log");
    expect(relativeTo("/var/log/nginx", "/var/log/nginx/2026/access.log.gz")).toBe("2026/access.log.gz");
    expect(relativeTo("C:\\inetpub\\logs", "C:\\inetpub\\logs\\W3SVC1\\u_ex260908.log")).toBe("W3SVC1\\u_ex260908.log");
    expect(relativeTo("/var/log/nginx", "/etc/other.log")).toBe("/etc/other.log");
  });
});

describe("compareNatural", () => {
  it("orders embedded numbers numerically", () => {
    const names = ["access.log.10.gz", "access.log.2.gz", "access.log", "access.log.1"];
    expect([...names].sort(compareNatural)).toEqual(["access.log", "access.log.1", "access.log.2.gz", "access.log.10.gz"]);
  });
});

describe("baseName", () => {
  it("returns the last path segment for both separators", () => {
    expect(baseName("/var/log/nginx")).toBe("nginx");
    expect(baseName("/var/log/nginx/")).toBe("nginx");
    expect(baseName("C:\\inetpub\\logs\\LogFiles")).toBe("LogFiles");
    expect(baseName("")).toBe("");
  });
});
