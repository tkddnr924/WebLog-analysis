import { describe, expect, it } from "vitest";
import { buildBlocks, buildProfile, guessErrorRoles, guessRoles, guessSeparator, inferTsFormat, restIndex, roleForKind, rolesFromProfile, separatorOf, tailLabels, tokenize, vocabFor } from "./puzzle";

const G = vocabFor("unknown");
const NG = vocabFor("nginx");
const AP = vocabFor("apache");
import type { FormatProfile } from "../types";

const COMBINED_LINE = '101.33.66.34 - - [06/Sep/2026:00:49:09 +0000] "GET / HTTP/1.1" 400 264 "-" "Mozilla/5.0 (iPhone; \\"x\\")"';

const combined: FormatProfile = {
  schema_version: 1,
  name: "combined",
  version: 1,
  server_hint: "unknown",
  timezone: { kind: "from_input" },
  strategy: {
    kind: "blocks",
    blocks: [
      { block: "field", name: "client_ip", kind: { kind: "client_ip" }, capture: { kind: "token" }, missing: ["-"] },
      { block: "whitespace" },
      { block: "field", name: "ident", kind: { kind: "text" }, capture: { kind: "token" }, missing: ["-"] },
      { block: "whitespace" },
      { block: "field", name: "remote_user", kind: { kind: "text" }, capture: { kind: "token" }, missing: ["-"] },
      { block: "whitespace" },
      { block: "field", name: "timestamp", kind: { kind: "timestamp", format: { kind: "clf" } }, capture: { kind: "bracketed" }, missing: ["-"] },
      { block: "whitespace" },
      { block: "field", name: "request", kind: { kind: "request_line" }, capture: { kind: "quoted" }, missing: ["-"] },
      { block: "whitespace" },
      { block: "field", name: "status", kind: { kind: "status" }, capture: { kind: "token" }, missing: ["-"] },
      { block: "whitespace" },
      { block: "field", name: "bytes_sent", kind: { kind: "bytes_sent" }, capture: { kind: "token" }, missing: ["-"] },
    ],
  },
};

describe("tokenize", () => {
  it("keeps quoted and bracketed spans as one piece", () => {
    const p = tokenize(COMBINED_LINE, "space");
    expect(p.map((x) => x.capture)).toEqual(["token", "token", "token", "bracketed", "quoted", "token", "token", "quoted", "quoted"]);
    expect(p[3].value).toBe("06/Sep/2026:00:49:09 +0000");
    expect(p[4].value).toBe("GET / HTTP/1.1");
    expect(p[8].value).toBe('Mozilla/5.0 (iPhone; \\"x\\")');
  });

  it("splits on a literal separator and keeps empty pieces", () => {
    const p = tokenize("2024-03-01T09:00:00|10.1.2.3|GET|/a|200||-", "|");
    expect(p.map((x) => x.value)).toEqual(["2024-03-01T09:00:00", "10.1.2.3", "GET", "/a", "200", "", "-"]);
    expect(p.every((x) => x.capture === "pattern")).toBe(true);
  });
});

describe("separator", () => {
  it("reads the literal from a profile and guesses from a line", () => {
    expect(separatorOf(combined)).toBe("space");
    expect(separatorOf({ ...combined, strategy: { kind: "w3c" } })).toBeNull();
    expect(guessSeparator("a|b|c|d")).toBe("|");
    expect(guessSeparator(COMBINED_LINE)).toBe("space");
  });
});

describe("roles", () => {
  it("maps preset fields onto pieces in order and ignores leftovers", () => {
    const pieces = tokenize(COMBINED_LINE, "space");
    const roles = rolesFromProfile(combined, pieces, G)!;
    expect(roles.map((r) => r.role)).toEqual(["client_ip", "ident", "remote_user", "timestamp", "request_line", "status", "bytes_sent", "ignore", "ignore"]);
    expect(roles[3].tsFormat).toEqual({ kind: "clf" });
    expect(rolesFromProfile(combined, pieces, NG)!.map((r) => r.role)).toEqual(["$remote_addr", "text", "$remote_user", "$time_local", "$request", "$status", "$body_bytes_sent", "ignore", "ignore"]);
    expect(rolesFromProfile(combined, pieces, AP)!.map((r) => r.role)).toEqual(["%h", "%l", "%u", "%t", "%r", "%>s", "%b", "ignore", "ignore"]);
  });

  it("guesses roles from value shapes", () => {
    const roles = guessRoles(tokenize(COMBINED_LINE, "space"), G).map((r) => r.role);
    expect(roles).toEqual(["client_ip", "ignore", "ignore", "timestamp", "request_line", "status", "bytes_sent", "ignore", "user_agent"]);
    const pipe = guessRoles(tokenize("2024-03-01T09:00:00|10.1.2.3|GET|/a|200|1234", "|"), NG);
    expect(pipe.map((r) => r.role)).toEqual(["$time_iso8601", "$remote_addr", "$request_method", "$request_uri", "$status", "$body_bytes_sent"]);
    expect(pipe[0].tsFormat).toEqual({ kind: "iso8601" });
  });
});

describe("buildBlocks", () => {
  it("emits whitespace between pieces, regex for ignored pieces, unique names", () => {
    const pieces = tokenize(COMBINED_LINE, "space");
    const roles = rolesFromProfile(combined, pieces, G)!;
    roles[8] = { role: "user_agent", tsFormat: { kind: "clf" } };
    const blocks = buildBlocks(pieces, roles, "space", G);
    expect(blocks).toHaveLength(17);
    expect(blocks[1]).toEqual({ block: "whitespace" });
    expect(blocks[2]).toMatchObject({ block: "field", name: "ident", kind: { kind: "text" }, capture: { kind: "token" } });
    expect(blocks[4]).toMatchObject({ block: "field", name: "remote_user", kind: { kind: "text" } });
    expect(blocks[14]).toEqual({ block: "regex", pattern: '"(?:[^"\\\\]|\\\\.)*"' });
    expect(blocks[16]).toMatchObject({ block: "field", name: "user_agent", capture: { kind: "quoted" } });
    const names = blocks.filter((b) => b.block === "field").map((b) => (b as { name: string }).name);
    expect(new Set(names).size).toBe(names.length);
  });

  it("names generic text fields by position and keeps names unique", () => {
    const pieces = tokenize("a b c", "space");
    const roles = [{ role: "text", tsFormat: { kind: "clf" } }, { role: "text", tsFormat: { kind: "clf" } }, { role: "status", tsFormat: { kind: "clf" } }] as const;
    const names = buildBlocks(pieces, [...roles], "space", G).filter((b) => b.block === "field").map((b) => (b as { name: string }).name);
    expect(names).toEqual(["field_1", "field_2", "status"]);
    const dup = [{ role: "$status", tsFormat: { kind: "clf" } }, { role: "$status", tsFormat: { kind: "clf" } }] as const;
    expect(buildBlocks(tokenize("200 404", "space"), [...dup], "space", NG).filter((b) => b.block === "field").map((b) => (b as { name: string }).name)).toEqual(["status", "status_2"]);
  });

  it("uses literal separators and separator-aware captures", () => {
    const pieces = tokenize("t|ip|x", "|");
    const blocks = buildBlocks(pieces, [{ role: "text", tsFormat: { kind: "clf" } }, { role: "client_ip", tsFormat: { kind: "clf" } }, { role: "ignore", tsFormat: { kind: "clf" } }], "|", G);
    expect(blocks[1]).toEqual({ block: "literal", text: "|" });
    expect(blocks[2]).toMatchObject({ capture: { kind: "pattern", pattern: "[^\\|]*" }, missing: ["-", ""] });
    expect(blocks[4]).toEqual({ block: "regex", pattern: "[^\\|]*" });
  });

  it("derives the edited profile name without stacking suffixes", () => {
    const pieces = tokenize("1.1.1.1 200", "space");
    const roles = guessRoles(pieces, AP);
    const p = buildProfile(combined, "apache", pieces, roles, "space", AP);
    expect(p.name).toBe("combined_edit");
    expect(buildProfile(p, "apache", pieces, roles, "space", AP).name).toBe("combined_edit");
    expect(p.strategy.kind === "blocks" && p.strategy.blocks[0]).toMatchObject({ block: "field", name: "client_ip", kind: { kind: "client_ip" } });
    expect(p.timezone).toEqual({ kind: "from_input" });
  });
});

describe("inferTsFormat", () => {
  it("picks ISO for ISO-like values and CLF otherwise", () => {
    expect(inferTsFormat("2024-03-01T09:00:00")).toEqual({ kind: "iso8601" });
    expect(inferTsFormat("06/Sep/2026:00:49:09 +0000")).toEqual({ kind: "clf" });
  });
});

describe("vocab", () => {
  it("has unique ids and names per server and fixed formats for nginx time labels", () => {
    for (const v of [G, NG, AP]) {
      expect(new Set(v.map((r) => r.id)).size).toBe(v.length);
      // 시간 라벨은 형식만 다르고 같은 timestamp 이름을 쓴다. 그 외 저장 이름은 겹치지 않아야 한다.
      const names = v.filter((r) => r.name && r.kind !== "timestamp").map((r) => r.name);
      expect(new Set(names).size).toBe(names.length);
    }
    expect(roleForKind(NG, { kind: "timestamp", format: { kind: "iso8601" } })).toBe("$time_iso8601");
    expect(roleForKind(NG, { kind: "timestamp", format: { kind: "clf" } })).toBe("$time_local");
    expect(roleForKind(AP, { kind: "text" }, "ident")).toBe("%l");
    expect(roleForKind(AP, { kind: "text" }, "nothing")).toBe("text");
  });
});

describe("error logs", () => {
  const NGE = vocabFor("nginx", "error");
  const APE = vocabFor("apache", "error");
  const nginxLine = '2026/09/06 00:49:09 [error] 1234#0: *5 open() "/var/www/x" failed (2: No such file), client: 1.2.3.4, server: example.com, request: "GET /x HTTP/1.1"';
  const apacheLine = "[Sat Sep 06 00:49:09.123456 2026] [core:error] [pid 123:tid 456] [client 1.2.3.4:5678] AH00126: Invalid URI in request GET /../ HTTP/1.1";

  it("merges a split date and time into one piece", () => {
    const p = tokenize(nginxLine, "space");
    expect(p[0]).toEqual({ text: "2026/09/06 00:49:09", value: "2026/09/06 00:49:09", capture: "pattern" });
    expect(p[1].value).toBe("error");
  });

  it("guesses time, level, pid, connection and then message to end of line", () => {
    const pieces = tokenize(nginxLine, "space");
    const roles = guessErrorRoles(pieces, NGE);
    expect(roles.slice(0, 5).map((r) => r.role)).toEqual(["err_time", "err_level", "err_pid", "err_conn", "message_detail"]);
    expect(restIndex(roles, NGE)).toBe(4);
    const blocks = buildBlocks(pieces, roles, "space", NGE);
    const fields = blocks.filter((b) => b.block === "field") as { name: string; capture: { kind: string; pattern?: string }; kind: { kind: string; format?: { kind: string; pattern?: string } } }[];
    expect(fields.map((f) => f.name)).toEqual(["timestamp", "level", "pid_tid", "message"]);
    // 연결 번호는 없는 줄도 있으므로 [공백, *숫자] 선택 그룹이다.
    const conn = blocks.find((b) => b.block === "optional_group" && b.blocks.some((x) => x.block === "field" && x.name === "connection"));
    expect(conn).toEqual({ block: "optional_group", blocks: [{ block: "whitespace" }, { block: "field", name: "connection", kind: { kind: "text" }, capture: { kind: "pattern", pattern: "\\*\\d+" }, missing: ["-"] }] });
    expect(fields[0].capture).toEqual({ kind: "pattern", pattern: "\\S+ \\S+" });
    expect(fields[0].kind.format).toEqual({ kind: "custom", pattern: "%Y/%m/%d %H:%M:%S" });
    const msg = blocks.find((b) => b.block === "field" && b.name === "message");
    expect(msg).toMatchObject({ capture: { kind: "pattern", pattern: ".*?" } });
    const groups = blocks.filter((b) => b.block === "optional_group");
    // 연결 번호 선택 그룹 1개 + 꼬리 항목 6개
    expect(groups).toHaveLength(7);
    expect(groups[1]).toEqual({
      block: "optional_group",
      blocks: [
        { block: "literal", text: ", client: " },
        { block: "field", name: "client_ip", kind: { kind: "client_ip" }, capture: { kind: "pattern", pattern: "[^,]+" }, missing: ["-"] },
      ],
    });
    expect(groups[3].block === "optional_group" && groups[3].blocks[0]).toEqual({ block: "literal", text: ', request: "' });
    expect(groups[3].block === "optional_group" && groups[3].blocks[2]).toEqual({ block: "literal", text: '"' });
    const labels = tailLabels(pieces, restIndex(roles, NGE)!, vocabFor("nginx", "error").find((r) => r.id === "message_detail")!.tail!);
    const clientKey = pieces.findIndex((p) => p.text === "client:");
    expect(labels.get(clientKey)).toEqual({ label: "클라이언트 IP", kind: "client_ip" });
    expect(labels.get(clientKey + 1)).toEqual({ label: "", kind: "client_ip" });
    // 일반 "메시지(줄 끝까지)"로 바꾸면 .* 필드 하나가 된다.
    const plain = [...roles];
    plain[4] = { role: "message", tsFormat: { kind: "clf" } };
    const pb = buildBlocks(pieces, plain, "space", NGE);
    expect(pb[pb.length - 1]).toMatchObject({ block: "field", name: "message", capture: { kind: "pattern", pattern: ".*" } });
  });

  it("handles the apache error log shape", () => {
    const pieces = tokenize(apacheLine, "space");
    const roles = guessErrorRoles(pieces, APE);
    expect(roles.slice(0, 6).map((r) => r.role)).toEqual(["err_time", "err_level", "err_pid", "err_client", "err_code", "message_detail"]);
    expect(roles[0].tsFormat).toEqual({ kind: "custom", pattern: "%a %b %d %H:%M:%S%.f %Y" });
    const noFrac = guessErrorRoles(tokenize("[Sun Sep 06 00:49:09 2026] [mpm_prefork:notice] [pid 123] AH00163: Apache/2.4 configured", "space"), APE);
    expect(noFrac[0]).toEqual({ role: "err_time_s", tsFormat: { kind: "custom", pattern: "%a %b %d %H:%M:%S %Y" } });
  });

  it("maps a rest capture back onto the message label", () => {
    const pieces = tokenize(nginxLine, "space");
    const profile = buildProfile(null, "nginx", pieces, guessErrorRoles(pieces, NGE), "space", NGE, "error");
    expect(profile.name).toBe("error_log_edit");
    expect(profile.timezone).toEqual({ kind: "fixed", offset_seconds: 32400 });
    const back = rolesFromProfile(profile, pieces, NGE)!;
    expect(back.slice(0, 5).map((r) => r.role)).toEqual(["err_time", "err_level", "err_pid", "err_conn", "message_detail"]);
  });
});
