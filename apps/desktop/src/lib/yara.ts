// YARA풍 룰 언어. 텍스트 룰을 엔진 조건식(FilterExpr)으로 컴파일한다. 순수 함수, 테스트 대상.
//
//   rule sqli_success {
//     meta:
//       name = "SQL Injection · 성공 응답"
//       description = "서명이 있고 2xx로 응답한 요청"
//     strings:
//       $union = /union[\s+]+select/ nocase
//       $quote = "%27"
//     condition:
//       status in 200..299 and ($union or $quote)
//   }
//
// 접근 로그 필드: status, bytes, method, ip, path, protocol, referrer, ua
// 에러 로그 필드: level, message(msg) — 문자열 연산만 쓴다.
// 연산: == != > >= < <=, in a..b, contains, icontains, startswith, endswith, matches, is null, is not null
// 문자열: $id = "텍스트" [nocase] | $id = /정규식/ [nocase]. 필드 없이 $id만 쓰면 기본 필드(접근 로그는 path, 에러 로그는 message)에 적용한다.
// 묶음: any of them, all of them, any of ($a, $b*), all of ($a*)
import type { CondField, CondOp, FilterExpr } from "../types";

export interface ParsedRule {
  /** 룰 식별자(영문·숫자·밑줄). */
  id: string;
  /** 표시 이름(meta.name 또는 식별자). */
  name: string;
  description: string;
  expr: FilterExpr;
}

export interface RuleError {
  line: number;
  col: number;
  message: string;
}

export type ParseResult = { ok: true; rule: ParsedRule } | { ok: false; errors: RuleError[] };

const FIELDS: Record<string, CondField> = {
  status: "status",
  bytes: "bytes_sent",
  bytes_sent: "bytes_sent",
  method: "method",
  ip: "client_ip",
  client_ip: "client_ip",
  path: "request_target",
  url: "request_target",
  target: "request_target",
  request_target: "request_target",
  protocol: "protocol",
  referrer: "referrer",
  referer: "referrer",
  ua: "user_agent",
  user_agent: "user_agent",
  level: "level",
  message: "message",
  msg: "message",
};

const NUMERIC: Set<CondField> = new Set(["status", "bytes_sent"]);

interface StringDef {
  id: string;
  kind: "text" | "regex";
  value: string;
  nocase: boolean;
  line: number;
}

type Tok =
  | { t: "ident"; v: string; line: number; col: number }
  | { t: "var"; v: string; line: number; col: number }
  | { t: "num"; v: number; line: number; col: number }
  | { t: "str"; v: string; line: number; col: number }
  | { t: "re"; v: string; flags: string; line: number; col: number }
  | { t: "op"; v: string; line: number; col: number }
  | { t: "eof"; line: number; col: number };

class RuleSyntaxError extends Error {
  constructor(
    public line: number,
    public col: number,
    message: string,
  ) {
    super(message);
  }
}

const OPS = ["==", "!=", ">=", "<=", "..", ">", "<", "=", "{", "}", "(", ")", ":", ","];

/** 토큰화. 정규식 리터럴은 `=`, `matches`, `contains` 뒤나 값 자리에서만 나오므로 앞 토큰으로 구분한다. */
function tokenize(src: string): Tok[] {
  const out: Tok[] = [];
  let i = 0;
  let line = 1;
  let col = 1;
  const n = src.length;
  const err = (m: string) => new RuleSyntaxError(line, col, m);
  const adv = (k = 1) => {
    for (let j = 0; j < k; j += 1) {
      if (src[i] === "\n") {
        line += 1;
        col = 1;
      } else col += 1;
      i += 1;
    }
  };
  const regexAllowed = () => {
    const prev = out[out.length - 1];
    if (!prev) return false;
    if (prev.t === "op") return prev.v === "=" || prev.v === "(" || prev.v === ",";
    return prev.t === "ident" && (prev.v === "matches" || prev.v === "contains" || prev.v === "icontains");
  };
  while (i < n) {
    const c = src[i];
    if (c === " " || c === "\t" || c === "\r" || c === "\n") {
      adv();
      continue;
    }
    if (c === "/" && src[i + 1] === "/") {
      while (i < n && src[i] !== "\n") adv();
      continue;
    }
    if (c === "/" && src[i + 1] === "*") {
      const end = src.indexOf("*/", i + 2);
      if (end === -1) throw err("주석이 닫히지 않았습니다 (*/)");
      adv(end + 2 - i);
      continue;
    }
    const startLine = line;
    const startCol = col;
    if (c === "/" && regexAllowed()) {
      let j = i + 1;
      let body = "";
      while (j < n && src[j] !== "/") {
        if (src[j] === "\\") {
          body += src[j] + (src[j + 1] ?? "");
          j += 2;
        } else if (src[j] === "\n") {
          throw err("정규식이 닫히지 않았습니다 (/)");
        } else {
          body += src[j];
          j += 1;
        }
      }
      if (j >= n) throw err("정규식이 닫히지 않았습니다 (/)");
      j += 1;
      let flags = "";
      while (j < n && /[a-z]/i.test(src[j])) {
        flags += src[j];
        j += 1;
      }
      adv(j - i);
      out.push({ t: "re", v: body, flags, line: startLine, col: startCol });
      continue;
    }
    if (c === '"') {
      let j = i + 1;
      let v = "";
      while (j < n && src[j] !== '"') {
        if (src[j] === "\\") {
          const nx = src[j + 1];
          v += nx === "n" ? "\n" : nx === "t" ? "\t" : (nx ?? "");
          j += 2;
        } else if (src[j] === "\n") {
          throw err('문자열이 닫히지 않았습니다 (")');
        } else {
          v += src[j];
          j += 1;
        }
      }
      if (j >= n) throw err('문자열이 닫히지 않았습니다 (")');
      adv(j + 1 - i);
      out.push({ t: "str", v, line: startLine, col: startCol });
      continue;
    }
    if (c === "$") {
      let j = i + 1;
      while (j < n && /[A-Za-z0-9_*]/.test(src[j])) j += 1;
      const v = src.slice(i + 1, j);
      if (v === "") throw err("$ 뒤에 이름이 필요합니다");
      adv(j - i);
      out.push({ t: "var", v, line: startLine, col: startCol });
      continue;
    }
    if (/[0-9]/.test(c)) {
      let j = i;
      while (j < n && /[0-9]/.test(src[j])) j += 1;
      const v = Number(src.slice(i, j));
      adv(j - i);
      out.push({ t: "num", v, line: startLine, col: startCol });
      continue;
    }
    if (/[A-Za-z_]/.test(c)) {
      let j = i;
      while (j < n && /[A-Za-z0-9_]/.test(src[j])) j += 1;
      const v = src.slice(i, j);
      adv(j - i);
      out.push({ t: "ident", v, line: startLine, col: startCol });
      continue;
    }
    const op = OPS.find((o) => src.startsWith(o, i));
    if (op) {
      adv(op.length);
      out.push({ t: "op", v: op, line: startLine, col: startCol });
      continue;
    }
    throw err(`알 수 없는 문자: ${c}`);
  }
  out.push({ t: "eof", line, col });
  return out;
}

class Parser {
  private pos = 0;
  private strings = new Map<string, StringDef>();
  constructor(
    private toks: Tok[],
    private defaultField: CondField,
  ) {}

  private peek(): Tok {
    return this.toks[this.pos];
  }
  private next(): Tok {
    const t = this.toks[this.pos];
    if (t.t !== "eof") this.pos += 1;
    return t;
  }
  private fail(t: Tok, m: string): never {
    throw new RuleSyntaxError(t.line, t.col, m);
  }
  private isIdent(v: string): boolean {
    const t = this.peek();
    return t.t === "ident" && t.v.toLowerCase() === v;
  }
  private isOp(v: string): boolean {
    const t = this.peek();
    return t.t === "op" && t.v === v;
  }
  private expectIdent(v: string): void {
    if (!this.isIdent(v)) this.fail(this.peek(), `'${v}'가 와야 합니다`);
    this.next();
  }
  private expectOp(v: string): void {
    if (!this.isOp(v)) this.fail(this.peek(), `'${v}'가 와야 합니다`);
    this.next();
  }

  rule(): ParsedRule {
    this.expectIdent("rule");
    const idTok = this.next();
    if (idTok.t !== "ident") this.fail(idTok, "룰 이름(영문·숫자·밑줄)이 와야 합니다");
    this.expectOp("{");
    const meta: Record<string, string> = {};
    let expr: FilterExpr | null = null;
    while (!this.isOp("}")) {
      const t = this.peek();
      if (t.t === "eof") this.fail(t, "'}'로 룰을 닫아야 합니다");
      if (this.isIdent("meta")) {
        this.next();
        this.expectOp(":");
        while (this.peek().t === "ident" && !this.sectionAhead()) {
          const k = this.next() as Tok & { t: "ident" };
          this.expectOp("=");
          const v = this.next();
          if (v.t === "str") meta[k.v] = v.v;
          else if (v.t === "num") meta[k.v] = String(v.v);
          else if (v.t === "ident" && (v.v === "true" || v.v === "false")) meta[k.v] = v.v;
          else this.fail(v, "meta 값은 문자열이나 숫자여야 합니다");
        }
      } else if (this.isIdent("strings")) {
        this.next();
        this.expectOp(":");
        while (this.peek().t === "var") {
          const id = this.next() as Tok & { t: "var" };
          if (id.v.includes("*")) this.fail(id, "문자열 이름에 *를 쓸 수 없습니다");
          this.expectOp("=");
          const v = this.next();
          let def: StringDef;
          if (v.t === "str") def = { id: id.v, kind: "text", value: v.v, nocase: false, line: id.line };
          else if (v.t === "re") {
            checkRegex(v.v, v);
            def = { id: id.v, kind: "regex", value: v.v, nocase: v.flags.includes("i"), line: id.line };
          } else this.fail(v, '문자열 값은 "텍스트" 또는 /정규식/ 이어야 합니다');
          while (this.isIdent("nocase") || this.isIdent("ascii") || this.isIdent("wide")) {
            if (this.isIdent("nocase")) def.nocase = true;
            this.next();
          }
          if (this.strings.has(def.id)) this.fail(id, `문자열 $${def.id}가 두 번 정의됐습니다`);
          this.strings.set(def.id, def);
        }
      } else if (this.isIdent("condition")) {
        this.next();
        this.expectOp(":");
        expr = this.orExpr();
      } else {
        this.fail(t, "meta:, strings:, condition: 중 하나가 와야 합니다");
      }
    }
    this.next();
    if (this.peek().t !== "eof") this.fail(this.peek(), "룰은 하나만 쓸 수 있습니다");
    if (!expr) this.fail(idTok, "condition: 절이 필요합니다");
    return { id: idTok.v, name: meta.name ?? idTok.v, description: meta.description ?? "", expr };
  }

  private sectionAhead(): boolean {
    return this.isIdent("strings") || this.isIdent("condition") || this.isIdent("meta");
  }

  private orExpr(): FilterExpr {
    const items = [this.andExpr()];
    while (this.isIdent("or")) {
      this.next();
      items.push(this.andExpr());
    }
    return items.length === 1 ? items[0] : { kind: "or", items };
  }
  private andExpr(): FilterExpr {
    const items = [this.notExpr()];
    while (this.isIdent("and")) {
      this.next();
      items.push(this.notExpr());
    }
    return items.length === 1 ? items[0] : { kind: "and", items };
  }
  private notExpr(): FilterExpr {
    if (this.isIdent("not")) {
      this.next();
      return { kind: "not", item: this.notExpr() };
    }
    if (this.isOp("(")) {
      this.next();
      const e = this.orExpr();
      this.expectOp(")");
      return e;
    }
    return this.term();
  }

  private term(): FilterExpr {
    const t = this.peek();
    if (t.t === "var") {
      this.next();
      return this.stringTerm(this.defaultField, t.v, t);
    }
    if (t.t === "ident") {
      const w = t.v.toLowerCase();
      if (w === "true") {
        this.next();
        return { kind: "true" };
      }
      if (w === "false") {
        this.next();
        return { kind: "not", item: { kind: "true" } };
      }
      if (w === "any" || w === "all") {
        this.next();
        this.expectIdent("of");
        const ids = this.stringSet();
        const items = ids.map((id) => this.stringTerm(this.defaultField, id, t));
        if (items.length === 0) this.fail(t, "해당하는 문자열이 없습니다");
        return w === "any" ? { kind: "or", items } : { kind: "and", items };
      }
      const field = FIELDS[w];
      if (!field) this.fail(t, `알 수 없는 필드: ${t.v} (status, bytes, method, ip, path, protocol, referrer, ua, level, message)`);
      this.next();
      return this.fieldTerm(field, t);
    }
    this.fail(t, "조건이 와야 합니다");
  }

  private stringSet(): string[] {
    if (this.isIdent("them")) {
      this.next();
      return [...this.strings.keys()];
    }
    this.expectOp("(");
    const ids: string[] = [];
    for (;;) {
      const v = this.next();
      if (v.t !== "var") this.fail(v, "$이름이 와야 합니다");
      if (v.v.endsWith("*")) {
        const prefix = v.v.slice(0, -1);
        for (const k of this.strings.keys()) if (k.startsWith(prefix)) ids.push(k);
      } else ids.push(v.v);
      if (this.isOp(",")) {
        this.next();
        continue;
      }
      this.expectOp(")");
      break;
    }
    return ids;
  }

  /** `$id` 단독 또는 `field contains/matches $id`. */
  private stringTerm(field: CondField, id: string, at: Tok): FilterExpr {
    const def = this.strings.get(id);
    if (!def) this.fail(at, `정의되지 않은 문자열: $${id}`);
    if (NUMERIC.has(field)) this.fail(at, "숫자 필드에는 문자열 서명을 쓸 수 없습니다");
    if (def.kind === "regex") return { kind: "cond", field, op: "regex", value: def.nocase ? `(?i)${def.value}` : def.value };
    return { kind: "cond", field, op: def.nocase ? "icontains" : "contains", value: def.value };
  }

  /** `필드 in ("a", "b")` / `필드 in (404, 500)`. 같은 필드의 eq를 OR로 묶는다. */
  private valueList(field: CondField, numeric: boolean, at: Tok): FilterExpr {
    this.expectOp("(");
    const items: FilterExpr[] = [];
    while (!this.isOp(")")) {
      const v = this.next();
      if (numeric) {
        if (v.t !== "num") this.fail(v, `${fieldName(field)} 목록에는 따옴표 없는 숫자를 씁니다`);
        items.push({ kind: "cond", field, op: "eq", value: String(v.v) });
      } else {
        if (v.t !== "str") this.fail(v, `${fieldName(field)} 목록의 값은 따옴표로 감쌉니다`);
        items.push({ kind: "cond", field, op: "eq", value: v.v });
      }
      if (this.isOp(",")) this.next();
      else break;
    }
    this.expectOp(")");
    if (items.length === 0) this.fail(at, "목록에는 값을 하나 이상 넣어야 합니다");
    return items.length === 1 ? items[0] : { kind: "or", items };
  }

  private fieldTerm(field: CondField, at: Tok): FilterExpr {
    const t = this.next();
    const numeric = NUMERIC.has(field);
    if (t.t === "op" && ["==", "!=", ">", ">=", "<", "<="].includes(t.v)) {
      const v = this.next();
      const op: CondOp = t.v === "==" ? "eq" : t.v === "!=" ? "ne" : t.v === ">" ? "gt" : t.v === ">=" ? "gte" : t.v === "<" ? "lt" : "lte";
      if (numeric) {
        if (v.t !== "num") this.fail(v, `${fieldName(field)}는 숫자와 비교해야 합니다`);
        return { kind: "cond", field, op, value: String(v.v) };
      }
      if (op !== "eq" && op !== "ne") this.fail(t, `${fieldName(field)}에는 ==, != 만 쓸 수 있습니다`);
      if (v.t === "str") return { kind: "cond", field, op, value: v.v };
      if (v.t === "var") {
        const def = this.strings.get(v.v);
        if (!def || def.kind !== "text") this.fail(v, "== 오른쪽에는 텍스트 문자열만 올 수 있습니다");
        return { kind: "cond", field, op, value: def.value };
      }
      this.fail(v, '"값"이 와야 합니다');
    }
    if (t.t === "ident") {
      const w = t.v.toLowerCase();
      if (w === "in") {
        // 목록: 같은 필드에 값 여러 개. 값이 둘 이상인 IP·상태코드를 한 줄로 쓴다.
        if (this.isOp("(")) return this.valueList(field, numeric, t);
        if (!numeric) this.fail(t, '문자열 필드는 in ("값", "값") 목록만 쓸 수 있습니다(숫자 필드만 a..b 범위)');
        const a = this.next();
        this.expectOp("..");
        const b = this.next();
        if (a.t !== "num" || b.t !== "num") this.fail(a, "범위는 숫자..숫자 꼴이어야 합니다");
        return {
          kind: "and",
          items: [
            { kind: "cond", field, op: "gte", value: String(a.v) },
            { kind: "cond", field, op: "lte", value: String(b.v) },
          ],
        };
      }
      if (w === "is") {
        let neg = false;
        if (this.isIdent("not")) {
          this.next();
          neg = true;
        }
        if (this.isIdent("null") || this.isIdent("empty")) this.next();
        else this.fail(this.peek(), "is 뒤에는 null 또는 not null이 와야 합니다");
        const c: FilterExpr = { kind: "cond", field, op: "is_null", value: "" };
        return neg ? { kind: "not", item: c } : c;
      }
      if (w === "contains" || w === "icontains" || w === "startswith" || w === "endswith" || w === "matches") {
        if (numeric) this.fail(t, `${fieldName(field)}에는 문자열 연산을 쓸 수 없습니다`);
        const v = this.next();
        if (v.t === "var") {
          if (w === "startswith" || w === "endswith") {
            const def = this.strings.get(v.v);
            if (!def || def.kind !== "text") this.fail(v, "startswith/endswith에는 텍스트 문자열만 쓸 수 있습니다");
            return { kind: "cond", field, op: w === "startswith" ? "starts_with" : "ends_with", value: def.value };
          }
          return this.stringTerm(field, v.v, v);
        }
        if (v.t === "re") {
          if (w !== "matches" && w !== "contains") this.fail(v, `${w}에는 정규식을 쓸 수 없습니다`);
          checkRegex(v.v, v);
          return { kind: "cond", field, op: "regex", value: v.flags.includes("i") ? `(?i)${v.v}` : v.v };
        }
        if (v.t === "str") {
          const op: CondOp = w === "contains" ? "contains" : w === "icontains" ? "icontains" : w === "startswith" ? "starts_with" : w === "endswith" ? "ends_with" : "regex";
          if (op === "regex") checkRegex(v.v, v);
          return { kind: "cond", field, op, value: v.v };
        }
        this.fail(v, '"텍스트", /정규식/, $이름 중 하나가 와야 합니다');
      }
    }
    this.fail(at, `${fieldName(field)} 뒤에 연산(==, contains, matches, in, is null…)이 와야 합니다`);
  }
}

function fieldName(f: CondField): string {
  return Object.entries(FIELDS).find(([, v]) => v === f)?.[0] ?? f;
}

function checkRegex(pattern: string, at: Tok): void {
  try {
    new RegExp(pattern.replace(/^\(\?i\)/, ""));
  } catch (e) {
    throw new RuleSyntaxError(at.line, at.col, `정규식 오류: ${e instanceof Error ? e.message : String(e)}`);
  }
  if (/\(\?[=!<]/.test(pattern)) throw new RuleSyntaxError(at.line, at.col, "룩어라운드(?=, ?!, ?<)는 지원하지 않습니다");
}

export interface ParseOptions {
  /** `$id`만 쓴 서명이 적용될 필드. 접근 로그는 경로, 에러 로그는 메시지. */
  defaultField?: CondField;
}

/** 룰 텍스트를 파싱한다. 실패하면 위치가 있는 오류 목록. */
export function parseRule(src: string, opts: ParseOptions = {}): ParseResult {
  try {
    const toks = tokenize(src);
    if (toks.length === 1) return { ok: false, errors: [{ line: 1, col: 1, message: "룰이 비어 있습니다" }] };
    const rule = new Parser(toks, opts.defaultField ?? "request_target").rule();
    return { ok: true, rule };
  } catch (e) {
    if (e instanceof RuleSyntaxError) return { ok: false, errors: [{ line: e.line, col: e.col, message: e.message }] };
    return { ok: false, errors: [{ line: 1, col: 1, message: e instanceof Error ? e.message : String(e) }] };
  }
}

/** 조건식을 한 줄 요약으로. 저장된 룰의 툴팁에 쓴다. */
export function describeExpr(e: FilterExpr): string {
  const name = (f: CondField) =>
    ({ status: "상태", bytes_sent: "응답 크기", client_ip: "IP", method: "메서드", request_target: "경로", protocol: "프로토콜", referrer: "리퍼러", user_agent: "UA", extra: "확장 필드", level: "레벨", message: "메시지" })[f];
  switch (e.kind) {
    case "true":
      return "전체";
    case "and":
      return e.items.map(describeExpr).join(" 그리고 ");
    case "or":
      return "(" + e.items.map(describeExpr).join(" 또는 ") + ")";
    case "not":
      return "아님(" + describeExpr(e.item) + ")";
    case "cond": {
      const n = name(e.field);
      const opText: Record<CondOp, string> = { eq: "=", ne: "≠", gt: ">", gte: "≥", lt: "<", lte: "≤", contains: "포함", icontains: "포함(대소문자 무시)", starts_with: "시작", ends_with: "끝", regex: "정규식", is_null: "없음" };
      return e.op === "is_null" ? `${n} 없음` : `${n} ${opText[e.op]} ${e.value}`;
    }
  }
}

/** 새 접근 룰 편집기의 시작 텍스트. */
export const RULE_TEMPLATE = `rule my_rule
{
    meta:
        name = "내 룰"
        description = "무엇을 찾는 룰인지"

    strings:
        $sig1 = "/admin"
        $sig2 = /union[\\s+]+select/ nocase

    condition:
        status in 400..499 and ($sig1 or $sig2)
}
`;

/** 새 에러 룰 편집기의 시작 텍스트. `$id`만 쓴 서명은 메시지에 걸린다. */
export const ERROR_RULE_TEMPLATE = `rule my_error_rule
{
    meta:
        name = "내 에러 룰"
        description = "무엇을 찾는 룰인지"

    strings:
        $sig1 = "Connection refused"
        $sig2 = /upstream timed out/ nocase

    condition:
        level matches /error|crit/i and any of them
}
`;
