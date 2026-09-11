// 표시용 순수 함수. 테스트 대상.
import type { JobStatus } from "../types";

export function formatBytes(n: number | null | undefined): string {
  if (n === null || n === undefined) return "–";
  if (n < 1024) return `${n} B`;
  const units = ["KiB", "MiB", "GiB", "TiB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i += 1;
  }
  return `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

export function formatCount(n: number | null | undefined): string {
  if (n === null || n === undefined) return "–";
  return n.toLocaleString("ko-KR");
}

/** 화면 표시·입력 시간대. 한국 시간(UTC+9)으로 고정한다. 저장은 UTC이며 여기서만 변환한다. */
export const DISPLAY_OFFSET_SECONDS = 9 * 3600;

/** UTC 마이크로초를 표시 시간대의 `YYYY-MM-DD HH:MM:SS`로. 시간이 없으면 미확정 표시. */
export function formatTime(us: number | null | undefined): string {
  if (us === null || us === undefined) return "(시간 미확정)";
  const d = new Date(Math.floor(us / 1000) + DISPLAY_OFFSET_SECONDS * 1000);
  const p = (v: number) => String(v).padStart(2, "0");
  return `${d.getUTCFullYear()}-${p(d.getUTCMonth() + 1)}-${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}`;
}

/** `YYYY-MM-DDTHH:MM` 입력(표시 시간대 기준)을 UTC 마이크로초로. 비어 있으면 null. 잘못된 값은 undefined. */
export function parseTimeInput(text: string): number | null | undefined {
  const t = text.trim();
  if (t === "") return null;
  const m = /^(\d{4})-(\d{2})-(\d{2})(?:[T ](\d{2}):(\d{2})(?::(\d{2}))?)?$/.exec(t);
  if (!m) return undefined;
  const ms = Date.UTC(Number(m[1]), Number(m[2]) - 1, Number(m[3]), Number(m[4] ?? 0), Number(m[5] ?? 0), Number(m[6] ?? 0));
  if (Number.isNaN(ms)) return undefined;
  return (ms - DISPLAY_OFFSET_SECONDS * 1000) * 1000;
}

export function formatOffset(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined) return "미확정";
  const sign = seconds < 0 ? "-" : "+";
  const a = Math.abs(seconds);
  const p = (v: number) => String(v).padStart(2, "0");
  return `UTC${sign}${p(Math.floor(a / 3600))}:${p(Math.floor((a % 3600) / 60))}`;
}

export function statusClass(status: number | null): "s2" | "s3" | "s4" | "s5" | "s0" {
  if (status === null) return "s0";
  if (status >= 500) return "s5";
  if (status >= 400) return "s4";
  if (status >= 300) return "s3";
  if (status >= 200) return "s2";
  return "s0";
}

/** 메서드 라벨 색. 읽기·쓰기·삭제로 나누고, 표준이 아닌 값은 따로 표시한다. */
export function methodClass(method: string | null): "m-read" | "m-write" | "m-del" | "m-odd" | "m-none" {
  const m = (method ?? "").toUpperCase();
  if (m === "") return "m-none";
  if (m === "GET" || m === "HEAD" || m === "OPTIONS" || m === "TRACE") return "m-read";
  if (m === "POST" || m === "PUT" || m === "PATCH" || m === "CONNECT") return "m-write";
  if (m === "DELETE") return "m-del";
  return "m-odd";
}

const JOB_STATUS_LABELS: Record<JobStatus, string> = {
  queued: "대기",
  running: "진행 중",
  completed: "완료",
  completed_with_errors: "완료(오류 있음)",
  cancelling: "취소 중",
  cancelled: "취소됨",
  failed: "실패",
  interrupted: "비정상 종료(복구 가능)",
};

/** Accepts the plain status strings that finish notices carry. */
export function jobStatusLabel(status: JobStatus | string): string {
  return JOB_STATUS_LABELS[status as JobStatus] ?? status;
}

