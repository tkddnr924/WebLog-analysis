// 분석 룰 사이드바. 로그 종류별로 기본 룰과 사용자 룰(저장된 뷰)을 나열하고, 고른 룰의 조건을 조회·통계에 적용한다.
import { Fragment, useCallback, useEffect, useState } from "react";
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { parseTimeInput } from "../lib/format";
import { emptyRange, openRange, parseRange } from "../lib/timeRange";
import { builtinRules, ruleFilter, rulesFor, type Rule } from "../lib/rules";
import type { ParsedRule } from "../lib/yara";
import type { LogKind, SavedView } from "../types";
import { RuleEditor } from "./RuleEditor";

type EditorState = { mode: "add" } | { mode: "edit"; rule: Rule } | null;

/** 종류별 기본 선택: 북마크 다음의 "전체" 룰. */
const defaultRuleId = (kind: LogKind) => builtinRules(kind)[1].id;

export function RulesSidebar({ kind }: { kind: LogKind }) {
  const { project, applyRule, setNotice } = useAppState();
  const [views, setViews] = useState<SavedView[]>([]);
  // 종류마다 고른 룰을 따로 기억한다. 종류를 오가도 선택이 남고 조건이 섞이지 않는다.
  const [selectedByKind, setSelectedByKind] = useState<Record<LogKind, string>>({ access: defaultRuleId("access"), error: defaultRuleId("error") });
  const [busy, setBusy] = useState(false);
  const [editor, setEditor] = useState<EditorState>(null);

  const loadViews = useCallback(async () => {
    if (!project) return;
    try {
      setViews(await api.listViews());
    } catch (e) {
      setNotice(errorText(e));
    }
  }, [project, setNotice]);

  useEffect(() => {
    void loadViews();
  }, [loadViews]);

  const rules = rulesFor(kind, views);
  const builtins = builtinRules(kind);
  const custom = rules.filter((r) => !r.builtin);
  const selected = selectedByKind[kind];
  const rule = rules.find((r) => r.id === selected) ?? builtins[1];

  const run = (r: Rule) => applyRule(r.name, ruleFilter(r, { active_only: true, log_kind: kind }));

  const pick = (r: Rule) => {
    setSelectedByKind({ ...selectedByKind, [kind]: r.id });
    run(r);
  };

  // 프로젝트를 열거나 로그 종류를 바꾸면 그 종류의 룰을 다시 적용한다(다른 종류의 조건이 남지 않게).
  useEffect(() => {
    if (project) run(rule);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project, kind]);

  const save = async (parsed: ParsedRule, source: string) => {
    // 저장한 룰은 지금 보고 있는 종류의 룰이 된다(목록 분리 기준).
    const filter = { ...ruleFilter({ ...parsed, id: "", source, builtin: false }), active_only: true, log_kind: kind };
    const v = await api.saveView(parsed.name, { filter, sort: "time_asc", columns: [], rule_source: source });
    // 이름을 바꿔 저장했으면 옛 항목은 지운다(저장은 이름 기준 upsert).
    if (editor?.mode === "edit" && editor.rule.viewId !== undefined && editor.rule.viewId !== v.view_id) {
      await api.deleteView(editor.rule.viewId);
    }
    await loadViews();
    setEditor(null);
    setSelectedByKind({ ...selectedByKind, [kind]: `view:${v.view_id}` });
    applyRule(parsed.name, filter);
    setNotice(null);
  };

  const remove = async () => {
    if (rule.builtin || rule.viewId === undefined) return;
    if (!window.confirm(`룰 '${rule.name}'을(를) 삭제합니다.`)) return;
    setBusy(true);
    try {
      await api.deleteView(rule.viewId);
      setSelectedByKind({ ...selectedByKind, [kind]: defaultRuleId(kind) });
      await loadViews();
      run(builtins[1]);
    } catch (e) {
      setNotice(`룰 삭제 실패: ${errorText(e)}`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <aside className="rules" aria-label="분석 룰">
      <div className="rules-head">
        <span>룰</span>
        <span className="grow" />
        <button className="icon" onClick={() => setEditor({ mode: "add" })} disabled={busy || !project} title="룰 추가" aria-label="룰 추가">
          +
        </button>
        <button className="icon" onClick={remove} disabled={busy || rule.builtin} title={rule.builtin ? "기본 룰은 지울 수 없습니다" : "선택한 룰 삭제"} aria-label="룰 삭제">
          −
        </button>
      </div>
      <RangeBox />
      <div className="rules-list" role="listbox" aria-label="룰 목록">
        {builtins.map((r, i) => (
          <Fragment key={r.id}>
            <RuleItem r={r} on={r.id === selected} onPick={() => pick(r)} onOpen={r.source ? () => setEditor({ mode: "edit", rule: r }) : undefined} />
            {i === 0 && <div className="rules-divider" aria-hidden="true" />}
          </Fragment>
        ))}
        {custom.length > 0 && <div className="rules-sep">내 룰</div>}
        {custom.map((r) => (
          <RuleItem key={r.id} r={r} on={r.id === selected} onPick={() => pick(r)} onOpen={() => setEditor({ mode: "edit", rule: r })} />
        ))}
      </div>
      {editor?.mode === "add" && <RuleEditor kind={kind} initial={null} title={kind === "error" ? "에러 룰 추가" : "접근 룰 추가"} onCancel={() => setEditor(null)} onSave={save} />}
      {editor?.mode === "edit" && (
        <RuleEditor
          kind={kind}
          initial={editor.rule.source || null}
          title={editor.rule.builtin ? `${editor.rule.name} (기본 룰 · 복사본으로 저장됨)` : `${editor.rule.name} 편집`}
          onCancel={() => setEditor(null)}
          onSave={save}
        />
      )}
    </aside>
  );
}

/**
 * 기간 상자. 룰보다 상위 조건이라 사이드바에 두고, 조회·통계 어느 탭에서도 같은 구간만 보게 한다.
 * 사고 시각이 특정됐을 때 탭을 오가도 구간이 풀리지 않는 것이 목적이다.
 */
function RangeBox() {
  const { range, setRange, appliedRange, applyRange, setNotice } = useAppState();
  const submit = (e: React.FormEvent) => {
    e.preventDefault();
    const parsed = parseRange(range);
    if (typeof parsed === "string") {
      setNotice(parsed);
      return;
    }
    setNotice(null);
    applyRange(parsed);
  };
  const clear = () => {
    setRange(emptyRange);
    setNotice(null);
    applyRange(openRange);
  };
  const dirty = (parseTimeInput(range.from) ?? null) !== appliedRange.from || (parseTimeInput(range.to) ?? null) !== appliedRange.to;
  const limited = appliedRange.from !== null || appliedRange.to !== null;
  return (
    <form className="range-box" onSubmit={submit}>
      <div className="range-head">
        <span>기간</span>
        {limited && (
          <button type="button" className="linklike" onClick={clear}>
            해제
          </button>
        )}
      </div>
      <label className="range-field">
        <span>시작</span>
        <input value={range.from} onChange={(e) => setRange({ ...range, from: e.target.value })} placeholder="2026-09-01T00:00" spellCheck={false} />
      </label>
      <label className="range-field">
        <span>끝(제외)</span>
        <input value={range.to} onChange={(e) => setRange({ ...range, to: e.target.value })} placeholder="2026-09-02T00:00" spellCheck={false} />
      </label>
      <button type="submit" className={dirty ? "primary" : ""}>
        기간 적용
      </button>
      <div className="range-note muted">{limited ? "이 기간 안에서만 룰이 적용됩니다." : "전체 기간을 봅니다."}</div>
    </form>
  );
}

/** 클릭은 적용, 더블클릭(또는 ✎)은 편집기. 기본 룰도 열어 볼 수 있고 저장하면 사용자 룰 복사본이 된다. */
function RuleItem({ r, on, onPick, onOpen }: { r: Rule; on: boolean; onPick: () => void; onOpen?: () => void }) {
  const bookmark = r.id === "builtin:bookmarks";
  return (
    <div className={`rule-item ${on ? "on" : ""} ${r.error ? "broken" : ""} ${bookmark ? "bookmark" : ""}`} role="option" aria-selected={on} title={r.description} onClick={onPick} onDoubleClick={onOpen}>
      {bookmark && (
        <span className="rule-star" aria-hidden="true">
          ★
        </span>
      )}
      <span className="rule-name">{r.name}</span>
      {onOpen && (
        <button
          type="button"
          className="rule-edit"
          aria-label={`${r.name} 열기`}
          title="원문 보기·편집"
          onClick={(e) => {
            e.stopPropagation();
            onOpen();
          }}
        >
          ✎
        </button>
      )}
    </div>
  );
}
