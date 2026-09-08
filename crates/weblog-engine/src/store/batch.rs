//! 배치 커밋. 성공 행·오류 위치·배치 확정·체크포인트를 하나의 트랜잭션으로 확정한다.

use duckdb::types::{TimeUnit, Value};
use duckdb::{params, Connection, OptionalExt};

use super::Store;
use crate::error::{EngineError, EngineResult};
use crate::parse::{LogRecord, ParseErrorCode};

/// 커밋 대기 배치.
#[derive(Debug, Default)]
pub struct PendingBatch {
    /// 작업 ID.
    pub job_id: i64,
    /// 파일 ID.
    pub source_id: i64,
    /// 파일 내 배치 순번(0부터). 재시도 식별자.
    pub batch_seq: i64,
    /// 처리 시작 오프셋.
    pub start_offset: u64,
    /// 다음 읽기 오프셋(체크포인트).
    pub next_offset: u64,
    /// 처리 시작 줄 번호.
    pub start_line: u64,
    /// 처리 끝 줄 번호(포함).
    pub end_line: u64,
    /// 배치 종료 시점의 헤더 상태.
    pub header_state_json: Option<String>,
    /// 성공 레코드.
    pub records: Vec<LogRecord>,
    /// 실패 위치.
    pub errors: Vec<BatchError>,
    /// 제외 건수(빈 줄·지시문).
    pub skipped_count: u64,
    /// 누적 대략 바이트(배치 상한 계산용).
    pub approx_bytes: usize,
}

/// 배치 내 실패 위치.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchError {
    /// 줄 번호.
    pub line_number: u64,
    /// 오류 코드.
    pub code: ParseErrorCode,
    /// 대상 필드 이름.
    pub field: Option<String>,
}

impl PendingBatch {
    /// 처리한 줄 수(성공+실패+제외).
    pub fn processed_lines(&self) -> u64 {
        self.records.len() as u64 + self.errors.len() as u64 + self.skipped_count
    }

    /// 비어 있는지(아무 줄도 처리하지 않았는지).
    pub fn is_empty(&self) -> bool {
        self.processed_lines() == 0
    }
}

/// 커밋 결과.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitOutcome {
    /// 새로 확정됨.
    Committed {
        /// 배치 ID.
        batch_id: i64,
    },
    /// 같은 (job, source, seq)가 이미 확정되어 있어 건너뜀.
    AlreadyCommitted {
        /// 기존 배치 ID.
        batch_id: i64,
    },
}

/// 재개용 체크포인트.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchCheckpoint {
    /// 마지막 확정 배치 ID.
    pub batch_id: i64,
    /// 마지막 확정 배치 순번.
    pub batch_seq: i64,
    /// 다음 읽기 오프셋.
    pub next_offset: u64,
    /// 마지막 처리 줄 번호.
    pub end_line: u64,
    /// 헤더 상태.
    pub header_state_json: Option<String>,
}

impl Store {
    /// 배치를 커밋한다.
    pub fn commit_batch(&mut self, batch: &PendingBatch) -> EngineResult<CommitOutcome> {
        self.commit_batch_with_hook(batch, |_| Ok(()))
    }

    /// COMMIT 직전에 `hook`을 실행한다. 장애 주입 테스트용이며 hook 실패 시 전체를 롤백한다.
    pub fn commit_batch_with_hook(
        &mut self,
        batch: &PendingBatch,
        hook: impl FnOnce(&Connection) -> EngineResult<()>,
    ) -> EngineResult<CommitOutcome> {
        let conn = &self.conn;
        conn.execute_batch("BEGIN")?;
        let result = write_batch(conn, batch).and_then(|outcome| {
            if let CommitOutcome::Committed { .. } = outcome {
                hook(conn)?;
            }
            Ok(outcome)
        });
        match result {
            Ok(outcome) => {
                conn.execute_batch("COMMIT")?;
                Ok(outcome)
            }
            Err(e) => {
                // 롤백 실패는 원래 오류를 가린다. 원래 오류를 우선 보고한다.
                let _ = conn.execute_batch("ROLLBACK");
                Err(e)
            }
        }
    }

    /// 파일의 마지막 확정 체크포인트.
    pub fn last_checkpoint(
        &self,
        job_id: i64,
        source_id: i64,
    ) -> EngineResult<Option<BatchCheckpoint>> {
        Ok(self
            .conn
            .query_row(
                "SELECT batch_id, batch_seq, next_offset, end_line, header_state_json FROM import_batches WHERE job_id = ? AND source_id = ? ORDER BY batch_seq DESC LIMIT 1",
                params![job_id, source_id],
                |r| {
                    Ok(BatchCheckpoint {
                        batch_id: r.get(0)?,
                        batch_seq: r.get(1)?,
                        next_offset: r.get::<_, i64>(2).map(|v| u64::try_from(v).unwrap_or(0))?,
                        end_line: r.get::<_, i64>(3).map(|v| u64::try_from(v).unwrap_or(0))?,
                        header_state_json: r.get(4)?,
                    })
                },
            )
            .optional()?)
    }
}

fn write_batch(conn: &Connection, batch: &PendingBatch) -> EngineResult<CommitOutcome> {
    if let Some(existing) = conn
        .query_row(
            "SELECT batch_id FROM import_batches WHERE job_id = ? AND source_id = ? AND batch_seq = ?",
            params![batch.job_id, batch.source_id, batch.batch_seq],
            |r| r.get::<_, i64>(0),
        )
        .optional()?
    {
        return Ok(CommitOutcome::AlreadyCommitted { batch_id: existing });
    }
    let batch_id: i64 = conn.query_row(
        "SELECT COALESCE(MAX(batch_id), 0) + 1 FROM import_batches",
        [],
        |r| r.get(0),
    )?;

    {
        let mut app = conn.appender("logs")?;
        for rec in &batch.records {
            let extra_json = if rec.extra.is_empty() {
                None
            } else {
                Some(serde_json::to_string(&rec.extra)?)
            };
            let ts = match rec.timestamp_utc {
                Some(us) => Value::Timestamp(TimeUnit::Microsecond, us),
                None => Value::Null,
            };
            app.append_row(params![
                batch.job_id,
                batch.source_id,
                batch_id,
                to_i64(rec.line_number)?,
                ts,
                rec.tz_offset_seconds,
                rec.client_ip,
                rec.method,
                rec.request_target,
                rec.protocol,
                rec.status.map(i32::from),
                rec.bytes_sent,
                rec.referrer,
                rec.user_agent,
                extra_json,
            ])?;
        }
        app.flush()?;
    }
    {
        let mut app = conn.appender("parse_errors")?;
        for err in &batch.errors {
            app.append_row(params![
                batch.job_id,
                batch.source_id,
                batch_id,
                to_i64(err.line_number)?,
                err.code.as_str(),
                err.field,
            ])?;
        }
        app.flush()?;
    }
    conn.execute(
        "INSERT INTO import_batches (batch_id, job_id, source_id, batch_seq, start_offset, next_offset, start_line, end_line, header_state_json, record_count, error_count, skipped_count, committed_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, current_timestamp)",
        params![
            batch_id,
            batch.job_id,
            batch.source_id,
            batch.batch_seq,
            to_i64(batch.start_offset)?,
            to_i64(batch.next_offset)?,
            to_i64(batch.start_line)?,
            to_i64(batch.end_line)?,
            batch.header_state_json,
            to_i32(batch.records.len())?,
            to_i32(batch.errors.len())?,
            to_i32(batch.skipped_count as usize)?,
        ],
    )?;
    conn.execute(
        "UPDATE import_jobs SET committed_records = committed_records + ?, committed_errors = committed_errors + ?, committed_skipped = committed_skipped + ? WHERE job_id = ?",
        params![
            to_i64(batch.records.len() as u64)?,
            to_i64(batch.errors.len() as u64)?,
            to_i64(batch.skipped_count)?,
            batch.job_id,
        ],
    )?;
    Ok(CommitOutcome::Committed { batch_id })
}

fn to_i64(v: u64) -> EngineResult<i64> {
    i64::try_from(v).map_err(|_| EngineError::Limit("값이 i64 범위를 넘음".to_owned()))
}

fn to_i32(v: usize) -> EngineResult<i32> {
    i32::try_from(v).map_err(|_| EngineError::Limit("배치 건수가 i32 범위를 넘음".to_owned()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::store::{LogQuery, StoreConfig};

    fn store_with_job() -> (Store, i64) {
        let store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let profile_id = store
            .upsert_profile(&crate::format::presets::apache_combined())
            .unwrap();
        let job = store.create_job(profile_id, &[1], None).unwrap();
        (store, job.job_id)
    }

    fn record(line: u64, ts: Option<i64>) -> LogRecord {
        LogRecord {
            line_number: line,
            timestamp_utc: ts,
            client_ip: Some("10.0.0.1".to_owned()),
            status: Some(200),
            ..LogRecord::default()
        }
    }

    fn batch(job_id: i64, seq: i64, lines: std::ops::RangeInclusive<u64>) -> PendingBatch {
        PendingBatch {
            job_id,
            source_id: 1,
            batch_seq: seq,
            start_line: *lines.start(),
            end_line: *lines.end(),
            records: lines
                .clone()
                .map(|l| record(l, Some(1_000_000 * l as i64)))
                .collect(),
            errors: vec![BatchError {
                line_number: *lines.end() + 1,
                code: ParseErrorCode::NoMatch,
                field: None,
            }],
            skipped_count: 1,
            ..PendingBatch::default()
        }
    }

    fn count(store: &Store, table: &str) -> i64 {
        store
            .conn()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn commit_persists_records_errors_batch_and_job_counts_together() {
        let (mut store, job_id) = store_with_job();
        let outcome = store.commit_batch(&batch(job_id, 0, 1..=3)).unwrap();
        assert_eq!(outcome, CommitOutcome::Committed { batch_id: 1 });
        assert_eq!(count(&store, "logs"), 3);
        assert_eq!(count(&store, "parse_errors"), 1);
        assert_eq!(count(&store, "import_batches"), 1);
        let job = store.job(job_id).unwrap();
        assert_eq!(
            (
                job.committed_records,
                job.committed_errors,
                job.committed_skipped
            ),
            (3, 1, 1)
        );
    }

    #[test]
    fn retry_of_same_batch_seq_does_not_duplicate_rows() {
        let (mut store, job_id) = store_with_job();
        store.commit_batch(&batch(job_id, 0, 1..=3)).unwrap();
        let second = store.commit_batch(&batch(job_id, 0, 1..=3)).unwrap();
        assert_eq!(second, CommitOutcome::AlreadyCommitted { batch_id: 1 });
        assert_eq!(count(&store, "logs"), 3);
        assert_eq!(store.job(job_id).unwrap().committed_records, 3);
    }

    #[test]
    fn identical_records_at_different_lines_are_both_kept() {
        let (mut store, job_id) = store_with_job();
        let mut b = batch(job_id, 0, 1..=2);
        b.records[1] = LogRecord {
            line_number: 2,
            ..b.records[0].clone()
        };
        store.commit_batch(&b).unwrap();
        assert_eq!(count(&store, "logs"), 2);
    }

    #[test]
    fn failure_before_commit_rolls_back_appended_rows_and_checkpoint() {
        let (mut store, job_id) = store_with_job();
        let err = store
            .commit_batch_with_hook(&batch(job_id, 0, 1..=3), |_| {
                Err(EngineError::Job("주입된 장애".to_owned()))
            })
            .unwrap_err();
        assert!(matches!(err, EngineError::Job(_)));
        assert_eq!(
            count(&store, "logs"),
            0,
            "appender rows must roll back with the transaction"
        );
        assert_eq!(count(&store, "parse_errors"), 0);
        assert_eq!(count(&store, "import_batches"), 0);
        assert_eq!(store.job(job_id).unwrap().committed_records, 0);
        assert_eq!(store.last_checkpoint(job_id, 1).unwrap(), None);
    }

    #[test]
    fn store_remains_usable_after_rolled_back_batch() {
        let (mut store, job_id) = store_with_job();
        let _ = store.commit_batch_with_hook(&batch(job_id, 0, 1..=3), |_| {
            Err(EngineError::Job("주입된 장애".to_owned()))
        });
        store.commit_batch(&batch(job_id, 0, 1..=3)).unwrap();
        assert_eq!(count(&store, "logs"), 3);
    }

    #[test]
    fn last_checkpoint_returns_latest_batch_position() {
        let (mut store, job_id) = store_with_job();
        let mut b0 = batch(job_id, 0, 1..=3);
        b0.next_offset = 100;
        let mut b1 = batch(job_id, 1, 4..=6);
        b1.next_offset = 200;
        b1.header_state_json = Some("{\"fields\":[\"date\"]}".to_owned());
        store.commit_batch(&b0).unwrap();
        store.commit_batch(&b1).unwrap();
        let cp = store.last_checkpoint(job_id, 1).unwrap().unwrap();
        assert_eq!(cp.batch_seq, 1);
        assert_eq!(cp.next_offset, 200);
        assert_eq!(cp.end_line, 6);
        assert_eq!(
            cp.header_state_json.as_deref(),
            Some("{\"fields\":[\"date\"]}")
        );
    }

    #[test]
    fn null_timestamp_is_stored_as_null() {
        let (mut store, job_id) = store_with_job();
        let mut b = batch(job_id, 0, 1..=1);
        b.records[0].timestamp_utc = None;
        store.commit_batch(&b).unwrap();
        let nulls: i64 = store
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM logs WHERE timestamp_utc IS NULL",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(nulls, 1);
    }
}
