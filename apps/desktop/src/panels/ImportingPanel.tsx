// 파싱 진행 화면. 접근 로그 → 에러 로그 순서로 큐를 돌고, 끝나면 "결과 보기"로 넘어간다.
import { api, errorText } from "../api";
import { useAppState } from "../state";
import { formatCount, jobStatusLabel } from "../lib/format";

export function ImportingPanel() {
  const { progress, finished, importRun, project, setStage, clearFinished, setNotice } = useAppState();

  const cancel = async () => {
    try {
      await api.cancelImport();
    } catch (e) {
      setNotice(errorText(e));
    }
  };

  const done = importRun ? importRun.done : finished !== null;
  const results = importRun?.results ?? (finished ? [{ label: "", finished }] : []);
  const allOk = results.length > 0 && results.every((r) => r.finished.error === null);
  const current = importRun?.items[importRun.index]?.label ?? "";
  const total = importRun?.items.length ?? 1;

  return (
    <section className="center">
      <h1>{done ? (allOk ? "파싱 완료" : "파싱이 끝나지 않았습니다") : `${current || "로그"} 파싱 중${total > 1 ? ` (${(importRun?.index ?? 0) + 1}/${total})` : ""}`}</h1>
      {!done && (
        <div className="progress-card" role="status">
          {progress ? (
            <div>
              파일 #{progress.source_id} · 읽은 줄 {formatCount(progress.lines_read)} · 확정 레코드 {formatCount(progress.committed_records)} · 배치 {progress.committed_batches} ·{" "}
              {progress.elapsed_secs.toFixed(0)}초
            </div>
          ) : (
            <div>시작하는 중…</div>
          )}
          <div className="bar">
            <div className="bar-fill indeterminate" />
          </div>
          <div className="muted small">전체 줄 수를 미리 세지 않으므로 비율 대신 확정 건수를 표시합니다. 취소해도 확정된 배치까지는 남습니다.</div>
        </div>
      )}
      {results.map((r) => {
        const job = project?.jobs.find((j) => j.job_id === r.finished.job_id) ?? null;
        const ok = r.finished.error === null;
        return (
          <div key={`${r.label}-${r.finished.job_id}`} className={`finish-card ${ok ? "" : "bad"}`} role="status">
            <b>
              {r.label ? `${r.label} · ` : ""}작업 {r.finished.job_id}: {jobStatusLabel(r.finished.status)}
            </b>
            {job && (
              <div>
                레코드 {formatCount(job.committed_records)} · 오류 {formatCount(job.committed_errors)} · 제외 {formatCount(job.committed_skipped)}
                {job.committed_errors > 0 && (
                  <div className="muted small">오류는 포맷에 맞지 않아 저장하지 못한 줄입니다(위치만 기록). 시작 화면에서 라벨을 다시 확인하고 파싱하면 줄어듭니다.</div>
                )}
              </div>
            )}
            {r.finished.error && <div>{r.finished.error}</div>}
          </div>
        );
      })}
      <div className="actions">
        {!done && (
          <button onClick={cancel} disabled={progress?.cancel_requested ?? false}>
            {progress?.cancel_requested ? "취소 중…" : "취소"}
          </button>
        )}
        {done && (
          <>
            <button
              className="primary"
              onClick={() => {
                clearFinished();
                setStage("results");
              }}
            >
              결과 보기
            </button>
            <button
              onClick={() => {
                clearFinished();
                setStage("start");
              }}
            >
              다른 로그 가져오기
            </button>
          </>
        )}
      </div>
    </section>
  );
}
