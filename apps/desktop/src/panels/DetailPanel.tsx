// 행 상세. 재구성한 한 줄을 맨 위에 두고, 저장된 필드를 사람이 읽는 라벨로 보여준다.
import { useEffect, useState } from "react";
import { api, errorText } from "../api";
import { formatOffset, formatTime, statusClass } from "../lib/format";
import { ByteValue } from "./ByteValue";
import { fieldLabel } from "../lib/fieldLabels";
import { baseName } from "../lib/paths";
import { isEncoded, safeDecode } from "../lib/urldecode";
import { decodeHexEscapes, describeBinary, hasHexEscapes } from "../lib/escapes";
import type { DetailView, LogRow } from "../types";

/** 오른쪽에서 나오는 드로어. 바깥을 누르거나 Esc로 닫는다. */
export function DetailPanel({ row, jobId, onClose, onToggleBookmark }: { row: LogRow; jobId: number | null; onClose: () => void; onToggleBookmark: () => void }) {
  const [view, setView] = useState<DetailView | null>(null);
  const [error, setError] = useState<string | null>(null);
  // 경로는 퍼센트 인코딩을 풀어 보여준다. 원문이 필요하면 토글로 되돌린다.
  const [decoded, setDecoded] = useState(true);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  // 부모가 행마다 key를 바꾸므로 상태는 행 단위로 초기화된다.
  useEffect(() => {
    let alive = true;
    api
      .logDetail(jobId, row.source_id, row.line_number)
      .then((v) => {
        if (alive) {
          setView(v);
          setError(v ? null : "레코드를 찾지 못했습니다.");
        }
      })
      .catch((e) => {
        if (alive) setError(errorText(e));
      });
    return () => {
      alive = false;
    };
  }, [row, jobId]);

  const d = view?.detail;
  const rawTarget = d?.request_target ?? null;
  const hex = rawTarget ? hasHexEscapes(rawTarget) : false;
  const encoded = rawTarget ? isEncoded(rawTarget) || hex : false;
  // 디코드: 퍼센트 인코딩을 풀고, \xHH 이스케이프는 바이트로 풀어 평문(출력 불가 문자는 ·)으로 만든다.
  const hexDecoded = rawTarget && hex ? decodeHexEscapes(rawTarget) : null;
  const target = rawTarget ? (decoded ? (hexDecoded ? safeDecode(hexDecoded.text) : safeDecode(rawTarget)) : rawTarget) : null;
  const binary = hexDecoded ? describeBinary(hexDecoded.bytes) : null;
  const request = d ? [d.method, target, d.protocol].filter((x): x is string => Boolean(x)).join(" ") : "";
  const referrer = d?.referrer ? (decoded ? safeDecode(d.referrer) : d.referrer) : null;

  return (
    <>
      <div className="drawer-backdrop" onClick={onClose} aria-hidden="true" />
      <aside
        className="detail drawer"
        role="dialog"
        aria-modal="true"
        aria-label="로그 상세"
      >
        <div className="detail-head">
          <div className="detail-title">
            <span className="mono" title={d?.source_path ?? ""}>
              {d ? baseName(d.source_path) : `파일 #${row.source_id}`}
            </span>
            <span className="muted">
              {" "}
              · {row.line_number.toLocaleString("ko-KR")}번째 줄
            </span>
          </div>
          <div className="detail-actions">
            <button type="button" className={`star star-lg boxed ${row.bookmarked ? "on" : ""}`} onClick={onToggleBookmark} aria-pressed={row.bookmarked} title={row.bookmarked ? "북마크 해제" : "북마크"}>
              {row.bookmarked ? "★" : "☆"}
            </button>
            <button type="button" className="icon close" onClick={onClose} title="닫기" aria-label="닫기">
              ✕
            </button>
          </div>
        </div>
        {error && <div className="notice inline">{error}</div>}
        {view && d && (
          <div className="detail-body">
            <pre
              className="recon-line"
              title="저장된 필드로 다시 조립한 텍스트입니다. 원문은 저장하지 않으며 바이트 단위로 같지 않습니다."
            >
              {view.reconstructed}
            </pre>
            <dl className="fields">
              <Field label="시간">
                {formatTime(d.timestamp_utc)}
                <span className="muted"> KST</span>
                {d.tz_offset_seconds !== null && <span className="muted"> · 원문 {formatOffset(d.tz_offset_seconds)}</span>}
              </Field>
              {d.client_ip && <Field label="IP">{d.client_ip}</Field>}
              {request && (
                <Field label="요청">
                  {request}
                  {encoded && (
                    <button type="button" className="linklike small decode-toggle" onClick={() => setDecoded(!decoded)}>
                      {decoded ? "원문 보기" : "평문으로"}
                    </button>
                  )}
                  {binary && (
                    <div className="binary-note">
                      {binary.label}
                      {binary.sni && (
                        <>
                          {" · "}SNI <span className="mono">{binary.sni}</span>
                        </>
                      )}
                    </div>
                  )}
                </Field>
              )}
              {(d.status !== null || d.bytes_sent !== null) && (
                <Field label="응답">
                  {d.status !== null && (
                    <span className={`chip ${statusClass(d.status)}`}>
                      {d.status}
                    </span>
                  )}
                  {d.bytes_sent !== null && (
                  <span>
                    {" "}
                    <ByteValue bytes={d.bytes_sent} />
                  </span>
                )}
                  {d.status !== null && d.bytes_sent === null && (
                    <span className="muted"> 크기 없음</span>
                  )}
                </Field>
              )}
              {referrer && <Field label="리퍼러">{referrer}</Field>}
              {d.user_agent && <Field label="브라우저">{d.user_agent}</Field>}
              {view.extra.map(([k, v]) => (
                <Field key={k} label={fieldLabel(k)}>
                  {v}
                </Field>
              ))}
            </dl>
          </div>
        )}
      </aside>
    </>
  );
}

function Field({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="field-row">
      <dt>{label}</dt>
      <dd className="mono">{children}</dd>
    </div>
  );
}
