//! Tauri 명령 계층: 입력 검증, 서비스 호출, 응답 변환. 인자 이름은 snake_case로 고정한다.

use std::path::PathBuf;
use std::sync::Arc;

use weblog_engine::export::ExportRequest;
use weblog_engine::format::FormatProfile;
use weblog_engine::preview::PreviewResult;
use weblog_engine::store::{LogFilter, LogPage, PageRequest};
use weblog_engine::store::{SavedView, StatsRequest, StatsResult, ViewDefinition};
use weblog_service::{
    CaseInfo, DetailView, ImportFinishedView, ImportProgressView, JobView, PresetView,
    PreviewRequest, ProfileListView, ProfileView, ProjectInfo, SampleLines, ScanRequest,
    ScanResponse, Service, ServiceError, ServiceResult, StartImportRequest, ValidationView,
};
use weblog_service::{ExportFinishedView, ExportProgressView};

use crate::ServiceState;

fn owned(state: &ServiceState<'_>) -> Arc<Service> {
    Arc::clone(state.inner())
}

/// 오래 걸리는 동기 작업을 블로킹 풀에서 실행한다.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> ServiceResult<T> + Send + 'static,
) -> ServiceResult<T> {
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| ServiceError::Invalid(format!("작업 스레드 오류: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn open_project(state: ServiceState<'_>, db_path: String) -> ServiceResult<ProjectInfo> {
    if db_path.trim().is_empty() {
        return Err(ServiceError::Invalid("DB 경로가 비었음".to_owned()));
    }
    let s = owned(&state);
    blocking(move || s.open_project(PathBuf::from(db_path))).await
}

/// cases/ 아래에 새 케이스 DB를 만들어 연다. 이름 힌트는 파일 이름에만 쓴다.
#[tauri::command(rename_all = "snake_case")]
pub async fn create_case(state: ServiceState<'_>, name_hint: String) -> ServiceResult<ProjectInfo> {
    let s = owned(&state);
    blocking(move || s.create_case(&name_hint)).await
}

/// cases/ 안의 케이스 DB 목록.
#[tauri::command(rename_all = "snake_case")]
pub async fn list_cases(state: ServiceState<'_>) -> ServiceResult<Vec<CaseInfo>> {
    let s = owned(&state);
    blocking(move || s.list_cases()).await
}

/// 케이스 DB 파일 삭제. cases/ 바로 아래 파일만.
#[tauri::command(rename_all = "snake_case")]
pub async fn delete_case(state: ServiceState<'_>, path: String) -> ServiceResult<()> {
    if path.trim().is_empty() {
        return Err(ServiceError::Invalid("케이스 경로가 비었음".to_owned()));
    }
    let s = owned(&state);
    blocking(move || s.delete_case(&PathBuf::from(path))).await
}

#[tauri::command(rename_all = "snake_case")]
pub fn close_project(state: ServiceState<'_>) -> ServiceResult<()> {
    state.close_project()
}

#[tauri::command(rename_all = "snake_case")]
pub fn current_project(state: ServiceState<'_>) -> ServiceResult<Option<ProjectInfo>> {
    state.current_project()
}

#[tauri::command(rename_all = "snake_case")]
pub fn list_presets(state: ServiceState<'_>) -> Vec<PresetView> {
    state.presets()
}

#[tauri::command(rename_all = "snake_case")]
pub fn list_profiles(state: ServiceState<'_>) -> ServiceResult<ProfileListView> {
    state.list_profiles()
}

#[tauri::command(rename_all = "snake_case")]
pub fn validate_profile(
    state: ServiceState<'_>,
    profile: FormatProfile,
) -> ServiceResult<ValidationView> {
    state.validate_profile(&profile)
}

#[tauri::command(rename_all = "snake_case")]
pub fn profile_from_yaml(state: ServiceState<'_>, yaml: String) -> ServiceResult<FormatProfile> {
    state.profile_from_yaml(&yaml)
}

#[tauri::command(rename_all = "snake_case")]
pub fn profile_to_yaml(state: ServiceState<'_>, profile: FormatProfile) -> ServiceResult<String> {
    state.profile_to_yaml(&profile)
}

#[tauri::command(rename_all = "snake_case")]
pub fn save_profile(state: ServiceState<'_>, profile: FormatProfile) -> ServiceResult<ProfileView> {
    state.save_profile(&profile)
}

#[tauri::command(rename_all = "snake_case")]
pub fn delete_profile(state: ServiceState<'_>, name: String) -> ServiceResult<bool> {
    state.delete_profile(&name)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn scan_files(
    state: ServiceState<'_>,
    request: ScanRequest,
) -> ServiceResult<ScanResponse> {
    if request.root.as_os_str().is_empty() {
        return Err(ServiceError::Invalid("탐색 경로가 비었음".to_owned()));
    }
    let s = owned(&state);
    blocking(move || s.scan(&request)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn preview_format(
    state: ServiceState<'_>,
    request: PreviewRequest,
) -> ServiceResult<PreviewResult> {
    let s = owned(&state);
    blocking(move || s.preview(&request)).await
}

/// 파일 선두 원문 샘플. 포맷 확인 화면 전용이며 저장하지 않는다.
#[tauri::command(rename_all = "snake_case")]
pub async fn sample_lines(
    state: ServiceState<'_>,
    path: PathBuf,
    max_lines: u64,
) -> ServiceResult<SampleLines> {
    if path.as_os_str().is_empty() {
        return Err(ServiceError::Invalid("파일 경로가 비었음".to_owned()));
    }
    let s = owned(&state);
    blocking(move || s.sample_lines(&path, max_lines)).await
}

#[tauri::command(rename_all = "snake_case")]
pub fn start_import(state: ServiceState<'_>, request: StartImportRequest) -> ServiceResult<i64> {
    state.inner().start_import(request)
}

#[tauri::command(rename_all = "snake_case")]
pub fn resume_job(state: ServiceState<'_>, job_id: i64, full_verify: bool) -> ServiceResult<i64> {
    state.inner().resume_job(job_id, full_verify)
}

#[tauri::command(rename_all = "snake_case")]
pub fn cancel_import(state: ServiceState<'_>) -> ServiceResult<()> {
    state.cancel_import()
}

/// 진행 스냅샷. `finished`가 있으면 그 가져오기는 끝났다.
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportStatusView {
    progress: ImportProgressView,
    finished: Option<ImportFinishedView>,
}

#[tauri::command(rename_all = "snake_case")]
pub fn import_status(state: ServiceState<'_>) -> ServiceResult<Option<ImportStatusView>> {
    Ok(state
        .import_status()?
        .map(|(progress, finished)| ImportStatusView { progress, finished }))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_jobs(state: ServiceState<'_>) -> ServiceResult<Vec<JobView>> {
    let s = owned(&state);
    blocking(move || s.list_jobs()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn activate_job(
    state: ServiceState<'_>,
    job_id: i64,
) -> ServiceResult<weblog_engine::store::JobInfo> {
    let s = owned(&state);
    blocking(move || s.activate_job(job_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_job_results(state: ServiceState<'_>, job_id: i64) -> ServiceResult<u64> {
    let s = owned(&state);
    blocking(move || s.delete_job_results(job_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn verify_source(
    state: ServiceState<'_>,
    source_id: i64,
    full: bool,
    relink: Option<String>,
) -> ServiceResult<weblog_engine::store::SourceVerification> {
    let s = owned(&state);
    blocking(move || s.verify_source(source_id, full, relink.map(PathBuf::from))).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn query_page(state: ServiceState<'_>, request: PageRequest) -> ServiceResult<LogPage> {
    let s = owned(&state);
    blocking(move || s.query_page(&request)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn count_logs(state: ServiceState<'_>, filter: LogFilter) -> ServiceResult<i64> {
    let s = owned(&state);
    blocking(move || s.count(&filter)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn status_histogram(
    state: ServiceState<'_>,
    filter: LogFilter,
) -> ServiceResult<Vec<(Option<i32>, i64)>> {
    let s = owned(&state);
    blocking(move || s.status_histogram(&filter)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn log_detail(
    state: ServiceState<'_>,
    job_id: Option<i64>,
    source_id: i64,
    line_number: i64,
) -> ServiceResult<Option<DetailView>> {
    let s = owned(&state);
    blocking(move || s.detail(job_id, source_id, line_number)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn compute_stats(
    state: ServiceState<'_>,
    request: StatsRequest,
) -> ServiceResult<StatsResult> {
    let s = owned(&state);
    blocking(move || s.stats(&request)).await
}

/// 실행 중인 건수·통계 조회를 중단한다.
#[tauri::command(rename_all = "snake_case")]
pub fn cancel_heavy(state: ServiceState<'_>) {
    state.cancel_heavy();
}

#[tauri::command(rename_all = "snake_case")]
pub async fn list_views(state: ServiceState<'_>) -> ServiceResult<Vec<SavedView>> {
    let s = owned(&state);
    blocking(move || s.list_views()).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn save_view(
    state: ServiceState<'_>,
    name: String,
    definition: ViewDefinition,
) -> ServiceResult<SavedView> {
    let s = owned(&state);
    blocking(move || s.save_view(&name, &definition)).await
}

#[tauri::command(rename_all = "snake_case")]
pub async fn delete_view(state: ServiceState<'_>, view_id: i64) -> ServiceResult<bool> {
    let s = owned(&state);
    blocking(move || s.delete_view(view_id)).await
}

#[tauri::command(rename_all = "snake_case")]
pub fn start_export(state: ServiceState<'_>, request: ExportRequest) -> ServiceResult<()> {
    state.inner().start_export(request)
}

#[tauri::command(rename_all = "snake_case")]
pub fn cancel_export(state: ServiceState<'_>) -> ServiceResult<()> {
    state.cancel_export()
}

/// 내보내기 상태. `finished`가 있으면 끝났다.
#[derive(serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ExportStatusView {
    progress: ExportProgressView,
    finished: Option<ExportFinishedView>,
}

#[tauri::command(rename_all = "snake_case")]
pub fn export_status(state: ServiceState<'_>) -> ServiceResult<Option<ExportStatusView>> {
    Ok(state
        .export_status()?
        .map(|(progress, finished)| ExportStatusView { progress, finished }))
}
