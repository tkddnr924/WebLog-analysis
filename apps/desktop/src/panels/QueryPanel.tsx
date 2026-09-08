import { useCallback, useEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { formatCount, formatTime, parseTimeInput, statusClass } from "../lib/format";
import { appendPage, emptyCache, type PageCache } from "../lib/pages";
import { emptyFilter, type FilterExpr, type LogFilter, type LogRow, type SortOrder } from "../types";
import { DetailPanel } from "./DetailPanel";

const PAGE_SIZE = 300;
/** UI 캐시 바이트 상한. 넘으면 앞쪽 행을 버린다. */
const CACHE_BYTES = 8 * 1024 * 1024;
const ROW_HEIGHT = 28;

interface Form {
  from: string;
  to: string;
  q: string;
}

const emptyForm: Form = { from: "", to: "", q: "" };

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

/** 룰 조건 위에 화면의 기간·검색을 얹는다. 잘못된 시각이면 오류 문자열. */
export function composeFilter(base: LogFilter, f: Form): LogFilter | string {
  const from = parseTimeInput(f.from);
  const to = parseTimeInput(f.to);
  if (from === undefined || to === undefined) return "시각은 YYYY-MM-DD 또는 YYYY-MM-DDTHH:MM 형식(한국 시간)으로 입력하세요.";
  const quick = quickSearchExpr(f.q);
  const expr: FilterExpr | null = quick ? (base.expr ? { kind: "and", items: [base.expr, quick] } : quick) : base.expr;
  return { ...base, time_from_micros: from, time_to_micros: to, expr };
}

export function QueryPanel() {
  const { project, setNotice, ruleRequest, setCurrentFilter } = useAppState();
  const [form, setForm] = useState<Form>(emptyForm);
  const [sort, setSort] = useState<SortOrder>("time_asc");
  const [applied, setApplied] = useState<{ filter: LogFilter; sort: SortOrder } | null>(null);
  const [cache, setCache] = useState<PageCache>(emptyCache(0));
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<LogRow | null>(null);
  const requestId = useRef(0);
  const cacheRef = useRef(cache);
  cacheRef.current = cache;
  const loadingRef = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  const fetchPage = useCallback(
    async (target: { filter: LogFilter; sort: SortOrder }, reset: boolean) => {
      if (!reset && (cacheRef.current.exhausted || loadingRef.current)) return;
      const id = reset ? ++requestId.current : requestId.current;
      const base = reset ? emptyCache(id) : cacheRef.current;
      if (reset) {
        setCache(base);
        setSelected(null);
      }
      loadingRef.current = true;
      setLoading(true);
      try {
        const page = await api.queryPage({ filter: target.filter, sort: target.sort, page_size: PAGE_SIZE, cursor: reset ? null : base.nextCursor });
        // appendPage가 requestId로 오래된 응답을 폐기한다.
        setCache((prev) => appendPage(prev.requestId === id ? prev : base, id, page, CACHE_BYTES));
      } catch (e) {
        if (id === requestId.current) setNotice(errorText(e));
      } finally {
        if (id === requestId.current) {
          loadingRef.current = false;
          setLoading(false);
        }
      }
    },
    [setNotice],
  );

  const applyFilter = useCallback(
    (f: LogFilter, s: SortOrder) => {
      const target = { filter: f, sort: s };
      setApplied(target);
      setCurrentFilter(f);
      setNotice(null);
      loadingRef.current = false;
      void fetchPage(target, true);
      scrollRef.current?.scrollTo({ top: 0 });
    },
    [fetchPage, setCurrentFilter, setNotice],
  );

  const base: LogFilter = ruleRequest?.filter ?? { ...emptyFilter(), active_only: true };

  const apply = () => {
    const f = composeFilter(base, form);
    if (typeof f === "string") {
      setNotice(f);
      return;
    }
    applyFilter(f, sort);
  };

  // 사이드바에서 룰을 고르면 화면의 기간·검색은 유지한 채 바로 조회한다.
  useEffect(() => {
    if (!ruleRequest || !project) return;
    const f = composeFilter(ruleRequest.filter, form);
    if (typeof f !== "string") applyFilter(f, sort);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ruleRequest?.nonce, project]);

  const rows = cache.rows;
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_HEIGHT,
    overscan: 20,
  });
  const items = virtualizer.getVirtualItems();
  const lastIndex = items.length > 0 ? items[items.length - 1].index : -1;

  // 끝 근처에 도달하면 다음 페이지를 요청한다.
  useEffect(() => {
    if (applied && rows.length > 0 && lastIndex >= rows.length - 40 && !cache.exhausted && !loading) {
      void fetchPage(applied, false);
    }
  }, [lastIndex, rows.length, cache.exhausted, loading, applied, fetchPage]);

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
          apply();
        }}
      >
        <label className="f f-time">
          <span>시작</span>
          <input value={form.from} onChange={(e) => setForm({ ...form, from: e.target.value })} placeholder="2026-09-01T00:00" spellCheck={false} />
        </label>
        <label className="f f-time">
          <span>끝(제외)</span>
          <input value={form.to} onChange={(e) => setForm({ ...form, to: e.target.value })} placeholder="2026-09-02T00:00" spellCheck={false} />
        </label>
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
                  className={`vrow ${isSel ? "sel" : ""}`}
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
                  <span className="mono">{formatTime(r.timestamp_utc)}</span>
                  <span className="mono">{r.client_ip ?? "–"}</span>
                  <span>{r.method ?? "–"}</span>
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
        <DetailPanel key={`${selected.source_id}:${selected.line_number}`} row={selected} jobId={applied?.filter.job_id ?? null} onClose={() => setSelected(null)} />
      )}
    </section>
  );
}
