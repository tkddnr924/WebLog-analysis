// 분석 룰 사이드바. 기본 룰과 사용자 룰(저장된 뷰)을 나열하고, 고른 룰의 조건을 조회·통계에 적용한다.
import { useCallback, useEffect, useState } from "react";
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { BUILTIN_RULES, ruleFilter, ruleFromView, type Rule } from "../lib/rules";
import type { ParsedRule } from "../lib/yara";
import type { SavedView } from "../types";
import { RuleEditor } from "./RuleEditor";

type EditorState = { mode: "add" } | { mode: "edit"; rule: Rule } | null;

export function RulesSidebar() {
  const { project, applyRule, ruleRequest, setNotice } = useAppState();
  const [views, setViews] = useState<SavedView[]>([]);
  const [selected, setSelected] = useState<string>(BUILTIN_RULES[1].id);
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

  const custom = views.map(ruleFromView);
  const rules: Rule[] = [...BUILTIN_RULES, ...custom];
  const rule = rules.find((r) => r.id === selected) ?? BUILTIN_RULES[1];

  const run = (r: Rule) => applyRule(r.name, ruleFilter(r, { active_only: true }));

  const pick = (r: Rule) => {
    setSelected(r.id);
    run(r);
  };

  // 첫 진입: 기본 룰을 한 번 적용해 결과 화면이 비어 있지 않게 한다.
  useEffect(() => {
    if (project && ruleRequest === null) run(BUILTIN_RULES[1]);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project]);

  const save = async (parsed: ParsedRule, source: string) => {
    const filter = { ...ruleFilter({ ...parsed, id: "", source, builtin: false }), active_only: true };
    const v = await api.saveView(parsed.name, { filter, sort: "time_asc", columns: [], rule_source: source });
    // 이름을 바꿔 저장했으면 옛 항목은 지운다(저장은 이름 기준 upsert).
    if (editor?.mode === "edit" && editor.rule.viewId !== undefined && editor.rule.viewId !== v.view_id) {
      await api.deleteView(editor.rule.viewId);
    }
    await loadViews();
    setEditor(null);
    setSelected(`view:${v.view_id}`);
    applyRule(parsed.name, filter);
    setNotice(null);
  };

  const remove = async () => {
    if (rule.builtin || rule.viewId === undefined) return;
    if (!window.confirm(`룰 '${rule.name}'을(를) 삭제합니다.`)) return;
    setBusy(true);
    try {
      await api.deleteView(rule.viewId);
      setSelected(BUILTIN_RULES[1].id);
      await loadViews();
      run(BUILTIN_RULES[1]);
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
      <div className="rules-list" role="listbox" aria-label="룰 목록">
        {BUILTIN_RULES.map((r) => (
          <RuleItem key={r.id} r={r} on={r.id === selected} onPick={() => pick(r)} onOpen={r.source ? () => setEditor({ mode: "edit", rule: r }) : undefined} />
        ))}
        {custom.length > 0 && <div className="rules-sep">내 룰</div>}
        {custom.map((r) => (
          <RuleItem key={r.id} r={r} on={r.id === selected} onPick={() => pick(r)} onOpen={() => setEditor({ mode: "edit", rule: r })} />
        ))}
      </div>
      {editor?.mode === "add" && <RuleEditor initial={null} title="룰 추가" onCancel={() => setEditor(null)} onSave={save} />}
      {editor?.mode === "edit" && (
        <RuleEditor
          initial={editor.rule.source || null}
          title={editor.rule.builtin ? `${editor.rule.name} (기본 룰 · 복사본으로 저장됨)` : `${editor.rule.name} 편집`}
          onCancel={() => setEditor(null)}
          onSave={save}
        />
      )}
    </aside>
  );
}

/** 클릭은 적용, 더블클릭(또는 ✎)은 편집기. 기본 룰도 열어 볼 수 있고 저장하면 사용자 룰 복사본이 된다. */
function RuleItem({ r, on, onPick, onOpen }: { r: Rule; on: boolean; onPick: () => void; onOpen?: () => void }) {
  const bookmark = r.id === "builtin:bookmarks";
  return (
    <div className={`rule-item ${on ? "on" : ""} ${r.error ? "broken" : ""} ${bookmark ? "bookmark" : ""}`} role="option" aria-selected={on} title={r.description} onClick={onPick} onDoubleClick={onOpen}>
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
