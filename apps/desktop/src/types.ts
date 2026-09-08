// IPC 자료형. Rust 쪽 serde 정의(snake_case)와 1:1로 맞춘다. 자동 변환을 가정하지 않는다.

export type JobStatus =
  | "queued"
  | "running"
  | "completed"
  | "completed_with_errors"
  | "cancelling"
  | "cancelled"
  | "failed"
  | "interrupted";

export interface JobInfo {
  job_id: number;
  result_version: number;
  status: JobStatus;
  profile_id: number;
  committed_records: number;
  committed_errors: number;
  committed_skipped: number;
  active: boolean;
  replaces_job_id: number | null;
  failure_reason: string | null;
}

export interface JobSource {
  source_id: number;
  ordinal: number;
  status: string;
  path: string;
}

export interface JobView {
  job: JobInfo;
  sources: JobSource[];
}

export interface ProjectInfo {
  db_path: string;
  db_file_bytes: number;
  interrupted_jobs: number[];
  jobs: JobInfo[];
}

export interface ScanRequest {
  root: string;
  recursive: boolean;
  max_depth: number | null;
  include: string[];
  exclude: string[];
  detect: boolean;
  sample_lines: number;
}

export interface ScannedFile {
  path: string;
  file_size: number;
  modified_unix: number | null;
  compression: string | null;
  best_profile: string | null;
  best_hash: string | null;
  match_rate: number | null;
  lines_checked: number | null;
  sample_records: number | null;
  sample_errors: number | null;
  detect_error: string | null;
}

export interface ScanResponse {
  files: ScannedFile[];
  errors: { path: string; message: string }[];
  truncated: boolean;
  directories_visited: number;
  filtered_out: number;
}

export type ServerHint = "apache" | "nginx" | "iis" | "unknown";

export type TimezonePolicy = { kind: "from_input" } | { kind: "fixed"; offset_seconds: number } | { kind: "utc" };
export type TimestampFormat = { kind: "clf" } | { kind: "iso8601" } | { kind: "custom"; pattern: string };
export type FieldKind =
  | { kind: "client_ip" }
  | { kind: "timestamp"; format: TimestampFormat }
  | { kind: "request_line" }
  | { kind: "method" }
  | { kind: "request_target" }
  | { kind: "protocol" }
  | { kind: "status" }
  | { kind: "bytes_sent" }
  | { kind: "referrer" }
  | { kind: "user_agent" }
  | { kind: "integer" }
  | { kind: "text" };
export type Capture = { kind: "token" } | { kind: "quoted" } | { kind: "bracketed" } | { kind: "pattern"; pattern: string };
export interface FieldDef {
  name: string;
  kind: FieldKind;
  capture: Capture;
  missing: string[];
}
export type Block =
  | { block: "literal"; text: string }
  | { block: "whitespace" }
  | ({ block: "field" } & FieldDef)
  | { block: "optional_group"; blocks: Block[] }
  | { block: "regex"; pattern: string };
export type Strategy = { kind: "blocks"; blocks: Block[] } | { kind: "w3c" };

/** Rust `FormatProfile`과 같은 구조. 퍼즐과 YAML이 이 정의를 편집한다. */
export interface FormatProfile {
  schema_version: number;
  name: string;
  version: number;
  server_hint: ServerHint;
  timezone: TimezonePolicy;
  strategy: Strategy;
}

export interface ProfileView {
  name: string;
  source: "builtin" | "user";
  profile: FormatProfile;
  yaml: string;
  path: string | null;
}

export interface ProfileListView {
  profiles: ProfileView[];
  errors: { path: string; message: string }[];
  user_dir: string | null;
}

export interface ValidationIssue {
  path: string;
  message: string;
}

export interface ValidationView {
  issues: ValidationIssue[];
  definition_hash: string | null;
  yaml: string | null;
}

export type ProfileSpec = { kind: "preset"; name: string } | { kind: "definition"; profile: FormatProfile };

export interface PresetView {
  name: string;
  profile: FormatProfile;
}

export interface LogRecord {
  line_number: number;
  timestamp_utc: number | null;
  tz_offset_seconds: number | null;
  client_ip: string | null;
  method: string | null;
  request_target: string | null;
  protocol: string | null;
  status: number | null;
  bytes_sent: number | null;
  referrer: string | null;
  user_agent: string | null;
  extra?: Record<string, string>;
}

export type LineOutcome =
  | ({ kind: "record" } & LogRecord)
  | { kind: "error"; line_number: number; code: string; field?: string }
  | { kind: "skipped"; line_number: number; reason: string };

/** cases/ 안의 케이스 DB. */
export interface CaseInfo {
  path: string;
  name: string;
  bytes: number;
  modified_unix: number | null;
  open: boolean;
}

/** 파일 선두 원문 샘플. 포맷 확인 화면에서만 잠시 보여주며 저장하지 않는다. */
export interface SampleLine {
  line_number: number;
  text: string | null;
}

export interface SampleLines {
  lines: SampleLine[];
  truncated: boolean;
}

export interface PreviewResult {
  lines_checked: number;
  records: number;
  errors: number;
  skipped: number;
  match_rate: number;
  outcomes: LineOutcome[];
  truncated: boolean;
}

export interface StartImportRequest {
  profile: ProfileSpec;
  paths: string[];
  replaces_job_id: number | null;
  batch_max_rows: number | null;
  batch_max_bytes: number | null;
}

export interface ImportProgressView {
  job_id: number;
  source_id: number;
  lines_read: number;
  committed_records: number;
  committed_batches: number;
  elapsed_secs: number;
  cancel_requested: boolean;
  resumed: boolean;
}

export interface ImportFinishedView {
  job_id: number;
  status: string;
  summary: unknown | null;
  error: string | null;
}

export interface ImportStatusView {
  progress: ImportProgressView;
  finished: ImportFinishedView | null;
}

export type ImportEvent =
  | ({ kind: "import_progress" } & ImportProgressView)
  | ({ kind: "import_finished" } & ImportFinishedView)
  | ({ kind: "export_progress" } & ExportProgressView)
  | ({ kind: "export_finished" } & ExportFinishedView);

export interface LogFilter {
  job_id: number | null;
  source_id: number | null;
  time_from_micros: number | null;
  time_to_micros: number | null;
  status: number | null;
  status_class: number | null;
  client_ip: string | null;
  method: string | null;
  target_contains: string | null;
  /** 요청 대상 정규식(RE2). 분석 룰의 서명 검사에 쓴다. */
  target_regex: string | null;
  /** 룰 조건식. 위 단순 조건과 AND로 결합한다. */
  expr: FilterExpr | null;
  /** 북마크한 행만. */
  bookmarked_only: boolean;
  active_only: boolean;
}

export type CondField = "status" | "bytes_sent" | "client_ip" | "method" | "request_target" | "protocol" | "referrer" | "user_agent";
export type CondOp = "eq" | "ne" | "gt" | "gte" | "lt" | "lte" | "contains" | "icontains" | "starts_with" | "ends_with" | "regex" | "is_null";

/** Rust `FilterExpr`와 같은 구조(serde tag = kind). */
export type FilterExpr =
  | { kind: "and"; items: FilterExpr[] }
  | { kind: "or"; items: FilterExpr[] }
  | { kind: "not"; item: FilterExpr }
  | { kind: "cond"; field: CondField; op: CondOp; value: string }
  | { kind: "true" };

export type SortOrder = "time_asc" | "time_desc";

export interface PageCursor {
  filter_hash: string;
  max_batch_id: number;
  segment: "timed" | "null_time";
  last_ts: number | null;
  last_source_id: number;
  last_line: number;
}

export interface PageRequest {
  filter: LogFilter;
  sort: SortOrder;
  page_size: number;
  cursor: PageCursor | null;
}

export interface LogRow {
  source_id: number;
  line_number: number;
  timestamp_utc: number | null;
  client_ip: string | null;
  method: string | null;
  request_target: string | null;
  status: number | null;
  bytes_sent: number | null;
  bookmarked: boolean;
}

export interface LogPage {
  rows: LogRow[];
  next_cursor: PageCursor | null;
  approx_bytes: number;
}

export interface LogDetail {
  job_id: number;
  batch_id: number;
  source_id: number;
  source_path: string;
  line_number: number;
  timestamp_utc: number | null;
  tz_offset_seconds: number | null;
  client_ip: string | null;
  method: string | null;
  request_target: string | null;
  protocol: string | null;
  status: number | null;
  bytes_sent: number | null;
  referrer: string | null;
  user_agent: string | null;
  extra_json: string | null;
}

export interface DetailView {
  detail: LogDetail;
  reconstructed: string;
  is_reconstruction: boolean;
  template: "blocks" | "standard";
  complete: boolean;
  profile_name: string;
  profile_version: number;
  extra: [string, string][];
}

export interface SourceVerification {
  source_id: number;
  path: string;
  matches: boolean;
  reason: string | null;
  full_checked: boolean;
}

export const emptyFilter = (): LogFilter => ({
  job_id: null,
  source_id: null,
  time_from_micros: null,
  time_to_micros: null,
  status: null,
  status_class: null,
  client_ip: null,
  method: null,
  target_contains: null,
  target_regex: null,
  expr: null,
  bookmarked_only: false,
  active_only: false,
});

export type TimeBucket = "auto" | "minute" | "hour" | "day";

export interface StatsRequest {
  filter: LogFilter;
  top_n: number;
  bucket: TimeBucket;
  /** 버킷 경계를 맞출 표시 시간대 오프셋(초). */
  tz_offset_seconds: number;
}

export interface StatsResult {
  max_batch_id: number;
  total: number;
  null_time_rows: number;
  status: [number | null, number][];
  methods: [string | null, number][];
  top_ips: [string, number][];
  /** 상위 IP의 최초·마지막 탐지와 접근 횟수. */
  ip_rows: { ip: string; first_seen: number | null; last_seen: number | null; count: number }[];
  top_targets: [string, number][];
  timeline: [number, number][];
  bucket_seconds: number;
  timeline_truncated: boolean;
  time_range: [number, number] | null;
}

export interface ViewDefinition {
  filter: LogFilter;
  sort: SortOrder;
  columns: string[];
  /** 룰 원문(YARA풍 텍스트). */
  rule_source: string | null;
}

export interface SavedView {
  view_id: number;
  name: string;
  definition: ViewDefinition;
}

export type ExportFormat = "csv" | "json_lines";

export interface ExportRequest {
  filter: LogFilter;
  sort: SortOrder;
  format: ExportFormat;
  out_path: string;
  max_rows: number | null;
  include_extra: boolean;
}

export interface ExportProgressView {
  out_path: string;
  rows: number;
  bytes: number;
  elapsed_secs: number;
  cancel_requested: boolean;
}

export interface ExportSummary {
  out_path: string;
  rows: number;
  bytes: number;
  elapsed_secs: number;
  max_batch_id: number;
  cancelled: boolean;
  truncated: boolean;
}

export interface ExportFinishedView {
  out_path: string;
  summary: ExportSummary | null;
  error: string | null;
}

export interface ExportStatusView {
  progress: ExportProgressView;
  finished: ExportFinishedView | null;
}
