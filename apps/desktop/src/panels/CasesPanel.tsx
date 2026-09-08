// 시작 화면 아래의 기존 케이스 목록. 열어서 결과 화면으로 가거나 파일을 지운다.
import { useCallback, useEffect, useState } from "react";
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { formatBytes } from "../lib/format";
import type { CaseInfo } from "../types";

export function CasesPanel() {
  const { project, openProject, refreshProject, setStage, setNotice, progress } = useAppState();
  const [cases, setCases] = useState<CaseInfo[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      setCases(await api.listCases());
    } catch (e) {
      setNotice(errorText(e));
    }
  }, [setNotice]);

  useEffect(() => {
    void load();
  }, [load, project?.db_path]);

  const open = async (c: CaseInfo) => {
    setBusy(c.path);
    try {
      await openProject(c.path);
      setNotice(null);
      setStage("results");
    } catch (e) {
      const msg = errorText(e);
      setNotice(msg.includes("replaying WAL") ? `이 케이스는 비정상 종료 뒤 복구할 수 없습니다(WAL 재생 실패). 삭제하고 다시 파싱하세요. (${msg})` : msg);
    } finally {
      setBusy(null);
    }
  };

  const remove = async (c: CaseInfo) => {
    if (!window.confirm(`케이스 '${c.name}'을(를) 삭제합니다. 파싱 결과가 사라지며 되돌릴 수 없습니다.`)) return;
    setBusy(c.path);
    try {
      await api.deleteCase(c.path);
      await refreshProject();
      await load();
      setNotice(null);
    } catch (e) {
      setNotice(errorText(e));
    } finally {
      setBusy(null);
    }
  };

  if (!cases || cases.length === 0) return null;
  const running = progress !== null;
  return (
    <section className="cases" aria-label="기존 케이스">
      <div className="files-head">
        <span>이전 케이스 {cases.length}개</span>
      </div>
      <div className="cases-list" role="list">
        {cases.map((c) => (
          <div key={c.path} className={`case-row ${c.open ? "open" : ""}`} role="listitem" title={c.path}>
            <span className="case-name">
              {c.name}
              {c.open && <span className="badge">열림</span>}
            </span>
            <span className="muted small">{formatWhen(c.modified_unix)}</span>
            <span className="num muted small">{formatBytes(c.bytes)}</span>
            <button onClick={() => open(c)} disabled={busy !== null || running}>
              {c.open ? "결과 보기" : "열기"}
            </button>
            <button onClick={() => remove(c)} disabled={busy !== null || running} title="파일 삭제">
              삭제
            </button>
          </div>
        ))}
      </div>
    </section>
  );
}

function formatWhen(unix: number | null): string {
  if (unix === null) return "";
  const d = new Date(unix * 1000);
  const p = (v: number) => String(v).padStart(2, "0");
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())} ${p(d.getHours())}:${p(d.getMinutes())}`;
}
