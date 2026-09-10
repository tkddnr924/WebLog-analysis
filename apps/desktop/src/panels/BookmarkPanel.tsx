// 북마크 목록. 접근 로그와 에러 로그가 별 하나를 공유하므로 종류를 가리지 않고 함께 보여준다.
// 두 종류의 컬럼이 다르므로 시간과 한 줄 요약만 쓴다.
import { useEffect, useState } from "react";
import { useAppState } from "../state";
import { formatCount, formatTime } from "../lib/format";
import { rowSummary } from "../lib/rowSummary";
import { withRange } from "../lib/timeRange";
import { emptyFilter, type LogFilter, type SortOrder } from "../types";
import { DetailPanel } from "./DetailPanel";
import { ROW_HEIGHT, useLogRows } from "./useLogRows";

export function BookmarkPanel() {
  const { project, ruleRequest, appliedRange } = useAppState();
  const [sort, setSort] = useState<SortOrder>("time_asc");
  const { rows, cache, loading, applied, selected, setSelected, scrollRef, virtualizer, items, applyFilter, toggleBookmark } = useLogRows();

  // 룰이 들고 있는 조건은 그대로 쓰되 로그 종류는 비운다(두 종류를 함께 본다).
  const base: LogFilter = { ...(ruleRequest?.filter ?? emptyFilter()), bookmarked_only: true, log_kind: null };

  useEffect(() => {
    if (project) applyFilter(withRange(base, appliedRange), sort);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ruleRequest?.nonce, appliedRange.nonce, project, sort]);

  if (!project) {
    return (
      <section>
        <h1>북마크</h1>
        <p className="muted">프로젝트를 열면 북마크한 줄을 볼 수 있습니다.</p>
      </section>
    );
  }

  return (
    <section className="query">
      <div className="filter-bar">
        <label className="f f-sort">
          <span>정렬</span>
          <select value={sort} onChange={(e) => setSort(e.target.value as SortOrder)}>
            <option value="time_asc">오래된순</option>
            <option value="time_desc">최신순</option>
          </select>
        </label>
        <span className="grow" />
      </div>

      <div className="summary-line">
        {applied ? (
          <>
            {formatCount(rows.length)}행 표시 · 접근 로그와 에러 로그의 북마크를 함께 봅니다
            {cache.exhausted ? " · 끝" : " · 스크롤하면 더 불러옴"}
          </>
        ) : (
          <span className="muted">행 앞의 별을 누르면 북마크됩니다.</span>
        )}
      </div>

      <div className="query-body">
        <div className="table-wrap virtual" ref={scrollRef}>
          <div className="vhead vbm">
            <span className="star-col" aria-label="북마크" />
            <span>시간(KST)</span>
            <span>내용</span>
          </div>
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {items.map((vi) => {
              const r = rows[vi.index];
              const isSel = selected !== null && selected.source_id === r.source_id && selected.line_number === r.line_number;
              const summary = rowSummary(r);
              return (
                <div
                  key={vi.key}
                  className={`vrow vbm ${isSel ? "sel" : ""} ${r.bookmarked ? "bm" : ""}`}
                  style={{ transform: `translateY(${vi.start}px)`, height: ROW_HEIGHT }}
                  onClick={() => setSelected(r)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === " ") {
                      e.preventDefault();
                      setSelected(r);
                    }
                  }}
                  tabIndex={0}
                  role="row"
                  aria-selected={isSel}
                >
                  <button
                    type="button"
                    className={`star ${r.bookmarked ? "on" : ""}`}
                    aria-label={r.bookmarked ? "북마크 해제" : "북마크"}
                    aria-pressed={r.bookmarked}
                    title={r.bookmarked ? "북마크 해제" : "북마크"}
                    onClick={(e) => {
                      e.stopPropagation();
                      void toggleBookmark(r);
                    }}
                  >
                    {r.bookmarked ? "★" : "☆"}
                  </button>
                  <span className="mono">{formatTime(r.timestamp_utc)}</span>
                  <span title={summary}>{summary}</span>
                </div>
              );
            })}
          </div>
          {rows.length === 0 && applied && !loading && <div className="empty">북마크한 줄이 없습니다. 조회·에러 목록에서 별을 눌러 표시하세요.</div>}
          {loading && <div className="loading">불러오는 중…</div>}
        </div>
      </div>
      {selected && (
        <DetailPanel
          key={`${selected.source_id}:${selected.line_number}`}
          row={selected}
          jobId={null}
          onClose={() => setSelected(null)}
          onToggleBookmark={() => void toggleBookmark(selected)}
        />
      )}
    </section>
  );
}
