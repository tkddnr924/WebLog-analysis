// 조회·에러 로그 목록이 함께 쓰는 페이징. 커서로 이어 받고, 오래된 응답은 버리며, 캐시 상한을 넘으면 앞쪽 행을 내린다.
import { useCallback, useEffect, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { appendPage, emptyCache, type PageCache } from "../lib/pages";
import type { LogFilter, LogRow, SortOrder } from "../types";

const PAGE_SIZE = 300;
/** UI 캐시 바이트 상한. 넘으면 앞쪽 행을 버린다. */
const CACHE_BYTES = 8 * 1024 * 1024;
export const ROW_HEIGHT = 28;

export interface AppliedQuery {
  filter: LogFilter;
  sort: SortOrder;
}

export function useLogRows() {
  const { setNotice } = useAppState();
  const [applied, setApplied] = useState<AppliedQuery | null>(null);
  const [cache, setCache] = useState<PageCache>(emptyCache(0));
  const [loading, setLoading] = useState(false);
  const [selected, setSelected] = useState<LogRow | null>(null);
  const requestId = useRef(0);
  const cacheRef = useRef(cache);
  cacheRef.current = cache;
  const loadingRef = useRef(false);
  const scrollRef = useRef<HTMLDivElement>(null);

  const fetchPage = useCallback(
    async (target: AppliedQuery, reset: boolean) => {
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
      setNotice(null);
      loadingRef.current = false;
      void fetchPage(target, true);
      scrollRef.current?.scrollTo({ top: 0 });
    },
    [fetchPage, setNotice],
  );

  /** 북마크 토글. 응답으로 캐시의 행을 제자리에서 갱신하고, 상세 창의 행도 맞춘다. */
  const toggleBookmark = async (r: LogRow) => {
    try {
      const on = await api.toggleBookmark(r.source_id, r.line_number);
      setCache((prev) => ({ ...prev, rows: prev.rows.map((x) => (x.source_id === r.source_id && x.line_number === r.line_number ? { ...x, bookmarked: on } : x)) }));
      setSelected((sel) => (sel && sel.source_id === r.source_id && sel.line_number === r.line_number ? { ...sel, bookmarked: on } : sel));
      // 북마크 뷰에서 해제하면 목록에서 바로 빠지도록 다시 조회한다.
      if (!on && applied?.filter.bookmarked_only) void fetchPage(applied, true);
    } catch (e) {
      setNotice(errorText(e));
    }
  };

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

  return { rows, cache, loading, applied, selected, setSelected, scrollRef, virtualizer, items, applyFilter, toggleBookmark };
}
