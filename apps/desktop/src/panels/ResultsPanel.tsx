// 결과 화면: 조회·통계·작업 탭. 실제 분석 작업은 여기서 한다.
import { useState } from "react";
import { QueryPanel } from "./QueryPanel";
import { StatsPanel } from "./StatsPanel";
import { RulesSidebar } from "./RulesSidebar";

type Tab = "query" | "stats";

const TABS: { id: Tab; label: string }[] = [
  { id: "query", label: "조회" },
  { id: "stats", label: "통계" },
];

export function ResultsPanel() {
  const [tab, setTab] = useState<Tab>("query");
  return (
    <section className="results">
      <RulesSidebar />
      <div className="results-main">
        <div className="tabs main-tabs" role="tablist">
        {TABS.map((t) => (
          <button key={t.id} role="tab" aria-selected={tab === t.id} className={tab === t.id ? "on" : ""} onClick={() => setTab(t.id)}>
            {t.label}
          </button>
        ))}
      </div>
        <div className="results-body">
          {tab === "query" && <QueryPanel />}
          {tab === "stats" && <StatsPanel />}
        </div>
      </div>
    </section>
  );
}
