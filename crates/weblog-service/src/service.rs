//! 서비스 본체.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use weblog_engine::detect::{default_candidates, detect_file};
use weblog_engine::export::{export_logs, ExportProgress, ExportRequest};
use weblog_engine::format::{presets, yaml, FormatProfile, ProfileLibrary};
use weblog_engine::importer::{
    resume_import, run_import, ImportConfig, ImportRequest, ImportSummary, Progress,
};
use weblog_engine::preview::{preview_file, PreviewConfig, PreviewResult};
use weblog_engine::reconstruct;
use weblog_engine::source::{scan_directory, LineContent, LineReader, ScanOptions};
use weblog_engine::store::stats::compute_stats;
use weblog_engine::store::{
    JobInfo, LogFilter, LogPage, LogQuery, PageRequest, SavedView, SourceVerification,
    StatsRequest, StatsResult, Store, StoreConfig, ViewDefinition, ViewQuery,
};
use weblog_engine::EngineError;

use crate::dto::*;
use crate::project::Project;

/// 서비스 오류. IPC로는 문자열로 직렬화된다.
#[derive(Debug, thiserror::Error)]
pub enum ServiceError {
    /// 엔진 오류.
    #[error("{0}")]
    Engine(#[from] EngineError),
    /// 열린 프로젝트가 없음.
    #[error("열린 프로젝트가 없음")]
    NoProject,
    /// 가져오기가 진행 중이라 쓰기 작업을 할 수 없음.
    #[error("가져오기가 진행 중(작업 {job_id})이라 지금은 할 수 없음")]
    ImportRunning {
        /// 진행 중 작업.
        job_id: i64,
    },
    /// 진행 중 가져오기가 없음.
    #[error("진행 중인 가져오기가 없음")]
    NoImport,
    /// 잘못된 요청.
    #[error("잘못된 요청: {0}")]
    Invalid(String),
}

impl serde::Serialize for ServiceError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

/// 서비스 결과.
pub type ServiceResult<T> = Result<T, ServiceError>;

/// 서비스 설정.
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// 저장소 설정.
    pub store: StoreConfig,
    /// 가져오기 기본 설정.
    pub import: ImportConfig,
    /// 읽기 연결 수.
    pub readers: usize,
    /// 진행 이벤트 최소 간격.
    pub progress_interval: Duration,
    /// 사용자 프리셋 디렉터리. 없으면 저장 기능을 끈다.
    pub profiles_dir: Option<PathBuf>,
    /// 케이스(프로젝트 DB) 자동 생성 디렉터리. 없으면 `create_case`를 거부한다.
    pub cases_dir: Option<PathBuf>,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            store: StoreConfig {
                memory_limit: Some("1GB".to_owned()),
                threads: Some(default_threads()),
                temp_directory: None,
                max_temp_directory_size: None,
            },
            import: ImportConfig::default(),
            readers: 2,
            progress_interval: Duration::from_millis(250),
            profiles_dir: None,
            cases_dir: None,
        }
    }
}

fn default_threads() -> u32 {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    u32::try_from((cores / 2).max(2)).unwrap_or(2)
}

/// 서비스가 UI로 보내는 이벤트.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ServiceEvent {
    /// 진행.
    ImportProgress(ImportProgressView),
    /// 종료.
    ImportFinished(ImportFinishedView),
    /// 내보내기 진행.
    ExportProgress(ExportProgressView),
    /// 내보내기 종료.
    ExportFinished(ExportFinishedView),
}

type EventSink = Arc<dyn Fn(ServiceEvent) + Send + Sync>;

struct RunningImport {
    job_id: Arc<Mutex<i64>>,
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<ImportProgressView>>,
    handle: Option<std::thread::JoinHandle<()>>,
    finished: Arc<Mutex<Option<ImportFinishedView>>>,
}

struct RunningExport {
    cancel: Arc<AtomicBool>,
    progress: Arc<Mutex<ExportProgressView>>,
    handle: Option<std::thread::JoinHandle<()>>,
    finished: Arc<Mutex<Option<ExportFinishedView>>>,
}

/// 애플리케이션 서비스. 스레드 간 공유 가능하다.
pub struct Service {
    cfg: ServiceConfig,
    project: Mutex<Option<Arc<Project>>>,
    import: Mutex<Option<RunningImport>>,
    export: Mutex<Option<RunningExport>>,
    /// 실행 중인 무거운 조회(통계·건수)의 중단 핸들.
    heavy_interrupt: Mutex<Option<Arc<weblog_engine::store::InterruptHandle>>>,
    sink: Mutex<Option<EventSink>>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Service")
    }
}

enum ImportKind {
    New(ImportRequest),
    Resume(i64),
}

impl Service {
    /// 새 서비스.
    pub fn new(cfg: ServiceConfig) -> Self {
        Self {
            cfg,
            project: Mutex::new(None),
            import: Mutex::new(None),
            export: Mutex::new(None),
            heavy_interrupt: Mutex::new(None),
            sink: Mutex::new(None),
        }
    }

    /// 이벤트 수신자를 등록한다(UI 계층이 Tauri emit으로 연결).
    pub fn set_event_sink(&self, sink: impl Fn(ServiceEvent) + Send + Sync + 'static) {
        *lock(&self.sink) = Some(Arc::new(sink));
    }

    fn emit(&self, event: ServiceEvent) {
        if let Some(sink) = lock(&self.sink).clone() {
            sink(event);
        }
    }

    // ----- 프로젝트 -----

    /// 프로젝트 DB를 열거나 만든다. 이미 열린 프로젝트는 닫는다(가져오기 중이면 거부).
    /// 케이스 디렉터리 안에 `<힌트>-<로컬 시각>.duckdb`를 만들어 연다. 이름이 겹치면 번호를 붙인다.
    /// 사용자에게 저장 위치를 묻지 않는 흐름을 위한 것이며, 위치는 설정의 `cases_dir`가 정한다.
    pub fn create_case(&self, name_hint: &str) -> ServiceResult<ProjectInfo> {
        let dir =
            self.cfg.cases_dir.clone().ok_or_else(|| {
                ServiceError::Invalid("케이스 디렉터리가 설정되지 않음".to_owned())
            })?;
        std::fs::create_dir_all(&dir).map_err(|e| {
            ServiceError::Invalid(format!(
                "케이스 디렉터리를 만들 수 없음 ({}): {e}",
                dir.display()
            ))
        })?;
        let stem = sanitize_case_name(name_hint);
        let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
        let base = format!("{stem}-{stamp}");
        let mut path = dir.join(format!("{base}.duckdb"));
        let mut n = 2;
        while path.exists() {
            path = dir.join(format!("{base}-{n}.duckdb"));
            n += 1;
        }
        self.open_project(path)
    }

    /// cases/ 안의 케이스 DB 목록(최근 수정 순). 디렉터리가 없으면 빈 목록.
    pub fn list_cases(&self) -> ServiceResult<Vec<CaseInfo>> {
        let Some(dir) = self.cfg.cases_dir.clone() else {
            return Ok(Vec::new());
        };
        let open_path = lock(&self.project).as_ref().map(|p| p.path.clone());
        let mut out = Vec::new();
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => {
                return Err(ServiceError::Invalid(format!(
                    "케이스 디렉터리를 읽을 수 없음 ({}): {e}",
                    dir.display()
                )))
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("duckdb") {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let modified_unix = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64);
            out.push(CaseInfo {
                name: path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                bytes: meta.len(),
                modified_unix,
                open: open_path.as_deref() == Some(path.as_path()),
                path: path.to_string_lossy().into_owned(),
            });
        }
        out.sort_by(|a, b| {
            b.modified_unix
                .cmp(&a.modified_unix)
                .then(a.name.cmp(&b.name))
        });
        Ok(out)
    }

    /// 케이스 DB 파일(과 WAL)을 지운다. cases/ 바로 아래 파일만 허용하며, 열려 있으면 먼저 닫는다.
    pub fn delete_case(&self, path: &Path) -> ServiceResult<()> {
        let dir =
            self.cfg.cases_dir.clone().ok_or_else(|| {
                ServiceError::Invalid("케이스 디렉터리가 설정되지 않음".to_owned())
            })?;
        let dir_canon = std::fs::canonicalize(&dir)
            .map_err(|e| ServiceError::Invalid(format!("케이스 디렉터리를 찾을 수 없음: {e}")))?;
        let target = std::fs::canonicalize(path)
            .map_err(|e| ServiceError::Invalid(format!("케이스 파일을 찾을 수 없음: {e}")))?;
        if target.parent() != Some(dir_canon.as_path())
            || target.extension().and_then(|e| e.to_str()) != Some("duckdb")
        {
            return Err(ServiceError::Invalid(
                "케이스 디렉터리 안의 .duckdb 파일만 지울 수 있음".to_owned(),
            ));
        }
        let is_open = lock(&self.project)
            .as_ref()
            .and_then(|p| std::fs::canonicalize(&p.path).ok())
            .is_some_and(|p| p == target);
        if is_open {
            self.close_project()?;
        }
        std::fs::remove_file(&target)
            .map_err(|e| ServiceError::Invalid(format!("케이스 파일 삭제 실패: {e}")))?;
        let wal = target.with_extension("duckdb.wal");
        if wal.exists() {
            let _ = std::fs::remove_file(wal);
        }
        let tmp = target.with_extension("duckdb.tmp");
        if tmp.is_dir() {
            let _ = std::fs::remove_dir_all(tmp);
        }
        Ok(())
    }

    /// 프로젝트 DB를 연다(없으면 만든다). 진행 중인 가져오기·내보내기가 있으면 거부한다.
    pub fn open_project(&self, db_path: PathBuf) -> ServiceResult<ProjectInfo> {
        self.ensure_no_import()?;
        self.ensure_no_export()?;
        // 기존 프로젝트를 먼저 닫아 같은 파일을 두 번 여는 일을 막는다.
        *lock(&self.project) = None;
        // 임시 디렉터리가 설정에 없으면 DB 옆 `<이름>.tmp`를 쓴다. DuckDB가 정렬·집계 중 메모리 상한에 걸리면 여기로 넘긴다.
        let mut store_cfg = self.cfg.store.clone();
        if store_cfg.temp_directory.is_none() {
            let tmp = db_path.with_extension("duckdb.tmp");
            if std::fs::create_dir_all(&tmp).is_ok() {
                store_cfg.temp_directory = Some(tmp);
            }
        }
        let (project, interrupted) = Project::open(db_path, &store_cfg, self.cfg.readers)?;
        let project = Arc::new(project);
        *lock(&self.project) = Some(Arc::clone(&project));
        self.project_info(&project, interrupted)
    }

    /// 프로젝트를 닫는다.
    pub fn close_project(&self) -> ServiceResult<()> {
        self.ensure_no_import()?;
        self.ensure_no_export()?;
        *lock(&self.project) = None;
        Ok(())
    }

    /// 현재 프로젝트 정보.
    pub fn current_project(&self) -> ServiceResult<Option<ProjectInfo>> {
        match lock(&self.project).clone() {
            Some(p) => Ok(Some(self.project_info(&p, Vec::new())?)),
            None => Ok(None),
        }
    }

    fn project_info(&self, project: &Project, interrupted: Vec<i64>) -> ServiceResult<ProjectInfo> {
        let jobs = project.readers.acquire().list_jobs()?;
        Ok(ProjectInfo {
            db_path: project.path.to_string_lossy().into_owned(),
            db_file_bytes: std::fs::metadata(&project.path)
                .map(|m| m.len())
                .unwrap_or(0),
            interrupted_jobs: interrupted,
            jobs,
        })
    }

    fn project(&self) -> ServiceResult<Arc<Project>> {
        lock(&self.project).clone().ok_or(ServiceError::NoProject)
    }

    // ----- 탐색·판별·미리보기 (프로젝트 불필요) -----

    /// 프리셋 목록.
    pub fn presets(&self) -> Vec<PresetView> {
        presets::PRESET_NAMES
            .iter()
            .filter_map(|n| {
                presets::by_name(n).map(|p| PresetView {
                    name: (*n).to_owned(),
                    profile: p,
                })
            })
            .collect()
    }

    /// 경로 아래를 탐색하고 파일별로 판별한다.
    pub fn scan(&self, req: &ScanRequest) -> ServiceResult<ScanResponse> {
        let opts = ScanOptions {
            recursive: req.recursive,
            max_depth: req.max_depth,
            include: req.include.clone(),
            exclude: req.exclude.clone(),
            ..ScanOptions::default()
        };
        let scan = scan_directory(&req.root, &opts);
        let candidates = default_candidates();
        let cfg = PreviewConfig {
            max_lines: req.sample_lines.clamp(1, 2000),
            max_line_bytes: self.cfg.import.max_line_bytes,
            ..PreviewConfig::default()
        };
        let files = scan
            .entries
            .iter()
            .map(|e| {
                let mut f = ScannedFile {
                    path: e.path.to_string_lossy().into_owned(),
                    file_size: e.file_size,
                    modified_unix: e.modified_unix,
                    compression: e.compression.map(|c| c.as_str().to_owned()),
                    best_profile: None,
                    best_hash: None,
                    match_rate: None,
                    lines_checked: None,
                    sample_records: None,
                    sample_errors: None,
                    detect_error: None,
                };
                if req.detect {
                    let d = detect_file(&e.path, &candidates, &cfg);
                    f.detect_error = d.error.clone();
                    if let Some(best) = d.best() {
                        f.best_profile = Some(best.profile_name.clone());
                        f.best_hash = Some(best.definition_hash.clone());
                        f.match_rate = Some(best.match_rate);
                        f.lines_checked = Some(best.lines_checked);
                        f.sample_records = Some(best.records);
                        f.sample_errors = Some(best.errors);
                    } else if let Some(first) = d.candidates.first() {
                        f.lines_checked = Some(first.lines_checked);
                        f.match_rate = Some(0.0);
                    }
                }
                f
            })
            .collect();
        Ok(ScanResponse {
            files,
            errors: scan
                .errors
                .into_iter()
                .map(|e| ScanIssue {
                    path: e.path.to_string_lossy().into_owned(),
                    message: e.message,
                })
                .collect(),
            truncated: scan.truncated,
            errors_truncated: scan.errors_truncated,
            directories_visited: scan.directories_visited,
            filtered_out: scan.filtered_out,
        })
    }

    fn library(&self) -> Option<ProfileLibrary> {
        self.cfg.profiles_dir.as_ref().map(ProfileLibrary::new)
    }

    /// 프로필 지정을 정의로 바꾼다. 이름은 내장 프리셋 → 사용자 프리셋 순으로 찾고, 정의는 검증한다.
    pub fn resolve_profile(&self, spec: &ProfileSpec) -> ServiceResult<FormatProfile> {
        match spec {
            ProfileSpec::Preset { name } => {
                if let Some(p) = presets::by_name(name) {
                    return Ok(p);
                }
                if let Some(lib) = self.library() {
                    if let Some(p) = lib.load(name)? {
                        return Ok(p);
                    }
                }
                Err(ServiceError::Invalid(format!("알 수 없는 프로필 {name}")))
            }
            ProfileSpec::Definition { profile } => {
                profile.ensure_valid()?;
                Ok(profile.clone())
            }
        }
    }

    /// 내장 + 사용자 프로필 목록.
    pub fn list_profiles(&self) -> ServiceResult<ProfileListView> {
        let mut profiles = Vec::new();
        for (name, profile) in presets::builtin() {
            profiles.push(ProfileView {
                name: name.to_owned(),
                source: "builtin".to_owned(),
                yaml: yaml::to_yaml(&profile)?,
                profile,
                path: None,
            });
        }
        let mut errors = Vec::new();
        let mut user_dir = None;
        if let Some(lib) = self.library() {
            user_dir = Some(lib.dir().to_string_lossy().into_owned());
            let listing = lib.list()?;
            for sp in listing.profiles {
                profiles.push(ProfileView {
                    name: sp.name,
                    source: "user".to_owned(),
                    yaml: yaml::to_yaml(&sp.profile)?,
                    profile: sp.profile,
                    path: Some(sp.path.to_string_lossy().into_owned()),
                });
            }
            errors.extend(listing.errors.into_iter().map(|(path, message)| ScanIssue {
                path: path.to_string_lossy().into_owned(),
                message,
            }));
        }
        Ok(ProfileListView {
            profiles,
            errors,
            user_dir,
        })
    }

    /// 정의를 검증한다. 유효하면 해시와 정규화 YAML을 함께 돌려준다.
    pub fn validate_profile(&self, profile: &FormatProfile) -> ServiceResult<ValidationView> {
        let issues = profile.validate();
        if issues.is_empty() {
            Ok(ValidationView {
                issues,
                definition_hash: Some(profile.definition_hash().map_err(EngineError::from)?),
                yaml: Some(yaml::to_yaml(profile)?),
            })
        } else {
            Ok(ValidationView {
                issues,
                definition_hash: None,
                yaml: None,
            })
        }
    }

    /// YAML을 정의로 읽는다(검증 포함).
    pub fn profile_from_yaml(&self, text: &str) -> ServiceResult<FormatProfile> {
        Ok(yaml::from_yaml(text)?)
    }

    /// 정의를 YAML로.
    pub fn profile_to_yaml(&self, profile: &FormatProfile) -> ServiceResult<String> {
        Ok(yaml::to_yaml(profile)?)
    }

    /// 사용자 프리셋으로 저장한다.
    pub fn save_profile(&self, profile: &FormatProfile) -> ServiceResult<ProfileView> {
        let lib = self.library().ok_or_else(|| {
            ServiceError::Invalid("사용자 프리셋 디렉터리가 설정되지 않음".to_owned())
        })?;
        let path = lib.save(profile)?;
        Ok(ProfileView {
            name: profile.name.clone(),
            source: "user".to_owned(),
            yaml: yaml::to_yaml(profile)?,
            profile: profile.clone(),
            path: Some(path.to_string_lossy().into_owned()),
        })
    }

    /// 사용자 프리셋을 삭제한다.
    pub fn delete_profile(&self, name: &str) -> ServiceResult<bool> {
        let lib = self.library().ok_or_else(|| {
            ServiceError::Invalid("사용자 프리셋 디렉터리가 설정되지 않음".to_owned())
        })?;
        Ok(lib.delete(name)?)
    }

    /// 파일 선두 미리보기. 원문은 반환하지 않는다.
    pub fn preview(&self, req: &PreviewRequest) -> ServiceResult<PreviewResult> {
        let profile = self.resolve_profile(&req.profile)?;
        let cfg = PreviewConfig {
            max_lines: req.max_lines.clamp(1, 2000),
            max_line_bytes: self.cfg.import.max_line_bytes,
            ..PreviewConfig::default()
        };
        Ok(preview_file(&req.path, &profile, &cfg)?)
    }

    /// 파일 선두 몇 줄의 원문. 포맷 확인 화면에서 사용자에게 잠깐 보여주기 위한 것이며
    /// 저장하지 않는다. 줄 수와 바이트 상한을 둔다.
    pub fn sample_lines(&self, path: &Path, max_lines: u64) -> ServiceResult<SampleLines> {
        const MAX_LINES: u64 = 20;
        const MAX_BYTES: u64 = 256 * 1024;
        let max_lines = max_lines.clamp(1, MAX_LINES);
        let mut reader = LineReader::open(path, self.cfg.import.max_line_bytes)?;
        let mut lines = Vec::new();
        let mut truncated = false;
        loop {
            if reader.line_number() >= max_lines || reader.offset() >= MAX_BYTES {
                truncated = true;
                break;
            }
            let Some(line) = reader.next_line()? else {
                break;
            };
            let text = match line.content {
                LineContent::Text(t) => Some(t.to_owned()),
                LineContent::InvalidUtf8 | LineContent::TooLong => None,
            };
            lines.push(SampleLine {
                line_number: line.line_number,
                text,
            });
        }
        Ok(SampleLines { lines, truncated })
    }

    // ----- 가져오기 -----

    /// 끝난 가져오기 기록을 정리하고, 진행 중이면 오류를 돌려준다.
    fn ensure_no_import(&self) -> ServiceResult<()> {
        let mut guard = lock(&self.import);
        if let Some(run) = guard.as_mut() {
            if run.handle.as_ref().is_some_and(|h| h.is_finished()) {
                if let Some(h) = run.handle.take() {
                    let _ = h.join();
                }
                *guard = None;
                return Ok(());
            }
            return Err(ServiceError::ImportRunning {
                job_id: *lock(&run.job_id),
            });
        }
        Ok(())
    }

    /// 새 가져오기를 백그라운드 스레드에서 시작한다. 진행·종료는 이벤트와 [`Service::import_status`]로 본다.
    /// 반환값은 작업 ID(파일 등록·작업 생성이 끝나면 확정되며, 그 전에 실패하면 오류).
    pub fn start_import(self: &Arc<Self>, req: StartImportRequest) -> ServiceResult<i64> {
        self.ensure_no_import()?;
        if req.paths.is_empty() {
            return Err(ServiceError::Invalid("파일이 선택되지 않음".to_owned()));
        }
        let project = self.project()?;
        let profile = self.resolve_profile(&req.profile)?;
        let mut cfg = self.cfg.import.clone();
        if let Some(r) = req.batch_max_rows {
            cfg.batch_max_rows = r.clamp(1_000, 1_000_000);
        }
        if let Some(b) = req.batch_max_bytes {
            cfg.batch_max_bytes = b.clamp(1 << 20, 1 << 30);
        }
        let engine_req = ImportRequest {
            profile,
            paths: req.paths,
            replaces_job_id: req.replaces_job_id,
            log_kind: req.log_kind,
        };
        self.spawn_import(project, cfg, ImportKind::New(engine_req))
    }

    /// 중단된 작업을 재개한다.
    pub fn resume_job(self: &Arc<Self>, job_id: i64, full_verify: bool) -> ServiceResult<i64> {
        self.ensure_no_import()?;
        let project = self.project()?;
        let mut cfg = self.cfg.import.clone();
        cfg.full_verify_on_resume = full_verify;
        self.spawn_import(project, cfg, ImportKind::Resume(job_id))
    }

    fn spawn_import(
        self: &Arc<Self>,
        project: Arc<Project>,
        cfg: ImportConfig,
        kind: ImportKind,
    ) -> ServiceResult<i64> {
        let cancel = Arc::new(AtomicBool::new(false));
        let job_id_cell = Arc::new(Mutex::new(0i64));
        let (job_id_tx, job_id_rx) = std::sync::mpsc::channel::<ServiceResult<i64>>();
        let resumed = matches!(kind, ImportKind::Resume(_));
        let progress = Arc::new(Mutex::new(ImportProgressView {
            job_id: 0,
            source_id: 0,
            lines_read: 0,
            committed_records: 0,
            committed_batches: 0,
            elapsed_secs: 0.0,
            cancel_requested: false,
            resumed,
        }));
        let finished = Arc::new(Mutex::new(None));
        let service = Arc::clone(self);
        let t_cancel = Arc::clone(&cancel);
        let t_progress = Arc::clone(&progress);
        let t_finished = Arc::clone(&finished);
        let t_job_id = Arc::clone(&job_id_cell);
        let interval = self.cfg.progress_interval;
        let handle = std::thread::Builder::new()
            .name("weblog-import".to_owned())
            .spawn(move || {
                let mut store = project.store.lock().unwrap_or_else(|e| e.into_inner());
                let started = Instant::now();
                let mut last_emit: Option<Instant> = None;
                let mut sent_job_id = false;
                let mut on_progress = |p: &Progress| {
                    if !sent_job_id {
                        sent_job_id = true;
                        *lock(&t_job_id) = p.job_id;
                        let _ = job_id_tx.send(Ok(p.job_id));
                    }
                    let view = {
                        let mut g = lock(&t_progress);
                        g.job_id = p.job_id;
                        g.source_id = p.source_id;
                        g.lines_read = p.lines_read;
                        g.committed_records = p.committed_records;
                        g.committed_batches = p.committed_batches;
                        g.elapsed_secs = started.elapsed().as_secs_f64();
                        g.cancel_requested = t_cancel.load(Ordering::Relaxed);
                        g.clone()
                    };
                    if last_emit.is_none_or(|t| t.elapsed() >= interval) {
                        last_emit = Some(Instant::now());
                        service.emit(ServiceEvent::ImportProgress(view));
                    }
                };
                let result: ServiceResult<ImportSummary> = match kind {
                    ImportKind::New(req) => {
                        let r = run_import(&mut store, &req, &cfg, &t_cancel, &mut on_progress);
                        if !sent_job_id {
                            // 첫 배치 전에 끝났거나 실패했다. 작업이 만들어졌다면 그 ID를 알린다.
                            let id = match &r {
                                Ok(s) => Ok(s.job_id),
                                Err(e) => match store.latest_job_id() {
                                    Ok(Some(id)) => Ok(id),
                                    _ => Err(ServiceError::Invalid(e.to_string())),
                                },
                            };
                            if let Ok(id) = &id {
                                *lock(&t_job_id) = *id;
                            }
                            let _ = job_id_tx.send(id);
                        }
                        r.map_err(ServiceError::from)
                    }
                    ImportKind::Resume(job_id) => {
                        *lock(&t_job_id) = job_id;
                        lock(&t_progress).job_id = job_id;
                        let _ = job_id_tx.send(Ok(job_id));
                        resume_import(&mut store, job_id, &cfg, &t_cancel, &mut on_progress)
                            .map_err(ServiceError::from)
                    }
                };
                // 확정된 배치를 데이터 파일에 반영해 두어, 이후 비정상 종료 시 긴 WAL 재생을 피한다. 읽기 트랜잭션이 열려 있으면 거부될 수 있어 실패는 무시한다.
                let _ = store.checkpoint();
                drop(store);
                let job_id = *lock(&t_job_id);
                let finished_view = match result {
                    Ok(summary) => ImportFinishedView {
                        job_id: summary.job_id,
                        status: summary.status.clone(),
                        summary: Some(summary),
                        error: None,
                    },
                    Err(e) => ImportFinishedView {
                        job_id,
                        status: "failed".to_owned(),
                        summary: None,
                        error: Some(e.to_string()),
                    },
                };
                *lock(&t_finished) = Some(finished_view.clone());
                service.emit(ServiceEvent::ImportFinished(finished_view));
            })
            .map_err(|e| ServiceError::Invalid(format!("가져오기 스레드 생성 실패: {e}")))?;
        *lock(&self.import) = Some(RunningImport {
            job_id: job_id_cell,
            cancel,
            progress,
            handle: Some(handle),
            finished,
        });
        // 파일 등록·작업 생성은 첫 배치 커밋 전에 끝나므로 첫 진행 통지(또는 조기 종료)에서 ID를 받는다.
        match job_id_rx.recv() {
            Ok(Ok(id)) => Ok(id),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(ServiceError::Invalid(
                "가져오기 스레드가 응답하지 않음".to_owned(),
            )),
        }
    }

    /// 협력적 취소를 요청한다. 현재 배치 이후 중단된다.
    pub fn cancel_import(&self) -> ServiceResult<()> {
        let guard = lock(&self.import);
        let run = guard.as_ref().ok_or(ServiceError::NoImport)?;
        run.cancel.store(true, Ordering::Relaxed);
        lock(&run.progress).cancel_requested = true;
        Ok(())
    }

    /// 진행 중 가져오기 스냅샷. 끝났으면 종료 통지를 함께 돌려주고 기록을 정리한다.
    pub fn import_status(
        &self,
    ) -> ServiceResult<Option<(ImportProgressView, Option<ImportFinishedView>)>> {
        let mut guard = lock(&self.import);
        let Some(run) = guard.as_mut() else {
            return Ok(None);
        };
        let progress = lock(&run.progress).clone();
        let finished = lock(&run.finished).clone();
        if finished.is_some() {
            if let Some(h) = run.handle.take() {
                let _ = h.join();
            }
            *guard = None;
        }
        Ok(Some((progress, finished)))
    }

    // ----- 작업 관리 -----

    /// 작업 목록(최신순).
    pub fn list_jobs(&self) -> ServiceResult<Vec<JobView>> {
        let project = self.project()?;
        let reader = project.readers.acquire();
        let mut out = Vec::new();
        for job in reader.list_jobs()? {
            let sources = reader.job_sources(job.job_id)?;
            out.push(JobView { job, sources });
        }
        Ok(out)
    }

    fn with_store<T>(&self, f: impl FnOnce(&mut Store) -> ServiceResult<T>) -> ServiceResult<T> {
        self.ensure_no_import()?;
        let project = self.project()?;
        // Poisoning means an earlier owner panicked; recover and go on. Only contention is an import.
        let mut store = match project.store.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(ServiceError::ImportRunning {
                    job_id: lock(&self.import).as_ref().map_or(0, |r| *lock(&r.job_id)),
                })
            }
        };
        f(&mut store)
    }

    /// 재파싱 결과를 활성화한다.
    pub fn activate_job(&self, job_id: i64) -> ServiceResult<JobInfo> {
        self.with_store(|s| {
            s.activate_job(job_id)?;
            Ok(s.job(job_id)?)
        })
    }

    /// 작업 결과를 삭제한다.
    pub fn delete_job_results(&self, job_id: i64) -> ServiceResult<u64> {
        self.with_store(|s| Ok(s.delete_job_results(job_id)?))
    }

    /// 파일을 검증한다. `relink`가 있으면 먼저 재연결한다.
    pub fn verify_source(
        &self,
        source_id: i64,
        full: bool,
        relink: Option<PathBuf>,
    ) -> ServiceResult<SourceVerification> {
        self.with_store(|s| {
            if let Some(p) = relink {
                s.relink_source(source_id, &p)?;
            }
            Ok(s.verify_source(source_id, full)?)
        })
    }

    // ----- 조회 -----

    /// 페이지 조회.
    pub fn query_page(&self, req: &PageRequest) -> ServiceResult<LogPage> {
        let project = self.project()?;
        let page = project.readers.acquire().query_page(req)?;
        Ok(page)
    }

    /// 무거운 조회를 동시 1개로 직렬화하고, 실행 중 중단 핸들을 기록한다.
    fn heavy<T>(
        &self,
        f: impl FnOnce(&weblog_engine::store::Reader) -> ServiceResult<T>,
    ) -> ServiceResult<T> {
        let project = self.project()?;
        let _heavy = project.readers.heavy();
        let reader = project.readers.acquire();
        *lock(&self.heavy_interrupt) = Some(reader.interrupt_handle());
        let result = f(&reader);
        *lock(&self.heavy_interrupt) = None;
        result.map_err(|e| match e {
            ServiceError::Engine(EngineError::Store(inner))
                if inner.to_string().contains("INTERRUPT") =>
            {
                ServiceError::Invalid("조회를 중단했습니다".to_owned())
            }
            other => other,
        })
    }

    /// 실행 중인 무거운 조회(건수·통계)를 중단한다. 없으면 아무것도 하지 않는다.
    pub fn cancel_heavy(&self) {
        if let Some(h) = lock(&self.heavy_interrupt).clone() {
            h.interrupt();
        }
    }

    /// 전체 건수. 무거운 조회이므로 동시 1개로 제한한다.
    pub fn count(&self, filter: &LogFilter) -> ServiceResult<i64> {
        self.heavy(|r| Ok(r.count_matching(filter)?))
    }

    /// 기본 통계. 무거운 조회.
    pub fn stats(&self, req: &StatsRequest) -> ServiceResult<StatsResult> {
        self.heavy(|r| Ok(compute_stats(r, req)?))
    }

    // ----- 저장된 뷰 -----

    /// 뷰 목록.
    pub fn list_views(&self) -> ServiceResult<Vec<SavedView>> {
        let project = self.project()?;
        let views = project.readers.acquire().list_views()?;
        Ok(views)
    }

    /// 뷰 저장(같은 이름은 덮어씀).
    pub fn save_view(&self, name: &str, definition: &ViewDefinition) -> ServiceResult<SavedView> {
        self.with_store(|s| Ok(s.save_view(name, definition)?))
    }

    /// 뷰 삭제.
    pub fn delete_view(&self, view_id: i64) -> ServiceResult<bool> {
        self.with_store(|s| Ok(s.delete_view(view_id)?))
    }

    /// 북마크 토글. 켜지면 true. 가져오기 중에는 쓰기 잠금 때문에 거부된다.
    pub fn toggle_bookmark(&self, source_id: i64, line_number: i64) -> ServiceResult<bool> {
        self.with_store(|s| Ok(s.toggle_bookmark(source_id, line_number)?))
    }

    // ----- 내보내기 -----

    fn ensure_no_export(&self) -> ServiceResult<()> {
        let mut guard = lock(&self.export);
        if let Some(run) = guard.as_mut() {
            if run.handle.as_ref().is_some_and(|h| h.is_finished()) {
                if let Some(h) = run.handle.take() {
                    let _ = h.join();
                }
                *guard = None;
                return Ok(());
            }
            return Err(ServiceError::Invalid("내보내기가 이미 진행 중".to_owned()));
        }
        Ok(())
    }

    /// 내보내기를 백그라운드에서 시작한다. 진행·종료는 이벤트와 [`Service::export_status`]로 본다.
    pub fn start_export(self: &Arc<Self>, req: ExportRequest) -> ServiceResult<()> {
        self.ensure_no_export()?;
        let project = self.project()?;
        let cancel = Arc::new(AtomicBool::new(false));
        let progress = Arc::new(Mutex::new(ExportProgressView {
            out_path: req.out_path.to_string_lossy().into_owned(),
            rows: 0,
            bytes: 0,
            elapsed_secs: 0.0,
            cancel_requested: false,
        }));
        let finished = Arc::new(Mutex::new(None));
        let service = Arc::clone(self);
        let t_cancel = Arc::clone(&cancel);
        let t_progress = Arc::clone(&progress);
        let t_finished = Arc::clone(&finished);
        let handle = std::thread::Builder::new()
            .name("weblog-export".to_owned())
            .spawn(move || {
                let reader = project
                    .export_reader
                    .lock()
                    .unwrap_or_else(|e| e.into_inner());
                let mut on_progress = |p: &ExportProgress| {
                    let view = {
                        let mut g = lock(&t_progress);
                        g.rows = p.rows;
                        g.bytes = p.bytes;
                        g.elapsed_secs = p.elapsed_secs;
                        g.cancel_requested = t_cancel.load(Ordering::Relaxed);
                        g.clone()
                    };
                    service.emit(ServiceEvent::ExportProgress(view));
                };
                let result = export_logs(&*reader, &req, &t_cancel, &mut on_progress);
                drop(reader);
                let out_path = req.out_path.to_string_lossy().into_owned();
                let view = match result {
                    Ok(summary) => ExportFinishedView {
                        out_path,
                        summary: Some(summary),
                        error: None,
                    },
                    Err(e) => ExportFinishedView {
                        out_path,
                        summary: None,
                        error: Some(e.to_string()),
                    },
                };
                *lock(&t_finished) = Some(view.clone());
                service.emit(ServiceEvent::ExportFinished(view));
            })
            .map_err(|e| ServiceError::Invalid(format!("내보내기 스레드 생성 실패: {e}")))?;
        *lock(&self.export) = Some(RunningExport {
            cancel,
            progress,
            handle: Some(handle),
            finished,
        });
        Ok(())
    }

    /// 내보내기 취소. 지금까지 쓴 파일은 남는다.
    pub fn cancel_export(&self) -> ServiceResult<()> {
        let guard = lock(&self.export);
        let run = guard
            .as_ref()
            .ok_or_else(|| ServiceError::Invalid("진행 중인 내보내기가 없음".to_owned()))?;
        run.cancel.store(true, Ordering::Relaxed);
        lock(&run.progress).cancel_requested = true;
        Ok(())
    }

    /// 내보내기 상태. 끝났으면 종료 통지를 한 번 돌려주고 정리한다.
    pub fn export_status(
        &self,
    ) -> ServiceResult<Option<(ExportProgressView, Option<ExportFinishedView>)>> {
        let mut guard = lock(&self.export);
        let Some(run) = guard.as_mut() else {
            return Ok(None);
        };
        let progress = lock(&run.progress).clone();
        let finished = lock(&run.finished).clone();
        if finished.is_some() {
            if let Some(h) = run.handle.take() {
                let _ = h.join();
            }
            *guard = None;
        }
        Ok(Some((progress, finished)))
    }

    /// 상세 + 재구성 로그.
    pub fn detail(
        &self,
        job_id: Option<i64>,
        source_id: i64,
        line_number: i64,
    ) -> ServiceResult<Option<DetailView>> {
        let project = self.project()?;
        let reader = project.readers.acquire();
        let Some(d) = reader.detail(job_id, source_id, line_number)? else {
            return Ok(None);
        };
        let extra_map: std::collections::BTreeMap<String, String> = d
            .extra_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
            .unwrap_or_default();
        // 그 작업이 쓴 프로필(포맷 스냅샷)로 재구성한다.
        let job = reader.job(d.job_id)?;
        let profile = reader.profile(job.profile_id)?;
        drop(reader);
        let rec = reconstruct::from_profile(&profile, &d, &extra_map);
        Ok(Some(DetailView {
            detail: d,
            reconstructed: rec.text,
            is_reconstruction: rec.is_reconstruction,
            template: rec.template.to_owned(),
            complete: rec.complete,
            profile_name: profile.name,
            profile_version: profile.version,
            extra: extra_map.into_iter().collect(),
        }))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Drop for Service {
    fn drop(&mut self) {
        if let Some(run) = lock(&self.import).as_mut() {
            run.cancel.store(true, Ordering::Relaxed);
            if let Some(h) = run.handle.take() {
                let _ = h.join();
            }
        }
        if let Some(run) = lock(&self.export).as_mut() {
            run.cancel.store(true, Ordering::Relaxed);
            if let Some(h) = run.handle.take() {
                let _ = h.join();
            }
        }
    }
}

/// 폴더 이름 등을 파일 이름으로 쓸 수 있게 다듬는다. 영문·숫자·밑줄·하이픈·점만 남기고 나머지는 밑줄로, 비면 `case`.
fn sanitize_case_name(hint: &str) -> String {
    let mut out = String::new();
    for c in hint.trim().chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
            out.push(c);
        } else if !out.ends_with('_') {
            // 공백·비ASCII·구분자는 밑줄 하나로 줄인다.
            out.push('_');
        }
    }
    out = out.trim_matches(|c| c == '.' || c == '_').to_owned();
    if out.is_empty() {
        "case".to_owned()
    } else {
        out.chars().take(40).collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use std::io::Write;
    use std::path::Path;
    use weblog_engine::store::LogKind;

    const LINE: &str =
        r#"10.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET /x HTTP/1.0" 200 10 "-" "ua""#;

    fn temp_log(dir: &Path, name: &str, n: usize) -> PathBuf {
        let p = dir.join(name);
        let mut f = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
        for _ in 0..n {
            writeln!(f, "{LINE}").unwrap();
        }
        f.flush().unwrap();
        p
    }

    fn wait_finished(service: &Service) -> ImportFinishedView {
        for _ in 0..3000 {
            if let Some((_, Some(done))) = service.import_status().unwrap() {
                return done;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("import did not finish");
    }

    fn preset(name: &str) -> ProfileSpec {
        ProfileSpec::Preset {
            name: name.to_owned(),
        }
    }

    fn job_of(service: &Service, job_id: i64) -> JobInfo {
        service
            .list_jobs()
            .unwrap()
            .into_iter()
            .find(|v| v.job.job_id == job_id)
            .expect("job exists")
            .job
    }

    #[test]
    fn open_scan_import_query_detail_flow() {
        let dir = tempfile::tempdir().unwrap();
        let log = temp_log(dir.path(), "a.log", 300);
        let service = Arc::new(Service::new(ServiceConfig::default()));
        let events = Arc::new(Mutex::new(Vec::new()));
        let ev = Arc::clone(&events);
        service.set_event_sink(move |e| lock(&ev).push(e));
        let info = service.open_project(dir.path().join("p.duckdb")).unwrap();
        assert!(info.jobs.is_empty());

        let scan = service
            .scan(&ScanRequest {
                root: dir.path().to_path_buf(),
                recursive: true,
                max_depth: None,
                include: vec!["*.log".to_owned()],
                exclude: vec![],
                detect: true,
                sample_lines: 50,
            })
            .unwrap();
        assert_eq!(scan.files.len(), 1);
        assert_eq!(scan.files[0].best_profile.as_deref(), Some("combined"));

        let job_id = service
            .start_import(StartImportRequest {
                profile: preset("apache_combined"),
                paths: vec![log],
                replaces_job_id: None,
                log_kind: LogKind::Access,
                batch_max_rows: Some(1_000),
                batch_max_bytes: None,
            })
            .unwrap();
        assert_eq!(job_id, 1);
        let done = wait_finished(&service);
        assert_eq!(done.status, "completed");
        assert!(lock(&events)
            .iter()
            .any(|e| matches!(e, ServiceEvent::ImportFinished(_))));

        let page = service
            .query_page(&PageRequest {
                filter: LogFilter::default(),
                sort: weblog_engine::store::SortOrder::TimeAsc,
                page_size: 10,
                cursor: None,
            })
            .unwrap();
        assert_eq!(page.rows.len(), 10);
        assert_eq!(service.count(&LogFilter::default()).unwrap(), 300);
        let d = service.detail(None, 1, 5).unwrap().unwrap();
        assert!(d.is_reconstruction);
        assert!(d.reconstructed.contains("GET /x HTTP/1.0"));
        assert_eq!(service.list_jobs().unwrap().len(), 1);
    }

    #[test]
    fn write_operations_are_refused_while_import_runs_and_cancel_then_resume_completes() {
        let dir = tempfile::tempdir().unwrap();
        let log = temp_log(dir.path(), "big.log", 200_000);
        let service = Arc::new(Service::new(ServiceConfig::default()));
        service.open_project(dir.path().join("p.duckdb")).unwrap();
        let job_id = service
            .start_import(StartImportRequest {
                profile: preset("combined"),
                paths: vec![log],
                replaces_job_id: None,
                log_kind: LogKind::Access,
                batch_max_rows: Some(5_000),
                batch_max_bytes: None,
            })
            .unwrap();
        let err = service.delete_job_results(job_id).unwrap_err();
        assert!(matches!(err, ServiceError::ImportRunning { .. }), "{err}");
        assert!(service
            .open_project(dir.path().join("other.duckdb"))
            .is_err());
        assert!(
            service.list_jobs().is_ok(),
            "reads are allowed during import"
        );
        service.cancel_import().unwrap();
        let done = wait_finished(&service);
        assert_eq!(done.status, "cancelled");
        assert!(job_of(&service, job_id).status.is_resumable());
        service.resume_job(job_id, false).unwrap();
        let done = wait_finished(&service);
        assert_eq!(done.status, "completed");
        assert_eq!(service.count(&LogFilter::default()).unwrap(), 200_000);
    }

    #[test]
    fn start_import_returns_job_id_before_first_batch_commit() {
        let dir = tempfile::tempdir().unwrap();
        let log = temp_log(dir.path(), "big.log", 200_000);
        let service = Arc::new(Service::new(ServiceConfig::default()));
        service.open_project(dir.path().join("p.duckdb")).unwrap();
        // A batch boundary past EOF means the only commit happens when the import ends.
        let job_id = service
            .start_import(StartImportRequest {
                profile: preset("combined"),
                paths: vec![log],
                replaces_job_id: None,
                log_kind: LogKind::Access,
                batch_max_rows: Some(1_000_000),
                batch_max_bytes: Some(1 << 30),
            })
            .unwrap();
        let (progress, finished) = service
            .import_status()
            .unwrap()
            .expect("반환 직후에는 가져오기가 진행 중이다");
        assert_eq!(progress.job_id, job_id, "작업 ID는 첫 커밋 전에 확정된다");
        assert_eq!(
            progress.committed_batches, 0,
            "start_import가 첫 배치 커밋까지 기다리면 안 됨"
        );
        assert!(finished.is_none(), "start_import가 종료까지 기다리면 안 됨");
        service.cancel_import().unwrap();
        assert_eq!(wait_finished(&service).status, "cancelled");
    }

    #[test]
    fn import_failure_before_first_batch_is_reported_as_error() {
        let dir = tempfile::tempdir().unwrap();
        let service = Arc::new(Service::new(ServiceConfig::default()));
        service.open_project(dir.path().join("p.duckdb")).unwrap();
        let err = service
            .start_import(StartImportRequest {
                profile: preset("combined"),
                paths: vec![dir.path().join("missing.log")],
                replaces_job_id: None,
                log_kind: LogKind::Access,
                batch_max_rows: None,
                batch_max_bytes: None,
            })
            .unwrap_err();
        assert!(matches!(err, ServiceError::Invalid(_)), "{err}");
        // 스레드가 이미 끝났을 수도, 아직일 수도 있다. 어느 쪽이든 종료 통지는 정확히 한 번 나온다.
        let first = service
            .import_status()
            .unwrap()
            .expect("record exists until finish is consumed");
        let done = match first.1 {
            Some(d) => d,
            None => wait_finished(&service),
        };
        assert_eq!(done.status, "failed");
        assert!(
            service.import_status().unwrap().is_none(),
            "finish is consumed once"
        );
        assert!(service.ensure_no_import().is_ok());
    }

    #[test]
    fn poisoned_store_lock_is_recovered_not_reported_as_import_running() {
        let dir = tempfile::tempdir().unwrap();
        let service = Arc::new(Service::new(ServiceConfig::default()));
        service.open_project(dir.path().join("p.duckdb")).unwrap();
        let project = service.project().unwrap();
        let store = Arc::clone(&project.store);
        // A thread that panics while holding the write store lock poisons the mutex (no import runs).
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let _ = std::thread::spawn(move || {
            let _guard = store.lock().unwrap_or_else(|e| e.into_inner());
            panic!("owner thread died");
        })
        .join();
        std::panic::set_hook(hook);
        assert!(project.store.is_poisoned(), "테스트 전제: 락이 포이즈닝됨");
        let view = service
            .save_view(
                "all",
                &ViewDefinition {
                    filter: LogFilter::default(),
                    sort: weblog_engine::store::SortOrder::TimeAsc,
                    columns: Vec::new(),
                    rule_source: None,
                },
            )
            .expect("포이즈닝은 복구하고 진행해야 함");
        assert_eq!(view.name, "all");
        assert_eq!(service.list_views().unwrap().len(), 1);
    }

    #[test]
    fn edited_profile_reparse_creates_new_version_and_detail_uses_that_profile() {
        let dir = tempfile::tempdir().unwrap();
        let log = temp_log(dir.path(), "a.log", 5);
        let cfg = ServiceConfig {
            profiles_dir: Some(dir.path().join("presets")),
            ..ServiceConfig::default()
        };
        let service = Arc::new(Service::new(cfg));
        service.open_project(dir.path().join("p.duckdb")).unwrap();
        let first = service
            .start_import(StartImportRequest {
                profile: preset("apache_combined"),
                paths: vec![log.clone()],
                replaces_job_id: None,
                log_kind: LogKind::Access,
                batch_max_rows: None,
                batch_max_bytes: None,
            })
            .unwrap();
        wait_finished(&service);
        // 편집: 이름·버전을 바꾼 사용자 프리셋으로 저장한 뒤 그 이름으로 재파싱한다.
        let mut edited = presets::apache_combined();
        edited.name = "site_v2".to_owned();
        edited.version = 2;
        let saved = service.save_profile(&edited).unwrap();
        assert_eq!(saved.source, "user");
        assert!(service
            .list_profiles()
            .unwrap()
            .profiles
            .iter()
            .any(|p| p.name == "site_v2"));
        let second = service
            .start_import(StartImportRequest {
                profile: preset("site_v2"),
                paths: vec![log],
                replaces_job_id: Some(first),
                log_kind: LogKind::Access,
                batch_max_rows: None,
                batch_max_bytes: None,
            })
            .unwrap();
        let done = wait_finished(&service);
        assert_eq!(done.status, "completed");
        assert!(!job_of(&service, second).active);
        service.activate_job(second).unwrap();
        let d = service.detail(Some(second), 1, 1).unwrap().unwrap();
        assert_eq!((d.profile_name.as_str(), d.profile_version), ("site_v2", 2));
        assert_eq!(d.template, "blocks");
        assert!(d
            .reconstructed
            .starts_with("10.0.0.1 - - [10/Oct/2000:13:55:36 -0700]"));
        let old = service.detail(Some(first), 1, 1).unwrap().unwrap();
        assert_eq!(old.profile_name, "combined");
        assert!(service.delete_profile("site_v2").unwrap());
    }

    #[test]
    fn invalid_definition_is_rejected_before_import_and_validation_lists_issues() {
        let dir = tempfile::tempdir().unwrap();
        let service = Arc::new(Service::new(ServiceConfig::default()));
        service.open_project(dir.path().join("p.duckdb")).unwrap();
        let mut bad = presets::apache_combined();
        bad.name = "no spaces allowed".to_owned();
        let v = service.validate_profile(&bad).unwrap();
        assert_eq!(v.issues[0].path, "name");
        assert!(v.yaml.is_none());
        let err = service
            .start_import(StartImportRequest {
                profile: ProfileSpec::Definition { profile: bad },
                paths: vec![dir.path().join("x.log")],
                replaces_job_id: None,
                log_kind: LogKind::Access,
                batch_max_rows: None,
                batch_max_bytes: None,
            })
            .unwrap_err();
        assert!(matches!(err, ServiceError::Engine(_)), "{err}");
        assert!(service.import_status().unwrap().is_none());
    }

    #[test]
    fn stats_views_and_export_work_through_the_service() {
        let dir = tempfile::tempdir().unwrap();
        let log = temp_log(dir.path(), "a.log", 1200);
        let service = Arc::new(Service::new(ServiceConfig::default()));
        service.open_project(dir.path().join("p.duckdb")).unwrap();
        service
            .start_import(StartImportRequest {
                profile: preset("apache_combined"),
                paths: vec![log],
                replaces_job_id: None,
                log_kind: LogKind::Access,
                batch_max_rows: None,
                batch_max_bytes: None,
            })
            .unwrap();
        wait_finished(&service);
        let stats = service
            .stats(&StatsRequest {
                filter: LogFilter::default(),
                top_n: 5,
                bucket: weblog_engine::store::TimeBucket::Auto,
                tz_offset_seconds: 0,
            })
            .unwrap();
        assert_eq!(stats.total, 1200);
        assert_eq!(stats.status, vec![(Some(200), 1200)]);
        let def = ViewDefinition {
            filter: LogFilter {
                status: Some(200),
                ..LogFilter::default()
            },
            sort: weblog_engine::store::SortOrder::TimeDesc,
            columns: vec![],
            rule_source: None,
        };
        let v = service.save_view("ok only", &def).unwrap();
        assert_eq!(service.list_views().unwrap().len(), 1);
        let out = dir.path().join("out.csv");
        service
            .start_export(ExportRequest {
                filter: LogFilter::default(),
                sort: weblog_engine::store::SortOrder::TimeAsc,
                format: weblog_engine::export::ExportFormat::Csv,
                out_path: out.clone(),
                max_rows: None,
                include_extra: false,
            })
            .unwrap();
        let mut done = None;
        for _ in 0..3000 {
            if let Some((_, Some(f))) = service.export_status().unwrap() {
                done = Some(f);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let done = done.expect("export finishes");
        assert!(done.error.is_none(), "{:?}", done.error);
        assert_eq!(done.summary.unwrap().rows, 1200);
        assert_eq!(std::fs::read_to_string(&out).unwrap().lines().count(), 1201);
        assert!(service.delete_view(v.view_id).unwrap());
        service.cancel_heavy();
    }

    #[test]
    fn errors_without_project_are_explicit() {
        let service = Arc::new(Service::new(ServiceConfig::default()));
        assert!(matches!(
            service.list_jobs().unwrap_err(),
            ServiceError::NoProject
        ));
        assert!(matches!(
            service.cancel_import().unwrap_err(),
            ServiceError::NoImport
        ));
        assert!(service.import_status().unwrap().is_none());
    }

    #[test]
    fn create_case_makes_db_under_cases_dir_without_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let cases = dir.path().join("cases");
        let service = Service::new(ServiceConfig {
            cases_dir: Some(cases.clone()),
            ..ServiceConfig::default()
        });
        let a = service.create_case("nginx 로그/2026").unwrap();
        assert!(a.db_path.starts_with(cases.to_str().unwrap()));
        assert!(a.db_path.contains("nginx_2026-"), "{}", a.db_path);
        assert!(a.db_path.ends_with(".duckdb"));
        assert!(Path::new(&a.db_path).exists());
        let b = service.create_case("nginx 로그/2026").unwrap();
        assert_ne!(
            a.db_path, b.db_path,
            "같은 초에 만들어도 다른 파일이어야 함"
        );
        assert_eq!(sanitize_case_name("  ../.. "), "case");
        assert_eq!(sanitize_case_name("C:\\inetpub\\logs"), "C_inetpub_logs");

        let none = Service::new(ServiceConfig::default());
        assert!(matches!(
            none.create_case("x").unwrap_err(),
            ServiceError::Invalid(_)
        ));
    }

    #[test]
    fn list_and_delete_cases() {
        let dir = tempfile::tempdir().unwrap();
        let cases = dir.path().join("cases");
        let service = Service::new(ServiceConfig {
            cases_dir: Some(cases.clone()),
            ..ServiceConfig::default()
        });
        assert!(
            service.list_cases().unwrap().is_empty(),
            "디렉터리가 없어도 빈 목록"
        );
        let a = service.create_case("a").unwrap();
        let list = service.list_cases().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].path, a.db_path);
        assert!(list[0].open);
        assert!(list[0].name.starts_with("a-"));

        // 디렉터리 밖 파일은 거부한다.
        let outside = dir.path().join("x.duckdb");
        std::fs::write(&outside, b"").unwrap();
        assert!(matches!(
            service.delete_case(&outside).unwrap_err(),
            ServiceError::Invalid(_)
        ));
        assert!(outside.exists());

        // 열린 케이스를 지우면 먼저 닫힌다.
        service.delete_case(Path::new(&a.db_path)).unwrap();
        assert!(!Path::new(&a.db_path).exists());
        assert!(service.current_project().unwrap().is_none());
        assert!(service.list_cases().unwrap().is_empty());
    }

    #[test]
    fn sample_lines_returns_leading_text_up_to_limit() {
        let dir = tempfile::tempdir().unwrap();
        let log = temp_log(dir.path(), "a.log", 5);
        let service = Service::new(ServiceConfig::default());
        let s = service.sample_lines(&log, 2).unwrap();
        assert_eq!(s.lines.len(), 2);
        assert_eq!(s.lines[0].line_number, 1);
        assert!(s.lines[0]
            .text
            .as_deref()
            .unwrap()
            .contains("13:55:36 -0700"));
        assert!(s.truncated);
        let all = service.sample_lines(&log, 20).unwrap();
        assert_eq!(all.lines.len(), 5);
        assert!(!all.truncated);
    }

    #[test]
    fn preview_returns_outcomes_without_raw_text() {
        let dir = tempfile::tempdir().unwrap();
        let log = temp_log(dir.path(), "a.log", 3);
        let service = Service::new(ServiceConfig::default());
        let p = service
            .preview(&PreviewRequest {
                path: log,
                profile: preset("combined"),
                max_lines: 10,
            })
            .unwrap();
        assert_eq!(p.records, 3);
        let json = serde_json::to_string(&p).unwrap();
        assert!(
            !json.contains("13:55:36 -0700"),
            "raw timestamp text must not appear in preview payload"
        );
    }
}
