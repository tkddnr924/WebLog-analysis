// 바이트 값. 기본은 정확한 바이트 수이고, 옆의 단위 버튼을 누르면 B → KB → MB → GB → TB로 바꿔 본다(1024 기준).
import { useState } from "react";

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;
type Unit = (typeof UNITS)[number];
const KEY = "weblog.byteUnit";

function load(): Unit {
  try {
    const v = localStorage.getItem(KEY);
    return UNITS.includes(v as Unit) ? (v as Unit) : "B";
  } catch {
    return "B";
  }
}

export function formatIn(bytes: number, unit: Unit): string {
  const i = UNITS.indexOf(unit);
  if (i === 0) return `${bytes.toLocaleString("ko-KR")} B`;
  const v = bytes / 1024 ** i;
  const digits = v >= 100 ? 0 : v >= 10 ? 1 : 2;
  return `${v.toLocaleString("ko-KR", { minimumFractionDigits: digits, maximumFractionDigits: digits })} ${unit}`;
}

export function ByteValue({ bytes }: { bytes: number }) {
  const [unit, setUnit] = useState<Unit>(load);
  const next = () => {
    const u = UNITS[(UNITS.indexOf(unit) + 1) % UNITS.length];
    setUnit(u);
    try {
      localStorage.setItem(KEY, u);
    } catch {
      // 저장 실패는 무시한다. 표시는 그대로 바뀐다.
    }
  };
  return (
    <span className="byte-value">
      <span title={`${bytes.toLocaleString("ko-KR")} bytes`}>{formatIn(bytes, unit)}</span>
      <button type="button" className="unit-toggle" onClick={next} title="단위 바꾸기 (B → KB → MB → GB → TB)" aria-label="바이트 단위 바꾸기">
        {UNITS[(UNITS.indexOf(unit) + 1) % UNITS.length]}
      </button>
    </span>
  );
}
