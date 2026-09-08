// 앱 전역 상태: 열린 프로젝트, 진행 중 가져오기, 단계 선택. 단순한 컨텍스트로 둔다.
import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, errorText } from "./api";
import type { ExportFinishedView, ExportProgressView, FormatProfile, ImportFinishedView, ImportProgressView, LogFilter, ProjectInfo, ScanResponse, ScannedFile, ServerHint, StartImportRequest } from "./types";

/** 화면 흐름: 시작(폴더·포맷 확인) → 파싱 중 → 결과. */
export type Stage = "start" | "importing" | "results";

/** 시작 화면의 탭. 접근 로그와 에러 로그는 폴더·패턴·라벨 어휘가 다르다. */
export type LogKind = "access" | "error";

/** 한 번의 "파싱 시작"으로 차례로 실행할 가져오기. 엔진은 한 번에 하나만 돌리므로 큐로 잇는다. */
export interface ImportItem {
  label: string;
  request: StartImportRequest;
}

export interface ImportRun {
  items: ImportItem[];
  /** 지금 실행 중(또는 방금 끝난) 항목 번호. */
  index: number;
  results: { label: string; finished: ImportFinishedView }[];
  /** 큐가 모두 끝났거나 실패로 멈췄는지. */
  done: boolean;
}

export interface Selection {
  /** 서버 종류 힌트(파일명 후보 선정용). */
  server: ServerHint;
  /** 선택한 로그 폴더. */
  root: string;
  /** 마지막 탐색 결과. 시작 화면을 벗어났다 돌아와도 유지한다. */
  scan: ScanResponse | null;
  /** 선택한 파일 목록(탐색 결과에서). */
  files: ScannedFile[];
  /** 포맷 단계에서 고른 프로필 이름(판별 결과 또는 사용자가 고른 것). */
  profileName: string;
  /** 포맷 단계에서 적용한 정의. 있으면 이름 대신 이 정의로 가져온다(편집본 포함). */
  profile: FormatProfile | null;
}

interface AppState {
  stage: Stage;
  setStage: (s: Stage) => void;
  logKind: LogKind;
  setLogKind: (k: LogKind) => void;
  /** 두 종류의 선택 상태 전체. 한 폴더를 두 패턴으로 탐색한 결과를 함께 갱신할 때 쓴다. */
  selections: Record<LogKind, Selection>;
  setSelections: (s: Record<LogKind, Selection>) => void;
  project: ProjectInfo | null;
  refreshProject: () => Promise<void>;
  openProject: (path: string) => Promise<void>;
  /** cases/ 아래에 새 케이스 DB를 만들어 연다. 이름 힌트는 파일 이름에만 쓴다. */
  createCase: (nameHint: string) => Promise<void>;
  progress: ImportProgressView | null;
  finished: ImportFinishedView | null;
  clearFinished: () => void;
  /** 큐로 여러 가져오기를 차례로 시작한다. 첫 항목이 시작될 때까지 기다린다. */
  startImports: (items: ImportItem[]) => Promise<void>;
  importRun: ImportRun | null;
  exportProgress: ExportProgressView | null;
  exportFinished: ExportFinishedView | null;
  clearExportFinished: () => void;
  selection: Selection;
  setSelection: (s: Selection) => void;
  notice: string | null;
  setNotice: (m: string | null) => void;
  /** 사이드바에서 고른 룰의 조건. nonce가 바뀔 때마다 조회·통계가 다시 적용한다. */
  ruleRequest: { name: string; filter: LogFilter; nonce: number } | null;
  applyRule: (name: string, filter: LogFilter) => void;
  /** 조회 화면이 마지막으로 적용한 조건. 사용자 룰로 저장할 때 쓴다. */
  currentFilter: LogFilter | null;
  setCurrentFilter: (f: LogFilter | null) => void;
}

const Ctx = createContext<AppState | null>(null);

export function AppStateProvider({ children }: { children: ReactNode }) {
  const [stage, setStage] = useState<Stage>("start");
  const [project, setProject] = useState<ProjectInfo | null>(null);
  const [progress, setProgress] = useState<ImportProgressView | null>(null);
  const [finished, setFinished] = useState<ImportFinishedView | null>(null);
  const [importRun, setImportRun] = useState<ImportRun | null>(null);
  // 이벤트 핸들러가 최신 큐를 보도록 ref를 함께 갱신한다(setImportRun과 항상 같이 쓴다).
  const runRef = useRef<ImportRun | null>(null);
  const [exportProgress, setExportProgress] = useState<ExportProgressView | null>(null);
  const [exportFinished, setExportFinished] = useState<ExportFinishedView | null>(null);
  const [logKind, setLogKind] = useState<LogKind>("access");
  // 탭마다 선택 상태를 따로 둔다. 탭을 오가도 고른 폴더와 라벨이 남는다.
  const [selections, setSelections] = useState<Record<LogKind, Selection>>({
    access: { server: "apache", root: "", scan: null, files: [], profileName: "combined", profile: null },
    error: { server: "apache", root: "", scan: null, files: [], profileName: "combined", profile: null },
  });
  const selection = selections[logKind];
  const setSelection = useCallback((s: Selection) => setSelections((prev) => ({ ...prev, [logKind]: s })), [logKind]);
  const [notice, setNotice] = useState<string | null>(null);
  const [ruleRequest, setRuleRequest] = useState<{ name: string; filter: LogFilter; nonce: number } | null>(null);
  const [currentFilter, setCurrentFilter] = useState<LogFilter | null>(null);
  const applyRule = useCallback((name: string, filter: LogFilter) => setRuleRequest((prev) => ({ name, filter, nonce: (prev?.nonce ?? 0) + 1 })), []);
  const pollRef = useRef<number | null>(null);

  const refreshProject = useCallback(async () => {
    try {
      const p = await api.currentProject();
      setProject(p);
    } catch (e) {
      setNotice(errorText(e));
    }
  }, []);

  const openProject = useCallback(async (path: string) => {
    const info = await api.openProject(path);
    setProject(info);
    if (info.interrupted_jobs.length > 0) {
      setNotice(`이전 실행이 비정상 종료된 작업 ${info.interrupted_jobs.join(", ")}이(가) 있습니다. 결과 화면의 작업 탭에서 재개할 수 있습니다.`);
    }
  }, []);

  const createCase = useCallback(async (nameHint: string) => {
    const info = await api.createCase(nameHint);
    setProject(info);
  }, []);

  const startImports = useCallback(async (items: ImportItem[]) => {
    if (items.length === 0) return;
    const run: ImportRun = { items, index: 0, results: [], done: false };
    runRef.current = run;
    setImportRun(run);
    setFinished(null);
    await api.startImport(items[0].request);
  }, []);

  /** 종료 통지 처리. 이벤트와 폴링이 겹쳐 같은 작업이 두 번 올 수 있으므로 작업 ID로 거른다. */
  const onFinished = useCallback(
    (f: ImportFinishedView) => {
      setProgress(null);
      void refreshProject();
      const run = runRef.current;
      if (!run || run.done) {
        setFinished(f);
        return;
      }
      if (run.results.some((r) => r.finished.job_id === f.job_id)) return;
      const results = [...run.results, { label: run.items[run.index]?.label ?? "", finished: f }];
      const next = run.index + 1;
      if (f.error === null && next < run.items.length) {
        const updated: ImportRun = { ...run, index: next, results };
        runRef.current = updated;
        setImportRun(updated);
        api.startImport(run.items[next].request).catch((e) => {
          const failed: ImportRun = { ...updated, done: true, results: [...results, { label: run.items[next].label, finished: { job_id: -1, status: "failed", summary: null, error: errorText(e) } }] };
          runRef.current = failed;
          setImportRun(failed);
          setFinished(failed.results[failed.results.length - 1].finished);
        });
      } else {
        const doneRun: ImportRun = { ...run, results, done: true };
        runRef.current = doneRun;
        setImportRun(doneRun);
        setFinished(f);
      }
    },
    [refreshProject],
  );

  // 이벤트 구독 + 이벤트가 유실됐을 때를 대비한 느린 폴링(2초). 재연결 시 현재 상태를 다시 조회한다.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    let disposed = false;
    api
      .onImportEvent((e) => {
        if (e.kind === "import_progress") {
          setProgress(e);
        } else if (e.kind === "import_finished") {
          onFinished(e);
        } else if (e.kind === "export_progress") {
          setExportProgress(e);
        } else {
          setExportProgress(null);
          setExportFinished(e);
        }
      })
      .then((u) => {
        if (disposed) u();
        else unlisten = u;
      })
      .catch((err) => setNotice(errorText(err)));
    const poll = async () => {
      try {
        const st = await api.importStatus();
        if (st !== null) {
          if (st.finished) {
            onFinished(st.finished);
          } else {
            setProgress(st.progress);
          }
        }
        const ex = await api.exportStatus();
        if (ex !== null) {
          if (ex.finished) {
            setExportProgress(null);
            setExportFinished(ex.finished);
          } else {
            setExportProgress(ex.progress);
          }
        }
      } catch {
        // 폴링 실패는 조용히 넘긴다. 다음 주기에 다시 시도한다.
      }
    };
    void poll();
    pollRef.current = window.setInterval(poll, 2000);
    return () => {
      disposed = true;
      if (unlisten) unlisten();
      if (pollRef.current !== null) window.clearInterval(pollRef.current);
    };
  }, [refreshProject, onFinished]);

  const value = useMemo<AppState>(
    () => ({
      stage,
      setStage,
      logKind,
      setLogKind,
      selections,
      setSelections,
      project,
      refreshProject,
      openProject,
      createCase,
      progress,
      finished,
      clearFinished: () => {
        setFinished(null);
        runRef.current = null;
        setImportRun(null);
      },
      startImports,
      importRun,
      exportProgress,
      exportFinished,
      clearExportFinished: () => setExportFinished(null),
      selection,
      setSelection,
      notice,
      setNotice,
      ruleRequest,
      applyRule,
      currentFilter,
      setCurrentFilter,
    }),
    [stage, logKind, selections, project, refreshProject, openProject, createCase, progress, finished, startImports, importRun, exportProgress, exportFinished, selection, setSelection, notice, ruleRequest, applyRule, currentFilter],
  );
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useAppState(): AppState {
  const v = useContext(Ctx);
  if (!v) throw new Error("AppStateProvider 밖에서 useAppState를 호출함");
  return v;
}
