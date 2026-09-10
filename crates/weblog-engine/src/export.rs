//! CSV / JSON Lines 스트리밍 내보내기. 커서 페이지 단위로 읽어 디스크로 바로 쓴다. 전체 결과를 모으지 않는다.

use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};
use crate::store::{ExportRow, LogFilter, LogQuery, SortOrder};

/// 내보내기 형식.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExportFormat {
    /// CSV(UTF-8, 헤더 포함).
    Csv,
    /// JSON Lines(행마다 객체 하나).
    JsonLines,
}

/// 내보내기 요청.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRequest {
    /// 조건.
    pub filter: LogFilter,
    /// 정렬.
    #[serde(default)]
    pub sort: SortOrder,
    /// 형식.
    pub format: ExportFormat,
    /// 출력 파일.
    pub out_path: PathBuf,
    /// 최대 행 수. `None`이면 전체.
    #[serde(default)]
    pub max_rows: Option<u64>,
    /// 확장 필드 JSON 컬럼 포함.
    #[serde(default = "default_true")]
    pub include_extra: bool,
}

fn default_true() -> bool {
    true
}

/// 진행 통지.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct ExportProgress {
    /// 쓴 행 수.
    pub rows: u64,
    /// 쓴 바이트 수.
    pub bytes: u64,
    /// 경과 초.
    pub elapsed_secs: f64,
}

/// 결과.
#[derive(Debug, Clone, Serialize)]
pub struct ExportSummary {
    /// 출력 파일.
    pub out_path: String,
    /// 쓴 행 수.
    pub rows: u64,
    /// 쓴 바이트 수.
    pub bytes: u64,
    /// 경과 초.
    pub elapsed_secs: f64,
    /// 고정한 확정 배치 상한.
    pub max_batch_id: i64,
    /// 취소되어 부분 결과인지.
    pub cancelled: bool,
    /// 최대 행 수에 걸려 잘렸는지.
    pub truncated: bool,
}

const PAGE: u32 = 10_000;
const HEADER: &[&str] = &[
    "timestamp_utc",
    "tz_offset_seconds",
    "client_ip",
    "method",
    "request_target",
    "protocol",
    "status",
    "bytes_sent",
    "referrer",
    "user_agent",
    "job_id",
    "source_id",
    "line_number",
];

pub(crate) fn iso_utc(micros: Option<i64>) -> String {
    micros
        .and_then(chrono::DateTime::from_timestamp_micros)
        .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Micros, true))
        .unwrap_or_default()
}

pub(crate) fn csv_field(out: &mut String, v: &str) {
    if v.contains([',', '"', '\n', '\r']) {
        out.push('"');
        out.push_str(&v.replace('"', "\"\""));
        out.push('"');
    } else {
        out.push_str(v);
    }
}

fn csv_line(row: &ExportRow, include_extra: bool) -> String {
    let mut s = String::with_capacity(256);
    let opt_s = |v: &Option<String>| v.clone().unwrap_or_default();
    let fields: Vec<String> = vec![
        iso_utc(row.timestamp_utc),
        row.tz_offset_seconds
            .map(|v| v.to_string())
            .unwrap_or_default(),
        opt_s(&row.client_ip),
        opt_s(&row.method),
        opt_s(&row.request_target),
        opt_s(&row.protocol),
        row.status.map(|v| v.to_string()).unwrap_or_default(),
        row.bytes_sent.map(|v| v.to_string()).unwrap_or_default(),
        opt_s(&row.referrer),
        opt_s(&row.user_agent),
        row.job_id.to_string(),
        row.source_id.to_string(),
        row.line_number.to_string(),
    ];
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        csv_field(&mut s, f);
    }
    if include_extra {
        s.push(',');
        csv_field(&mut s, row.extra_json.as_deref().unwrap_or(""));
    }
    s.push('\n');
    s
}

fn json_line(row: &ExportRow, include_extra: bool) -> EngineResult<String> {
    let extra: Option<serde_json::Value> = if include_extra {
        row.extra_json
            .as_deref()
            .and_then(|j| serde_json::from_str(j).ok())
    } else {
        None
    };
    let obj = serde_json::json!({
        "timestamp_utc": row.timestamp_utc.map(|_| iso_utc(row.timestamp_utc)),
        "tz_offset_seconds": row.tz_offset_seconds,
        "client_ip": row.client_ip,
        "method": row.method,
        "request_target": row.request_target,
        "protocol": row.protocol,
        "status": row.status,
        "bytes_sent": row.bytes_sent,
        "referrer": row.referrer,
        "user_agent": row.user_agent,
        "job_id": row.job_id,
        "source_id": row.source_id,
        "line_number": row.line_number,
        "extra": extra,
    });
    let mut s = serde_json::to_string(&obj)?;
    s.push('\n');
    Ok(s)
}

/// 내보낸다. 취소되면 지금까지 쓴 파일을 남기고 `cancelled = true`로 돌려준다.
pub fn export_logs(
    q: &impl LogQuery,
    req: &ExportRequest,
    cancel: &AtomicBool,
    on_progress: &mut dyn FnMut(&ExportProgress),
) -> EngineResult<ExportSummary> {
    let started = Instant::now();
    if req.out_path.as_os_str().is_empty() {
        return Err(EngineError::Query("출력 경로가 비었음".to_owned()));
    }
    let parent: &Path = req.out_path.parent().unwrap_or_else(|| Path::new("."));
    if !parent.as_os_str().is_empty() && !parent.exists() {
        return Err(EngineError::Query("출력 폴더가 없음".to_owned()));
    }
    let file = std::fs::File::create(&req.out_path)?;
    let mut w = BufWriter::with_capacity(1 << 20, file);
    let mut bytes: u64 = 0;
    let mut rows: u64 = 0;
    if req.format == ExportFormat::Csv {
        let mut header = HEADER.join(",");
        if req.include_extra {
            header.push_str(",extra_json");
        }
        header.push('\n');
        w.write_all(header.as_bytes())?;
        bytes += header.len() as u64;
    }
    let mut cursor = None;
    let mut max_batch_id = None;
    let mut cancelled = false;
    let mut truncated = false;
    let mut last_report = Instant::now();
    'pages: loop {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }
        let (page, next) = q.export_page(&req.filter, req.sort, PAGE, cursor.as_ref())?;
        if max_batch_id.is_none() {
            max_batch_id = Some(next.as_ref().map_or_else(
                || q.max_committed_batch_id(req.filter.job_id),
                |c| Ok(c.max_batch_id),
            )?);
        }
        for row in &page {
            if req.max_rows.is_some_and(|m| rows >= m) {
                truncated = true;
                break 'pages;
            }
            let line = match req.format {
                ExportFormat::Csv => csv_line(row, req.include_extra),
                ExportFormat::JsonLines => json_line(row, req.include_extra)?,
            };
            w.write_all(line.as_bytes())?;
            bytes += line.len() as u64;
            rows += 1;
        }
        if last_report.elapsed().as_millis() >= 250 {
            last_report = Instant::now();
            on_progress(&ExportProgress {
                rows,
                bytes,
                elapsed_secs: started.elapsed().as_secs_f64(),
            });
        }
        match next {
            Some(c) => cursor = Some(c),
            None => break,
        }
    }
    w.flush()?;
    on_progress(&ExportProgress {
        rows,
        bytes,
        elapsed_secs: started.elapsed().as_secs_f64(),
    });
    Ok(ExportSummary {
        out_path: req.out_path.to_string_lossy().into_owned(),
        rows,
        bytes,
        elapsed_secs: started.elapsed().as_secs_f64(),
        max_batch_id: max_batch_id.unwrap_or(0),
        cancelled,
        truncated,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::parse::LogRecord;
    use crate::store::{PendingBatch, Store, StoreConfig};

    fn seeded(n: u64) -> Store {
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let profile_id = store
            .upsert_profile(&crate::format::presets::apache_combined())
            .unwrap();
        let job = store
            .create_job(profile_id, &[1], None, crate::store::LogKind::Access)
            .unwrap();
        let records = (1..=n)
            .map(|i| LogRecord {
                line_number: i,
                timestamp_utc: Some(i as i64 * 1_000_000),
                client_ip: Some("10.0.0.1".to_owned()),
                request_target: Some(format!("/p,\"{i}\"")),
                user_agent: Some("UA \"x\"".to_owned()),
                status: Some(200),
                extra: [("k".to_owned(), "v".to_owned())].into_iter().collect(),
                ..LogRecord::default()
            })
            .collect();
        store
            .commit_batch(&PendingBatch {
                job_id: job.job_id,
                source_id: 1,
                batch_seq: 0,
                records,
                ..PendingBatch::default()
            })
            .unwrap();
        store
    }

    fn request(dir: &Path, format: ExportFormat, name: &str) -> ExportRequest {
        ExportRequest {
            filter: LogFilter::default(),
            sort: SortOrder::TimeAsc,
            format,
            out_path: dir.join(name),
            max_rows: None,
            include_extra: true,
        }
    }

    #[test]
    fn csv_export_quotes_fields_and_writes_all_rows_across_pages() {
        let store = seeded(2500);
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), ExportFormat::Csv, "out.csv");
        let s = export_logs(&store, &req, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(s.rows, 2500);
        assert!(!s.cancelled && !s.truncated);
        let text = std::fs::read_to_string(&req.out_path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2501);
        assert!(lines[0].starts_with("timestamp_utc,tz_offset_seconds,client_ip"));
        assert!(lines[1].contains("\"/p,\"\"1\"\"\""), "{}", lines[1]);
        assert!(lines[1].starts_with("1970-01-01T00:00:01.000000Z,"));
        assert!(
            lines[2500].ends_with(",1,1,2500,\"{\"\"k\"\":\"\"v\"\"}\""),
            "{}",
            lines[2500]
        );
    }

    #[test]
    fn jsonl_export_has_one_object_per_row_with_parsed_extra() {
        let store = seeded(3);
        let dir = tempfile::tempdir().unwrap();
        let req = request(dir.path(), ExportFormat::JsonLines, "out.jsonl");
        export_logs(&store, &req, &AtomicBool::new(false), &mut |_| {}).unwrap();
        let text = std::fs::read_to_string(&req.out_path).unwrap();
        let objs: Vec<serde_json::Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(objs.len(), 3);
        assert_eq!(objs[0]["extra"]["k"], "v");
        assert_eq!(objs[2]["line_number"], 3);
    }

    #[test]
    fn max_rows_truncates_and_cancel_leaves_partial_file() {
        let store = seeded(50);
        let dir = tempfile::tempdir().unwrap();
        let mut req = request(dir.path(), ExportFormat::Csv, "cap.csv");
        req.max_rows = Some(10);
        let s = export_logs(&store, &req, &AtomicBool::new(false), &mut |_| {}).unwrap();
        assert_eq!(s.rows, 10);
        assert!(s.truncated);
        let req2 = request(dir.path(), ExportFormat::Csv, "cancel.csv");
        let cancel = AtomicBool::new(true);
        let s2 = export_logs(&store, &req2, &cancel, &mut |_| {}).unwrap();
        assert!(s2.cancelled);
        assert_eq!(s2.rows, 0);
        assert!(req2.out_path.exists());
    }

    #[test]
    fn missing_output_folder_is_an_error_before_any_query() {
        let store = seeded(1);
        let req = request(
            Path::new("/definitely/missing/dir"),
            ExportFormat::Csv,
            "x.csv",
        );
        assert!(export_logs(&store, &req, &AtomicBool::new(false), &mut |_| {}).is_err());
    }
}
