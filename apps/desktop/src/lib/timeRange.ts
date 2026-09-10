// 기간 조건. 룰보다 상위에 있어 조회·통계 어느 탭에서도 같은 구간만 본다.
import { parseTimeInput } from "./format";
import type { LogFilter } from "../types";

/** 화면 입력 그대로. 비어 있으면 그쪽 끝은 열려 있다. */
export interface TimeRange {
  from: string;
  to: string;
}

/** 적용된 구간. UTC 마이크로초, null이면 열린 끝. */
export interface RangeMicros {
  from: number | null;
  to: number | null;
}

export const emptyRange: TimeRange = { from: "", to: "" };
export const openRange: RangeMicros = { from: null, to: null };

/** 입력을 구간으로. 형식이 틀리거나 끝이 시작보다 앞이면 사람이 읽는 오류 문자열. */
export function parseRange(r: TimeRange): RangeMicros | string {
  const from = parseTimeInput(r.from);
  const to = parseTimeInput(r.to);
  if (from === undefined || to === undefined) return "시각은 YYYY-MM-DD 또는 YYYY-MM-DDTHH:MM 형식(한국 시간)으로 입력하세요.";
  if (from !== null && to !== null && to <= from) return "끝 시각은 시작 시각보다 뒤여야 합니다.";
  return { from, to };
}

/** 룰 조건 위에 구간을 얹는다. 룰이 들고 있던 구간은 덮어쓴다. */
export function withRange<T extends LogFilter>(filter: T, range: RangeMicros): T {
  return { ...filter, time_from_micros: range.from, time_to_micros: range.to };
}

/** 요약 표시용. 열린 끝은 물결로 둔다. */
export function rangeLabel(r: TimeRange): string {
  if (r.from.trim() === "" && r.to.trim() === "") return "전체 기간";
  return `${r.from.trim() || "처음"} ~ ${r.to.trim() || "끝"}`;
}
