// 편집기 문법 색칠용 토큰화. 파서와 달리 실패하지 않고 끝까지 색을 입힌다. 순수 함수, 테스트 대상.

export type HlKind = "comment" | "string" | "regex" | "var" | "keyword" | "field" | "number" | "punct" | "text";

export interface HlToken {
  kind: HlKind;
  text: string;
}

const KEYWORDS = new Set([
  "rule", "meta", "strings", "condition", "and", "or", "not", "in", "any", "all", "of", "them", "is", "null", "empty", "nocase", "ascii", "wide", "true", "false",
  "contains", "icontains", "startswith", "endswith", "matches",
]);
export const FIELDS = new Set(["status", "bytes", "bytes_sent", "method", "ip", "client_ip", "path", "url", "target", "request_target", "protocol", "referrer", "referer", "ua", "user_agent", "level", "message", "msg"]);

/** 한 줄씩 색칠한다. 블록 주석은 줄을 넘어갈 수 있어 상태를 넘긴다. */
export function highlightLines(src: string): HlToken[][] {
  const out: HlToken[][] = [];
  let inBlock = false;
  for (const line of src.split("\n")) {
    const toks: HlToken[] = [];
    let i = 0;
    let prevWord = "";
    let prevPunct = "";
    const push = (kind: HlKind, text: string) => {
      if (text !== "") toks.push({ kind, text });
    };
    while (i < line.length) {
      if (inBlock) {
        const end = line.indexOf("*/", i);
        if (end === -1) {
          push("comment", line.slice(i));
          i = line.length;
        } else {
          push("comment", line.slice(i, end + 2));
          i = end + 2;
          inBlock = false;
        }
        continue;
      }
      const c = line[i];
      if (c === "/" && line[i + 1] === "/") {
        push("comment", line.slice(i));
        break;
      }
      if (c === "/" && line[i + 1] === "*") {
        inBlock = true;
        continue;
      }
      const regexAllowed = prevPunct === "=" || prevPunct === "(" || prevPunct === "," || ["matches", "contains", "icontains"].includes(prevWord);
      if (c === "/" && regexAllowed) {
        let j = i + 1;
        while (j < line.length && line[j] !== "/") j += line[j] === "\\" ? 2 : 1;
        j = Math.min(j + 1, line.length);
        while (j < line.length && /[a-z]/i.test(line[j])) j += 1;
        push("regex", line.slice(i, j));
        i = j;
        prevWord = "";
        prevPunct = "";
        continue;
      }
      if (c === '"') {
        let j = i + 1;
        while (j < line.length && line[j] !== '"') j += line[j] === "\\" ? 2 : 1;
        j = Math.min(j + 1, line.length);
        push("string", line.slice(i, j));
        i = j;
        prevWord = "";
        prevPunct = "";
        continue;
      }
      if (c === "$") {
        let j = i + 1;
        while (j < line.length && /[A-Za-z0-9_*]/.test(line[j])) j += 1;
        push("var", line.slice(i, j));
        i = j;
        prevWord = "";
        prevPunct = "";
        continue;
      }
      if (/[0-9]/.test(c)) {
        let j = i;
        while (j < line.length && /[0-9]/.test(line[j])) j += 1;
        push("number", line.slice(i, j));
        i = j;
        prevWord = "";
        prevPunct = "";
        continue;
      }
      if (/[A-Za-z_]/.test(c)) {
        let j = i;
        while (j < line.length && /[A-Za-z0-9_]/.test(line[j])) j += 1;
        const w = line.slice(i, j);
        const lw = w.toLowerCase();
        push(KEYWORDS.has(lw) ? "keyword" : FIELDS.has(lw) ? "field" : "text", w);
        i = j;
        prevWord = lw;
        prevPunct = "";
        continue;
      }
      if (/\s/.test(c)) {
        let j = i;
        while (j < line.length && /\s/.test(line[j])) j += 1;
        push("text", line.slice(i, j));
        i = j;
        continue;
      }
      push("punct", c);
      prevPunct = c;
      prevWord = "";
      i += 1;
    }
    out.push(toks);
  }
  return out;
}

/** 자동완성 후보. `$`로 시작하면 문자열 이름, 아니면 필드·키워드. */
export function completions(word: string, strings: string[]): string[] {
  if (word.startsWith("$")) {
    const p = word.slice(1).toLowerCase();
    return strings.filter((s) => s.toLowerCase().startsWith(p)).map((s) => `$${s}`);
  }
  const p = word.toLowerCase();
  if (p === "") return [];
  const pool = ["status", "bytes", "method", "ip", "path", "protocol", "referrer", "ua", "level", "message", "contains", "icontains", "startswith", "endswith", "matches", "in", "is null", "is not null", "and", "or", "not", "any of them", "all of them", "nocase", "true", "false", "meta:", "strings:", "condition:"];
  return pool.filter((k) => k.startsWith(p) && k !== p);
}
