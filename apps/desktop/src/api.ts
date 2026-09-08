// Tauri 명령 래퍼. 인자 이름은 Rust 쪽 `rename_all = "snake_case"`와 같다.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  CaseInfo,
  DetailView,
  ExportRequest,
  ExportStatusView,
  SavedView,
  StatsRequest,
  StatsResult,
  ViewDefinition,
  FormatProfile,
  ProfileListView,
  ProfileView,
  ValidationView,
  ImportEvent,
  ImportStatusView,
  JobInfo,
  JobView,
  LogFilter,
  LogPage,
  PageRequest,
  PresetView,
  PreviewResult,
  ProfileSpec,
  ProjectInfo,
  SampleLines,
  ScanRequest,
  ScanResponse,
  SourceVerification,
  StartImportRequest,
} from "./types";

export const IMPORT_EVENT = "weblog://import";

export const api = {
  openProject: (db_path: string) => invoke<ProjectInfo>("open_project", { db_path }),
  createCase: (name_hint: string) => invoke<ProjectInfo>("create_case", { name_hint }),
  listCases: () => invoke<CaseInfo[]>("list_cases"),
  deleteCase: (path: string) => invoke<void>("delete_case", { path }),
  closeProject: () => invoke<void>("close_project"),
  currentProject: () => invoke<ProjectInfo | null>("current_project"),
  listPresets: () => invoke<PresetView[]>("list_presets"),
  listProfiles: () => invoke<ProfileListView>("list_profiles"),
  validateProfile: (profile: FormatProfile) => invoke<ValidationView>("validate_profile", { profile }),
  profileFromYaml: (yaml: string) => invoke<FormatProfile>("profile_from_yaml", { yaml }),
  profileToYaml: (profile: FormatProfile) => invoke<string>("profile_to_yaml", { profile }),
  saveProfile: (profile: FormatProfile) => invoke<ProfileView>("save_profile", { profile }),
  deleteProfile: (name: string) => invoke<boolean>("delete_profile", { name }),
  scanFiles: (request: ScanRequest) => invoke<ScanResponse>("scan_files", { request }),
  previewFormat: (path: string, profile: ProfileSpec, max_lines: number) =>
    invoke<PreviewResult>("preview_format", { request: { path, profile, max_lines } }),
  sampleLines: (path: string, max_lines: number) => invoke<SampleLines>("sample_lines", { path, max_lines }),
  startImport: (request: StartImportRequest) => invoke<number>("start_import", { request }),
  resumeJob: (job_id: number, full_verify: boolean) => invoke<number>("resume_job", { job_id, full_verify }),
  cancelImport: () => invoke<void>("cancel_import"),
  importStatus: () => invoke<ImportStatusView | null>("import_status"),
  listJobs: () => invoke<JobView[]>("list_jobs"),
  activateJob: (job_id: number) => invoke<JobInfo>("activate_job", { job_id }),
  deleteJobResults: (job_id: number) => invoke<number>("delete_job_results", { job_id }),
  verifySource: (source_id: number, full: boolean, relink: string | null) =>
    invoke<SourceVerification>("verify_source", { source_id, full, relink }),
  queryPage: (request: PageRequest) => invoke<LogPage>("query_page", { request }),
  countLogs: (filter: LogFilter) => invoke<number>("count_logs", { filter }),
  logDetail: (job_id: number | null, source_id: number, line_number: number) =>
    invoke<DetailView | null>("log_detail", { job_id, source_id, line_number }),
  computeStats: (request: StatsRequest) => invoke<StatsResult>("compute_stats", { request }),
  cancelHeavy: () => invoke<void>("cancel_heavy"),
  listViews: () => invoke<SavedView[]>("list_views"),
  saveView: (name: string, definition: ViewDefinition) => invoke<SavedView>("save_view", { name, definition }),
  deleteView: (view_id: number) => invoke<boolean>("delete_view", { view_id }),
  startExport: (request: ExportRequest) => invoke<void>("start_export", { request }),
  cancelExport: () => invoke<void>("cancel_export"),
  exportStatus: () => invoke<ExportStatusView | null>("export_status"),
  onImportEvent: (handler: (e: ImportEvent) => void): Promise<UnlistenFn> =>
    listen<ImportEvent>(IMPORT_EVENT, (ev) => handler(ev.payload)),
};

export function errorText(e: unknown): string {
  let text: string;
  if (typeof e === "string") text = e;
  else if (e instanceof Error) text = e.message;
  else {
    try {
      text = JSON.stringify(e);
    } catch {
      text = String(e);
    }
  }
  // DuckDB 오류는 뒤에 스택 트레이스가 붙는다. 첫 줄만, 길이도 제한한다.
  const first = text.split(/\r?\n/, 1)[0] ?? text;
  return first.length > 400 ? `${first.slice(0, 400)}…` : first;
}
