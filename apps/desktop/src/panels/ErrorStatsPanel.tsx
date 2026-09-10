// 에러 로그 통계. 접근 로그 통계와 같은 구조이고 항목만 레벨·메시지 중심으로 바꾼다.
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { save } from "@tauri-apps/plugin-dialog";
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { DISPLAY_OFFSET_SECONDS, formatCount, formatTime } from "../lib/format";
import { applyScope } from "../lib/scope";
import { emptyFilter, type StatsResult } from "../types";
import { Bars, IpTable } from "./StatsPanel";

/** 버킷 크기를 사람이 읽는 단위로. */
function bucketLabel(seconds: number): string {
  if (seconds >= 86_400) return `${Math.round(seconds / 86_400)}일`;
  if (seconds >= 3_600) return `${Math.round(seconds / 3_600)}시간`;
  if (seconds >= 60) return `${Math.round(seconds / 60)}분`;
  return `${seconds}초`;
}

export function ErrorStatsPanel() {
  const { project, setNotice, ruleRequest, scope } = useAppState();
  const [activeOnly, setActiveOnly] = useState(true);
  const [topN, setTopN] = useState(20);
  const [stats, setStats] = useState<StatsResult | null>(null);
  const [loading, setLoading] = useState(false);
  // Only the newest request may write results; late answers to older conditions are dropped.
  const requestRef = useRef(0);

  const statsFilter = useMemo(
    () => applyScope({ ...(ruleRequest?.filter.log_kind === "error" ? ruleRequest.filter : emptyFilter()), active_only: activeOnly, log_kind: "error" }, scope),
    [ruleRequest, activeOnly, scope],
  );

  const run = useCallback(async () => {
    requestRef.current += 1;
    const id = requestRef.current;
    setLoading(true);
    try {
      const result = await api.computeStats({
        // 에러 룰의 조건(레벨·메시지) 위에 이 화면의 시간·활성 조건을 얹는다. 다른 종류의 룰 조건은 쓰지 않는다.
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
        <h1>에러 통계</h1>
        <p className="muted">프로젝트를 열면 에러 로그 통계를 볼 수 있습니다.</p>
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
            <Bars title="레벨 분포" rows={stats.levels.map(([l, n]) => [l ?? "(없음)", n])} unit="건" />
            <Bars title={`상위 ${topN} 메시지`} rows={stats.top_messages} unit="건" />
            <Bars
              title={`시간축 · ${bucketLabel(stats.bucket_seconds)} 단위${stats.timeline_truncated ? " (앞쪽만)" : ""}`}
              rows={stats.timeline.map(([t, n]) => [formatTime(t), n])}
              unit="건"
            />
          </div>
        </>
      )}
    </section>
  );
}
