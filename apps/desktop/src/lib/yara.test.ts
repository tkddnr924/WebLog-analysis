import { describe, expect, it } from "vitest";
import { RULE_TEMPLATE, describeExpr, parseRule } from "./yara";
import type { FilterExpr } from "../types";

function ok(src: string) {
  const r = parseRule(src);
  if (!r.ok) throw new Error(r.errors.map((e) => `${e.line}:${e.col} ${e.message}`).join("; "));
  return r.rule;
}
function bad(src: string): string {
  const r = parseRule(src);
  if (r.ok) throw new Error("파싱이 성공하면 안 됨");
  return `${r.errors[0].line}:${r.errors[0].col} ${r.errors[0].message}`;
}
const cond = (field: string, op: string, value: string): FilterExpr => ({ kind: "cond", field: field as never, op: op as never, value });

describe("parseRule", () => {
  it("parses the template", () => {
    const r = ok(RULE_TEMPLATE);
    expect(r.id).toBe("my_rule");
    expect(r.name).toBe("내 룰");
    expect(r.description).toBe("무엇을 찾는 룰인지");
    expect(r.expr).toEqual({
      kind: "and",
      items: [
        { kind: "and", items: [cond("status", "gte", "400"), cond("status", "lte", "499")] },
        { kind: "or", items: [cond("request_target", "contains", "/admin"), cond("request_target", "regex", "(?i)union[\\s+]+select")] },
      ],
    });
  });

  it("handles meta, comments, nocase strings, precedence and parentheses", () => {
    const r = ok(`
      // 주석
      rule r1 {
        meta:
          name = "이름"
          description = "설명"
          severity = 3
        strings:
          $a = "Admin" nocase
          $b = /^\\/api\\//
        condition:
          /* 블록 주석 */
          $a or $b and not status == 200
      }`);
    expect(r.name).toBe("이름");
    expect(r.expr).toEqual({
      kind: "or",
      items: [cond("request_target", "icontains", "Admin"), { kind: "and", items: [cond("request_target", "regex", "^\\/api\\/"), { kind: "not", item: cond("status", "eq", "200") }] }],
    });
    const p = ok(`rule r { strings: $a = "x" $b = "y" condition: ($a or $b) and status == 500 }`);
    expect(p.expr.kind).toBe("and");
  });

  it("supports every field operator", () => {
    const r = ok(`rule ops {
      strings:
        $bot = /python|curl/i
      condition:
        method == "POST" and ip != "10.0.0.1" and bytes > 1000 and bytes <= 5000
        and path startswith "/api" and path endswith ".php" and ua contains $bot
        and referrer icontains "google" and protocol matches /HTTP\\/1\\.[01]/
        and referrer is not null and ua is null and status in 500..599
    }`);
    expect(r.expr.kind).toBe("and");
    const items = (r.expr as { items: FilterExpr[] }).items;
    expect(items).toContainEqual(cond("method", "eq", "POST"));
    expect(items).toContainEqual(cond("client_ip", "ne", "10.0.0.1"));
    expect(items).toContainEqual(cond("bytes_sent", "gt", "1000"));
    expect(items).toContainEqual(cond("bytes_sent", "lte", "5000"));
    expect(items).toContainEqual(cond("request_target", "starts_with", "/api"));
    expect(items).toContainEqual(cond("request_target", "ends_with", ".php"));
    expect(items).toContainEqual(cond("user_agent", "regex", "(?i)python|curl"));
    expect(items).toContainEqual(cond("referrer", "icontains", "google"));
    expect(items).toContainEqual(cond("protocol", "regex", "HTTP\\/1\\.[01]"));
    expect(items).toContainEqual({ kind: "not", item: cond("referrer", "is_null", "") });
    expect(items).toContainEqual(cond("user_agent", "is_null", ""));
    expect(items).toContainEqual({ kind: "and", items: [cond("status", "gte", "500"), cond("status", "lte", "599")] });
  });

  it("expands any/all of them and wildcard sets", () => {
    const r = ok(`rule s { strings: $a1 = "x" $a2 = "y" $b = "z" condition: any of ($a*) and all of them }`);
    expect(r.expr).toEqual({
      kind: "and",
      items: [
        { kind: "or", items: [cond("request_target", "contains", "x"), cond("request_target", "contains", "y")] },
        { kind: "and", items: [cond("request_target", "contains", "x"), cond("request_target", "contains", "y"), cond("request_target", "contains", "z")] },
      ],
    });
  });

  it("accepts true/false and string escapes", () => {
    expect(ok(`rule t { condition: true }`).expr).toEqual({ kind: "true" });
    expect(ok(`rule t { condition: path contains "a\\"b\\\\c" }`).expr).toEqual(cond("request_target", "contains", 'a"b\\c'));
  });

  it("reports precise errors", () => {
    expect(bad(`rule { condition: true }`)).toMatch(/^1:6 룰 이름/);
    expect(bad(`rule r { condition: statuss == 200 }`)).toMatch(/알 수 없는 필드: statuss/);
    expect(bad(`rule r { condition: status == "200" }`)).toMatch(/숫자와 비교/);
    expect(bad(`rule r { condition: method > "GET" }`)).toMatch(/==, != 만/);
    expect(bad(`rule r { condition: $nope }`)).toMatch(/정의되지 않은 문자열: \$nope/);
    expect(bad(`rule r { strings: $a = "x" $a = "y" condition: $a }`)).toMatch(/두 번 정의/);
    expect(bad(`rule r { condition: path matches /(/ }`)).toMatch(/정규식 오류/);
    expect(bad(`rule r { condition: path matches /(?=x)/ }`)).toMatch(/룩어라운드/);
    expect(bad(`rule r { strings: $a = "x" }`)).toMatch(/condition: 절이 필요/);
    expect(bad(`rule r { condition: true } rule q { condition: true }`)).toMatch(/하나만/);
    expect(bad(`rule r { condition: status in 1 }`)).toMatch(/'\.\.'가 와야/);
    expect(bad(`rule r {\n  condition:\n    status == 200 and\n}`)).toMatch(/^4:1 조건이 와야/);
    expect(bad(`rule r { condition: path contains "unterminated }`)).toMatch(/닫히지 않았습니다/);
    expect(bad(``)).toMatch(/비어 있습니다/);
    expect(bad(`rule r { condition: bytes contains "x" }`)).toMatch(/문자열 연산을 쓸 수 없습니다/);
  });

  it("reads the error log fields and keeps them textual", () => {
    const r = ok(`rule e {
      strings:
        $refused = "Connection refused"
        $sev = /crit|alert|emerg/ nocase
      condition:
        level == "error" or message contains $refused or msg icontains "PHP message" or level matches $sev
    }`);
    if (r.expr.kind !== "or") throw new Error("or 조건식이어야 함");
    const items = r.expr.items;
    expect(items).toContainEqual(cond("level", "eq", "error"));
    expect(items).toContainEqual(cond("message", "contains", "Connection refused"));
    expect(items).toContainEqual(cond("message", "icontains", "PHP message"));
    expect(items).toContainEqual(cond("level", "regex", "(?i)crit|alert|emerg"));
    expect(bad(`rule e { condition: level > 3 }`)).toMatch(/==, != 만/);
    expect(bad(`rule e { condition: message in 1..2 }`)).toMatch(/목록만 쓸 수 있습니다/);
  });

  it("puts bare signatures on the given default field", () => {
    const src = `rule e { strings: $a = "No such file" $b = /timed out/ nocase condition: any of them }`;
    expect(ok(src).expr).toEqual({ kind: "or", items: [cond("request_target", "contains", "No such file"), cond("request_target", "regex", "(?i)timed out")] });
    const r = parseRule(src, { defaultField: "message" });
    if (!r.ok) throw new Error(r.errors[0].message);
    expect(r.rule.expr).toEqual({ kind: "or", items: [cond("message", "contains", "No such file"), cond("message", "regex", "(?i)timed out")] });
  });

  it("describes the error fields in Korean", () => {
    expect(describeExpr(cond("level", "eq", "error"))).toBe("레벨 = error");
    expect(describeExpr(cond("message", "icontains", "PHP"))).toBe("메시지 포함(대소문자 무시) PHP");
  });

  it("matches a list of values with one condition", () => {
    const r = ok(`rule ips { condition: ip in ("10.0.0.1", "10.0.0.2", "203.0.113.7") }`);
    expect(r.expr).toEqual({
      kind: "or",
      items: [cond("client_ip", "eq", "10.0.0.1"), cond("client_ip", "eq", "10.0.0.2"), cond("client_ip", "eq", "203.0.113.7")],
    });
    const s = ok(`rule st { condition: status in (404, 500) }`);
    expect(s.expr).toEqual({ kind: "or", items: [cond("status", "eq", "404"), cond("status", "eq", "500")] });
  });

  it("keeps the numeric range form and rejects broken lists", () => {
    const r = ok(`rule r { condition: status in 400..499 }`);
    expect(r.expr).toEqual({ kind: "and", items: [cond("status", "gte", "400"), cond("status", "lte", "499")] });
    expect(bad(`rule e { condition: ip in () }`)).toContain("값을 하나 이상");
    expect(bad(`rule e { condition: status in ("404") }`)).toContain("숫자");
    expect(bad(`rule e { condition: ip in (10) }`)).toContain("따옴표");
    expect(bad(`rule e { condition: ip in 1..2 }`)).toContain("숫자 필드");
  });

  it("describes a list as a single line", () => {
    const r = ok(`rule ips { condition: ip in ("10.0.0.1", "10.0.0.2") }`);
    expect(describeExpr(r.expr)).toBe("(IP = 10.0.0.1 또는 IP = 10.0.0.2)");
  });

  it("describes expressions in Korean", () => {
    const r = ok(`rule d { strings: $a = "x" condition: status in 400..499 and not $a }`);
    expect(describeExpr(r.expr)).toBe("상태 ≥ 400 그리고 상태 ≤ 499 그리고 아님(경로 포함 x)");
  });
});
