import { useEffect } from "react";
import { api, errorText } from "./api";
import { AppStateProvider, useAppState, type Stage } from "./state";
import { StartPanel } from "./panels/StartPanel";
import { ImportingPanel } from "./panels/ImportingPanel";
import { ResultsPanel } from "./panels/ResultsPanel";
import { ErrorBoundary } from "./panels/ErrorBoundary";
import { formatBytes, formatCount } from "./lib/format";

const STAGE_LABELS: Record<Stage, string> = { start: "시작 화면", importing: "파싱 화면", results: "결과 화면" };

export function App() {
  return (
    <AppStateProvider>
      <Shell />
    </AppStateProvider>
  );
}

function Shell() {
  const { stage, setStage, project, progress, exportProgress, notice, setNotice, refreshProject } = useAppState();

  useEffect(() => {
    void refreshProject();
  }, [refreshProject]);

  const cancelImport = () => api.cancelImport().catch((e) => setNotice(errorText(e)));
  const cancelExport = () => api.cancelExport().catch((e) => setNotice(errorText(e)));

  return (
    <div className="shell">
      <header className="topbar">
        <button className="brand" onClick={() => setStage("start")} title="시작 화면으로">
          WebLog
        </button>
        {project && (
          <span className="project-path" title={project.db_path}>
            {project.db_path} <span className="muted">({formatBytes(project.db_file_bytes)})</span>
          </span>
        )}
        <span className="grow" />
        {exportProgress && (
          <div className="import-strip" role="status">
            <span>
              내보내는 중 · {formatCount(exportProgress.rows)}행 · {formatBytes(exportProgress.bytes)} · {exportProgress.elapsed_secs.toFixed(0)}초
            </span>
            <button onClick={cancelExport} disabled={exportProgress.cancel_requested}>
              {exportProgress.cancel_requested ? "취소 중" : "취소"}
            </button>
          </div>
        )}
        {progress && stage !== "importing" && (
          <div className="import-strip" role="status">
            <button className="linklike" onClick={() => setStage("importing")}>
              파싱 중 · {formatCount(progress.committed_records)}건 확정
            </button>
            <button onClick={cancelImport} disabled={progress.cancel_requested}>
              {progress.cancel_requested ? "취소 중" : "취소"}
            </button>
          </div>
        )}
        {project && stage !== "results" && stage !== "importing" && <button onClick={() => setStage("results")}>결과 보기</button>}
        {stage === "results" && <button onClick={() => setStage("start")}>새 로그 가져오기</button>}
      </header>
      {notice && (
        <div className="notice" role="alert">
          <span>{notice}</span>
          <button className="linklike" onClick={() => setNotice(null)}>
            닫기
          </button>
        </div>
      )}
      <main className="panel">
        <ErrorBoundary label={STAGE_LABELS[stage]} resetKey={stage}>
          {stage === "start" && <StartPanel />}
          {stage === "importing" && <ImportingPanel />}
          {stage === "results" && <ResultsPanel />}
        </ErrorBoundary>
      </main>
    </div>
  );
}
