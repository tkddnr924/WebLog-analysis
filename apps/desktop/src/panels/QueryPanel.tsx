import { useCallback, useEffect, useState } from "react";
import { useAppState } from "../state";
import { formatCount, formatTime, methodClass, statusClass } from "../lib/format";
import { applyScope, type Scope } from "../lib/scope";
import { emptyFilter, type FilterExpr, type LogFilter, type SortOrder } from "../types";
import { DetailPanel } from "./DetailPanel";
import { ROW_HEIGHT, useLogRows } from "./useLogRows";

interface Form {
  q: string;
}

const emptyForm: Form = { q: "" };

/** 빠른 검색: 경로·IP·리퍼러·브라우저 중 하나라도 검색어를 포함(대소문자 무시). IP는 정확히 일치도 본다. */
export function quickSearchExpr(q: string): FilterExpr | null {
  const v = q.trim();
  if (v === "") return null;
  return {
    kind: "or",
    items: [
      { kind: "cond", field: "request_target", op: "icontains", value: v },
      { kind: "cond", field: "client_ip", op: "eq", value: v },
      { kind: "cond", field: "referrer", op: "icontains", value: v },
      { kind: "cond", field: "user_agent", op: "icontains", value: v },
    ],
  };
}

/** 룰 조건 위에 사이드바의 기간과 화면의 검색을 얹는다. */
export function composeFilter(base: LogFilter, f: Form, scope: Scope): LogFilter {
  const quick = quickSearchExpr(f.q);
  const expr: FilterExpr | null = quick ? (base.expr ? { kind: "and", items: [base.expr, quick] } : quick) : base.expr;
  return applyScope({ ...base, expr }, scope);
}

export function QueryPanel() {
  const { project, ruleRequest, setCurrentFilter, scope } = useAppState();
  const [form, setForm] = useState<Form>(emptyForm);
  const [sort, setSort] = useState<SortOrder>("time_asc");
  const { rows, cache, loading, applied, selected, setSelected, scrollRef, virtualizer, items, applyFilter, toggleBookmark } = useLogRows();

  // 사용자 룰로 저장할 수 있게 마지막 조건을 전역에 남긴다.
  const applyAndRemember = useCallback(
    (f: LogFilter, s: SortOrder) => {
      setCurrentFilter(f);
      applyFilter(f, s);
    },
    [applyFilter, setCurrentFilter],
  );

  // 접근 룰의 조건만 쓴다. 에러 룰이 선택된 상태의 조건(레벨·메시지)은 접근 로그와 상관이 없으므로 버린다.
  const accessRule = ruleRequest?.filter.log_kind === "access" ? ruleRequest.filter : null;
  const base: LogFilter = { ...(accessRule ?? { ...emptyFilter(), active_only: true }), log_kind: "access" };

  // 사이드바에서 룰이나 기간을 바꾸면 화면의 검색은 유지한 채 바로 조회한다.
  useEffect(() => {
    if (!project) return;
    applyAndRemember(composeFilter(base, form, scope), sort);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ruleRequest?.nonce, scope.nonce, project]);

  if (!project) {
    return (
      <section>
        <h1>조회</h1>
        <p className="muted">프로젝트를 열면 저장된 로그를 조회할 수 있습니다.</p>
      </section>
    );
  }

  return (
    <section className="query">
      <form
        className="filter-bar"
        onSubmit={(e) => {
          e.preventDefault();
          applyAndRemember(composeFilter(base, form, scope), sort);
        }}
      >
        <label className="f f-search">
          <span>검색 (경로 · IP · 리퍼러 · 브라우저)</span>
          <input value={form.q} onChange={(e) => setForm({ ...form, q: e.target.value })} placeholder="예: /admin, 10.0.0.1, python" spellCheck={false} />
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
            setForm(emptyForm);
            applyAndRemember(composeFilter(base, emptyForm, scope), sort);
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
          <span className="muted">조건을 정하고 조회를 누르세요. 시간 필터는 시간이 미확정인 행을 제외합니다.</span>
        )}
      </div>

      <div className="query-body">
        <div className="table-wrap virtual" ref={scrollRef}>
          <div className="vhead">
            <span className="star-col" aria-label="북마크" />
            <span>시간(KST)</span>
            <span>IP</span>
            <span>메서드</span>
            <span>대상</span>
            <span>상태</span>
            <span className="num">바이트</span>
            <span>출처</span>
          </div>
          <div style={{ height: virtualizer.getTotalSize(), position: "relative" }}>
            {items.map((vi) => {
              const r = rows[vi.index];
              const isSel = selected !== null && selected.source_id === r.source_id && selected.line_number === r.line_number;
              return (
                <div
                  key={vi.key}
                  className={`vrow ${isSel ? "sel" : ""} ${r.bookmarked ? "bm" : ""}`}
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
                  <span className="mono">{r.client_ip ?? "–"}</span>
                  <span>
                    <span className={`chip ${methodClass(r.method)}`} title={r.method ?? ""}>
                      {r.method ?? "–"}
                    </span>
                  </span>
                  <span className="mono" title={r.request_target ?? ""}>
                    {r.request_target ?? "–"}
                  </span>
                  <span>
                    <span className={`chip ${statusClass(r.status)}`}>{r.status ?? "–"}</span>
                  </span>
                  <span className="num">{r.bytes_sent ?? "–"}</span>
                  <span className="mono muted">
                    #{r.source_id}:{r.line_number}
                  </span>
                </div>
              );
            })}
          </div>
          {rows.length === 0 && applied && !loading && <div className="empty">조건에 맞는 로그가 없습니다.</div>}
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
