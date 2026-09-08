//! fixture 로그를 파서와 가져오기 경로 양쪽으로 검증한다.
//! 미리보기(파서 직접 실행)와 실제 DB 결과가 같은지 확인한다.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use weblog_engine::format::{presets, FormatProfile};
use weblog_engine::importer::{run_import, ImportConfig, ImportRequest};
use weblog_engine::parse::LineOutcome;
use weblog_engine::preview::{preview_file, PreviewConfig};
use weblog_engine::store::{LogFilter, LogQuery, PageRequest, SortOrder, Store, StoreConfig};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(name)
}

fn load_profile(spec: &str) -> FormatProfile {
    if let Some(p) = presets::by_name(spec) {
        return p;
    }
    let json = std::fs::read_to_string(fixture(spec)).expect("profile json");
    FormatProfile::from_json(&json).expect("valid profile")
}

fn expected(name: &str) -> Vec<serde_json::Value> {
    let text = std::fs::read_to_string(fixture(name)).expect("expected json");
    serde_json::from_str(&text).expect("valid expected json")
}

const CASES: &[(&str, &str, &str)] = &[
    (
        "apache_combined.log",
        "apache_combined",
        "apache_combined.expected.json",
    ),
    (
        "apache_combined_bom_crlf.log",
        "apache_combined",
        "apache_combined_bom_crlf.expected.json",
    ),
    ("nginx_common.log", "common", "nginx_common.expected.json"),
    ("iis_w3c.log", "iis_w3c", "iis_w3c.expected.json"),
    (
        "iis_w3c_noheader.log",
        "iis_w3c",
        "iis_w3c_noheader.expected.json",
    ),
    (
        "custom_pipe.log",
        "custom_pipe.profile.json",
        "custom_pipe.expected.json",
    ),
];

#[test]
fn preview_outcomes_match_hand_written_expectations() {
    for (log, profile, exp) in CASES {
        let result = preview_file(
            &fixture(log),
            &load_profile(profile),
            &PreviewConfig::default(),
        )
        .unwrap();
        let actual: Vec<serde_json::Value> = result
            .outcomes
            .iter()
            .map(|o| serde_json::to_value(o).unwrap())
            .collect();
        let want = expected(exp);
        assert_eq!(actual.len(), want.len(), "{log}: line count");
        for (a, w) in actual.iter().zip(&want) {
            assert_eq!(a, w, "{log}: line {}", w["line_number"]);
        }
    }
}

#[test]
fn imported_rows_match_preview_records_field_by_field() {
    for (log, profile, exp) in CASES {
        let want = expected(exp);
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let req = ImportRequest {
            profile: load_profile(profile),
            paths: vec![fixture(log)],
            replaces_job_id: None,
        };
        let cfg = ImportConfig {
            batch_max_rows: 3,
            ..ImportConfig::default()
        };
        let summary =
            run_import(&mut store, &req, &cfg, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let expected_records: Vec<&serde_json::Value> =
            want.iter().filter(|v| v["kind"] == "record").collect();
        let expected_errors = want.iter().filter(|v| v["kind"] == "error").count() as u64;
        let expected_skipped = want.iter().filter(|v| v["kind"] == "skipped").count() as u64;
        assert_eq!(
            summary.records,
            expected_records.len() as u64,
            "{log}: records"
        );
        assert_eq!(summary.errors, expected_errors, "{log}: errors");
        assert_eq!(summary.skipped, expected_skipped, "{log}: skipped");

        let source_id = summary.sources[0].source_id;
        for w in expected_records {
            let line = w["line_number"].as_i64().unwrap();
            let d = store
                .detail(Some(summary.job_id), source_id, line)
                .unwrap()
                .unwrap_or_else(|| panic!("{log}: line {line} missing in DB"));
            assert_eq!(
                serde_json::to_value(d.timestamp_utc).unwrap(),
                w["timestamp_utc"],
                "{log}:{line} timestamp"
            );
            assert_eq!(
                serde_json::to_value(d.tz_offset_seconds).unwrap(),
                w["tz_offset_seconds"],
                "{log}:{line} tz"
            );
            assert_eq!(
                serde_json::to_value(&d.client_ip).unwrap(),
                w["client_ip"],
                "{log}:{line} ip"
            );
            assert_eq!(
                serde_json::to_value(&d.method).unwrap(),
                w["method"],
                "{log}:{line} method"
            );
            assert_eq!(
                serde_json::to_value(&d.request_target).unwrap(),
                w["request_target"],
                "{log}:{line} target"
            );
            assert_eq!(
                serde_json::to_value(&d.protocol).unwrap(),
                w["protocol"],
                "{log}:{line} protocol"
            );
            assert_eq!(
                serde_json::to_value(d.status.map(|s| s as i64)).unwrap(),
                w["status"],
                "{log}:{line} status"
            );
            assert_eq!(
                serde_json::to_value(d.bytes_sent).unwrap(),
                w["bytes_sent"],
                "{log}:{line} bytes"
            );
            assert_eq!(
                serde_json::to_value(&d.referrer).unwrap(),
                w["referrer"],
                "{log}:{line} referrer"
            );
            assert_eq!(
                serde_json::to_value(&d.user_agent).unwrap(),
                w["user_agent"],
                "{log}:{line} ua"
            );
            let extra: serde_json::Value = d
                .extra_json
                .as_deref()
                .map(|j| serde_json::from_str(j).unwrap())
                .unwrap_or(serde_json::Value::Null);
            let want_extra = w.get("extra").cloned().unwrap_or(serde_json::Value::Null);
            assert_eq!(extra, want_extra, "{log}:{line} extra");
        }
        // 페이지 조회로도 같은 건수가 나와야 한다.
        let page = store
            .query_page(&PageRequest {
                filter: LogFilter {
                    job_id: Some(summary.job_id),
                    ..LogFilter::default()
                },
                sort: SortOrder::TimeAsc,
                page_size: 100,
                cursor: None,
            })
            .unwrap();
        assert_eq!(page.rows.len() as u64, summary.records, "{log}: page rows");
    }
}

#[test]
fn error_rows_carry_position_and_code_but_never_input_text() {
    let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
    let req = ImportRequest {
        profile: presets::apache_combined(),
        paths: vec![fixture("apache_combined.log")],
        replaces_job_id: None,
    };
    let summary = run_import(
        &mut store,
        &req,
        &ImportConfig::default(),
        &AtomicBool::new(false),
        &mut |_| {},
    )
    .unwrap();
    let want = expected("apache_combined.expected.json");
    for w in want.iter().filter(|v| v["kind"] == "error") {
        let line = w["line_number"].as_i64().unwrap();
        let (code, field): (String, Option<String>) = store
            .conn_for_tests()
            .query_row(
                "SELECT error_code, field_name FROM parse_errors WHERE job_id = ? AND line_number = ?",
                duckdb::params![summary.job_id, line],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(code, w["code"].as_str().unwrap());
        assert_eq!(
            serde_json::to_value(field).unwrap(),
            w.get("field").cloned().unwrap_or(serde_json::Value::Null)
        );
    }
    // 어떤 테이블에도 원문 조각("this is not a log line")이 없어야 한다.
    let leaked: i64 = store
        .conn_for_tests()
        .query_row(
            "SELECT COUNT(*) FROM (SELECT error_code AS v FROM parse_errors UNION ALL SELECT field_name FROM parse_errors UNION ALL SELECT failure_reason FROM import_jobs) WHERE v LIKE '%not a log line%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(leaked, 0);
}

#[test]
fn gzip_input_produces_identical_results_to_plain_input() {
    use std::io::Write;
    let plain = std::fs::read(fixture("apache_combined.log")).unwrap();
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(&plain).unwrap();
    let gz_bytes = enc.finish().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let gz_path = dir.path().join("apache_combined.log.gz");
    std::fs::write(&gz_path, gz_bytes).unwrap();

    let profile = presets::apache_combined();
    let a = preview_file(
        &fixture("apache_combined.log"),
        &profile,
        &PreviewConfig::default(),
    )
    .unwrap();
    let b = preview_file(&gz_path, &profile, &PreviewConfig::default()).unwrap();
    assert_eq!(a.outcomes, b.outcomes);
    assert!(a
        .outcomes
        .iter()
        .any(|o| matches!(o, LineOutcome::Record(_))));
}
