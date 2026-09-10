//! 커밋 경계 장애 주입 → 재개 후 중복·누락 없음. 파일 변경·헤더 상태·gzip 재생·동시 조회를 함께 검증한다.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use weblog_engine::format::presets;
use weblog_engine::importer::{resume_import, run_import, ImportConfig, ImportRequest};
use weblog_engine::store::{
    JobStatus, LogFilter, LogQuery, PageRequest, SortOrder, Store, StoreConfig,
};
use weblog_engine::EngineError;

const LINE: &str =
    r#"10.0.{hi}.{lo} - - [10/Oct/2000:13:55:36 -0700] "GET /p{n} HTTP/1.0" 200 {n} "-" "ua""#;

fn write_lines(path: &Path, n: u64, gzip: bool) {
    let mut body = Vec::new();
    for i in 1..=n {
        let l = LINE
            .replace("{n}", &i.to_string())
            .replace("{hi}", &(i / 256).to_string())
            .replace("{lo}", &(i % 256).to_string());
        body.extend_from_slice(l.as_bytes());
        body.push(b'\n');
    }
    if gzip {
        let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        enc.write_all(&body).unwrap();
        std::fs::write(path, enc.finish().unwrap()).unwrap();
    } else {
        std::fs::write(path, body).unwrap();
    }
}

fn cfg(batch_rows: usize) -> ImportConfig {
    ImportConfig {
        batch_max_rows: batch_rows,
        ..ImportConfig::default()
    }
}

fn request(paths: Vec<PathBuf>) -> ImportRequest {
    ImportRequest {
        profile: presets::apache_combined(),
        paths,
        replaces_job_id: None,
    }
}

/// 진행 콜백에서 panic을 일으켜 프로세스 비정상 종료를 흉내 낸다. 배치 `crash_after`가 커밋된 직후 죽는다.
fn import_and_crash(db: &Path, paths: Vec<PathBuf>, batch_rows: usize, crash_after: u64) {
    let mut store = Store::open(db, &StoreConfig::default()).unwrap();
    let req = request(paths);
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_import(
            &mut store,
            &req,
            &cfg(batch_rows),
            &AtomicBool::new(false),
            &mut |p| {
                if p.committed_batches == crash_after {
                    panic!("simulated crash after batch {crash_after}");
                }
            },
        )
    }));
    assert!(
        result.is_err(),
        "the simulated crash must unwind out of run_import"
    );
    // store가 drop되며 연결이 닫힌다. 작업 상태는 running으로 남는다.
}

/// (source_id, line_number) 목록. 중복·누락 검사용.
fn all_keys(store: &impl LogQuery, job_id: i64) -> Vec<(i64, i64)> {
    let mut req = PageRequest {
        filter: LogFilter {
            job_id: Some(job_id),
            ..LogFilter::default()
        },
        sort: SortOrder::TimeAsc,
        page_size: 1000,
        cursor: None,
    };
    let mut keys = Vec::new();
    loop {
        let page = store.query_page(&req).unwrap();
        keys.extend(page.rows.iter().map(|r| (r.source_id, r.line_number)));
        match page.next_cursor {
            Some(c) => req.cursor = Some(c),
            None => break,
        }
    }
    keys
}

fn assert_complete_without_duplicates(store: &Store, job_id: i64, expected_lines: u64) {
    let mut keys = all_keys(store, job_id);
    let before = keys.len();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(
        keys.len(),
        before,
        "duplicate (source, line) keys after resume"
    );
    let lines: Vec<i64> = keys.iter().map(|k| k.1).collect();
    assert_eq!(
        lines,
        (1..=expected_lines as i64).collect::<Vec<_>>(),
        "missing or extra lines after resume"
    );
    let job = store.job(job_id).unwrap();
    assert_eq!(job.committed_records as u64, expected_lines);
    assert_eq!(job.status, JobStatus::Completed);
}

#[test]
fn crash_after_commit_then_resume_has_no_duplicates_or_gaps_plain() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("a.log");
    let db = dir.path().join("a.duckdb");
    write_lines(&log, 2000, false);
    import_and_crash(&db, vec![log.clone()], 100, 3);

    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let job_id = store.latest_job_id().unwrap().unwrap();
    assert_eq!(
        store.job(job_id).unwrap().status,
        JobStatus::Interrupted,
        "open must mark stale running jobs"
    );
    // 파싱이 쓰기보다 앞서므로 죽는 시점의 확정 배치 수는 3 이상이다. 배치 경계는 유지된다.
    let committed = store.job(job_id).unwrap().committed_records as u64;
    assert!(
        (300..2000).contains(&committed) && committed % 100 == 0,
        "committed at crash: {committed}"
    );

    let summary = resume_import(
        &mut store,
        job_id,
        &cfg(100),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert!(summary.resumed);
    assert_eq!(summary.sources[0].resumed_from_line, committed + 1);
    assert_eq!(summary.sources[0].records, 2000 - committed);
    assert_complete_without_duplicates(&store, job_id, 2000);
}

#[test]
fn crash_then_resume_works_for_gzip_by_replaying_the_stream() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("a.log.gz");
    let db = dir.path().join("a.duckdb");
    write_lines(&log, 2000, true);
    import_and_crash(&db, vec![log.clone()], 100, 2);

    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let job_id = store.latest_job_id().unwrap().unwrap();
    let committed = store.job(job_id).unwrap().committed_records as u64;
    assert!(
        (200..2000).contains(&committed) && committed % 100 == 0,
        "committed at crash: {committed}"
    );
    let summary = resume_import(
        &mut store,
        job_id,
        &cfg(100),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(summary.sources[0].resumed_from_line, committed + 1);
    assert_complete_without_duplicates(&store, job_id, 2000);
}

#[test]
fn resume_after_crash_between_files_continues_with_next_file() {
    let dir = tempfile::tempdir().unwrap();
    let a = dir.path().join("a.log");
    let b = dir.path().join("b.log");
    let db = dir.path().join("a.duckdb");
    write_lines(&a, 5, false);
    write_lines(&b, 5, false);
    // 배치 크기 5 → 파일 a가 배치 1개로 끝난 직후 죽는다.
    import_and_crash(&db, vec![a.clone(), b.clone()], 5, 1);

    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let job_id = store.latest_job_id().unwrap().unwrap();
    let sources = store.job_sources(job_id).unwrap();
    // 파일 a의 마지막 배치는 커밋됐지만 'done' 표시 전에 죽었으므로 pending으로 돌아온다.
    assert_eq!(sources[0].status, "pending");
    assert_eq!(sources[1].status, "pending");
    let summary = resume_import(
        &mut store,
        job_id,
        &cfg(5),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(summary.sources.len(), 2);
    assert_eq!(
        summary.sources[0].lines_read, 0,
        "file a resumes at EOF and adds nothing"
    );
    assert_eq!(summary.sources[0].resumed_from_line, 6);
    assert_eq!(summary.sources[1].path, b.to_string_lossy());
    assert_eq!(summary.sources[1].records, 5);
    let keys = all_keys(&store, job_id);
    assert_eq!(keys.len(), 10);
    assert_eq!(store.job(job_id).unwrap().status, JobStatus::Completed);
}

#[test]
fn resuming_twice_is_harmless_and_completed_job_cannot_be_resumed() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("a.log");
    let db = dir.path().join("a.duckdb");
    write_lines(&log, 9, false);
    import_and_crash(&db, vec![log.clone()], 3, 1);
    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let job_id = store.latest_job_id().unwrap().unwrap();
    resume_import(
        &mut store,
        job_id,
        &cfg(3),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    let err = resume_import(
        &mut store,
        job_id,
        &cfg(3),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(err, EngineError::Job(_)));
    assert_complete_without_duplicates(&store, job_id, 9);
}

#[test]
fn resume_refuses_when_file_changed_since_checkpoint() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("a.log");
    let db = dir.path().join("a.duckdb");
    write_lines(&log, 2000, false);
    import_and_crash(&db, vec![log.clone()], 4, 1);
    // 선두 내용을 바꾼다(크기는 유지).
    let mut bytes = std::fs::read(&log).unwrap();
    bytes[0] = b'9';
    std::fs::write(&log, bytes).unwrap();

    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let job_id = store.latest_job_id().unwrap().unwrap();
    let err = resume_import(
        &mut store,
        job_id,
        &cfg(4),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap_err();
    assert!(matches!(err, EngineError::SourceChanged { .. }), "{err}");
    let job = store.job(job_id).unwrap();
    assert_eq!(job.status, JobStatus::Failed);
    assert!(
        (4..2000).contains(&job.committed_records) && job.committed_records % 4 == 0,
        "committed batches survive: {}",
        job.committed_records
    );
    assert!(job.failure_reason.unwrap().contains("선두 내용 해시"));
}

#[test]
fn resume_with_full_verification_catches_change_past_head_window() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("a.log");
    let db = dir.path().join("a.duckdb");
    // 64KiB 선두 창을 넘기도록 충분히 큰 파일.
    write_lines(&log, 1500, false);
    assert!(std::fs::metadata(&log).unwrap().len() > 70_000);
    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let job = run_import(
        &mut store,
        &request(vec![log.clone()]),
        &cfg(500),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    let source_id = job.sources[0].source_id;
    store.verify_source(source_id, true).unwrap();
    drop(store);
    // 꼬리 쪽 한 바이트만 바꾼다.
    let mut bytes = std::fs::read(&log).unwrap();
    let last = bytes.len() - 5;
    bytes[last] = b'Z';
    std::fs::write(&log, bytes).unwrap();

    let store = Store::open(&db, &StoreConfig::default()).unwrap();
    assert!(
        store.verify_source(source_id, false).unwrap().matches,
        "fast check cannot see it"
    );
    let full = store.verify_source(source_id, true).unwrap();
    assert!(!full.matches);
    assert_eq!(full.reason.as_deref(), Some("전체 내용 해시 불일치"));
}

#[test]
fn w3c_header_change_before_crash_is_restored_on_resume() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("u_ex.log");
    let db = dir.path().join("a.duckdb");
    let mut text = String::from("#Fields: date time c-ip cs-method cs-uri-stem sc-status\n");
    for i in 1..=4 {
        text.push_str(&format!("2024-01-02 03:04:0{i} 10.0.0.{i} GET /a{i} 200\n"));
    }
    text.push_str("#Fields: date time c-ip sc-status cs-uri-stem sc-bytes\n");
    for i in 5..=9 {
        text.push_str(&format!("2024-01-02 03:04:0{i} 10.0.0.{i} 404 /b{i} {i}\n"));
    }
    std::fs::write(&log, text).unwrap();

    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let req = ImportRequest {
        profile: presets::iis_w3c(),
        paths: vec![log.clone()],
        replaces_job_id: None,
    };
    // 배치 6줄: 헤더1 + 4데이터 + 헤더2 → 첫 배치 끝에 두 번째 헤더 상태가 저장된다. 그 직후 죽는다.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_import(
            &mut store,
            &req,
            &cfg(6),
            &AtomicBool::new(false),
            &mut |p| {
                if p.committed_batches == 1 {
                    panic!("crash");
                }
            },
        )
    }));
    assert!(result.is_err());
    drop(store);

    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let job_id = store.latest_job_id().unwrap().unwrap();
    let cp = store.last_checkpoint(job_id, 1).unwrap().unwrap();
    assert!(cp.header_state_json.unwrap().contains("sc-bytes"));
    let summary = resume_import(
        &mut store,
        job_id,
        &cfg(6),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(
        summary.errors, 0,
        "resumed rows must parse with the second header"
    );
    // 줄 9 = 두 번째 헤더 뒤 세 번째 데이터(i=7).
    let d = store.detail(Some(job_id), 1, 9).unwrap().unwrap();
    assert_eq!(d.status, Some(404));
    assert_eq!(d.request_target.as_deref(), Some("/b7"));
    assert_eq!(d.bytes_sent, Some(7));
    assert_eq!(store.job(job_id).unwrap().committed_records, 9);
}

#[test]
fn cancelled_job_can_be_resumed_to_completion() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("a.log");
    let db = dir.path().join("a.duckdb");
    write_lines(&log, 2000, false);
    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let cancel = AtomicBool::new(false);
    let s = run_import(
        &mut store,
        &request(vec![log.clone()]),
        &cfg(5),
        &cancel,
        &mut |p| {
            if p.committed_batches == 2 {
                cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        },
    )
    .unwrap();
    assert_eq!(s.status, "cancelled");
    assert!(
        (10..2000).contains(&s.records) && s.records % 5 == 0,
        "cancel keeps whole batches and stops early: {}",
        s.records
    );
    let summary = resume_import(
        &mut store,
        s.job_id,
        &cfg(5),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(summary.sources[0].resumed_from_line, s.records + 1);
    assert_complete_without_duplicates(&store, s.job_id, 2000);
}

#[test]
fn reader_on_another_thread_queries_while_import_runs_and_cursor_stays_frozen() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("a.log");
    let db = dir.path().join("a.duckdb");
    write_lines(&log, 3000, false);
    let mut store = Store::open(&db, &StoreConfig::default()).unwrap();
    let reader = store.open_reader().unwrap();
    let (tx, rx) = std::sync::mpsc::channel::<u64>();
    let (ack_tx, ack_rx) = std::sync::mpsc::channel::<()>();
    let (done_tx, done_rx) = std::sync::mpsc::channel::<()>();

    let query_thread = std::thread::spawn(move || {
        // 첫 배치가 커밋되면 첫 페이지를 읽고(그동안 가져오기는 ack를 기다린다),
        // 가져오기가 끝난 뒤 같은 커서로 다음 페이지를 읽는다.
        let _first_batch = rx.recv().unwrap();
        let filter = LogFilter::default();
        let first = reader
            .query_page(&PageRequest {
                filter: filter.clone(),
                sort: SortOrder::TimeAsc,
                page_size: 100,
                cursor: None,
            })
            .unwrap();
        let frozen_batch = first.next_cursor.as_ref().unwrap().max_batch_id;
        let visible_then = reader.count_matching(&filter).unwrap();
        ack_tx.send(()).unwrap();
        done_rx.recv().unwrap();
        let mut req = PageRequest {
            filter: filter.clone(),
            sort: SortOrder::TimeAsc,
            page_size: 1000,
            cursor: first.next_cursor.clone(),
        };
        let mut rows = first.rows.len();
        loop {
            let page = reader.query_page(&req).unwrap();
            rows += page.rows.len();
            match page.next_cursor {
                Some(c) => req.cursor = Some(c),
                None => break,
            }
        }
        let total_after = reader.count_matching(&filter).unwrap();
        (frozen_batch, visible_then, rows, total_after)
    });

    run_import(
        &mut store,
        &request(vec![log.clone()]),
        &cfg(500),
        &AtomicBool::new(false),
        &mut |p| {
            if p.committed_batches == 1 {
                tx.send(p.committed_batches).unwrap();
                ack_rx.recv().unwrap();
            }
        },
    )
    .unwrap();
    done_tx.send(()).unwrap();
    let (frozen_batch, visible_then, rows_via_cursor, total_after) = query_thread.join().unwrap();
    // 파싱이 쓰기보다 앞서므로 멈춘 시점의 확정 배치 수는 1 이상이다. 읽는 쪽은 그 시점까지만 본다.
    assert!((1..6).contains(&frozen_batch), "frozen at {frozen_batch}");
    let visible_rows = frozen_batch * 500;
    assert_eq!(
        visible_then, visible_rows,
        "reader sees only committed batches during import"
    );
    assert_eq!(
        i64::try_from(rows_via_cursor).unwrap(),
        visible_rows,
        "cursor stays frozen at the batch range it was created with"
    );
    assert_eq!(total_after, 3000);
}
