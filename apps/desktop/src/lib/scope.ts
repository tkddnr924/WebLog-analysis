// 조회 범위: 기간과 화이트리스트 IP. 둘 다 룰보다 위에 있어 어느 화면에서도 같게 걸린다.
// 화이트리스트는 분석가·모니터링 장비처럼 결과에서 빼고 봐야 하는 주소를 담는다.
import type { FilterExpr, LogFilter } from "../types";
import type { RangeMicros } from "./timeRange";

export interface Scope extends RangeMicros {
  /** 결과에서 제외할 IP. `10.0.0.1`(정확히) 또는 `1.1.*`(앞자리 일치). */
  ips: string[];
}

export const openScope: Scope = { from: null, to: null, ips: [] };

/** 입력을 저장할 형태로. 주소 문자와 끝의 `*` 하나만 허용하고, 그 밖에는 null. */
export function normalizeIpPattern(text: string): string | null {
  const v = text.trim();
  if (v === "" || v === "*") return null;
  return /^[0-9a-fA-F.:]+\*?$/.test(v) ? v : null;
}

/** 화이트리스트 한 항목의 조건. `*`로 끝나면 앞자리 일치. */
function ipCond(pattern: string): FilterExpr {
  return pattern.endsWith("*")
    ? { kind: "cond", field: "client_ip", op: "starts_with", value: pattern.slice(0, -1) }
    : { kind: "cond", field: "client_ip", op: "eq", value: pattern };
}

/** 룰 조건 위에 기간과 화이트리스트를 얹는다. 화이트리스트는 AND NOT으로 빼낸다. */
export function applyScope<T extends LogFilter>(filter: T, scope: Scope): T {
  const hits = scope.ips.map(ipCond);
  const excluded: FilterExpr | null = hits.length === 0 ? null : { kind: "not", item: hits.length === 1 ? hits[0] : { kind: "or", items: hits } };
  const expr = excluded === null ? filter.expr : filter.expr ? { kind: "and" as const, items: [filter.expr, excluded] } : excluded;
  return { ...filter, time_from_micros: scope.from, time_to_micros: scope.to, expr };
}
