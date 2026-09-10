// 결과 화면. 최상위는 로그 종류(접근·에러) 선택이고, 그 아래에 조회·통계 탭이 있다. 룰 사이드바는 종류에 맞는 목록을 보여준다.
import { useState, type ReactElement } from "react";
import { QueryPanel } from "./QueryPanel";
import { ErrorPanel } from "./ErrorPanel";
import { StatsPanel } from "./StatsPanel";
import { ErrorStatsPanel } from "./ErrorStatsPanel";
import { RulesSidebar } from "./RulesSidebar";
import type { LogKind } from "../types";

type Tab = "query" | "stats";

const KINDS: { id: LogKind; label: string }[] = [
  { id: "access", label: "접근 로그" },
  { id: "error", label: "에러 로그" },
];

const TABS: { id: Tab; label: string }[] = [
  { id: "query", label: "조회" },
  { id: "stats", label: "통계" },
];

/** 종류를 바꾸면 다른 패널이 새로 마운트되므로 조회 상태도 그 종류의 것으로 갈린다. */
const PANELS: Record<LogKind, Record<Tab, () => ReactElement>> = {
  access: { query: QueryPanel, stats: StatsPanel },
  error: { query: ErrorPanel, stats: ErrorStatsPanel },
};

export function ResultsPanel() {
  const [kind, setKind] = useState<LogKind>("access");
  const [tab, setTab] = useState<Tab>("query");
  const Panel = PANELS[kind][tab];
  return (
    <section className="results">
      <RulesSidebar kind={kind} />
      <div className="results-main">
        <div className="kind-switch" role="tablist" aria-label="로그 종류">
          {KINDS.map((k) => (
            <button key={k.id} role="tab" aria-selected={kind === k.id} className={kind === k.id ? "on" : ""} onClick={() => setKind(k.id)}>
              {k.label}
            </button>
          ))}
        </div>
        <div className="tabs main-tabs" role="tablist" aria-label={kind === "error" ? "에러 로그 화면" : "접근 로그 화면"}>
          {TABS.map((t) => (
            <button key={t.id} role="tab" aria-selected={tab === t.id} className={tab === t.id ? "on" : ""} onClick={() => setTab(t.id)}>
              {t.label}
            </button>
          ))}
        </div>
        <div className="results-body">
          <Panel />
        </div>
      </div>
    </section>
  );
}
