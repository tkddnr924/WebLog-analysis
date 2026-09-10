// 룰 편집기. 왼쪽은 원문(줄 번호 포함), 오른쪽은 실시간 파싱 결과와 문법 안내. YARA 룰을 쓰듯 텍스트로 작성한다.
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type DragEvent,
  type KeyboardEvent,
} from "react";
import {
  ERROR_RULE_TEMPLATE,
  RULE_TEMPLATE,
  describeExpr,
  parseRule,
  type ParsedRule,
} from "../lib/yara";
import { completions, highlightLines } from "../lib/yaraHighlight";
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { defaultRuleField } from "../lib/rules";
import { emptyFilter, type LogKind } from "../types";
import { formatCount } from "../lib/format";

export function RuleEditor({
  kind,
  initial,
  title,
  onCancel,
  /** 사용자 룰을 열었을 때만 온다. 누르면 확인 후 지운다. */
  onDelete,
  onSave,
}: {
  /** 편집 중인 룰이 어느 로그 종류의 것인지. 기본 필드·예시·일치 건수가 달라진다. */
  kind: LogKind;
  initial: string | null;
  title: string;
  onCancel: () => void;
  onDelete?: () => Promise<void>;
  onSave: (rule: ParsedRule, source: string) => Promise<void>;
}) {
  const { project } = useAppState();
  const [src, setSrc] = useState(initial ?? (kind === "error" ? ERROR_RULE_TEMPLATE : RULE_TEMPLATE));
  const [busy, setBusy] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [count, setCount] = useState<
    { n: number; forSrc: string } | "loading" | null
  >(null);
  const [ac, setAc] = useState<{
    items: string[];
    index: number;
    start: number;
  } | null>(null);
  const textRef = useRef<HTMLTextAreaElement>(null);
  const gutterRef = useRef<HTMLDivElement>(null);
  const hlRef = useRef<HTMLPreElement>(null);
  const parsed = useMemo(() => parseRule(src, { defaultField: defaultRuleField(kind) }), [src, kind]);
  const lines = src.split("\n").length;
  const hl = useMemo(() => highlightLines(src), [src]);
  const errorLine = parsed.ok ? -1 : parsed.errors[0].line;
  const stringNames = useMemo(() => {
    const out: string[] = [];
    for (const m of src.matchAll(/\$([A-Za-z0-9_]+)\s*=/g))
      if (!out.includes(m[1])) out.push(m[1]);
    return out;
  }, [src]);

  const syncScroll = (el: HTMLElement) => {
    if (gutterRef.current) gutterRef.current.scrollTop = el.scrollTop;
    if (hlRef.current) {
      hlRef.current.scrollTop = el.scrollTop;
      hlRef.current.scrollLeft = el.scrollLeft;
    }
  };

  /** 커서 앞의 단어(식별자 또는 $이름)로 자동완성 후보를 갱신한다. */
  const refreshAc = (text: string, caret: number) => {
    const before = text.slice(0, caret);
    const m = /(\$?[A-Za-z_][A-Za-z0-9_]*|\$)$/.exec(before);
    if (!m) {
      setAc(null);
      return;
    }
    const items = completions(m[1], stringNames).slice(0, 8);
    setAc(
      items.length > 0 ? { items, index: 0, start: caret - m[1].length } : null,
    );
  };

  const acceptAc = (choice: string) => {
    const ta = textRef.current;
    if (!ta || !ac) return;
    const caret = ta.selectionStart;
    const next = src.slice(0, ac.start) + choice + src.slice(caret);
    setSrc(next);
    setAc(null);
    const pos = ac.start + choice.length;
    requestAnimationFrame(() => ta.setSelectionRange(pos, pos));
  };

  /** strings: 절에 새 문자열 정의를 넣는다. 절이 없으면 condition: 앞에 만든다. */
  const addString = () => {
    let n = 1;
    while (stringNames.includes(`s${n}`)) n += 1;
    const line = `        $s${n} = "…"`;
    let next: string;
    let pos: number;
    const stringsAt = src.search(/^[ \t]*strings:[ \t]*$/m);
    if (stringsAt !== -1) {
      const eol = src.indexOf("\n", stringsAt);
      const at = eol === -1 ? src.length : eol;
      next = `${src.slice(0, at)}\n${line}${src.slice(at)}`;
      pos = at + 1;
    } else {
      const condAt = src.search(/^[ \t]*condition:/m);
      const at = condAt === -1 ? src.lastIndexOf("}") : condAt;
      const block = `    strings:\n${line}\n\n`;
      next = `${src.slice(0, at)}${block}${src.slice(at)}`;
      pos = at + "    strings:\n".length;
    }
    setSrc(next);
    const q = next.indexOf('"…"', pos);
    requestAnimationFrame(() => {
      textRef.current?.focus();
      if (q !== -1) textRef.current?.setSelectionRange(q + 1, q + 2);
    });
  };

  /** 현재 케이스에서 몇 건이 맞는지 센다. 전체를 훑는 작업이라 버튼으로만 돌린다. */
  const runCount = async () => {
    if (!parsed.ok || !project) return;
    setCount("loading");
    const mine = src;
    try {
      const n = await api.countLogs({
        ...emptyFilter(),
        active_only: true,
        log_kind: kind,
        expr: parsed.rule.expr,
      });
      setCount({ n, forSrc: mine });
    } catch (e) {
      setSaveError(errorText(e));
      setCount(null);
    }
  };

  const acOpen = useRef(false);
  useEffect(() => {
    acOpen.current = ac !== null;
  }, [ac]);
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === "Escape" && !acOpen.current) onCancel();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onCancel]);

  const onKeyDown = (e: KeyboardEvent<HTMLTextAreaElement>) => {
    const ta = e.currentTarget;
    if (ac) {
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        e.preventDefault();
        const d = e.key === "ArrowDown" ? 1 : -1;
        setAc({
          ...ac,
          index: (ac.index + d + ac.items.length) % ac.items.length,
        });
        return;
      }
      if (e.key === "Tab" || e.key === "Enter") {
        e.preventDefault();
        acceptAc(ac.items[ac.index]);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        e.stopPropagation();
        setAc(null);
        return;
      }
    }
    if (e.key === "Tab") {
      e.preventDefault();
      const { selectionStart: a, selectionEnd: b } = ta;
      const next = `${src.slice(0, a)}    ${src.slice(b)}`;
      setSrc(next);
      requestAnimationFrame(() => ta.setSelectionRange(a + 4, a + 4));
    } else if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
      e.preventDefault();
      void submit();
    } else if (e.key === "Enter") {
      // 앞 줄의 들여쓰기를 유지한다.
      const a = ta.selectionStart;
      const lineStart = src.lastIndexOf("\n", a - 1) + 1;
      const indent = /^[ \t]*/.exec(src.slice(lineStart, a))?.[0] ?? "";
      if (indent) {
        e.preventDefault();
        const next = `${src.slice(0, a)}\n${indent}${src.slice(ta.selectionEnd)}`;
        setSrc(next);
        requestAnimationFrame(() =>
          ta.setSelectionRange(a + 1 + indent.length, a + 1 + indent.length),
        );
      }
    }
  };

  /** 커서 위치(선택 영역을 대체)에 조각을 넣는다. 앞뒤가 붙지 않도록 필요한 공백을 더한다. */
  const insert = (snippet: string) => {
    const ta = textRef.current;
    if (!ta) return;
    const a = ta.selectionStart;
    const b = ta.selectionEnd;
    const before = src.slice(0, a);
    const after = src.slice(b);
    const pad = (left: string, right: string) =>
      (left === "" || /[\s(]$/.test(left) ? "" : " ") + right;
    const text =
      pad(before, snippet) + (after === "" || /^[\s)]/.test(after) ? "" : " ");
    const next = before + text + after;
    setSrc(next);
    const caret = a + text.length;
    requestAnimationFrame(() => {
      ta.focus();
      // 값 자리("…", /…/)가 있으면 그 안을 선택해 바로 타이핑하게 한다.
      const m = /"…"|\/…\//.exec(text);
      if (m) ta.setSelectionRange(a + m.index + 1, a + m.index + 2);
      else ta.setSelectionRange(caret, caret);
    });
  };

  const onDragStart = (snippet: string) => (e: DragEvent<HTMLElement>) => {
    // textarea는 text/plain을 떨어뜨리면 포인터 위치에 스스로 삽입한다(브라우저 기본 동작).
    e.dataTransfer.setData("text/plain", snippet);
    e.dataTransfer.effectAllowed = "copy";
  };

  const submit = async () => {
    if (!parsed.ok) return;
    setBusy(true);
    setSaveError(null);
    try {
      await onSave(parsed.rule, src);
    } catch (e) {
      setSaveError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const jump = (line: number, col: number) => {
    const ta = textRef.current;
    if (!ta) return;
    const rows = src.split("\n");
    let pos = 0;
    for (let i = 0; i < line - 1 && i < rows.length; i += 1)
      pos += rows[i].length + 1;
    pos += Math.max(0, col - 1);
    ta.focus();
    ta.setSelectionRange(pos, pos);
  };

  return (
    <>
      <div className="drawer-backdrop" onClick={onCancel} aria-hidden="true" />
      <div
        className="dialog editor-dialog"
        role="dialog"
        aria-modal="true"
        aria-label={title}
      >
        <div className="dialog-head">
          <span>{title}</span>
          <span className="grow" />
          <span className="muted small">⌘/Ctrl+Enter 저장 · Esc 닫기</span>
        </div>
        <div className="editor-split">
          <div className="code-col">
            <div className="code-toolbar">
              <button
                type="button"
                onClick={addString}
                title={'strings: 절에 $s = "…" 한 줄을 넣습니다'}
              >
                + 문자열
              </button>
              <button
                type="button"
                onClick={() => void runCount()}
                disabled={!parsed.ok || !project || count === "loading"}
                title="현재 케이스 전체를 훑어 몇 건이 맞는지 셉니다"
              >
                {count === "loading" ? "세는 중…" : "일치 건수 확인"}
              </button>
              {count !== null && count !== "loading" && (
                <span
                  className={`small ${count.forSrc === src ? "ok" : "muted"}`}
                >
                  {formatCount(count.n)}건 일치
                  {count.forSrc === src ? "" : " (수정 전 기준)"}
                </span>
              )}
            </div>
            <div className="code">
              <div className="code-gutter" ref={gutterRef} aria-hidden="true">
                {Array.from({ length: lines }, (_, i) => (
                  <div key={i} className={errorLine === i + 1 ? "err" : ""}>
                    {i + 1}
                  </div>
                ))}
              </div>
              <div className="code-area">
                <pre className="code-hl mono" ref={hlRef} aria-hidden="true">
                  {hl.map((toks, i) => (
                    <div
                      key={i}
                      className={`hl-line ${errorLine === i + 1 ? "err" : ""}`}
                    >
                      {toks.map((t, j) => (
                        <span key={j} className={`hl-${t.kind}`}>
                          {t.text}
                        </span>
                      ))}
                      {"\n"}
                    </div>
                  ))}
                </pre>
                <textarea
                  ref={textRef}
                  className="code-text mono"
                  value={src}
                  onChange={(e) => {
                    setSrc(e.target.value);
                    refreshAc(e.target.value, e.target.selectionStart);
                  }}
                  onKeyDown={onKeyDown}
                  onClick={() => setAc(null)}
                  onBlur={() => window.setTimeout(() => setAc(null), 150)}
                  onScroll={(e) => syncScroll(e.currentTarget)}
                  spellCheck={false}
                  autoCapitalize="off"
                  autoCorrect="off"
                  wrap="off"
                  aria-label="룰 원문"
                />
                {ac && (
                  <div className="ac" role="listbox">
                    {ac.items.map((it, i) => (
                      <button
                        key={it}
                        type="button"
                        role="option"
                        aria-selected={i === ac.index}
                        className={i === ac.index ? "on" : ""}
                        onMouseDown={(e) => e.preventDefault()}
                        onClick={() => acceptAc(it)}
                      >
                        {it}
                      </button>
                    ))}
                    <span className="muted small">
                      Tab/Enter 선택 · ↑↓ 이동
                    </span>
                  </div>
                )}
              </div>
            </div>
          </div>
          <aside className="editor-side">
            {parsed.ok ? (
              <div className="parse-ok">
                <div className="parse-title">
                  <span className="dot ok" /> {parsed.rule.name}
                  <span className="muted small"> ({parsed.rule.id})</span>
                </div>
                {parsed.rule.description && (
                  <div className="muted small">{parsed.rule.description}</div>
                )}
                <div className="parse-cond mono small">
                  {describeExpr(parsed.rule.expr)}
                </div>
              </div>
            ) : (
              <div className="parse-bad">
                {parsed.errors.map((er, i) => (
                  <button
                    key={i}
                    type="button"
                    className="linklike err-link"
                    onClick={() => jump(er.line, er.col)}
                  >
                    {er.line}:{er.col} {er.message}
                  </button>
                ))}
              </div>
            )}
            {saveError && <div className="issue">{saveError}</div>}
            <Palette kind={kind} src={src} onInsert={insert} onDragStart={onDragStart} />
            <details className="help">
              <summary className="help-title">문법 전체</summary>
              <pre className="mono small">{HELP}</pre>
            </details>
          </aside>
        </div>
        <div className="dialog-actions">
          {onDelete && (
            <button type="button" className="danger" onClick={() => void onDelete()} disabled={busy}>
              룰 삭제
            </button>
          )}
          <span className="grow" />
          <button type="button" onClick={onCancel} disabled={busy}>
            취소
          </button>
          <button
            type="button"
            className="primary"
            onClick={() => void submit()}
            disabled={busy || !parsed.ok}
          >
            저장
          </button>
        </div>
      </div>
    </>
  );
}

interface Snip {
  label: string;
  text: string;
  cls?: string;
  hint?: string;
}

const FIELD_SNIPS: Snip[] = [
  {
    label: "status",
    text: "status",
    cls: "kind-status",
    hint: "상태코드(숫자)",
  },
  {
    label: "bytes",
    text: "bytes",
    cls: "kind-bytes_sent",
    hint: "응답 바이트(숫자)",
  },
  { label: "method", text: "method", cls: "kind-method", hint: "GET, POST…" },
  { label: "ip", text: "ip", cls: "kind-client_ip", hint: "클라이언트 IP" },
  {
    label: "path",
    text: "path",
    cls: "kind-request_target",
    hint: "요청 경로+쿼리",
  },
  {
    label: "protocol",
    text: "protocol",
    cls: "kind-protocol",
    hint: "HTTP/1.1",
  },
  {
    label: "referrer",
    text: "referrer",
    cls: "kind-referrer",
    hint: "Referer 헤더",
  },
  { label: "ua", text: "ua", cls: "kind-user_agent", hint: "User-Agent" },
];
const ERROR_FIELD_SNIPS: Snip[] = [
  { label: "level", text: "level", cls: "kind-status", hint: "에러 레벨(error, crit, warn…)" },
  { label: "message", text: "message", cls: "kind-request_target", hint: "에러 메시지 본문" },
  { label: "ip", text: "ip", cls: "kind-client_ip", hint: "클라이언트 IP" },
];
const OP_SNIPS: Snip[] = [
  { label: "==", text: '== "…"' },
  { label: "!=", text: '!= "…"' },
  { label: ">=", text: ">= 500" },
  { label: "<", text: "< 100" },
  { label: "in a..b", text: "in 400..499", hint: "숫자 범위(양 끝 포함)" },
  { label: "in (…)", text: 'in ("10.0.0.1", "10.0.0.2")', hint: "값 여러 개 중 하나(IP·메서드 등). 숫자 필드는 in (404, 500)" },
  {
    label: "contains",
    text: 'contains "…"',
    hint: "부분 문자열(대소문자 구분)",
  },
  {
    label: "icontains",
    text: 'icontains "…"',
    hint: "부분 문자열(대소문자 무시)",
  },
  { label: "startswith", text: 'startswith "…"' },
  { label: "endswith", text: 'endswith "…"' },
  { label: "matches", text: "matches /…/", hint: "정규식(RE2)" },
  { label: "is null", text: "is null" },
  { label: "is not null", text: "is not null" },
];
const LOGIC_SNIPS: Snip[] = [
  { label: "and", text: "and" },
  { label: "or", text: "or" },
  { label: "not", text: "not" },
  { label: "( )", text: "( … )" },
  {
    label: "any of them",
    text: "any of them",
    hint: "정의한 문자열 중 하나라도",
  },
  { label: "all of them", text: "all of them" },
  { label: "any of ($a*)", text: "any of ($…*)" },
];
const BLOCK_SNIPS: Snip[] = [
  {
    label: '$s = "텍스트"',
    text: '$s = "…"',
    hint: "strings: 안에 넣는 문자열 서명",
  },
  { label: "$s = /정규식/ nocase", text: "$s = /…/ nocase" },
  { label: "상태 2xx", text: "status in 200..299" },
  { label: "상태 5xx", text: "status >= 500" },
  { label: "POST만", text: 'method == "POST"' },
  { label: "봇 UA", text: "ua matches /python|curl|wget/i" },
  { label: "관리자 경로", text: 'path startswith "/admin"' },
];
const ERROR_BLOCK_SNIPS: Snip[] = [
  { label: '$s = "텍스트"', text: '$s = "…"', hint: "strings: 안에 넣는 문자열 서명" },
  { label: "$s = /정규식/ nocase", text: "$s = /…/ nocase" },
  { label: "심각도", text: "level matches /crit|alert|emerg/i" },
  { label: "오류만", text: "level matches /error/i" },
  { label: "파일 없음", text: 'message contains "No such file or directory"' },
  { label: "업스트림 실패", text: 'message contains "Connection refused"' },
];

/** 끌어 놓거나 눌러서 넣는 조각 팔레트. 현재 원문에 정의된 $문자열도 함께 보여준다. */
function Palette({
  kind,
  src,
  onInsert,
  onDragStart,
}: {
  kind: LogKind;
  src: string;
  onInsert: (s: string) => void;
  onDragStart: (s: string) => (e: DragEvent<HTMLElement>) => void;
}) {
  const strings = useMemo(() => {
    const out: string[] = [];
    for (const m of src.matchAll(/\$([A-Za-z0-9_]+)\s*=/g))
      if (!out.includes(m[1])) out.push(m[1]);
    return out;
  }, [src]);
  const group = (title: string, items: Snip[]) =>
    items.length === 0 ? null : (
      <div className="snip-group">
        <div className="snip-title">{title}</div>
        <div className="snip-row">
          {items.map((it) => (
            <button
              key={it.label}
              type="button"
              className={`snip ${it.cls ?? ""}`}
              draggable
              title={it.hint ?? it.text}
              onDragStart={onDragStart(it.text)}
              onClick={() => onInsert(it.text)}
            >
              {it.label}
            </button>
          ))}
        </div>
      </div>
    );
  return (
    <div className="palette" aria-label="조각 팔레트">
      <div className="palette-head">
        <span>조각</span>
        <span className="muted small">끌어 놓거나 눌러서 넣기</span>
      </div>
      {group(
        "문자열",
        strings.map((id) => ({
          label: `$${id}`,
          text: `$${id}`,
          cls: "kind-text",
        })),
      )}
      {group("필드", kind === "error" ? ERROR_FIELD_SNIPS : FIELD_SNIPS)}
      {group("연산", OP_SNIPS)}
      {group("논리", LOGIC_SNIPS)}
      {group("조각", kind === "error" ? ERROR_BLOCK_SNIPS : BLOCK_SNIPS)}
    </div>
  );
}

const HELP = `rule 식별자 {
  meta:
    name = "표시 이름"
    description = "설명"
  strings:
    $a = "텍스트"           부분 문자열 (nocase: 대소문자 무시)
    $b = /정규식/ nocase    RE2 문법, 룩어라운드 없음
  condition:
    $a                     기본 필드(접근: path, 에러: message)에 $a가 있음
    ua contains $b         다른 필드에 적용
    any of them            정의한 문자열 중 하나라도
    all of ($a*)           이름이 a로 시작하는 문자열 모두
    status == 404          == != > >= < <=
    status in 500..599     범위(숫자 필드)
    status in (404, 500)   목록 — 값이 여럿이면 이렇게 한 줄로
    ip in ("10.0.0.1", "10.0.0.2", "203.0.113.7")
    bytes > 100000
    method == "POST"
    ip == "10.0.0.1"       값이 하나일 때
    path startswith "/api"   endswith, contains, icontains, matches
    referrer is null       is not null
    level matches /crit/i  에러 로그 레벨
    message contains "…"   에러 로그 메시지
    and · or · not · ( )
}

접근 로그 필드: status  bytes  method  ip  path  protocol  referrer  ua
에러 로그 필드: level  message(msg)`;
