//! 가져오기 파이프라인: 스트리밍 읽기 → 파싱 → 제한된 배치 → 트랜잭션 커밋.
//! 파일 전체를 메모리에 올리지 않으며 배치는 행 수와 바이트 수 모두로 제한한다.
//! 중단된 작업은 마지막 확정 배치 다음부터 재개한다.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{EngineError, EngineResult};
use crate::format::FormatProfile;
use crate::parse::{LineOutcome, LineParser, ParseErrorCode};
use crate::source::{LineContent, LineReader, SourceIdentity, StatSnapshot};
use crate::store::batch::BatchError;
use crate::store::{CommitOutcome, JobStatus, LogQuery, PendingBatch, Store};

/// 가져오기 설정. 기본값은 선행 실험의 출발점이며 확정 성능 목표가 아니다.
#[derive(Debug, Clone)]
pub struct ImportConfig {
    /// 배치 최대 행 수(성공+실패+제외).
    pub batch_max_rows: usize,
    /// 배치 최대 대략 바이트.
    pub batch_max_bytes: usize,
    /// 줄 길이 상한. 넘으면 `line_too_long` 오류로 기록하고 내용을 버린다.
    pub max_line_bytes: usize,
    /// 배치당 오류 저장 한도. 넘으면 작업을 실패로 중단한다.
    pub max_errors_per_batch: usize,
    /// 재개 전 전체 파일 해시까지 검증할지. 파일 전체를 한 번 더 읽는다.
    pub full_verify_on_resume: bool,
}

impl Default for ImportConfig {
    fn default() -> Self {
        Self {
            batch_max_rows: 100_000,
            batch_max_bytes: 48 * 1024 * 1024,
            max_line_bytes: 64 * 1024,
            max_errors_per_batch: 100_000,
            full_verify_on_resume: false,
        }
    }
}

/// 가져오기 요청.
#[derive(Debug, Clone)]
pub struct ImportRequest {
    /// 포맷 프로필.
    pub profile: FormatProfile,
    /// 입력 파일 목록(순서대로 처리).
    pub paths: Vec<PathBuf>,
    /// 재파싱이면 대체할 이전 작업. 새 결과는 비활성으로 시작하며 완료 후 명시적으로 전환한다.
    pub replaces_job_id: Option<i64>,
}

/// 파일별 결과(이번 실행분).
#[derive(Debug, Clone, Serialize)]
pub struct SourceSummary {
    /// 파일 ID.
    pub source_id: i64,
    /// 경로.
    pub path: String,
    /// 재개 시작 줄 번호(처음부터면 1).
    pub resumed_from_line: u64,
    /// 이번 실행에서 읽은 줄 수.
    pub lines_read: u64,
    /// 이번 실행에서 확정한 레코드.
    pub records: u64,
    /// 이번 실행에서 확정한 오류.
    pub errors: u64,
    /// 이번 실행에서 제외.
    pub skipped: u64,
    /// 이번 실행에서 확정한 배치 수.
    pub batches: u64,
    /// 이번 실행에서 읽은 논리 바이트(압축 해제 후).
    pub logical_bytes: u64,
    /// 소요 시간(초).
    pub elapsed_secs: f64,
}

/// 작업 결과. 건수 합계는 작업 전체(재개 이전 포함)의 확정값이다.
#[derive(Debug, Clone, Serialize)]
pub struct ImportSummary {
    /// 작업 ID.
    pub job_id: i64,
    /// 결과 버전.
    pub result_version: i64,
    /// 최종 상태.
    pub status: String,
    /// 재개 실행이었는지.
    pub resumed: bool,
    /// 파일별 결과(이번 실행분).
    pub sources: Vec<SourceSummary>,
    /// 이번 실행에서 읽은 줄 합계.
    pub lines_read: u64,
    /// 작업 전체 확정 레코드.
    pub records: u64,
    /// 작업 전체 확정 오류.
    pub errors: u64,
    /// 작업 전체 제외.
    pub skipped: u64,
    /// 전체 소요(초).
    pub elapsed_secs: f64,
    /// 읽기+파싱에 쓴 시간(초).
    pub parse_secs: f64,
    /// 커밋에 쓴 시간(초).
    pub commit_secs: f64,
}

/// Progress event. A new job reports once right after creation (zero counts), then once per committed batch.
/// The first report precedes any commit, so callers learn `job_id` without waiting for a large batch.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct Progress {
    /// 작업 ID.
    pub job_id: i64,
    /// 파일 ID.
    pub source_id: i64,
    /// 파일에서 읽은 줄 수(재개 이전 포함한 누적 줄 번호).
    pub lines_read: u64,
    /// 작업 전체 확정 레코드(이번 실행분).
    pub committed_records: u64,
    /// 이번 실행에서 확정한 배치 수.
    pub committed_batches: u64,
}

struct Timers {
    parse: Duration,
    commit: Duration,
}

struct SourceTask {
    source_id: i64,
    path: PathBuf,
    resume: bool,
}

/// 새 작업으로 가져오기를 실행한다. `cancel`이 true가 되면 미커밋 배치를 버리고 확정 배치를 보존한 채 cancelled로 끝낸다.
pub fn run_import(
    store: &mut Store,
    req: &ImportRequest,
    cfg: &ImportConfig,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> EngineResult<ImportSummary> {
    if req.paths.is_empty() {
        return Err(EngineError::Job("입력 파일이 없음".to_owned()));
    }
    let profile_id = store.upsert_profile(&req.profile)?;
    // 파일 식별은 작업 생성 전에 끝낸다. 열 수 없는 파일은 작업을 만들지 않고 보고한다.
    let mut tasks = Vec::with_capacity(req.paths.len());
    for path in &req.paths {
        let identity = SourceIdentity::read(path)?;
        let source_id = store.register_source(path, &identity)?;
        tasks.push(SourceTask {
            source_id,
            path: path.clone(),
            resume: false,
        });
    }
    let source_ids: Vec<i64> = tasks.iter().map(|t| t.source_id).collect();
    let job = store.create_job(profile_id, &source_ids, req.replaces_job_id)?;
    // Report the job id without waiting for the first batch commit.
    on_progress(&Progress {
        job_id: job.job_id,
        source_id: source_ids.first().copied().unwrap_or(0),
        lines_read: 0,
        committed_records: 0,
        committed_batches: 0,
    });
    run_job(
        store,
        job.job_id,
        &req.profile,
        tasks,
        cfg,
        cancel,
        on_progress,
        false,
    )
}

/// 중단된(interrupted/cancelled/failed) 작업을 마지막 확정 배치 다음부터 재개한다.
/// 재개 전 파일 식별 정보를 검증하며, 불일치면 `SourceChanged`로 실패한다.
pub fn resume_import(
    store: &mut Store,
    job_id: i64,
    cfg: &ImportConfig,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
) -> EngineResult<ImportSummary> {
    let info = store.job(job_id)?;
    if !info.status.is_resumable() {
        return Err(EngineError::Job(format!(
            "작업 {job_id}은 {} 상태라 재개할 수 없음",
            info.status.as_str()
        )));
    }
    let profile = store.profile(info.profile_id)?;
    let tasks: Vec<SourceTask> = store
        .job_sources(job_id)?
        .into_iter()
        .filter(|s| s.status != "done")
        .map(|s| SourceTask {
            source_id: s.source_id,
            path: PathBuf::from(s.path),
            resume: true,
        })
        .collect();
    store.set_job_status(job_id, JobStatus::Running)?;
    run_job(
        store,
        job_id,
        &profile,
        tasks,
        cfg,
        cancel,
        on_progress,
        true,
    )
}

#[allow(clippy::too_many_arguments)]
fn run_job(
    store: &mut Store,
    job_id: i64,
    profile: &FormatProfile,
    tasks: Vec<SourceTask>,
    cfg: &ImportConfig,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&Progress),
    resumed: bool,
) -> EngineResult<ImportSummary> {
    let started = Instant::now();
    let mut timers = Timers {
        parse: Duration::ZERO,
        commit: Duration::ZERO,
    };
    let mut summaries = Vec::with_capacity(tasks.len());
    let mut committed_records = 0u64;
    let mut committed_batches = 0u64;
    let mut final_status = JobStatus::Completed;
    let mut failure: Option<EngineError> = None;

    for task in &tasks {
        if cancel.load(Ordering::Relaxed) {
            final_status = JobStatus::Cancelled;
            break;
        }
        store.set_job_source_status(job_id, task.source_id, "running")?;
        let result = import_source(
            store,
            job_id,
            task,
            profile,
            cfg,
            cancel,
            &mut timers,
            &mut committed_records,
            &mut committed_batches,
            on_progress,
        );
        match result {
            Ok((summary, cancelled)) => {
                let status = if cancelled { "cancelled" } else { "done" };
                store.set_job_source_status(job_id, task.source_id, status)?;
                summaries.push(summary);
                if cancelled {
                    final_status = JobStatus::Cancelled;
                    break;
                }
            }
            Err(e) => {
                store.set_job_source_status(job_id, task.source_id, "failed")?;
                final_status = JobStatus::Failed;
                failure = Some(e);
                break;
            }
        }
    }

    let info = store.job(job_id)?;
    if final_status == JobStatus::Completed && info.committed_errors > 0 {
        final_status = JobStatus::CompletedWithErrors;
    }
    let reason = failure.as_ref().map(ToString::to_string);
    store.finish_job(job_id, final_status, reason.as_deref())?;
    if let Some(e) = failure {
        return Err(e);
    }
    Ok(ImportSummary {
        job_id,
        result_version: info.result_version,
        status: final_status.as_str().to_owned(),
        resumed,
        lines_read: summaries.iter().map(|s| s.lines_read).sum(),
        records: u64::try_from(info.committed_records).unwrap_or(0),
        errors: u64::try_from(info.committed_errors).unwrap_or(0),
        skipped: u64::try_from(info.committed_skipped).unwrap_or(0),
        sources: summaries,
        elapsed_secs: started.elapsed().as_secs_f64(),
        parse_secs: timers.parse.as_secs_f64(),
        commit_secs: timers.commit.as_secs_f64(),
    })
}

/// Reads and parses one file while a writer thread commits finished batches.
/// The queue holds one batch, so reading/parsing overlaps the DuckDB transaction.
#[allow(clippy::too_many_arguments)]
fn import_source(
    store: &mut Store,
    job_id: i64,
    task: &SourceTask,
    profile: &FormatProfile,
    cfg: &ImportConfig,
    cancel: &AtomicBool,
    timers: &mut Timers,
    committed_records: &mut u64,
    committed_batches: &mut u64,
    on_progress: &mut dyn FnMut(&Progress),
) -> EngineResult<(SourceSummary, bool)> {
    let started = Instant::now();
    let source_id = task.source_id;
    let path: &Path = &task.path;
    if task.resume {
        let v = store.verify_source(source_id, cfg.full_verify_on_resume)?;
        if !v.matches {
            return Err(EngineError::SourceChanged {
                source_id,
                reason: v.reason.unwrap_or_else(|| "불일치".to_owned()),
            });
        }
    }
    let stat_at_start = StatSnapshot::read(path)?;
    let mut parser = LineParser::from_profile(profile)?;
    let mut reader = LineReader::open(path, cfg.max_line_bytes)?;
    let mut batch_seq: i64 = 0;
    if task.resume {
        if let Some(cp) = store.last_checkpoint(job_id, source_id)? {
            reader.resume_at(cp.next_offset, cp.end_line)?;
            parser.restore_header_state(cp.header_state_json.as_deref())?;
            batch_seq = cp.batch_seq + 1;
        }
    }
    let mut batch = new_batch(
        job_id,
        source_id,
        batch_seq,
        reader.offset(),
        reader.line_number() + 1,
    );
    let mut totals = SourceSummary {
        source_id,
        path: path.to_string_lossy().into_owned(),
        resumed_from_line: reader.line_number() + 1,
        lines_read: 0,
        records: 0,
        errors: 0,
        skipped: 0,
        batches: 0,
        logical_bytes: 0,
        elapsed_secs: 0.0,
    };
    let mut cancelled = false;
    let mut parse_error: Option<EngineError> = None;
    let (batch_tx, batch_rx) = mpsc::sync_channel::<PendingBatch>(1);
    let (report_tx, report_rx) = mpsc::channel::<CommitReport>();

    let joined = std::thread::scope(|scope| {
        let writer_store: &mut Store = &mut *store;
        let writer = scope.spawn(move || writer_loop(writer_store, batch_rx, report_tx));
        let mut segment = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                cancelled = true;
                break;
            }
            let line = match reader.next_line() {
                Ok(Some(line)) => line,
                Ok(None) => break,
                Err(e) => {
                    parse_error = Some(e);
                    break;
                }
            };
            totals.lines_read += 1;
            let line_bytes = line.next_offset - line.start_offset;
            totals.logical_bytes += line_bytes;
            batch.next_offset = line.next_offset;
            batch.end_line = line.line_number;
            batch.approx_bytes += usize::try_from(line_bytes).unwrap_or(usize::MAX / 4);
            match line.content {
                LineContent::Text(text) => match parser.parse_line(line.line_number, text) {
                    LineOutcome::Record(rec) => {
                        batch.approx_bytes += rec.approx_bytes();
                        batch.records.push(rec);
                    }
                    LineOutcome::Error {
                        line_number,
                        code,
                        field,
                    } => {
                        batch.errors.push(BatchError {
                            line_number,
                            code,
                            field,
                        });
                    }
                    LineOutcome::Skipped { .. } => batch.skipped_count += 1,
                },
                LineContent::InvalidUtf8 => batch.errors.push(BatchError {
                    line_number: line.line_number,
                    code: ParseErrorCode::InvalidUtf8,
                    field: None,
                }),
                LineContent::TooLong => batch.errors.push(BatchError {
                    line_number: line.line_number,
                    code: ParseErrorCode::LineTooLong,
                    field: None,
                }),
            }
            if batch.errors.len() > cfg.max_errors_per_batch {
                parse_error = Some(EngineError::ErrorLimit {
                    count: batch.errors.len(),
                    limit: cfg.max_errors_per_batch,
                });
                break;
            }
            if batch.processed_lines() as usize >= cfg.batch_max_rows
                || batch.approx_bytes >= cfg.batch_max_bytes
            {
                timers.parse += segment.elapsed();
                // Report earlier batches first: the caller may cancel before this one is queued.
                drain_reports(
                    &report_rx,
                    job_id,
                    &mut totals,
                    committed_records,
                    committed_batches,
                    on_progress,
                    false,
                );
                let full = std::mem::replace(
                    &mut batch,
                    new_batch(
                        job_id,
                        source_id,
                        batch_seq + 1,
                        reader.offset(),
                        reader.line_number() + 1,
                    ),
                );
                batch_seq += 1;
                if let Err(e) = queue(full, path, source_id, stat_at_start, &mut parser, &batch_tx)
                {
                    parse_error = e;
                    break;
                }
                // Sending may block on the queue; reports that landed meanwhile go out now.
                drain_reports(
                    &report_rx,
                    job_id,
                    &mut totals,
                    committed_records,
                    committed_batches,
                    on_progress,
                    false,
                );
                segment = Instant::now();
            }
        }
        if parse_error.is_none() {
            timers.parse += segment.elapsed();
            if !batch.is_empty() {
                let last = std::mem::take(&mut batch);
                if let Err(e) = queue(last, path, source_id, stat_at_start, &mut parser, &batch_tx)
                {
                    parse_error = e;
                }
            }
        }
        drop(batch_tx);
        writer.join()
    });

    let (commit_time, writer_result) =
        joined.map_err(|_| EngineError::Job("배치 쓰기 스레드가 비정상 종료됨".to_owned()))?;
    timers.commit += commit_time;
    drain_reports(
        &report_rx,
        job_id,
        &mut totals,
        committed_records,
        committed_batches,
        on_progress,
        true,
    );
    writer_result?;
    if let Some(e) = parse_error {
        return Err(e);
    }
    if cancelled {
        store.set_job_status(job_id, JobStatus::Cancelling)?;
    }
    totals.elapsed_secs = started.elapsed().as_secs_f64();
    Ok((totals, cancelled))
}

/// Finishes a batch (file-changed check, header state) and hands it to the writer thread.
/// `Err(None)` means the writer already stopped; its own error is reported on join.
fn queue(
    mut batch: PendingBatch,
    path: &Path,
    source_id: i64,
    stat_at_start: StatSnapshot,
    parser: &mut LineParser,
    tx: &mpsc::SyncSender<PendingBatch>,
) -> Result<(), Option<EngineError>> {
    ensure_unchanged(path, source_id, stat_at_start).map_err(Some)?;
    batch.header_state_json = parser.header_state_json().map_err(Some)?;
    tx.send(batch).map_err(|_| None)
}

/// 가져오는 도중 파일이 바뀌면 배치를 커밋하지 않고 중단한다. 최초 버전은 고정된 파일을 기준으로 한다.
fn ensure_unchanged(path: &Path, source_id: i64, at_start: StatSnapshot) -> EngineResult<()> {
    let now = StatSnapshot::read(path)?;
    if now != at_start {
        return Err(EngineError::SourceChanged {
            source_id,
            reason: format!(
                "가져오는 중 변경됨: 크기 {} → {}, 수정 시각 {:?} → {:?}",
                at_start.file_size, now.file_size, at_start.modified_unix, now.modified_unix
            ),
        });
    }
    Ok(())
}

fn new_batch(
    job_id: i64,
    source_id: i64,
    batch_seq: i64,
    start_offset: u64,
    start_line: u64,
) -> PendingBatch {
    PendingBatch {
        job_id,
        source_id,
        batch_seq,
        start_offset,
        next_offset: start_offset,
        start_line,
        end_line: start_line.saturating_sub(1),
        ..PendingBatch::default()
    }
}

/// One committed batch, reported back to the parsing thread.
struct CommitReport {
    source_id: i64,
    end_line: u64,
    records: u64,
    errors: u64,
    skipped: u64,
    /// False when the same (job, source, seq) was already committed (retry).
    fresh: bool,
}

/// Owns the store and commits queued batches until the queue closes.
/// Returns the accumulated commit time even when a commit fails.
fn writer_loop(
    store: &mut Store,
    batches: mpsc::Receiver<PendingBatch>,
    reports: mpsc::Sender<CommitReport>,
) -> (Duration, EngineResult<()>) {
    let mut commit_time = Duration::ZERO;
    while let Ok(batch) = batches.recv() {
        let started = Instant::now();
        let outcome = store.commit_batch(&batch);
        commit_time += started.elapsed();
        match outcome {
            Ok(outcome) => {
                let report = CommitReport {
                    source_id: batch.source_id,
                    end_line: batch.end_line,
                    records: batch.records.len() as u64,
                    errors: batch.errors.len() as u64,
                    skipped: batch.skipped_count,
                    fresh: matches!(outcome, CommitOutcome::Committed { .. }),
                };
                if reports.send(report).is_err() {
                    return (commit_time, Ok(()));
                }
            }
            Err(e) => return (commit_time, Err(e)),
        }
    }
    (commit_time, Ok(()))
}

/// Applies commit reports to the running totals and notifies the caller.
/// `until_closed` blocks until the writer thread is gone; otherwise it only takes what is ready.
#[allow(clippy::too_many_arguments)]
fn drain_reports(
    reports: &mpsc::Receiver<CommitReport>,
    job_id: i64,
    totals: &mut SourceSummary,
    committed_records: &mut u64,
    committed_batches: &mut u64,
    on_progress: &mut dyn FnMut(&Progress),
    until_closed: bool,
) {
    loop {
        let report = if until_closed {
            reports.recv().ok()
        } else {
            reports.try_recv().ok()
        };
        let Some(report) = report else { return };
        if report.fresh {
            totals.records += report.records;
            totals.errors += report.errors;
            totals.skipped += report.skipped;
            totals.batches += 1;
            *committed_records += report.records;
            *committed_batches += 1;
        }
        on_progress(&Progress {
            job_id,
            source_id: report.source_id,
            lines_read: report.end_line,
            committed_records: *committed_records,
            committed_batches: *committed_batches,
        });
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::format::presets;
    use crate::store::{LogFilter, LogQuery, PageRequest, SortOrder, StoreConfig};
    use std::io::Write;

    const LINE: &str =
        r#"127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET /x HTTP/1.0" 200 10 "-" "ua""#;

    fn temp_log(lines: &[&str]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        for l in lines {
            writeln!(f, "{l}").unwrap();
        }
        f.flush().unwrap();
        f
    }

    fn request(paths: Vec<PathBuf>) -> ImportRequest {
        ImportRequest {
            profile: presets::apache_combined(),
            paths,
            replaces_job_id: None,
        }
    }

    fn run(
        store: &mut Store,
        paths: Vec<PathBuf>,
        cfg: &ImportConfig,
    ) -> EngineResult<ImportSummary> {
        run_import(
            store,
            &request(paths),
            cfg,
            &AtomicBool::new(false),
            &mut |_| {},
        )
    }

    #[test]
    fn import_commits_records_errors_and_skips_with_batch_boundaries() {
        let f = temp_log(&[LINE, "", "garbage", LINE, LINE]);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let cfg = ImportConfig {
            batch_max_rows: 2,
            ..ImportConfig::default()
        };
        let s = run(&mut store, vec![f.path().to_path_buf()], &cfg).unwrap();
        assert_eq!((s.lines_read, s.records, s.errors, s.skipped), (5, 3, 1, 1));
        assert_eq!(s.sources[0].batches, 3);
        assert_eq!(s.status, "completed_with_errors");
        let cp = store
            .last_checkpoint(s.job_id, s.sources[0].source_id)
            .unwrap()
            .unwrap();
        assert_eq!(cp.end_line, 5);
        assert_eq!(cp.next_offset, s.sources[0].logical_bytes);
    }

    #[test]
    fn import_without_errors_is_completed() {
        let f = temp_log(&[LINE, LINE]);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let s = run(
            &mut store,
            vec![f.path().to_path_buf()],
            &ImportConfig::default(),
        )
        .unwrap();
        assert_eq!(s.status, "completed");
        assert_eq!(
            store
                .count_matching(&LogFilter {
                    job_id: Some(s.job_id),
                    ..LogFilter::default()
                })
                .unwrap(),
            2
        );
    }

    #[test]
    fn first_progress_reports_job_id_before_any_batch_commit() {
        let f = temp_log(&[LINE, LINE, LINE]);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let mut seen: Vec<Progress> = Vec::new();
        let s = run_import(
            &mut store,
            &request(vec![f.path().to_path_buf()]),
            &ImportConfig::default(),
            &AtomicBool::new(false),
            &mut |p| seen.push(*p),
        )
        .unwrap();
        let first = *seen.first().expect("진행 통지가 최소 한 번은 있어야 함");
        assert_eq!(first.job_id, s.job_id, "작업 ID는 첫 통지에서 확정된다");
        assert_eq!(
            (first.committed_records, first.committed_batches),
            (0, 0),
            "첫 통지는 배치 커밋 전에 온다"
        );
        assert_eq!(seen.len(), 2, "작업 생성 통지 + 마지막 배치 커밋 통지");
    }

    #[test]
    fn same_line_content_at_different_positions_is_stored_twice() {
        let f = temp_log(&[LINE, LINE]);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let s = run(
            &mut store,
            vec![f.path().to_path_buf()],
            &ImportConfig::default(),
        )
        .unwrap();
        let page = store
            .query_page(&PageRequest {
                filter: LogFilter {
                    job_id: Some(s.job_id),
                    ..LogFilter::default()
                },
                sort: SortOrder::TimeAsc,
                page_size: 10,
                cursor: None,
            })
            .unwrap();
        assert_eq!(
            page.rows.iter().map(|r| r.line_number).collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn reimporting_same_file_creates_new_result_version_not_duplicates_within_a_job() {
        let f = temp_log(&[LINE]);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let a = run(
            &mut store,
            vec![f.path().to_path_buf()],
            &ImportConfig::default(),
        )
        .unwrap();
        let b = run(
            &mut store,
            vec![f.path().to_path_buf()],
            &ImportConfig::default(),
        )
        .unwrap();
        assert_ne!(a.result_version, b.result_version);
        assert_eq!(
            a.sources[0].source_id, b.sources[0].source_id,
            "same file content reuses the source"
        );
        assert_eq!(
            store
                .count_matching(&LogFilter {
                    job_id: Some(b.job_id),
                    ..LogFilter::default()
                })
                .unwrap(),
            1
        );
    }

    #[test]
    fn reparse_creates_inactive_version_until_activated() {
        let f = temp_log(&[LINE]);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let a = run(
            &mut store,
            vec![f.path().to_path_buf()],
            &ImportConfig::default(),
        )
        .unwrap();
        let mut req = request(vec![f.path().to_path_buf()]);
        req.replaces_job_id = Some(a.job_id);
        let b = run_import(
            &mut store,
            &req,
            &ImportConfig::default(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
        let active = LogFilter {
            active_only: true,
            ..LogFilter::default()
        };
        assert_eq!(
            store.count_matching(&active).unwrap(),
            1,
            "only the old version is active"
        );
        store.activate_job(b.job_id).unwrap();
        assert_eq!(store.count_matching(&active).unwrap(), 1);
        assert!(!store.job(a.job_id).unwrap().active);
    }

    #[test]
    fn missing_file_fails_before_creating_a_job() {
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let err = run(
            &mut store,
            vec![PathBuf::from("/nonexistent/x.log")],
            &ImportConfig::default(),
        )
        .unwrap_err();
        assert!(matches!(err, EngineError::Io(_)));
        assert_eq!(store.latest_job_id().unwrap(), None);
    }

    #[test]
    fn error_limit_aborts_job_as_failed_and_keeps_committed_batches() {
        let mut lines = vec![LINE; 3];
        lines.extend(["bad"; 5]);
        let f = temp_log(&lines);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let cfg = ImportConfig {
            batch_max_rows: 3,
            max_errors_per_batch: 2,
            ..ImportConfig::default()
        };
        let err = run(&mut store, vec![f.path().to_path_buf()], &cfg).unwrap_err();
        assert!(matches!(err, EngineError::ErrorLimit { .. }));
        let job_id = store.latest_job_id().unwrap().unwrap();
        let job = store.job(job_id).unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert_eq!(job.committed_records, 3, "first batch stays committed");
    }

    #[test]
    fn cancel_keeps_committed_batches_and_marks_job_cancelled() {
        let f = temp_log(&[LINE; 40]);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let cfg = ImportConfig {
            batch_max_rows: 4,
            ..ImportConfig::default()
        };
        let cancel = AtomicBool::new(false);
        let req = request(vec![f.path().to_path_buf()]);
        let s = run_import(&mut store, &req, &cfg, &cancel, &mut |p| {
            if p.committed_batches == 1 {
                cancel.store(true, Ordering::Relaxed);
            }
        })
        .unwrap();
        assert_eq!(s.status, "cancelled");
        // Parsing runs one batch ahead of the writer, so cancellation lands within a batch or two.
        assert!(s.records >= 4 && s.records < 40, "records {}", s.records);
        assert_eq!(s.records % 4, 0, "only whole batches are committed");
        assert_eq!(store.job(s.job_id).unwrap().status, JobStatus::Cancelled);
    }

    #[test]
    fn file_appended_during_import_aborts_before_committing_the_batch() {
        let f = temp_log(&[LINE; 20]);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let cfg = ImportConfig {
            batch_max_rows: 2,
            ..ImportConfig::default()
        };
        let path = f.path().to_path_buf();
        let req = request(vec![path.clone()]);
        let err = run_import(&mut store, &req, &cfg, &AtomicBool::new(false), &mut |p| {
            if p.committed_batches == 1 {
                let mut w = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .unwrap();
                writeln!(w, "{LINE}").unwrap();
            }
        })
        .unwrap_err();
        assert!(matches!(err, EngineError::SourceChanged { .. }));
        let job = store.job(store.latest_job_id().unwrap().unwrap()).unwrap();
        assert_eq!(job.status, JobStatus::Failed);
        assert!(
            job.committed_records >= 2 && job.committed_records < 20,
            "batches before the change stay committed, the rest is not imported: {}",
            job.committed_records
        );
    }

    #[test]
    fn invalid_utf8_and_long_lines_are_recorded_as_errors_without_content() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(LINE.as_bytes()).unwrap();
        f.write_all(b"\n\xFF\xFE\n").unwrap();
        f.write_all(&vec![b'a'; 300]).unwrap();
        f.write_all(b"\n").unwrap();
        f.flush().unwrap();
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let cfg = ImportConfig {
            max_line_bytes: 256,
            ..ImportConfig::default()
        };
        let s = run(&mut store, vec![f.path().to_path_buf()], &cfg).unwrap();
        assert_eq!((s.records, s.errors), (1, 2));
        let codes: Vec<String> = store
            .conn_for_tests()
            .prepare("SELECT error_code FROM parse_errors ORDER BY line_number")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        assert_eq!(codes, vec!["invalid_utf8", "line_too_long"]);
    }
}
