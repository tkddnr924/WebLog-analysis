import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { DISPLAY_OFFSET_SECONDS, formatCount, formatTime } from "../lib/format";
import { applyScope } from "../lib/scope";
import { emptyFilter, type StatsResult } from "../types";

/** 시간축 막대 그래프. 단일 계열이라 색 하나만 쓰고, 값은 호버 툴팁과 표로 읽는다. */
type IpSort = "count" | "first" | "last";

/**
 * IP별 최초·마지막 탐지와 접근 횟수. 머리글을 눌러 정렬한다. 에러 통계도 같은 표를 쓴다.
 * 표는 상위 N만 보여주므로 전체 목록이 필요하면 CSV로 내보낸다.
 */
export function IpTable({ rows, onExport }: { rows: StatsResult["ip_rows"]; onExport: () => Promise<void> }) {
  const [sort, setSort] = useState<{ key: IpSort; desc: boolean }>({ key: "count", desc: true });
  const [saving, setSaving] = useState(false);
  const sorted = useMemo(() => {
    const v = (r: StatsResult["ip_rows"][number]) => (sort.key === "count" ? r.count : sort.key === "first" ? (r.first_seen ?? Number.MAX_SAFE_INTEGER) : (r.last_seen ?? -1));
    return [...rows].sort((a, b) => (sort.desc ? v(b) - v(a) : v(a) - v(b)) || a.ip.localeCompare(b.ip));
  }, [rows, sort]);
  const toggle = (key: IpSort) => setSort((s) => ({ key, desc: s.key === key ? !s.desc : key === "count" }));
  const head = (key: IpSort, label: string, cls = "") => (
    <th className={`sortable ${cls} ${sort.key === key ? "on" : ""}`} onClick={() => toggle(key)} aria-sort={sort.key === key ? (sort.desc ? "descending" : "ascending") : "none"}>
      {label}
      <span className="sort-mark">{sort.key === key ? (sort.desc ? "▼" : "▲") : ""}</span>
    </th>
  );
  const runExport = async () => {
    setSaving(true);
    try {
      await onExport();
    } finally {
      setSaving(false);
    }
  };
  return (
    <div className="chart ip-table">
      <div className="chart-head">
        <span>클라이언트 IP · 상위 {rows.length}개</span>
        <span className="muted small">머리글을 눌러 정렬</span>
        <span className="grow" />
        <button type="button" className="chart-action" onClick={() => void runExport()} disabled={saving} title="조건에 맞는 IP 전체를 CSV로 저장합니다(상위 N 제한 없음)">
          {saving ? "저장 중…" : "전체 CSV 저장"}
        </button>
      </div>
      <div className="table-wrap">
        <table className="grid">
          <thead>
            <tr>
              <th className="num rank">#</th>
              <th>IP</th>
              {head("first", "최초 탐지")}
              {head("last", "마지막 탐지")}
              {head("count", "접근 횟수", "num")}
            </tr>
          </thead>
          <tbody>
            {sorted.map((r, i) => (
              <tr key={r.ip}>
                <td className="num rank muted">{i + 1}</td>
                <td className="mono">{r.ip}</td>
                <td className="mono">{formatTime(r.first_seen)}</td>
                <td className="mono">{formatTime(r.last_seen)}</td>
                <td className="num">{formatCount(r.count)}</td>
              </tr>
            ))}
            {rows.length === 0 && (
              <tr>
                <td colSpan={5} className="muted">
                  조건에 맞는 IP가 없습니다.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
    </div>
  );
}

export function Bars({ title, rows, unit }: { title: string; rows: [string, number][]; unit: string }) {
  const max = Math.max(...rows.map((r) => r[1]), 1);
  return (
    <div className="chart">
      <div className="chart-head">
        <span>{title}</span>
      </div>
      {rows.length === 0 ? (
        <p className="muted">없음</p>
      ) : (
        <div className="hbars">
          {rows.map(([label, n]) => (
            <div key={label} className="hbar-row" title={`${label}: ${formatCount(n)}${unit}`}>
              <span className="hbar-label mono ellipsis">{label}</span>
              <span className="hbar-track">
                <span className="hbar-fill" style={{ width: `${(n / max) * 100}%` }} />
              </span>
              <span className="hbar-value num">{formatCount(n)}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

export function StatsPanel() {
  const { project, setNotice, ruleRequest, scope } = useAppState();
  const [activeOnly, setActiveOnly] = useState(true);
  const [topN, setTopN] = useState(20);
  const [stats, setStats] = useState<StatsResult | null>(null);
  const [loading, setLoading] = useState(false);
  // Only the newest request may write results; late answers to older conditions are dropped.
  const requestRef = useRef(0);

  const statsFilter = useMemo(
    () => applyScope({ ...(ruleRequest?.filter.log_kind === "access" ? ruleRequest.filter : emptyFilter()), active_only: activeOnly, log_kind: "access" }, scope),
    [ruleRequest, activeOnly, scope],
  );

  const run = useCallback(async () => {
    requestRef.current += 1;
    const id = requestRef.current;
    setLoading(true);
    try {
      const result = await api.computeStats({
        // 사이드바 접근 룰의 조건(상태·메서드·IP·경로) 위에 이 화면의 시간·작업·활성 조건을 얹는다. 집계는 접근 로그만 본다.
        filter: statsFilter,
        top_n: topN,
        bucket: "auto",
        tz_offset_seconds: DISPLAY_OFFSET_SECONDS,
      });
      if (id !== requestRef.current) return;
      setStats(result);
      setNotice(null);
    } catch (e) {
      if (id !== requestRef.current) return;
      setNotice(errorText(e));
    } finally {
      if (id === requestRef.current) setLoading(false);
    }
  }, [statsFilter, topN, setNotice]);

  // 룰을 고르거나 탭을 열면 바로 집계한다. 조건을 바꾼 뒤에는 "계산"으로 다시 돌린다.
  useEffect(() => {
    if (project) void run();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [project?.db_path, ruleRequest?.nonce, scope.nonce]);

  /** 표에 보이는 상위 N이 아니라 조건에 맞는 IP 전체를 CSV로 저장한다. */
  const exportIps = async () => {
    try {
      const path = await save({ defaultPath: "client-ips.csv", filters: [{ name: "CSV", extensions: ["csv"] }] });
      if (typeof path !== "string" || path === "") return;
      const rows = await api.exportIpStats(path, statsFilter);
      setNotice(`IP ${formatCount(rows)}개를 ${path}에 저장했습니다.`);
    } catch (e) {
      setNotice(errorText(e));
    }
  };

  const cancel = async () => {
    try {
      await api.cancelHeavy();
    } catch (e) {
      setNotice(errorText(e));
    }
  };

  if (!project) {
    return (
      <section>
        <h1>통계</h1>
        <p className="muted">프로젝트를 열면 통계를 볼 수 있습니다.</p>
      </section>
    );
  }

  return (
    <section>
      <form
        className="filter-bar"
        onSubmit={(e) => {
          e.preventDefault();
          void run();
        }}
      >
        <label className="f f-xs">
          <span>상위 N</span>
          <input type="number" min={1} max={100} value={topN} onChange={(e) => setTopN(Number(e.target.value))} />
        </label>
        <label className="f f-check">
          <input type="checkbox" checked={activeOnly} onChange={(e) => setActiveOnly(e.target.checked)} />
          <span>활성 결과만</span>
        </label>
        <span className="grow" />
        {loading && (
          <button type="button" onClick={cancel} className="f-submit">
            중단
          </button>
        )}
        <button type="submit" className="primary f-submit" disabled={loading}>
          {loading ? "계산 중…" : "계산"}
        </button>
      </form>

      {stats && (
        <>
          <div className="summary-line">
            전체 {formatCount(stats.total)}행 · 시간 미확정 {formatCount(stats.null_time_rows)}행 · 배치 {stats.max_batch_id}까지
            {stats.time_range && ` · ${formatTime(stats.time_range[0])} ~ ${formatTime(stats.time_range[1])}`}
          </div>
          <IpTable rows={stats.ip_rows} onExport={exportIps} />
          <div className="chart-grid">
            <Bars title="상태코드" rows={stats.status.map(([s, n]) => [s === null ? "(없음)" : String(s), n])} unit="건" />
            <Bars title="메서드" rows={stats.methods.map(([m, n]) => [m ?? "(없음)", n])} unit="건" />
            <Bars title={`상위 ${topN} 요청 대상`} rows={stats.top_targets} unit="건" />
          </div>
        </>
      )}
    </section>
  );
}
