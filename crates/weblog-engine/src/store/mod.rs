//! DuckDB 저장소 계층. 모든 DB 접근은 여기에 모은다. UI에 연결이나 임의 SQL을 노출하지 않는다.

pub mod batch;
pub mod query;
pub mod schema;
pub mod stats;
pub mod views;

use std::path::{Path, PathBuf};

use duckdb::{params, Connection, OptionalExt};
use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};
use crate::format::FormatProfile;
use crate::source::{Compression, SourceIdentity};

pub use batch::{BatchCheckpoint, CommitOutcome, PendingBatch};
/// 실행 중 쿼리 중단 핸들(DuckDB). 서비스 계층이 무거운 조회 취소에 쓴다.
pub use duckdb::InterruptHandle;
pub use query::{
    CondField, CondOp, ExportRow, FilterExpr, LogDetail, LogFilter, LogPage, LogQuery, LogRow,
    PageCursor, PageRequest, Reader, SortOrder,
};
pub use stats::{IpRow, StatsRequest, StatsResult, TimeBucket};
pub use views::{SavedView, ViewDefinition, ViewQuery};

/// 저장소 설정. DuckDB memory_limit는 프로세스 전체 상한이 아니다.
#[derive(Debug, Clone, Default)]
pub struct StoreConfig {
    /// DuckDB memory_limit(예: `4GB`).
    pub memory_limit: Option<String>,
    /// DuckDB threads.
    pub threads: Option<u32>,
    /// 쿼리 임시 디렉터리.
    pub temp_directory: Option<PathBuf>,
    /// 임시 디렉터리 최대 크기(예: `20GB`).
    pub max_temp_directory_size: Option<String>,
}

/// 작업 상태.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobStatus {
    /// 대기.
    Queued,
    /// 실행 중.
    Running,
    /// 오류 없이 완료.
    Completed,
    /// 파싱 오류를 포함해 완료.
    CompletedWithErrors,
    /// 취소 요청을 받아 현재 배치 이후 중단하는 중.
    Cancelling,
    /// 취소됨(확정 배치 유지).
    Cancelled,
    /// 실패(확정 배치 유지).
    Failed,
    /// 비정상 종료 후 복구 대기.
    Interrupted,
}

impl JobStatus {
    /// 저장용 문자열.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::CompletedWithErrors => "completed_with_errors",
            Self::Cancelling => "cancelling",
            Self::Cancelled => "cancelled",
            Self::Failed => "failed",
            Self::Interrupted => "interrupted",
        }
    }

    /// 문자열에서 복원한다.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "completed" => Self::Completed,
            "completed_with_errors" => Self::CompletedWithErrors,
            "cancelling" => Self::Cancelling,
            "cancelled" => Self::Cancelled,
            "failed" => Self::Failed,
            "interrupted" => Self::Interrupted,
            _ => return None,
        })
    }

    /// 재개할 수 있는 상태인지. 확정 배치가 보존된 중단 상태만 해당한다.
    pub fn is_resumable(self) -> bool {
        matches!(self, Self::Interrupted | Self::Cancelled | Self::Failed)
    }
}

/// 로그 종류. 접근 로그와 에러 로그는 컬럼 구성이 달라 화면에서 나눠 본다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LogKind {
    /// 접근 로그.
    #[default]
    Access,
    /// 에러 로그.
    Error,
}

impl LogKind {
    /// 저장용 문자열.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Access => "access",
            Self::Error => "error",
        }
    }

    /// 문자열에서 복원한다. 값이 없거나 모르는 값이면 접근 로그로 본다.
    pub fn parse(s: &str) -> Self {
        if s == "error" {
            Self::Error
        } else {
            Self::Access
        }
    }
}

/// 생성된 작업 식별자.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobHandle {
    /// 작업 ID.
    pub job_id: i64,
    /// 결과 버전.
    pub result_version: i64,
}

/// 작업 요약(집계 컬럼은 커밋된 값이다).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobInfo {
    /// 작업 ID.
    pub job_id: i64,
    /// 결과 버전.
    pub result_version: i64,
    /// 상태.
    pub status: JobStatus,
    /// 프로필 ID.
    pub profile_id: i64,
    /// 로그 종류.
    pub log_kind: LogKind,
    /// 확정 레코드 수.
    pub committed_records: i64,
    /// 확정 오류 수.
    pub committed_errors: i64,
    /// 확정 제외 수.
    pub committed_skipped: i64,
    /// 조회 대상 활성 결과인지.
    pub active: bool,
    /// 재파싱으로 대체하려는 이전 작업.
    pub replaces_job_id: Option<i64>,
    /// 실패 사유(입력 내용 없음).
    pub failure_reason: Option<String>,
}

/// 작업에 포함된 파일.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobSource {
    /// 파일 ID.
    pub source_id: i64,
    /// 처리 순서.
    pub ordinal: i64,
    /// pending / running / done / cancelled / failed.
    pub status: String,
    /// 현재 연결 경로.
    pub path: String,
}

/// 파일 검증 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceVerification {
    /// 파일 ID.
    pub source_id: i64,
    /// 경로.
    pub path: String,
    /// 저장된 식별 정보와 일치하는지.
    pub matches: bool,
    /// 불일치 사유.
    pub reason: Option<String>,
    /// 전체 해시까지 비교했는지.
    pub full_checked: bool,
}

/// 저장소. 단일 소유 프로세스에서 하나의 쓰기 연결을 가진다.
pub struct Store {
    conn: Connection,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Store {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Store")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl Store {
    /// 파일 저장소를 열고 마이그레이션을 적용한다. 열 때 running 상태로 남은 작업은 interrupted로 바꾼다.
    pub fn open(path: &Path, config: &StoreConfig) -> EngineResult<Self> {
        let conn = Connection::open(path)?;
        let mut store = Self {
            conn,
            path: Some(path.to_path_buf()),
        };
        store.configure(config)?;
        schema::migrate(&store.conn)?;
        // Raw log text is never stored; refuse a database that carries such a column.
        schema::assert_no_raw_columns(&store.conn)?;
        store.mark_interrupted_jobs()?;
        // DDL이 WAL에 남은 채 비정상 종료되면 다음 열기에서 재생에 실패할 수 있다. 열자마자 체크포인트로 비운다.
        store.checkpoint()?;
        Ok(store)
    }

    /// WAL을 데이터 파일에 반영한다. 다른 트랜잭션이 열려 있으면 DuckDB가 거부하므로 호출자가 실패를 무시할 수 있다.
    pub fn checkpoint(&self) -> EngineResult<()> {
        self.conn.execute_batch("CHECKPOINT")?;
        Ok(())
    }

    /// 메모리 저장소(테스트·실험용).
    pub fn open_in_memory(config: &StoreConfig) -> EngineResult<Self> {
        let conn = Connection::open_in_memory()?;
        let mut store = Self { conn, path: None };
        store.configure(config)?;
        schema::migrate(&store.conn)?;
        schema::assert_no_raw_columns(&store.conn)?;
        Ok(store)
    }

    fn configure(&mut self, config: &StoreConfig) -> EngineResult<()> {
        if let Some(limit) = &config.memory_limit {
            validate_setting(limit)?;
            self.conn
                .execute_batch(&format!("SET memory_limit = '{limit}'"))?;
        }
        if let Some(threads) = config.threads {
            self.conn
                .execute_batch(&format!("SET threads = {threads}"))?;
        }
        if let Some(dir) = &config.temp_directory {
            let dir = dir.to_string_lossy().replace('\'', "''");
            self.conn
                .execute_batch(&format!("SET temp_directory = '{dir}'"))?;
        }
        if let Some(size) = &config.max_temp_directory_size {
            validate_setting(size)?;
            self.conn
                .execute_batch(&format!("SET max_temp_directory_size = '{size}'"))?;
        }
        Ok(())
    }

    /// 내부 연결. crate 안의 조회 모듈만 사용한다.
    pub(crate) fn conn(&self) -> &Connection {
        &self.conn
    }

    /// 통합 테스트·CLI 통계용 읽기 접근. 앱 UI 계층에는 노출하지 않는다.
    #[doc(hidden)]
    pub fn conn_for_tests(&self) -> &Connection {
        &self.conn
    }

    /// 같은 DB에 대한 읽기 전용 연결. 가져오기와 조회를 다른 스레드에서 동시에 수행할 때 쓴다.
    pub fn open_reader(&self) -> EngineResult<Reader> {
        Ok(Reader::new(self.conn.try_clone()?))
    }

    // ----- 프로필 -----

    /// 파서 프로필을 등록한다. 같은 정의 해시가 있으면 기존 ID를 돌려준다.
    pub fn upsert_profile(&self, profile: &FormatProfile) -> EngineResult<i64> {
        let hash = profile.definition_hash()?;
        if let Some(id) = self
            .conn
            .query_row(
                "SELECT profile_id FROM parser_profiles WHERE definition_hash = ?",
                params![hash],
                |r| r.get::<_, i64>(0),
            )
            .optional()?
        {
            return Ok(id);
        }
        let id = self.next_id("parser_profiles", "profile_id")?;
        self.conn.execute(
            "INSERT INTO parser_profiles (profile_id, name, version, definition_json, definition_hash) VALUES (?, ?, ?, ?, ?)",
            params![id, profile.name, i64::from(profile.version), profile.to_json()?, hash],
        )?;
        Ok(id)
    }

    // ----- 파일 -----

    /// 입력 파일을 등록한다. 같은 경로·같은 내용 식별이면 기존 ID를 재사용한다.
    pub fn register_source(&self, path: &Path, identity: &SourceIdentity) -> EngineResult<i64> {
        let path_str = path.to_string_lossy().into_owned();
        if let Some((id, stored)) = self.latest_source_at(&path_str)? {
            if stored.matches(identity) {
                if identity.full_hash.is_some() && stored.full_hash.is_none() {
                    self.conn.execute(
                        "UPDATE sources SET full_hash = ? WHERE source_id = ?",
                        params![identity.full_hash, id],
                    )?;
                }
                return Ok(id);
            }
        }
        let id = self.next_id("sources", "source_id")?;
        self.conn.execute(
            "INSERT INTO sources (source_id, original_path, current_path, file_size, modified_unix, encoding, compression, head_hash, head_bytes, full_hash) VALUES (?, ?, ?, ?, ?, 'utf-8', ?, ?, ?, ?)",
            params![
                id,
                path_str,
                path_str,
                i64::try_from(identity.file_size).unwrap_or(i64::MAX),
                identity.modified_unix,
                identity.compression.as_str(),
                identity.head_hash,
                i64::try_from(identity.head_bytes).unwrap_or(0),
                identity.full_hash,
            ],
        )?;
        Ok(id)
    }

    fn latest_source_at(&self, path: &str) -> EngineResult<Option<(i64, SourceIdentity)>> {
        let row = self
            .conn
            .query_row(
                "SELECT source_id, file_size, modified_unix, compression, head_hash, head_bytes, full_hash FROM sources WHERE current_path = ? ORDER BY source_id DESC LIMIT 1",
                params![path],
                map_source_identity,
            )
            .optional()?;
        Ok(row)
    }

    /// 파일 연결 경로를 바꾼다(이동된 원본 재연결). 내용 검증은 호출자가 [`Store::verify_source`]로 수행한다.
    pub fn relink_source(&self, source_id: i64, new_path: &Path) -> EngineResult<()> {
        let changed = self.conn.execute(
            "UPDATE sources SET current_path = ? WHERE source_id = ?",
            params![new_path.to_string_lossy().into_owned(), source_id],
        )?;
        if changed == 0 {
            return Err(EngineError::Job(format!(
                "파일 {source_id}이 등록되지 않음"
            )));
        }
        Ok(())
    }

    /// 현재 파일이 등록 당시와 같은지 검증한다. `full`이면 전체 해시를 스트리밍으로 계산해 비교하고,
    /// 저장된 전체 해시가 없으면 이번 값을 기록한다.
    pub fn verify_source(&self, source_id: i64, full: bool) -> EngineResult<SourceVerification> {
        let (path, stored) = self.source(source_id)?;
        let p = Path::new(&path);
        let current = if full {
            SourceIdentity::read_full(p)?
        } else {
            SourceIdentity::read(p)?
        };
        let reason = stored.mismatch_reason(&current);
        let matches = reason.is_none();
        if matches && full && stored.full_hash.is_none() {
            self.conn.execute(
                "UPDATE sources SET full_hash = ? WHERE source_id = ?",
                params![current.full_hash, source_id],
            )?;
        }
        Ok(SourceVerification {
            source_id,
            path,
            matches,
            reason,
            full_checked: full && stored.full_hash.is_some(),
        })
    }

    // ----- 작업 -----

    /// 작업을 만든다. 상태는 running으로 시작한다. `replaces_job_id`가 있으면 재파싱이며 비활성으로 시작한다.
    pub fn create_job(
        &self,
        profile_id: i64,
        source_ids: &[i64],
        replaces_job_id: Option<i64>,
        log_kind: LogKind,
    ) -> EngineResult<JobHandle> {
        if let Some(prev) = replaces_job_id {
            self.job(prev)?;
        }
        let job_id = self.next_id("import_jobs", "job_id")?;
        let result_version = self.next_id("import_jobs", "result_version")?;
        self.conn.execute(
            "INSERT INTO import_jobs (job_id, result_version, status, profile_id, started_at, active, replaces_job_id, log_kind) VALUES (?, ?, ?, ?, current_timestamp, ?, ?, ?)",
            params![
                job_id,
                result_version,
                JobStatus::Running.as_str(),
                profile_id,
                replaces_job_id.is_none(),
                replaces_job_id,
                log_kind.as_str()
            ],
        )?;
        for (ordinal, source_id) in source_ids.iter().enumerate() {
            self.conn.execute(
                "INSERT INTO import_job_sources (job_id, source_id, ordinal, status) VALUES (?, ?, ?, 'pending')",
                params![job_id, source_id, i64::try_from(ordinal).unwrap_or(i64::MAX)],
            )?;
        }
        Ok(JobHandle {
            job_id,
            result_version,
        })
    }

    /// 작업 내 파일 상태를 갱신한다.
    pub fn set_job_source_status(
        &self,
        job_id: i64,
        source_id: i64,
        status: &str,
    ) -> EngineResult<()> {
        self.conn.execute(
            "UPDATE import_job_sources SET status = ? WHERE job_id = ? AND source_id = ?",
            params![status, job_id, source_id],
        )?;
        Ok(())
    }

    /// 작업 상태만 바꾼다(cancelling, running 재개 등).
    pub fn set_job_status(&self, job_id: i64, status: JobStatus) -> EngineResult<()> {
        let changed = self.conn.execute(
            "UPDATE import_jobs SET status = ?, finished_at = NULL, failure_reason = NULL WHERE job_id = ?",
            params![status.as_str(), job_id],
        )?;
        if changed == 0 {
            return Err(EngineError::Job(format!("작업 {job_id}이 없음")));
        }
        Ok(())
    }

    /// 작업을 종료 상태로 바꾼다.
    pub fn finish_job(
        &self,
        job_id: i64,
        status: JobStatus,
        failure_reason: Option<&str>,
    ) -> EngineResult<()> {
        let changed = self.conn.execute(
            "UPDATE import_jobs SET status = ?, finished_at = current_timestamp, failure_reason = ? WHERE job_id = ?",
            params![status.as_str(), failure_reason, job_id],
        )?;
        if changed == 0 {
            return Err(EngineError::Job(format!("작업 {job_id}이 없음")));
        }
        Ok(())
    }

    /// running/cancelling 상태로 남은 작업을 interrupted로 바꾼다. 단일 소유 프로세스이므로
    /// 열 때 그런 작업이 있다는 것은 이전 실행이 비정상 종료했다는 뜻이다.
    pub fn mark_interrupted_jobs(&self) -> EngineResult<Vec<i64>> {
        let ids: Vec<i64> = self
            .conn
            .prepare("SELECT job_id FROM import_jobs WHERE status IN ('running', 'cancelling') ORDER BY job_id")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for id in &ids {
            self.conn.execute(
                "UPDATE import_jobs SET status = ? WHERE job_id = ?",
                params![JobStatus::Interrupted.as_str(), id],
            )?;
            self.conn.execute(
                "UPDATE import_job_sources SET status = 'pending' WHERE job_id = ? AND status = 'running'",
                params![id],
            )?;
        }
        Ok(ids)
    }

    /// 가장 최근 작업 ID.
    pub fn latest_job_id(&self) -> EngineResult<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT MAX(job_id) FROM import_jobs", [], |r| {
                r.get::<_, Option<i64>>(0)
            })?)
    }

    /// 완료된 재파싱 결과를 활성화하고, 대체 대상 작업을 비활성화한다. 명시적 전환만 허용한다.
    pub fn activate_job(&self, job_id: i64) -> EngineResult<()> {
        let info = self.job(job_id)?;
        if !matches!(
            info.status,
            JobStatus::Completed | JobStatus::CompletedWithErrors
        ) {
            return Err(EngineError::Job(format!(
                "완료되지 않은 작업 {job_id}({})은 활성화할 수 없음",
                info.status.as_str()
            )));
        }
        self.conn.execute_batch("BEGIN")?;
        let result = (|| -> EngineResult<()> {
            self.conn.execute(
                "UPDATE import_jobs SET active = TRUE WHERE job_id = ?",
                params![job_id],
            )?;
            if let Some(prev) = info.replaces_job_id {
                self.conn.execute(
                    "UPDATE import_jobs SET active = FALSE WHERE job_id = ?",
                    params![prev],
                )?;
            }
            Ok(())
        })();
        match result {
            Ok(()) => self.conn.execute_batch("COMMIT")?,
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                return Err(e);
            }
        }
        Ok(())
    }

    /// 작업의 결과(로그·오류·배치·작업 행)를 삭제한다. 명시적 작업이며 활성 작업은 먼저 비활성화해야 한다.
    pub fn delete_job_results(&self, job_id: i64) -> EngineResult<u64> {
        let info = self.job(job_id)?;
        if matches!(info.status, JobStatus::Running | JobStatus::Cancelling) {
            return Err(EngineError::Job(format!(
                "실행 중인 작업 {job_id}은 삭제할 수 없음"
            )));
        }
        if info.active
            && matches!(
                info.status,
                JobStatus::Completed | JobStatus::CompletedWithErrors
            )
        {
            return Err(EngineError::Job(format!(
                "활성 결과 {job_id}은 삭제할 수 없음. 다른 결과를 활성화한 뒤 삭제한다"
            )));
        }
        self.conn.execute_batch("BEGIN")?;
        let result = (|| -> EngineResult<u64> {
            let logs = self
                .conn
                .execute("DELETE FROM logs WHERE job_id = ?", params![job_id])?;
            self.conn
                .execute("DELETE FROM parse_errors WHERE job_id = ?", params![job_id])?;
            self.conn.execute(
                "DELETE FROM import_batches WHERE job_id = ?",
                params![job_id],
            )?;
            self.conn.execute(
                "DELETE FROM import_job_sources WHERE job_id = ?",
                params![job_id],
            )?;
            self.conn
                .execute("DELETE FROM import_jobs WHERE job_id = ?", params![job_id])?;
            Ok(logs as u64)
        })();
        match result {
            Ok(n) => {
                self.conn.execute_batch("COMMIT")?;
                Ok(n)
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    fn next_id(&self, table: &str, column: &str) -> EngineResult<i64> {
        next_id(&self.conn, table, column)
    }
}

/// Next id for a table without a sequence. Table and column come from crate constants only.
pub(crate) fn next_id(conn: &Connection, table: &str, column: &str) -> EngineResult<i64> {
    let v: i64 = conn.query_row(
        &format!("SELECT COALESCE(MAX({column}), 0) + 1 FROM {table}"),
        [],
        |r| r.get(0),
    )?;
    Ok(v)
}

pub(crate) const JOB_SELECT: &str = "SELECT job_id, result_version, status, profile_id, committed_records, committed_errors, committed_skipped, active, replaces_job_id, failure_reason, log_kind FROM import_jobs";

pub(crate) fn map_job(r: &duckdb::Row<'_>) -> duckdb::Result<JobInfo> {
    let status: String = r.get(2)?;
    let kind: Option<String> = r.get(10)?;
    Ok(JobInfo {
        job_id: r.get(0)?,
        result_version: r.get(1)?,
        status: JobStatus::parse(&status).unwrap_or(JobStatus::Failed),
        profile_id: r.get(3)?,
        log_kind: kind.as_deref().map_or(LogKind::Access, LogKind::parse),
        committed_records: r.get(4)?,
        committed_errors: r.get(5)?,
        committed_skipped: r.get(6)?,
        active: r.get::<_, Option<bool>>(7)?.unwrap_or(true),
        replaces_job_id: r.get(8)?,
        failure_reason: r.get(9)?,
    })
}

fn map_source_identity(r: &duckdb::Row<'_>) -> duckdb::Result<(i64, SourceIdentity)> {
    Ok((
        r.get(0)?,
        SourceIdentity {
            file_size: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
            modified_unix: r.get(2)?,
            compression: parse_compression(&r.get::<_, String>(3)?),
            head_hash: r.get(4)?,
            head_bytes: u64::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
            full_hash: r.get(6)?,
        },
    ))
}

pub(crate) fn parse_compression(s: &str) -> Compression {
    if s == "gzip" {
        Compression::Gzip
    } else {
        Compression::None
    }
}

/// SET 값에 따옴표나 세미콜론이 섞이지 않게 한다.
fn validate_setting(value: &str) -> EngineResult<()> {
    if value.is_empty()
        || value
            .chars()
            .any(|c| !(c.is_ascii_alphanumeric() || c == '.' || c == '%'))
    {
        return Err(EngineError::Query(format!(
            "허용되지 않는 설정 값 형식: {value}"
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::format::presets;
    use crate::store::LogQuery;

    fn store() -> Store {
        Store::open_in_memory(&StoreConfig::default()).unwrap()
    }

    fn temp_source(store: &Store, content: &[u8]) -> (tempfile::NamedTempFile, i64) {
        let f = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(f.path(), content).unwrap();
        let id = store
            .register_source(f.path(), &SourceIdentity::read(f.path()).unwrap())
            .unwrap();
        (f, id)
    }

    #[test]
    fn interrupted_marking_only_touches_running_jobs() {
        let s = store();
        let p = s.upsert_profile(&presets::apache_combined()).unwrap();
        let a = s.create_job(p, &[], None, LogKind::Access).unwrap();
        let b = s.create_job(p, &[], None, LogKind::Access).unwrap();
        s.finish_job(b.job_id, JobStatus::Completed, None).unwrap();
        assert_eq!(s.mark_interrupted_jobs().unwrap(), vec![a.job_id]);
        assert_eq!(s.job(a.job_id).unwrap().status, JobStatus::Interrupted);
        assert_eq!(s.job(b.job_id).unwrap().status, JobStatus::Completed);
    }

    #[test]
    fn reparse_job_starts_inactive_and_activation_swaps_with_replaced_job() {
        let s = store();
        let p = s.upsert_profile(&presets::apache_combined()).unwrap();
        let first = s.create_job(p, &[], None, LogKind::Access).unwrap();
        s.finish_job(first.job_id, JobStatus::Completed, None)
            .unwrap();
        let second = s
            .create_job(p, &[], Some(first.job_id), LogKind::Access)
            .unwrap();
        assert!(!s.job(second.job_id).unwrap().active);
        assert!(
            s.activate_job(second.job_id).is_err(),
            "running job cannot be activated"
        );
        s.finish_job(second.job_id, JobStatus::Completed, None)
            .unwrap();
        s.activate_job(second.job_id).unwrap();
        assert!(s.job(second.job_id).unwrap().active);
        assert!(!s.job(first.job_id).unwrap().active);
    }

    #[test]
    fn active_completed_job_cannot_be_deleted_but_inactive_can() {
        let s = store();
        let p = s.upsert_profile(&presets::apache_combined()).unwrap();
        let first = s.create_job(p, &[], None, LogKind::Access).unwrap();
        s.finish_job(first.job_id, JobStatus::Completed, None)
            .unwrap();
        assert!(s.delete_job_results(first.job_id).is_err());
        let second = s
            .create_job(p, &[], Some(first.job_id), LogKind::Access)
            .unwrap();
        s.finish_job(second.job_id, JobStatus::Completed, None)
            .unwrap();
        s.activate_job(second.job_id).unwrap();
        s.delete_job_results(first.job_id).unwrap();
        assert!(s.job(first.job_id).is_err());
        assert_eq!(s.list_jobs().unwrap().len(), 1);
    }

    #[test]
    fn verify_source_detects_change_and_records_full_hash() {
        let s = store();
        let (f, id) = temp_source(&s, b"line one\n");
        let v = s.verify_source(id, true).unwrap();
        assert!(v.matches);
        assert!(
            !v.full_checked,
            "first full verification only records the hash"
        );
        assert!(s.source(id).unwrap().1.full_hash.is_some());
        assert!(s.verify_source(id, true).unwrap().full_checked);
        std::fs::write(f.path(), b"line one changed\n").unwrap();
        let v = s.verify_source(id, false).unwrap();
        assert!(!v.matches);
        assert!(v.reason.unwrap().starts_with("크기"));
    }

    #[test]
    fn relink_then_verify_accepts_moved_file_with_same_content() {
        let s = store();
        let (f, id) = temp_source(&s, b"same\n");
        let dir = tempfile::tempdir().unwrap();
        let moved = dir.path().join("moved.log");
        std::fs::copy(f.path(), &moved).unwrap();
        s.relink_source(id, &moved).unwrap();
        assert!(s.verify_source(id, true).unwrap().matches);
        assert_eq!(s.source(id).unwrap().0, moved.to_string_lossy());
    }

    #[test]
    fn job_sources_lists_files_in_order_with_paths() {
        let s = store();
        let (_a, ida) = temp_source(&s, b"a\n");
        let (_b, idb) = temp_source(&s, b"b\n");
        let p = s.upsert_profile(&presets::apache_combined()).unwrap();
        let job = s.create_job(p, &[idb, ida], None, LogKind::Access).unwrap();
        let list = s.job_sources(job.job_id).unwrap();
        assert_eq!(
            list.iter().map(|j| j.source_id).collect::<Vec<_>>(),
            vec![idb, ida]
        );
        assert_eq!(list[0].status, "pending");
    }

    #[test]
    fn reader_sees_committed_rows_from_a_separate_connection() {
        let mut s = store();
        let p = s.upsert_profile(&presets::apache_combined()).unwrap();
        let (_f, sid) = temp_source(&s, b"x\n");
        let job = s.create_job(p, &[sid], None, LogKind::Access).unwrap();
        let reader = s.open_reader().unwrap();
        let batch = PendingBatch {
            job_id: job.job_id,
            source_id: sid,
            batch_seq: 0,
            records: vec![crate::parse::LogRecord {
                line_number: 1,
                timestamp_utc: Some(5),
                ..Default::default()
            }],
            ..PendingBatch::default()
        };
        s.commit_batch(&batch).unwrap();
        let n = reader
            .count_matching(&LogFilter {
                job_id: Some(job.job_id),
                ..LogFilter::default()
            })
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn open_leaves_no_wal_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cp.duckdb");
        let store = Store::open(&path, &StoreConfig::default()).unwrap();
        let wal = dir.path().join("cp.duckdb.wal");
        let wal_len = std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0);
        assert_eq!(wal_len, 0, "스키마 DDL이 WAL에 남아 있으면 안 됨");
        drop(store);
        Store::open(&path, &StoreConfig::default()).unwrap();
    }

    #[test]
    fn opening_a_store_with_a_raw_column_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("raw.duckdb");
        let store = Store::open(&path, &StoreConfig::default()).unwrap();
        store
            .conn()
            .execute_batch("CREATE TABLE sidecar (raw_line TEXT)")
            .unwrap();
        store.checkpoint().unwrap();
        drop(store);
        let err = Store::open(&path, &StoreConfig::default()).unwrap_err();
        assert!(
            matches!(err, EngineError::Format(_)),
            "원문 저장 의심 컬럼은 열기 단계에서 막아야 함: {err}"
        );
    }
}
