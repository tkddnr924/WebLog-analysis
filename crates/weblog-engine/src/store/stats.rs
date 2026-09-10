//! 기본 통계. 확정 배치 범위를 고정하고 결과 크기를 상한으로 제한한다. 무거운 조회이므로 호출자가 직렬화·취소를 관리한다.

use duckdb::params_from_iter;
use duckdb::types::Value;
use serde::{Deserialize, Serialize};

use super::query::{filter_sql, LogFilter, LogQuery};
use super::LogKind;
use crate::error::{EngineError, EngineResult};

/// 상위 N 상한.
pub const MAX_TOP_N: u32 = 100;
/// 시간 버킷 수 상한.
pub const MAX_BUCKETS: i64 = 2000;
/// 자동 버킷이 목표로 하는 버킷 수.
const TARGET_BUCKETS: i64 = 400;

/// 시간 버킷.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TimeBucket {
    /// 시간 범위에 맞춰 자동 선택.
    #[default]
    Auto,
    /// 1분.
    Minute,
    /// 1시간.
    Hour,
    /// 1일.
    Day,
}

/// 통계 요청.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatsRequest {
    /// 조건.
    pub filter: LogFilter,
    /// 상위 N(IP·경로).
    #[serde(default = "default_top_n")]
    pub top_n: u32,
    /// 시간 버킷.
    #[serde(default)]
    pub bucket: TimeBucket,
    /// 버킷 경계를 맞출 표시 시간대 오프셋(초). 일 단위 버킷이 그 시간대의 자정에서 시작한다. ±18시간.
    #[serde(default)]
    pub tz_offset_seconds: i32,
}

fn default_top_n() -> u32 {
    20
}

/// 통계 결과. `max_batch_id`까지의 확정 배치만 반영한다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StatsResult {
    /// 고정한 확정 배치 상한.
    pub max_batch_id: i64,
    /// 조건에 맞는 전체 행 수.
    pub total: i64,
    /// 시간이 NULL인 행 수(시간축에는 나타나지 않음).
    pub null_time_rows: i64,
    /// 상태코드별 건수.
    pub status: Vec<(Option<i32>, i64)>,
    /// 메서드별 건수.
    pub methods: Vec<(Option<String>, i64)>,
    /// 에러 로그 레벨별 건수(많은 순). 접근 로그 조건이면 빈 배열.
    pub levels: Vec<(Option<String>, i64)>,
    /// 에러 로그 상위 메시지(많은 순, top_n개). 접근 로그 조건이면 빈 배열.
    pub top_messages: Vec<(String, i64)>,
    /// 상위 클라이언트 IP.
    pub top_ips: Vec<(String, i64)>,
    /// 상위 클라이언트 IP의 최초·마지막 탐지 시각과 접근 횟수(접근 횟수 내림차순, top_n개).
    pub ip_rows: Vec<IpRow>,
    /// 상위 요청 대상.
    pub top_targets: Vec<(String, i64)>,
    /// 시간축: (버킷 시작 UTC 마이크로초, 건수). 버킷 수 상한을 넘으면 앞쪽만.
    pub timeline: Vec<(i64, i64)>,
    /// 사용한 버킷 크기(초).
    pub bucket_seconds: i64,
    /// 시간축이 버킷 상한에 걸려 잘렸는지.
    pub timeline_truncated: bool,
    /// 조건 안의 최소·최대 시각(마이크로초).
    pub time_range: Option<(i64, i64)>,
}

/// IP 하나의 활동 요약.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpRow {
    /// 클라이언트 IP.
    pub ip: String,
    /// 최초 탐지(UTC 마이크로초). 시간이 없는 행만 있으면 None.
    pub first_seen: Option<i64>,
    /// 마지막 탐지(UTC 마이크로초).
    pub last_seen: Option<i64>,
    /// 접근 횟수.
    pub count: i64,
}

fn choose_bucket(range_micros: i64) -> i64 {
    const CANDIDATES: [i64; 9] = [
        60,
        300,
        900,
        3600,
        6 * 3600,
        86_400,
        7 * 86_400,
        30 * 86_400,
        365 * 86_400,
    ];
    let range_secs = (range_micros / 1_000_000).max(1);
    for c in CANDIDATES {
        if range_secs / c <= TARGET_BUCKETS {
            return c;
        }
    }
    365 * 86_400
}

/// 통계를 계산한다. 각 쿼리는 독립 실행되며 같은 `max_batch_id`로 범위를 고정한다.
pub fn compute_stats(q: &impl LogQuery, req: &StatsRequest) -> EngineResult<StatsResult> {
    if req.top_n == 0 || req.top_n > MAX_TOP_N {
        return Err(EngineError::Query(format!(
            "top_n은 1~{MAX_TOP_N} 사이여야 함"
        )));
    }
    let max_batch_id = q.max_committed_batch_id(req.filter.job_id)?;
    let base = filter_sql(&req.filter)?;
    let conn = q.query_conn();
    let where_sql = format!("{} AND batch_id <= ?", base.where_sql);
    let params = |extra: Vec<Value>| {
        let mut p = base.params.clone();
        p.push(Value::BigInt(max_batch_id));
        p.extend(extra);
        params_from_iter(p)
    };

    let (total, null_time_rows, min_ts, max_ts): (i64, i64, Option<i64>, Option<i64>) = conn.query_row(
        &format!(
            "SELECT COUNT(*), COUNT(*) FILTER (WHERE timestamp_utc IS NULL), epoch_us(MIN(timestamp_utc)), epoch_us(MAX(timestamp_utc)) FROM logs WHERE {where_sql}"
        ),
        params(vec![]),
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;

    let top_n = Value::BigInt(i64::from(req.top_n));
    // Error logs have no status/method/target; they aggregate level and message instead.
    let mut status = Vec::new();
    let mut methods = Vec::new();
    let mut top_targets = Vec::new();
    let mut levels = Vec::new();
    let mut top_messages = Vec::new();
    if matches!(req.filter.log_kind, Some(LogKind::Error)) {
        levels = conn
            .prepare(&format!(
                "SELECT json_extract_string(extra_json, '$.level') AS lvl, COUNT(*) AS n FROM logs WHERE {where_sql} GROUP BY lvl ORDER BY n DESC, lvl LIMIT 50"
            ))?
            .query_map(params(vec![]), |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        top_messages = conn
            .prepare(&format!(
                "SELECT json_extract_string(extra_json, '$.message') AS msg, COUNT(*) AS n FROM logs WHERE {where_sql} AND json_extract_string(extra_json, '$.message') IS NOT NULL GROUP BY msg ORDER BY n DESC, msg LIMIT ?"
            ))?
            .query_map(params(vec![top_n.clone()]), |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
    } else {
        status = conn
            .prepare(&format!(
                "SELECT status, COUNT(*) FROM logs WHERE {where_sql} GROUP BY status ORDER BY status"
            ))?
            .query_map(params(vec![]), |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        methods = conn
            .prepare(&format!(
                "SELECT method, COUNT(*) AS n FROM logs WHERE {where_sql} GROUP BY method ORDER BY n DESC LIMIT 50"
            ))?
            .query_map(params(vec![]), |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        top_targets = conn
            .prepare(&format!(
                "SELECT request_target, COUNT(*) AS n FROM logs WHERE {where_sql} AND request_target IS NOT NULL GROUP BY request_target ORDER BY n DESC, request_target LIMIT ?"
            ))?
            .query_map(params(vec![top_n.clone()]), |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
    }
    let top_ips = conn
        .prepare(&format!(
            "SELECT client_ip, COUNT(*) AS n FROM logs WHERE {where_sql} AND client_ip IS NOT NULL GROUP BY client_ip ORDER BY n DESC, client_ip LIMIT ?"
        ))?
        .query_map(params(vec![top_n.clone()]), |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<Vec<_>, _>>()?;
    let ip_rows = conn
        .prepare(&format!(
            "SELECT client_ip, epoch_us(MIN(timestamp_utc)), epoch_us(MAX(timestamp_utc)), COUNT(*) AS n FROM logs WHERE {where_sql} AND client_ip IS NOT NULL GROUP BY client_ip ORDER BY n DESC, client_ip LIMIT ?"
        ))?
        .query_map(params(vec![top_n.clone()]), |r| {
            Ok(IpRow {
                ip: r.get(0)?,
                first_seen: r.get(1)?,
                last_seen: r.get(2)?,
                count: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;

    let time_range = match (min_ts, max_ts) {
        (Some(a), Some(b)) => Some((a, b)),
        _ => None,
    };
    let bucket_seconds = match req.bucket {
        TimeBucket::Minute => 60,
        TimeBucket::Hour => 3600,
        TimeBucket::Day => 86_400,
        TimeBucket::Auto => time_range.map_or(3600, |(a, b)| choose_bucket(b - a)),
    };
    let mut timeline = Vec::new();
    let mut timeline_truncated = false;
    if time_range.is_some() {
        let limit = MAX_BUCKETS + 1;
        // 버킷 크기는 crate 내부 상수 목록에서만, 오프셋은 범위를 검사한 정수만 SQL 문자열에 넣는다.
        let off = req.tz_offset_seconds;
        if !(-64_800..=64_800).contains(&off) {
            return Err(EngineError::Query(
                "tz_offset_seconds는 ±18시간 이내".to_owned(),
            ));
        }
        let sql = format!(
            "SELECT epoch_us(time_bucket(INTERVAL '{bucket_seconds} seconds', timestamp_utc + INTERVAL '{off} seconds') - INTERVAL '{off} seconds') AS b, COUNT(*) FROM logs WHERE {where_sql} AND timestamp_utc IS NOT NULL GROUP BY b ORDER BY b LIMIT {limit}"
        );
        timeline = conn
            .prepare(&sql)?
            .query_map(params(vec![]), |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if timeline.len() as i64 > MAX_BUCKETS {
            timeline.truncate(MAX_BUCKETS as usize);
            timeline_truncated = true;
        }
    }
    Ok(StatsResult {
        max_batch_id,
        total,
        null_time_rows,
        status,
        methods,
        levels,
        top_messages,
        top_ips,
        ip_rows,
        top_targets,
        timeline,
        bucket_seconds,
        timeline_truncated,
        time_range,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::parse::LogRecord;
    use crate::store::{PendingBatch, Store, StoreConfig};

    fn seeded() -> (Store, i64) {
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let profile_id = store
            .upsert_profile(&crate::format::presets::apache_combined())
            .unwrap();
        store
            .conn_for_tests()
            .execute(
                "INSERT INTO sources (source_id, original_path, current_path, file_size, encoding, compression, head_hash, head_bytes) VALUES (1, 'a', 'a', 0, 'utf-8', 'none', '0', 0)",
                [],
            )
            .unwrap();
        let job = store
            .create_job(profile_id, &[1], None, crate::store::LogKind::Access)
            .unwrap();
        let mut records = Vec::new();
        for i in 0..100u64 {
            records.push(LogRecord {
                line_number: i + 1,
                timestamp_utc: if i == 99 {
                    None
                } else {
                    Some(i as i64 * 60_000_000)
                },
                client_ip: Some(format!("10.0.0.{}", i % 3)),
                method: Some(if i % 10 == 0 {
                    "POST".into()
                } else {
                    "GET".into()
                }),
                request_target: Some(format!("/p{}", i % 5)),
                status: Some(if i % 20 == 0 { 500 } else { 200 }),
                ..LogRecord::default()
            });
        }
        store
            .commit_batch(&PendingBatch {
                job_id: job.job_id,
                source_id: 1,
                batch_seq: 0,
                records,
                ..PendingBatch::default()
            })
            .unwrap();
        (store, job.job_id)
    }

    #[test]
    fn stats_cover_status_methods_top_n_and_timeline() {
        let (store, job_id) = seeded();
        let r = compute_stats(
            &store,
            &StatsRequest {
                filter: LogFilter {
                    job_id: Some(job_id),
                    ..LogFilter::default()
                },
                top_n: 2,
                bucket: TimeBucket::Auto,
                tz_offset_seconds: 0,
            },
        )
        .unwrap();
        assert_eq!(r.total, 100);
        assert_eq!(r.null_time_rows, 1);
        assert_eq!(r.status, vec![(Some(200), 95), (Some(500), 5)]);
        assert_eq!(r.methods[0], (Some("GET".to_owned()), 90));
        assert_eq!(r.top_ips.len(), 2);
        assert_eq!(r.top_ips[0].1, 34);
        // IP 요약: 10.0.0.0은 i%3==0인 34행, 첫 행 0분·마지막 96분(99번째 행은 시간 없음).
        assert_eq!(r.ip_rows.len(), 2);
        assert_eq!(r.ip_rows[0].ip, "10.0.0.0");
        assert_eq!(r.ip_rows[0].count, 34);
        assert_eq!(r.ip_rows[0].first_seen, Some(0));
        assert_eq!(r.ip_rows[0].last_seen, Some(96 * 60_000_000));
        assert_eq!(r.top_targets.len(), 2);
        // 99분 범위 → 60초 버킷 99개.
        assert_eq!(r.bucket_seconds, 60);
        assert_eq!(r.timeline.len(), 99);
        assert_eq!(r.timeline.iter().map(|t| t.1).sum::<i64>(), 99);
        assert!(!r.timeline_truncated);
    }

    #[test]
    fn day_buckets_follow_display_offset() {
        let (store, job_id) = seeded();
        let mk = |off: i32| StatsRequest {
            filter: LogFilter {
                job_id: Some(job_id),
                ..LogFilter::default()
            },
            top_n: 1,
            bucket: TimeBucket::Day,
            tz_offset_seconds: off,
        };
        // 0:00~1:38 UTC → UTC 기준 하루 버킷 하나(0에서 시작).
        assert_eq!(
            compute_stats(&store, &mk(0)).unwrap().timeline,
            vec![(0, 99)]
        );
        // 같은 행이 KST로는 9:00~10:38이므로 KST 자정(UTC 전날 15:00)에서 시작하는 버킷 하나.
        assert_eq!(
            compute_stats(&store, &mk(32_400)).unwrap().timeline,
            vec![(-32_400_000_000, 99)]
        );
        assert!(compute_stats(&store, &mk(100_000)).is_err());
    }

    #[test]
    fn stats_respect_filter_and_reject_bad_top_n() {
        let (store, job_id) = seeded();
        let r = compute_stats(
            &store,
            &StatsRequest {
                filter: LogFilter {
                    job_id: Some(job_id),
                    status: Some(500),
                    ..LogFilter::default()
                },
                top_n: 5,
                bucket: TimeBucket::Hour,
                tz_offset_seconds: 0,
            },
        )
        .unwrap();
        assert_eq!(r.total, 5);
        assert_eq!(r.bucket_seconds, 3600);
        assert_eq!(r.timeline.len(), 2);
        assert!(compute_stats(
            &store,
            &StatsRequest {
                filter: LogFilter::default(),
                top_n: 0,
                bucket: TimeBucket::Auto,
                tz_offset_seconds: 0,
            }
        )
        .is_err());
    }

    #[test]
    fn auto_bucket_grows_with_range() {
        assert_eq!(choose_bucket(60 * 1_000_000), 60);
        assert_eq!(choose_bucket(30 * 86_400 * 1_000_000), 6 * 3600);
        assert_eq!(choose_bucket(3 * 365 * 86_400 * 1_000_000), 7 * 86_400);
    }

    /// 접근 로그 작업(source 1)과 에러 로그 작업(source 2)을 한 저장소에 넣는다.
    fn kinded_seeded() -> Store {
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let profile_id = store
            .upsert_profile(&crate::format::presets::apache_combined())
            .unwrap();
        for (id, path) in [(1i64, "access.log"), (2, "error.log")] {
            store
                .conn_for_tests()
                .execute(
                    "INSERT INTO sources (source_id, original_path, current_path, file_size, encoding, compression, head_hash, head_bytes) VALUES (?, ?, ?, 0, 'utf-8', 'none', '0', 0)",
                    duckdb::params![id, path, path],
                )
                .unwrap();
        }
        let access = store
            .create_job(profile_id, &[1], None, crate::store::LogKind::Access)
            .unwrap();
        let error = store
            .create_job(profile_id, &[2], None, crate::store::LogKind::Error)
            .unwrap();
        let access_rec = |line: u64| LogRecord {
            line_number: line,
            timestamp_utc: Some(i64::try_from(line).unwrap() * 60_000_000),
            client_ip: Some("10.0.0.1".to_owned()),
            method: Some(if line == 1 {
                "POST".into()
            } else {
                "GET".into()
            }),
            request_target: Some("/p".to_owned()),
            status: Some(if line == 1 { 500 } else { 200 }),
            ..LogRecord::default()
        };
        // 레벨 error 3건·warn 2건·레벨 없음 1건, 메시지는 2건·2건·1건·1건.
        let error_rec = |line: u64, level: Option<&str>, message: &str| LogRecord {
            line_number: line,
            timestamp_utc: Some(i64::try_from(line).unwrap() * 60_000_000),
            client_ip: Some("10.0.2.7".to_owned()),
            extra: level
                .map(|l| ("level".to_owned(), l.to_owned()))
                .into_iter()
                .chain(std::iter::once(("message".to_owned(), message.to_owned())))
                .collect(),
            ..LogRecord::default()
        };
        store
            .commit_batch(&PendingBatch {
                job_id: access.job_id,
                source_id: 1,
                batch_seq: 0,
                records: vec![access_rec(1), access_rec(2), access_rec(3), access_rec(4)],
                ..PendingBatch::default()
            })
            .unwrap();
        store
            .commit_batch(&PendingBatch {
                job_id: error.job_id,
                source_id: 2,
                batch_seq: 0,
                records: vec![
                    error_rec(1, Some("error"), "open() failed"),
                    error_rec(2, Some("error"), "open() failed"),
                    error_rec(3, Some("error"), "upstream timeout"),
                    error_rec(4, Some("warn"), "upstream timeout"),
                    error_rec(5, Some("warn"), "cache miss"),
                    error_rec(6, None, "no level here"),
                ],
                ..PendingBatch::default()
            })
            .unwrap();
        store
    }

    fn kinded_request(kind: crate::store::LogKind, top_n: u32) -> StatsRequest {
        StatsRequest {
            filter: LogFilter {
                log_kind: Some(kind),
                ..LogFilter::default()
            },
            top_n,
            bucket: TimeBucket::Minute,
            tz_offset_seconds: 0,
        }
    }

    #[test]
    fn error_stats_group_levels_and_messages() {
        let store = kinded_seeded();
        let r = compute_stats(&store, &kinded_request(crate::store::LogKind::Error, 2)).unwrap();
        assert_eq!(r.total, 6);
        assert_eq!(
            r.levels,
            vec![
                (Some("error".to_owned()), 3),
                (Some("warn".to_owned()), 2),
                (None, 1),
            ],
            "레벨은 많은 순이고 NULL도 한 항목"
        );
        assert_eq!(
            r.top_messages,
            vec![
                ("open() failed".to_owned(), 2),
                ("upstream timeout".to_owned(), 2)
            ],
            "메시지는 top_n 상한을 지킨다"
        );
        assert!(r.status.is_empty(), "에러 로그에는 상태코드가 없다");
        assert!(r.methods.is_empty(), "에러 로그에는 메서드가 없다");
        assert!(r.top_targets.is_empty(), "에러 로그에는 요청 대상이 없다");
        assert_eq!(r.null_time_rows, 0);
        assert_eq!(r.top_ips, vec![("10.0.2.7".to_owned(), 6)]);
        assert_eq!(r.bucket_seconds, 60);
        assert_eq!(r.timeline.len(), 6);
        assert_eq!(r.time_range, Some((60_000_000, 6 * 60_000_000)));
    }

    #[test]
    fn access_stats_leave_error_aggregates_empty() {
        let store = kinded_seeded();
        let r = compute_stats(&store, &kinded_request(crate::store::LogKind::Access, 5)).unwrap();
        assert_eq!(r.total, 4);
        assert_eq!(r.status, vec![(Some(200), 3), (Some(500), 1)]);
        assert_eq!(r.methods[0], (Some("GET".to_owned()), 3));
        assert_eq!(r.top_targets, vec![("/p".to_owned(), 4)]);
        assert!(r.levels.is_empty(), "접근 로그 조건이면 레벨 집계는 없다");
        assert!(
            r.top_messages.is_empty(),
            "접근 로그 조건이면 메시지 집계는 없다"
        );
    }
}
