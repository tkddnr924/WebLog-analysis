//! 버전이 있는 마이그레이션. 원문 테이블·raw_line 컬럼은 만들지 않는다.

use duckdb::{params, Connection, OptionalExt};

use crate::error::{EngineError, EngineResult};

/// 현재 스키마 버전.
pub const SCHEMA_VERSION: i64 = 4;

const V1: &str = r#"
CREATE TABLE IF NOT EXISTS sources (
    source_id       BIGINT PRIMARY KEY,
    original_path   VARCHAR NOT NULL,
    current_path    VARCHAR NOT NULL,
    file_size       BIGINT NOT NULL,
    modified_unix   BIGINT,
    encoding        VARCHAR NOT NULL,
    compression     VARCHAR NOT NULL,
    head_hash       VARCHAR NOT NULL,
    head_bytes      BIGINT NOT NULL,
    registered_at   TIMESTAMP NOT NULL DEFAULT current_timestamp,
    full_hash       VARCHAR
);
CREATE TABLE IF NOT EXISTS parser_profiles (
    profile_id      BIGINT PRIMARY KEY,
    name            VARCHAR NOT NULL,
    version         INTEGER NOT NULL,
    definition_json VARCHAR NOT NULL,
    definition_hash VARCHAR NOT NULL UNIQUE
);
CREATE TABLE IF NOT EXISTS import_jobs (
    job_id            BIGINT PRIMARY KEY,
    result_version    BIGINT NOT NULL,
    status            VARCHAR NOT NULL,
    profile_id        BIGINT NOT NULL,
    started_at        TIMESTAMP NOT NULL,
    finished_at       TIMESTAMP,
    committed_records BIGINT NOT NULL DEFAULT 0,
    committed_errors  BIGINT NOT NULL DEFAULT 0,
    committed_skipped BIGINT NOT NULL DEFAULT 0,
    failure_reason    VARCHAR,
    active            BOOLEAN DEFAULT TRUE,
    replaces_job_id   BIGINT,
    log_kind          VARCHAR
);
CREATE TABLE IF NOT EXISTS import_job_sources (
    job_id     BIGINT NOT NULL,
    source_id  BIGINT NOT NULL,
    ordinal    INTEGER NOT NULL,
    status     VARCHAR NOT NULL,
    PRIMARY KEY (job_id, source_id)
);
CREATE TABLE IF NOT EXISTS import_batches (
    batch_id          BIGINT PRIMARY KEY,
    job_id            BIGINT NOT NULL,
    source_id         BIGINT NOT NULL,
    batch_seq         INTEGER NOT NULL,
    start_offset      BIGINT NOT NULL,
    next_offset       BIGINT NOT NULL,
    start_line        BIGINT NOT NULL,
    end_line          BIGINT NOT NULL,
    header_state_json VARCHAR,
    record_count      INTEGER NOT NULL,
    error_count       INTEGER NOT NULL,
    skipped_count     INTEGER NOT NULL,
    committed_at      TIMESTAMP NOT NULL,
    UNIQUE (job_id, source_id, batch_seq)
);
CREATE TABLE IF NOT EXISTS logs (
    job_id            BIGINT NOT NULL,
    source_id         BIGINT NOT NULL,
    batch_id          BIGINT NOT NULL,
    line_number       BIGINT NOT NULL,
    timestamp_utc     TIMESTAMP,
    tz_offset_seconds INTEGER,
    client_ip         VARCHAR,
    method            VARCHAR,
    request_target    VARCHAR,
    protocol          VARCHAR,
    status            INTEGER,
    bytes_sent        BIGINT,
    referrer          VARCHAR,
    user_agent        VARCHAR,
    extra_json        VARCHAR
);
CREATE TABLE IF NOT EXISTS parse_errors (
    job_id      BIGINT NOT NULL,
    source_id   BIGINT NOT NULL,
    batch_id    BIGINT NOT NULL,
    line_number BIGINT NOT NULL,
    error_code  VARCHAR NOT NULL,
    field_name  VARCHAR
);
CREATE TABLE IF NOT EXISTS saved_views (
    view_id         BIGINT PRIMARY KEY,
    name            VARCHAR NOT NULL,
    definition_json VARCHAR NOT NULL
);
CREATE TABLE IF NOT EXISTS bookmarks (
    source_id   BIGINT NOT NULL,
    line_number BIGINT NOT NULL,
    created_at  TIMESTAMP NOT NULL DEFAULT current_timestamp,
    PRIMARY KEY (source_id, line_number)
);
"#;

/// v2: 결과 버전 전환(active, replaces_job_id)과 전체 파일 해시.
/// 새 저장소는 v1 CREATE에 이 컬럼이 이미 들어 있어 아래 ALTER는 모두 no-op이다. 이렇게 두는 이유:
/// DuckDB는 비정상 종료 뒤 WAL을 재생할 때 `ADD COLUMN … DEFAULT`를 다시 바인딩하다 실패할 수 있어
/// (GetDefaultDatabase with no default database set), 새 파일의 WAL에 ALTER가 남지 않게 한다.
const V2: &str = r#"
ALTER TABLE import_jobs ADD COLUMN IF NOT EXISTS active BOOLEAN DEFAULT TRUE;
ALTER TABLE import_jobs ADD COLUMN IF NOT EXISTS replaces_job_id BIGINT;
ALTER TABLE sources ADD COLUMN IF NOT EXISTS full_hash VARCHAR;
"#;

/// v3: 북마크(파일·줄 기준). 새 저장소는 v1 CREATE에 이미 있어 no-op이다.
const V3: &str = r#"
CREATE TABLE IF NOT EXISTS bookmarks (
    source_id   BIGINT NOT NULL,
    line_number BIGINT NOT NULL,
    created_at  TIMESTAMP NOT NULL DEFAULT current_timestamp,
    PRIMARY KEY (source_id, line_number)
);
"#;

/// v4: 작업의 로그 종류(access|error). 값이 없는 기존 작업은 프로필 이름으로 채우고 나머지는 접근 로그로 본다.
const V4: &str = r#"
ALTER TABLE import_jobs ADD COLUMN IF NOT EXISTS log_kind VARCHAR;
UPDATE import_jobs SET log_kind = 'error' WHERE log_kind IS NULL AND profile_id IN (SELECT profile_id FROM parser_profiles WHERE name LIKE 'error_log%');
UPDATE import_jobs SET log_kind = 'access' WHERE log_kind IS NULL;
"#;

/// 마이그레이션 목록. 인덱스 0이 버전 1이다.
const MIGRATIONS: &[&str] = &[V1, V2, V3, V4];

/// 스키마를 최신 버전으로 올린다. 저장소가 엔진보다 새 버전이면 오류.
pub fn migrate(conn: &Connection) -> EngineResult<()> {
    migrate_to(conn, SCHEMA_VERSION)
}

/// 지정 버전까지 올린다(테스트·점진 검증용).
pub fn migrate_to(conn: &Connection, target: i64) -> EngineResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (version BIGINT PRIMARY KEY, applied_at TIMESTAMP NOT NULL DEFAULT current_timestamp)",
    )?;
    let current = current_version(conn)?;
    if current > SCHEMA_VERSION {
        return Err(EngineError::SchemaVersion {
            found: current,
            expected: SCHEMA_VERSION,
        });
    }
    for (idx, sql) in MIGRATIONS.iter().enumerate() {
        let version = idx as i64 + 1;
        if version <= current || version > target {
            continue;
        }
        // DuckDB는 한 트랜잭션 안에서 같은 테이블을 두 번 ALTER하면 커밋 시 충돌한다.
        // 그래서 DDL은 문장 단위 autocommit으로 실행하고, 각 문장은 IF NOT EXISTS로 멱등하게 둔다.
        // 버전 기록은 모든 문장이 성공한 뒤에만 남긴다.
        for statement in sql.split(';').map(str::trim).filter(|s| !s.is_empty()) {
            conn.execute_batch(statement)?;
        }
        conn.execute(
            "INSERT INTO schema_migrations (version) VALUES (?)",
            params![version],
        )?;
    }
    Ok(())
}

/// 저장소의 현재 스키마 버전(없으면 0).
pub fn current_version(conn: &Connection) -> EngineResult<i64> {
    let v = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .optional()?
        .flatten()
        .unwrap_or(0);
    Ok(v)
}

/// 원문 저장 금지 검사: 이름에 raw/original/line_text가 들어간 컬럼이 없어야 한다.
pub fn assert_no_raw_columns(conn: &Connection) -> EngineResult<()> {
    let mut stmt = conn.prepare(
        "SELECT table_name, column_name FROM information_schema.columns WHERE lower(column_name) LIKE '%raw%' OR lower(column_name) LIKE '%original_line%' OR lower(column_name) LIKE '%line_text%' OR lower(table_name) LIKE '%raw%'",
    )?;
    let offenders: Vec<(String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect::<Result<_, _>>()?;
    if offenders.is_empty() {
        Ok(())
    } else {
        Err(EngineError::Format(format!(
            "원문 저장 의심 컬럼: {offenders:?}"
        )))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn migrate_applies_v1_and_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), SCHEMA_VERSION);
    }

    #[test]
    fn v1_store_upgrades_to_v2_keeping_rows() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_to(&conn, 1).unwrap();
        conn.execute(
            "INSERT INTO import_jobs (job_id, result_version, status, profile_id, started_at) VALUES (1, 1, 'completed', 1, current_timestamp)",
            [],
        )
        .unwrap();
        migrate(&conn).unwrap();
        assert_eq!(current_version(&conn).unwrap(), SCHEMA_VERSION);
        let active: bool = conn
            .query_row("SELECT active FROM import_jobs WHERE job_id = 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert!(active, "existing jobs stay active after upgrade");
    }

    #[test]
    fn v3_store_upgrades_to_v4_backfilling_log_kind() {
        let conn = Connection::open_in_memory().unwrap();
        migrate_to(&conn, 3).unwrap();
        for (id, name) in [(1i64, "apache_combined"), (2, "error_log_edit")] {
            conn.execute(
                "INSERT INTO parser_profiles (profile_id, name, version, definition_json, definition_hash) VALUES (?, ?, 1, '{}', ?)",
                params![id, name, name],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO import_jobs (job_id, result_version, status, profile_id, started_at) VALUES (?, ?, 'completed', ?, current_timestamp)",
                params![id, id, id],
            )
            .unwrap();
        }
        migrate(&conn).unwrap();
        let kind = |job_id: i64| -> String {
            conn.query_row(
                "SELECT log_kind FROM import_jobs WHERE job_id = ?",
                params![job_id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(kind(1), "access", "접근 로그 프로필은 access로 채운다");
        assert_eq!(kind(2), "error", "이름이 error_log인 프로필은 error");
    }

    #[test]
    fn schema_has_no_raw_line_storage() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        assert_no_raw_columns(&conn).unwrap();
    }

    #[test]
    fn newer_store_version_is_rejected() {
        let conn = Connection::open_in_memory().unwrap();
        migrate(&conn).unwrap();
        conn.execute("INSERT INTO schema_migrations (version) VALUES (999)", [])
            .unwrap();
        assert!(matches!(
            migrate(&conn),
            Err(EngineError::SchemaVersion { found: 999, .. })
        ));
    }
}
