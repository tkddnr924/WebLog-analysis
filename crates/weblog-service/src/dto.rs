//! IPC 경계용 자료형. 필드 이름은 모두 snake_case로 명시하며 자동 변환을 가정하지 않는다.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use weblog_engine::format::FormatProfile;
use weblog_engine::importer::ImportSummary;
use weblog_engine::store::{JobInfo, JobSource, LogDetail, LogKind};

/// 열린 프로젝트 정보.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProjectInfo {
    /// DB 파일 경로.
    pub db_path: String,
    /// DB 파일 크기.
    pub db_file_bytes: u64,
    /// 열 때 interrupted로 표시된 작업.
    pub interrupted_jobs: Vec<i64>,
    /// 작업 목록(최신순).
    pub jobs: Vec<JobInfo>,
}

/// cases/ 안의 케이스 DB 파일 하나.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct CaseInfo {
    /// DB 파일 경로.
    pub path: String,
    /// 파일 이름(확장자 제외).
    pub name: String,
    /// DB 파일 크기(WAL 제외).
    pub bytes: u64,
    /// 마지막 수정 시각(유닉스 초). 알 수 없으면 None.
    pub modified_unix: Option<i64>,
    /// 지금 열려 있는 프로젝트인지.
    pub open: bool,
}

/// 파일 탐색 요청.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ScanRequest {
    /// 루트 경로.
    pub root: PathBuf,
    /// 재귀.
    #[serde(default = "default_true")]
    pub recursive: bool,
    /// 최대 깊이.
    #[serde(default)]
    pub max_depth: Option<usize>,
    /// 포함 패턴.
    #[serde(default)]
    pub include: Vec<String>,
    /// 제외 패턴.
    #[serde(default)]
    pub exclude: Vec<String>,
    /// 파일별 포맷 판별 수행.
    #[serde(default = "default_true")]
    pub detect: bool,
    /// 판별 샘플 줄 수.
    #[serde(default = "default_sample_lines")]
    pub sample_lines: u64,
}

fn default_true() -> bool {
    true
}

fn default_sample_lines() -> u64 {
    200
}

/// 탐색 결과의 파일 한 줄. 최고 후보와 매칭률을 함께 담는다.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScannedFile {
    /// 경로.
    pub path: String,
    /// 크기.
    pub file_size: u64,
    /// 수정 시각(Unix 초).
    pub modified_unix: Option<i64>,
    /// 압축 방식 문자열.
    pub compression: Option<String>,
    /// 최고 후보 프로필 이름.
    pub best_profile: Option<String>,
    /// 최고 후보 정의 해시.
    pub best_hash: Option<String>,
    /// 샘플 매칭률(서버 식별 확률이 아님).
    pub match_rate: Option<f64>,
    /// 샘플에서 검사한 줄 수.
    pub lines_checked: Option<u64>,
    /// 샘플 레코드 수.
    pub sample_records: Option<u64>,
    /// 샘플 오류 수.
    pub sample_errors: Option<u64>,
    /// 판별 실패 사유.
    pub detect_error: Option<String>,
}

/// 탐색 응답.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScanResponse {
    /// 파일 목록.
    pub files: Vec<ScannedFile>,
    /// 항목별 오류.
    pub errors: Vec<ScanIssue>,
    /// 항목 수 상한으로 잘렸는지.
    pub truncated: bool,
    /// 오류 목록이 상한으로 잘렸는지.
    pub errors_truncated: bool,
    /// 살펴본 디렉터리 수.
    pub directories_visited: usize,
    /// 패턴에 걸러진 파일 수.
    pub filtered_out: usize,
}

/// 탐색 오류 항목.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ScanIssue {
    /// 경로.
    pub path: String,
    /// 메시지.
    pub message: String,
}

/// 프로필 지정: 프리셋 이름 또는 정의 JSON.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum ProfileSpec {
    /// 프리셋 이름.
    Preset {
        /// 이름.
        name: String,
    },
    /// 정의 전체.
    Definition {
        /// 프로필.
        profile: FormatProfile,
    },
}

/// 미리보기 요청.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PreviewRequest {
    /// 파일.
    pub path: PathBuf,
    /// 프로필.
    pub profile: ProfileSpec,
    /// 최대 줄 수.
    #[serde(default = "default_sample_lines")]
    pub max_lines: u64,
}

/// 파일 선두 샘플 줄 하나. 포맷 확인 화면에서만 잠시 보여주며 저장하지 않는다.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SampleLine {
    /// 논리 줄 번호(1부터).
    pub line_number: u64,
    /// 텍스트. UTF-8이 아니거나 줄 길이 상한을 넘으면 None.
    pub text: Option<String>,
}

/// 파일 선두 샘플.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct SampleLines {
    /// 읽은 줄.
    pub lines: Vec<SampleLine>,
    /// 줄 수·바이트 한도에 걸려 중단했는지.
    pub truncated: bool,
}

/// 가져오기 시작 요청.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StartImportRequest {
    /// 프로필.
    pub profile: ProfileSpec,
    /// 파일 목록.
    pub paths: Vec<PathBuf>,
    /// 대체할 이전 작업(재파싱).
    #[serde(default)]
    pub replaces_job_id: Option<i64>,
    /// 로그 종류(접근·에러). 지정하지 않으면 접근 로그.
    #[serde(default)]
    pub log_kind: LogKind,
    /// 배치 최대 행 수.
    #[serde(default)]
    pub batch_max_rows: Option<usize>,
    /// 배치 최대 바이트.
    #[serde(default)]
    pub batch_max_bytes: Option<usize>,
}

/// 진행 중 가져오기 스냅샷.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportProgressView {
    /// 작업 ID.
    pub job_id: i64,
    /// 현재 파일 ID.
    pub source_id: i64,
    /// 현재 파일에서 읽은 줄.
    pub lines_read: u64,
    /// 이번 실행에서 확정한 레코드.
    pub committed_records: u64,
    /// 이번 실행에서 확정한 배치.
    pub committed_batches: u64,
    /// 경과 초.
    pub elapsed_secs: f64,
    /// 취소 요청 여부.
    pub cancel_requested: bool,
    /// 재개 실행인지.
    pub resumed: bool,
}

/// 가져오기 종료 통지.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ImportFinishedView {
    /// 작업 ID.
    pub job_id: i64,
    /// 최종 상태 문자열.
    pub status: String,
    /// 요약(성공 시).
    pub summary: Option<ImportSummary>,
    /// 오류 메시지(실패 시, 입력 내용 없음).
    pub error: Option<String>,
}

/// 작업과 파일 목록.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct JobView {
    /// 작업.
    pub job: JobInfo,
    /// 파일.
    pub sources: Vec<JobSource>,
}

/// 상세 레코드 + 재구성 로그.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct DetailView {
    /// 저장 필드.
    pub detail: LogDetail,
    /// 재구성 로그 텍스트. 원문이 아니다.
    pub reconstructed: String,
    /// 항상 true.
    pub is_reconstruction: bool,
    /// 재구성에 쓴 템플릿(`blocks` 또는 `standard`).
    pub template: String,
    /// 역변환 불가능한 부분이 없었는지.
    pub complete: bool,
    /// 재구성에 쓴 프로필 이름과 버전.
    pub profile_name: String,
    /// 프로필 버전.
    pub profile_version: u32,
    /// 확장 필드(파싱된 JSON).
    pub extra: Vec<(String, String)>,
}

/// 프리셋 목록 항목.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct PresetView {
    /// 이름.
    pub name: String,
    /// 정의.
    pub profile: FormatProfile,
}

/// 프로필 목록 항목(내장 + 사용자).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileView {
    /// 이름.
    pub name: String,
    /// `builtin` 또는 `user`.
    pub source: String,
    /// 정의.
    pub profile: FormatProfile,
    /// YAML 표현.
    pub yaml: String,
    /// 사용자 프로필의 파일 경로.
    pub path: Option<String>,
}

/// 프로필 목록 응답.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProfileListView {
    /// 프로필.
    pub profiles: Vec<ProfileView>,
    /// 읽지 못한 사용자 파일.
    pub errors: Vec<ScanIssue>,
    /// 사용자 프리셋 디렉터리.
    pub user_dir: Option<String>,
}

/// 검증 응답.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ValidationView {
    /// 문제 목록. 비어 있으면 유효.
    pub issues: Vec<weblog_engine::format::ValidationIssue>,
    /// 유효한 경우 정의 해시.
    pub definition_hash: Option<String>,
    /// 유효한 경우 정규화된 YAML.
    pub yaml: Option<String>,
}

/// 내보내기 진행 스냅샷.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ExportProgressView {
    /// 출력 파일.
    pub out_path: String,
    /// 쓴 행 수.
    pub rows: u64,
    /// 쓴 바이트.
    pub bytes: u64,
    /// 경과 초.
    pub elapsed_secs: f64,
    /// 취소 요청 여부.
    pub cancel_requested: bool,
}

/// 내보내기 종료 통지.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ExportFinishedView {
    /// 출력 파일.
    pub out_path: String,
    /// 요약(성공·취소 시).
    pub summary: Option<weblog_engine::export::ExportSummary>,
    /// 오류(실패 시).
    pub error: Option<String>,
}
