// 페이지 캐시: 커서 순서대로 받은 페이지를 이어 붙이되 바이트 상한으로 앞쪽을 버린다.
// 응답 식별자로 오래된 응답을 폐기한다. 순수 함수라 테스트할 수 있다.
import type { LogPage, LogRow, PageCursor } from "../types";

export interface PageCache {
  /** 요청 식별자. 필터·정렬이 바뀔 때마다 증가한다. */
  requestId: number;
  rows: LogRow[];
  approxBytes: number;
  nextCursor: PageCursor | null;
  /** 바이트 상한 때문에 버린 앞쪽 행 수. */
  droppedRows: number;
  exhausted: boolean;
}

export const emptyCache = (requestId: number): PageCache => ({
  requestId,
  rows: [],
  approxBytes: 0,
  nextCursor: null,
  droppedRows: 0,
  exhausted: false,
});

/** 응답이 현재 요청에 속하면 캐시에 붙인다. 아니면 그대로 돌려준다. */
export function appendPage(cache: PageCache, responseRequestId: number, page: LogPage, maxBytes: number): PageCache {
  if (responseRequestId !== cache.requestId) return cache;
  let rows = cache.rows.concat(page.rows);
  let approxBytes = cache.approxBytes + page.approx_bytes;
  let dropped = cache.droppedRows;
  // 상한을 넘으면 앞쪽부터 버린다. 행당 바이트는 평균치로 근사한다.
  if (approxBytes > maxBytes && rows.length > page.rows.length) {
    const perRow = approxBytes / rows.length;
    const toDrop = Math.min(rows.length - page.rows.length, Math.ceil((approxBytes - maxBytes) / perRow));
    rows = rows.slice(toDrop);
    approxBytes = Math.round(perRow * rows.length);
    dropped += toDrop;
  }
  return {
    requestId: cache.requestId,
    rows,
    approxBytes,
    nextCursor: page.next_cursor,
    droppedRows: dropped,
    exhausted: page.next_cursor === null,
  };
}
