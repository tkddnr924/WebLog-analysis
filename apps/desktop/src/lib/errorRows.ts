// 에러 로그 행의 표시값. extra_json은 파싱 정의가 만든 키를 담고 있어 서버마다 다르므로 여기서 흡수한다.
// 순수 함수, 테스트 대상.
import type { LogRow } from "../types";

export interface ErrorFields {
  level: string;
  client: string;
  message: string;
}

const EMPTY: ErrorFields = { level: "", client: "", message: "" };

/** Columns of their own; skipped in the fallback summary. */
const OWN_COLUMN = ["level", "client"];

function text(v: unknown): string {
  if (v === null || v === undefined) return "";
  return typeof v === "object" ? JSON.stringify(v) : String(v);
}

/** 깨진 JSON·객체가 아닌 값은 빈 객체로 본다. 조회 화면이 예외로 멈추지 않게 한다. */
function parseExtra(json: string | null): Record<string, unknown> {
  if (json === null) return {};
  try {
    const v: unknown = JSON.parse(json);
    return typeof v === "object" && v !== null && !Array.isArray(v) ? (v as Record<string, unknown>) : {};
  } catch {
    return {};
  }
}

/** 목록 한 줄에 보여줄 값. 없는 값은 빈 문자열이며 렌더에서 "–"로 바꾼다. */
export function errorFields(row: LogRow): ErrorFields {
  const o = parseExtra(row.extra_json);
  const level = text(o.level);
  // 표준 컬럼이 있으면 우선한다. apache는 포트가 붙은 client 키만 남긴다.
  const client = row.client_ip ?? text(o.client);
  let message = text(o.message);
  if (message !== "") {
    const code = text(o.error_code);
    if (code !== "") message = `${code} ${message}`;
  } else {
    // 메시지 키가 없는 정의도 있다. 남은 키를 그대로 이어 붙여 보여준다.
    message = Object.entries(o)
      .filter(([k, v]) => !OWN_COLUMN.includes(k) && text(v) !== "")
      .map(([k, v]) => `${k}=${text(v)}`)
      .join(", ");
  }
  if (level === "" && client === "" && message === "") return EMPTY;
  return { level, client, message };
}

/** 레벨 chip 색. apache는 모듈 이름이 앞에 붙으므로(core:error) 포함 여부로 본다. */
export function levelClass(level: string): "s5" | "s4" | "s0" {
  const v = level.toLowerCase();
  if (v.includes("emerg") || v.includes("alert") || v.includes("crit") || v.includes("error")) return "s5";
  if (v.includes("warn")) return "s4";
  return "s0";
}
