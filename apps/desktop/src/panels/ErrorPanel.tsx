// 에러 로그 조회. 접근 로그와 컬럼이 다르고, 룰 사이드바의 에러 룰 조건을 그대로 받는다.
import { useEffect, useState } from "react";
import { useAppState } from "../state";
import { formatCount, formatTime } from "../lib/format";
import { errorFields, levelClass } from "../lib/errorRows";
import { emptyFilter, type LogFilter, type SortOrder } from "../types";
import { composeErrorFilter, emptyErrorForm, type ErrorForm } from "./errorFilter";
import { DetailPanel } from "./DetailPanel";
import { ROW_HEIGHT, useLogRows } from "./useLogRows";

export function ErrorPanel() {
  const { project, ruleRequest, scope } = useAppState();
  const [form, setForm] = useState<ErrorForm>(emptyErrorForm);
  const [sort, setSort] = useState<SortOrder>("time_asc");
  const { rows, cache, loading, applied, selected, setSelected, scrollRef, virtualizer, items, applyFilter, toggleBookmark } = useLogRows();

  // 다른 종류(접근 로그)의 룰 조건이 남아 있으면 쓰지 않는다.
  const base: LogFilter = { ...(ruleRequest?.filter.log_kind === "error" ? ruleRequest.filter : { ...emptyFilter(), active_only: true }), log_kind: "error" };

  // 사이드바에서 룰이나 기간을 바꾸면 화면의 검색은 유지한 채 바로 조회한다.
  useEffect(() => {
    if (project) applyFilter(composeErrorFilter(base, form, scope), sort);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ruleRequest?.nonce, scope.nonce, project]);

  if (!project) {
    return (
      <section>
        <h1>에러 로그</h1>
        <p className="muted">프로젝트를 열면 저장된 에러 로그를 조회할 수 있습니다.</p>
      </section>
    );
  }

  return (
    <section className="query">
      <form
        className="filter-bar"
        onSubmit={(e) => {
          e.preventDefault();
          applyFilter(composeErrorFilter(base, form, scope), sort);
        }}
      >
        <label className="f f-search">
          <span>검색 (메시지 · IP)</span>
          <input value={form.q} onChange={(e) => setForm({ ...form, q: e.target.value })} placeholder="예: No such file, 10.0.0.1" spellCheck={false} />
        </label>
        <label className="f f-sort">
          <span>정렬</span>
          <select value={sort} onChange={(e) => setSort(e.target.value as SortOrder)}>
            <option value="time_asc">오래된순</option>
            <option value="time_desc">최신순</option>
          </select>
        </label>
        <button type="submit" className="primary f-submit" disabled={loading}>
          조회
        </button>
        <button
          type="button"
          className="f-submit"
          disabled={loading || form.q === ""}
          title="검색어를 지우고 다시 조회합니다(룰·기간은 그대로)"
          onClick={() => {
            setForm(emptyErrorForm);
            applyFilter(composeErrorFilter(base, emptyErrorForm, scope), sort);
          }}
        >
          초기화
        </button>
      </form>

      <div className="summary-line">
        {applied ? (
          <>
            {formatCount(rows.length)}행 표시
            {cache.droppedRows > 0 && ` (앞쪽 ${formatCount(cache.droppedRows)}행은 메모리 상한으로 내림)`}
            {cache.exhausted ? " · 끝" : " · 스크롤하면 더 불러옴"}
          </>
        ) : (
          <span className="muted">왼쪽에서 룰을 고르거나 조건을 정하고 조회를 누르세요. 검색어는 메시지·클라이언트 IP를 함께 봅니다.</span>
        )}
      </div>

      <div className="query-body">
        <div className="table-wrap virtual" ref={scrollRef}>
          <div className="vhead verr">
            <span className="star-col" aria-label="북마크" />
            <span>시간(KST)</span>
            <span>레벨</span>
            <span>클라이언트</span>
            <span>메시지</span>
            <span>출처</span>
          </div>
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {items.map((vi) => {
              const r = rows[vi.index];
              const f = errorFields(r);
              const isSel = selected !== null && selected.source_id === r.source_id && selected.line_number === r.line_number;
              return (
                <div
                  key={vi.key}
                  className={`vrow verr ${isSel ? "sel" : ""} ${r.bookmarked ? "bm" : ""}`}
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
                  <span>{f.level === "" ? "–" : <span className={`chip ${levelClass(f.level)}`}>{f.level}</span>}</span>
                  <span className="mono">{f.client === "" ? "–" : f.client}</span>
                  <span title={f.message}>{f.message === "" ? "–" : f.message}</span>
                  <span className="mono muted">
                    #{r.source_id}:{r.line_number}
                  </span>
                </div>
              );
            })}
          </div>
          {rows.length === 0 && applied && !loading && <div className="empty">조건에 맞는 에러 로그가 없습니다.</div>}
          {loading && <div className="loading">불러오는 중…</div>}
        </div>
      </div>
      {selected && (
        <DetailPanel
          key={`${selected.source_id}:${selected.line_number}`}
          row={selected}
          jobId={applied?.filter.job_id ?? null}
          onClose={() => setSelected(null)}
          onToggleBookmark={() => void toggleBookmark(selected)}
        />
      )}
    </section>
  );
}
