// 에러 조회 화면의 조건 구성. 순수 함수, 테스트 대상.
import { parseTimeInput } from "../lib/format";
import type { FilterExpr, LogFilter } from "../types";

export interface ErrorForm {
  from: string;
  to: string;
  q: string;
}

export const emptyErrorForm: ErrorForm = { from: "", to: "", q: "" };

/**
 * 룰 조건(base) 위에 화면의 기간·검색을 얹어 에러 로그 조건을 만든다. 잘못된 시각이면 오류 문자열.
 * 검색어는 메시지(대소문자 무시)와 클라이언트 IP 중 하나만 맞아도 걸리고, 룰 조건과는 AND로 묶인다.
 */
export function composeErrorFilter(base: LogFilter, f: ErrorForm): LogFilter | string {
  const from = parseTimeInput(f.from);
  const to = parseTimeInput(f.to);
  if (from === undefined || to === undefined) return "시각은 YYYY-MM-DD 또는 YYYY-MM-DDTHH:MM 형식(한국 시간)으로 입력하세요.";
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
  return { ...base, log_kind: "error", time_from_micros: from, time_to_micros: to, expr };
}
