// 북마크 목록처럼 두 로그 종류가 섞이는 곳에서 쓰는 한 줄 요약. 순수 함수, 테스트 대상.
import { errorFields } from "./errorRows";
import type { LogRow } from "../types";

/** 접근 로그면 `메서드 대상 · 상태 · IP`, 에러 로그면 `레벨 · 메시지 · IP`. 없는 값은 뺀다. */
export function rowSummary(row: LogRow): string {
  const f = errorFields(row);
  const parts: string[] = [];
  if (f.message === "") {
    const request = [row.method, row.request_target].filter((v) => v !== null && v !== "").join(" ");
    if (request !== "") parts.push(request);
    if (row.status !== null) parts.push(String(row.status));
  } else {
    if (f.level !== "") parts.push(f.level);
    parts.push(f.message);
  }
  if (f.client !== "") parts.push(f.client);
  return parts.length === 0 ? "(내용 없음)" : parts.join(" · ");
}
