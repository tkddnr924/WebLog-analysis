// 에러 조회 화면의 조건 구성. 순수 함수, 테스트 대상.
import { withRange, type RangeMicros } from "../lib/timeRange";
import type { FilterExpr, LogFilter } from "../types";

export interface ErrorForm {
  q: string;
}

export const emptyErrorForm: ErrorForm = { q: "" };

/**
 * 룰 조건(base) 위에 사이드바의 기간과 화면 검색을 얹어 에러 로그 조건을 만든다.
 * 검색어는 메시지(대소문자 무시)와 클라이언트 IP 중 하나만 맞아도 걸리고, 룰 조건과는 AND로 묶인다.
 */
export function composeErrorFilter(base: LogFilter, f: ErrorForm, range: RangeMicros): LogFilter {
  const q = f.q.trim();
  const search: FilterExpr | null =
    q === ""
      ? null
      : {
          kind: "or",
          items: [
            { kind: "cond", field: "message", op: "icontains", value: q },
            { kind: "cond", field: "client_ip", op: "eq", value: q },
          ],
        };
  const expr: FilterExpr | null = search ? (base.expr ? { kind: "and", items: [base.expr, search] } : search) : base.expr;
  return withRange({ ...base, log_kind: "error", expr }, range);
}
