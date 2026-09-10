//! 페이지 조회. UI는 조건 객체를 보내고 여기서 바인딩 SQL을 만든다. 큰 OFFSET 대신 커서를 쓴다.

use duckdb::types::Value;
use duckdb::{params_from_iter, OptionalExt};
use serde::{Deserialize, Serialize};

use super::{map_job, parse_compression, JobInfo, JobSource, LogKind, Store, JOB_SELECT};
use crate::error::{EngineError, EngineResult};
use crate::format::FormatProfile;
use crate::source::SourceIdentity;
use duckdb::params;

/// 페이지 크기 상한.
pub const MAX_PAGE_SIZE: u32 = 1000;
/// 페이지 응답 바이트 상한(대략값).
pub const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;
/// 내보내기 페이지 상한. 화면 페이지보다 크게 잡아 커서 왕복 횟수를 줄인다(디스크로 바로 쓰므로 응답 크기 부담이 없다).
pub const MAX_EXPORT_PAGE_SIZE: u32 = 20_000;
/// 문자열 포함 검색 길이 상한.
const MAX_SEARCH_BYTES: usize = 1024;

/// 조회 조건. 값은 모두 바인딩된다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogFilter {
    /// 작업 ID. `None`이면 전체.
    pub job_id: Option<i64>,
    /// 파일 ID.
    pub source_id: Option<i64>,
    /// 시작 시각(UTC 마이크로초, 포함). 시간 필터는 NULL 시간 행을 제외한다.
    pub time_from_micros: Option<i64>,
    /// 끝 시각(UTC 마이크로초, 제외).
    pub time_to_micros: Option<i64>,
    /// 상태코드 일치.
    pub status: Option<u16>,
    /// 상태코드 클래스(2 → 200~299).
    pub status_class: Option<u8>,
    /// 클라이언트 IP 일치.
    pub client_ip: Option<String>,
    /// 메서드 일치.
    pub method: Option<String>,
    /// 요청 대상 부분 문자열(대소문자 구분).
    pub target_contains: Option<String>,
    /// 요청 대상 정규식(RE2 문법, `(?i)`로 대소문자 무시). 분석 포맷의 패턴 검사에 쓴다.
    #[serde(default)]
    pub target_regex: Option<String>,
    /// 로그 종류. `None`이면 접근·에러를 모두 본다.
    #[serde(default)]
    pub log_kind: Option<LogKind>,
    /// 룰 조건식(and/or/not 조합). 위의 단순 조건과 AND로 결합한다.
    #[serde(default)]
    pub expr: Option<FilterExpr>,
    /// 북마크한 행만.
    #[serde(default)]
    pub bookmarked_only: bool,
    /// 활성 결과 버전만 조회.
    #[serde(default)]
    pub active_only: bool,
}

/// 조건식이 볼 수 있는 필드. SQL 식이 문자열로 조립되므로 열거형으로만 만든다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CondField {
    /// 상태코드(정수).
    Status,
    /// 응답 바이트(정수).
    BytesSent,
    /// 클라이언트 IP.
    ClientIp,
    /// 메서드.
    Method,
    /// 요청 대상(경로+쿼리).
    RequestTarget,
    /// 프로토콜.
    Protocol,
    /// 리퍼러.
    Referrer,
    /// User-Agent.
    UserAgent,
    /// 확장 필드 JSON 원문(에러 로그의 레벨·메시지 등).
    Extra,
    /// 에러 로그 레벨(확장 필드에서 뽑음).
    Level,
    /// 에러 로그 메시지(확장 필드에서 뽑음).
    Message,
}

impl CondField {
    // SQL expression, not always a bare column: only enum variants reach here, so no injection.
    fn column(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::BytesSent => "bytes_sent",
            Self::ClientIp => "client_ip",
            Self::Method => "method",
            Self::RequestTarget => "request_target",
            Self::Protocol => "protocol",
            Self::Referrer => "referrer",
            Self::UserAgent => "user_agent",
            Self::Extra => "extra_json",
            Self::Level => "json_extract_string(extra_json, '$.level')",
            Self::Message => "json_extract_string(extra_json, '$.message')",
        }
    }

    // Name shown in error messages; SQL expressions must not leak to the UI.
    fn label(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::BytesSent => "bytes_sent",
            Self::ClientIp => "client_ip",
            Self::Method => "method",
            Self::RequestTarget => "request_target",
            Self::Protocol => "protocol",
            Self::Referrer => "referrer",
            Self::UserAgent => "user_agent",
            Self::Extra => "extra",
            Self::Level => "level",
            Self::Message => "message",
        }
    }

    fn is_numeric(self) -> bool {
        matches!(self, Self::Status | Self::BytesSent)
    }
}

/// 조건 연산. 문자열 연산은 텍스트 컬럼에, 크기 비교는 정수 컬럼에만 쓴다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CondOp {
    /// 같음.
    Eq,
    /// 다름.
    Ne,
    /// 초과.
    Gt,
    /// 이상.
    Gte,
    /// 미만.
    Lt,
    /// 이하.
    Lte,
    /// 부분 문자열 포함(대소문자 구분).
    Contains,
    /// 부분 문자열 포함(대소문자 무시).
    Icontains,
    /// 접두.
    StartsWith,
    /// 접미.
    EndsWith,
    /// 정규식(RE2).
    Regex,
    /// 값이 없음(NULL).
    IsNull,
}

/// 룰 조건식. 노드 수와 깊이에 상한이 있다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FilterExpr {
    /// 모두 참.
    And {
        /// 항목.
        items: Vec<FilterExpr>,
    },
    /// 하나라도 참.
    Or {
        /// 항목.
        items: Vec<FilterExpr>,
    },
    /// 부정.
    Not {
        /// 항목.
        item: Box<FilterExpr>,
    },
    /// 단일 조건.
    Cond {
        /// 컬럼.
        field: CondField,
        /// 연산.
        op: CondOp,
        /// 값. 정수 컬럼의 크기 비교는 10진 정수여야 한다. IsNull은 무시한다.
        value: String,
    },
    /// 항상 참.
    True,
}

const MAX_EXPR_NODES: usize = 256;
const MAX_EXPR_DEPTH: usize = 16;

/// 조건식을 SQL 조각으로. 값은 모두 바인딩하고 컬럼 이름은 열거형에서만 나온다.
fn expr_sql(
    e: &FilterExpr,
    out: &mut String,
    params: &mut Vec<Value>,
    nodes: &mut usize,
    depth: usize,
) -> EngineResult<()> {
    *nodes += 1;
    if *nodes > MAX_EXPR_NODES {
        return Err(EngineError::Query(format!(
            "조건식 노드가 상한 {MAX_EXPR_NODES}개를 넘음"
        )));
    }
    if depth > MAX_EXPR_DEPTH {
        return Err(EngineError::Query("조건식 중첩이 너무 깊음".to_owned()));
    }
    match e {
        FilterExpr::True => out.push_str("TRUE"),
        FilterExpr::And { items } | FilterExpr::Or { items } => {
            if items.is_empty() {
                out.push_str("TRUE");
                return Ok(());
            }
            let joiner = if matches!(e, FilterExpr::And { .. }) {
                " AND "
            } else {
                " OR "
            };
            out.push('(');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(joiner);
                }
                expr_sql(item, out, params, nodes, depth + 1)?;
            }
            out.push(')');
        }
        FilterExpr::Not { item } => {
            out.push_str("(NOT ");
            expr_sql(item, out, params, nodes, depth + 1)?;
            out.push(')');
        }
        FilterExpr::Cond { field, op, value } => {
            if value.len() > MAX_SEARCH_BYTES {
                return Err(EngineError::Query("조건 값이 상한을 넘음".to_owned()));
            }
            let col = field.column();
            let label = field.label();
            let numeric = field.is_numeric();
            match op {
                CondOp::IsNull => out.push_str(&format!("{col} IS NULL")),
                CondOp::Eq | CondOp::Ne | CondOp::Gt | CondOp::Gte | CondOp::Lt | CondOp::Lte => {
                    let sym = match op {
                        CondOp::Eq => "=",
                        CondOp::Ne => "<>",
                        CondOp::Gt => ">",
                        CondOp::Gte => ">=",
                        CondOp::Lt => "<",
                        _ => "<=",
                    };
                    if numeric {
                        let n: i64 = value.trim().parse().map_err(|_| {
                            EngineError::Query(format!("{label} 비교 값은 정수여야 함"))
                        })?;
                        out.push_str(&format!("{col} {sym} ?"));
                        params.push(Value::BigInt(n));
                    } else {
                        if !matches!(op, CondOp::Eq | CondOp::Ne) {
                            return Err(EngineError::Query(format!(
                                "{label}에는 크기 비교를 쓸 수 없음"
                            )));
                        }
                        out.push_str(&format!("{col} {sym} ?"));
                        params.push(Value::Text(value.clone()));
                    }
                }
                CondOp::Contains
                | CondOp::Icontains
                | CondOp::StartsWith
                | CondOp::EndsWith
                | CondOp::Regex => {
                    if numeric {
                        return Err(EngineError::Query(format!(
                            "{label}에는 문자열 연산을 쓸 수 없음"
                        )));
                    }
                    match op {
                        CondOp::Contains => out.push_str(&format!("contains({col}, ?)")),
                        CondOp::Icontains => {
                            out.push_str(&format!("contains(lower({col}), lower(?))"))
                        }
                        CondOp::StartsWith => out.push_str(&format!("starts_with({col}, ?)")),
                        CondOp::EndsWith => out.push_str(&format!("ends_with({col}, ?)")),
                        _ => {
                            if let Err(err) = regex::Regex::new(value) {
                                return Err(EngineError::Query(format!("조건 정규식 오류: {err}")));
                            }
                            out.push_str(&format!("regexp_matches({col}, ?)"));
                        }
                    }
                    params.push(Value::Text(value.clone()));
                }
            }
        }
    }
    Ok(())
}

/// 정렬. 기본 시간 정렬만 지원하며 NULL 시간은 항상 마지막 구간이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SortOrder {
    /// 시간 오름차순.
    #[default]
    TimeAsc,
    /// 시간 내림차순.
    TimeDesc,
}

/// 커서 구간.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CursorSegment {
    /// 시간이 있는 행.
    Timed,
    /// 시간이 NULL인 행.
    NullTime,
}

/// 페이지 커서. 필터·정렬 해시, 확정 배치 경계, 마지막 행 키를 담는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageCursor {
    /// 필터+정렬 해시. 다른 요청에 재사용하면 거부한다.
    pub filter_hash: String,
    /// 조회 범위를 고정하는 최대 배치 ID.
    pub max_batch_id: i64,
    /// 구간.
    pub segment: CursorSegment,
    /// 마지막 행 시간(마이크로초).
    pub last_ts: Option<i64>,
    /// 마지막 행 파일 ID.
    pub last_source_id: i64,
    /// 마지막 행 줄 번호.
    pub last_line: i64,
}

/// 페이지 요청.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PageRequest {
    /// 조건.
    pub filter: LogFilter,
    /// 정렬.
    #[serde(default)]
    pub sort: SortOrder,
    /// 페이지 크기(상한 [`MAX_PAGE_SIZE`]).
    pub page_size: u32,
    /// 이전 페이지 커서.
    #[serde(default)]
    pub cursor: Option<PageCursor>,
}

/// 테이블 행. 긴 User-Agent·확장 필드는 상세 조회로 분리한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogRow {
    /// 파일 ID.
    pub source_id: i64,
    /// 줄 번호.
    pub line_number: i64,
    /// UTC 마이크로초.
    pub timestamp_utc: Option<i64>,
    /// 클라이언트 IP.
    pub client_ip: Option<String>,
    /// 메서드.
    pub method: Option<String>,
    /// 요청 대상.
    pub request_target: Option<String>,
    /// 상태코드.
    pub status: Option<i32>,
    /// 전송 바이트.
    pub bytes_sent: Option<i64>,
    /// 북마크 여부.
    #[serde(default)]
    pub bookmarked: bool,
    /// 확장 필드 JSON. 에러 로그의 레벨·메시지가 여기 들어간다.
    #[serde(default)]
    pub extra_json: Option<String>,
}

impl LogRow {
    fn approx_bytes(&self) -> usize {
        48 + self.client_ip.as_ref().map_or(0, String::len)
            + self.method.as_ref().map_or(0, String::len)
            + self.request_target.as_ref().map_or(0, String::len)
            + self.extra_json.as_ref().map_or(0, String::len)
    }
}

/// 페이지 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogPage {
    /// 행.
    pub rows: Vec<LogRow>,
    /// 다음 페이지 커서. 없으면 마지막 페이지.
    pub next_cursor: Option<PageCursor>,
    /// 응답 대략 바이트.
    pub approx_bytes: usize,
}

/// 상세 레코드. 재구성 로그의 입력이 된다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogDetail {
    /// 작업 ID.
    pub job_id: i64,
    /// 배치 ID.
    pub batch_id: i64,
    /// 파일 ID.
    pub source_id: i64,
    /// 파일 경로(현재 연결 경로).
    pub source_path: String,
    /// 줄 번호.
    pub line_number: i64,
    /// UTC 마이크로초.
    pub timestamp_utc: Option<i64>,
    /// 적용 오프셋(초).
    pub tz_offset_seconds: Option<i32>,
    /// 클라이언트 IP.
    pub client_ip: Option<String>,
    /// 메서드.
    pub method: Option<String>,
    /// 요청 대상.
    pub request_target: Option<String>,
    /// 프로토콜.
    pub protocol: Option<String>,
    /// 상태코드.
    pub status: Option<i32>,
    /// 전송 바이트.
    pub bytes_sent: Option<i64>,
    /// Referrer.
    pub referrer: Option<String>,
    /// User-Agent.
    pub user_agent: Option<String>,
    /// 확장 필드 JSON.
    pub extra_json: Option<String>,
}

pub(crate) struct SqlParts {
    pub(crate) where_sql: String,
    pub(crate) params: Vec<Value>,
}

pub(crate) fn filter_sql(filter: &LogFilter) -> EngineResult<SqlParts> {
    let mut where_sql = String::from("1=1");
    let mut params = Vec::new();
    if let Some(v) = filter.job_id {
        where_sql.push_str(" AND job_id = ?");
        params.push(Value::BigInt(v));
    }
    if let Some(v) = filter.source_id {
        where_sql.push_str(" AND source_id = ?");
        params.push(Value::BigInt(v));
    }
    if filter.active_only {
        where_sql.push_str(" AND job_id IN (SELECT job_id FROM import_jobs WHERE active)");
    }
    if let Some(v) = filter.time_from_micros {
        where_sql.push_str(" AND timestamp_utc >= make_timestamp(?)");
        params.push(Value::BigInt(v));
    }
    if let Some(v) = filter.time_to_micros {
        where_sql.push_str(" AND timestamp_utc < make_timestamp(?)");
        params.push(Value::BigInt(v));
    }
    if let Some(v) = filter.status {
        where_sql.push_str(" AND status = ?");
        params.push(Value::Int(i32::from(v)));
    }
    if let Some(c) = filter.status_class {
        if !(1..=5).contains(&c) {
            return Err(EngineError::Query("status_class는 1~5여야 함".to_owned()));
        }
        where_sql.push_str(" AND status >= ? AND status < ?");
        params.push(Value::Int(i32::from(c) * 100));
        params.push(Value::Int(i32::from(c) * 100 + 100));
    }
    if let Some(v) = &filter.client_ip {
        where_sql.push_str(" AND client_ip = ?");
        params.push(Value::Text(v.clone()));
    }
    if let Some(v) = &filter.method {
        where_sql.push_str(" AND method = ?");
        params.push(Value::Text(v.clone()));
    }
    if let Some(v) = &filter.target_contains {
        if v.len() > MAX_SEARCH_BYTES {
            return Err(EngineError::Query("검색 문자열이 상한을 넘음".to_owned()));
        }
        where_sql.push_str(" AND contains(request_target, ?)");
        params.push(Value::Text(v.clone()));
    }
    if let Some(v) = &filter.target_regex {
        if v.len() > MAX_SEARCH_BYTES {
            return Err(EngineError::Query("정규식이 상한을 넘음".to_owned()));
        }
        // DuckDB(RE2)에 넘기기 전에 문법을 확인해 오류 메시지를 분명히 한다. 패턴 자체는 로그 값이 아니다.
        if let Err(e) = regex::Regex::new(v) {
            return Err(EngineError::Query(format!("요청 대상 정규식 오류: {e}")));
        }
        where_sql.push_str(" AND regexp_matches(request_target, ?)");
        params.push(Value::Text(v.clone()));
    }
    if let Some(kind) = filter.log_kind {
        // 종류는 작업에 기록한다. 값이 없는 예전 작업은 접근 로그로 본다.
        where_sql.push_str(" AND job_id IN (SELECT job_id FROM import_jobs WHERE coalesce(log_kind, 'access') = ?)");
        params.push(Value::Text(kind.as_str().to_owned()));
    }
    if filter.bookmarked_only {
        where_sql.push_str(" AND EXISTS (SELECT 1 FROM bookmarks b WHERE b.source_id = logs.source_id AND b.line_number = logs.line_number)");
    }
    if let Some(e) = &filter.expr {
        where_sql.push_str(" AND ");
        let mut nodes = 0;
        expr_sql(e, &mut where_sql, &mut params, &mut nodes, 0)?;
    }
    Ok(SqlParts { where_sql, params })
}

fn filter_hash(filter: &LogFilter, sort: SortOrder) -> EngineResult<String> {
    let bytes = serde_json::to_vec(&(filter, sort))?;
    Ok(crate::format::model::fnv1a_hex(&bytes))
}

const ROW_COLUMNS: &str = "source_id, line_number, epoch_us(timestamp_utc), client_ip, method, request_target, status, bytes_sent, EXISTS (SELECT 1 FROM bookmarks b WHERE b.source_id = logs.source_id AND b.line_number = logs.line_number), extra_json";

fn read_row(r: &duckdb::Row<'_>) -> duckdb::Result<LogRow> {
    Ok(LogRow {
        source_id: r.get(0)?,
        line_number: r.get(1)?,
        timestamp_utc: r.get(2)?,
        client_ip: r.get(3)?,
        method: r.get(4)?,
        request_target: r.get(5)?,
        status: r.get(6)?,
        bytes_sent: r.get(7)?,
        bookmarked: r.get(8)?,
        extra_json: r.get(9)?,
    })
}

/// 커서 페이징이 필요로 하는 키를 가진 행.
pub trait KeyedRow {
    /// (timestamp_utc, source_id, line_number).
    fn key(&self) -> (Option<i64>, i64, i64);
    /// 응답 바이트 근사치.
    fn approx_bytes(&self) -> usize;
}

impl KeyedRow for LogRow {
    fn key(&self) -> (Option<i64>, i64, i64) {
        (self.timestamp_utc, self.source_id, self.line_number)
    }
    fn approx_bytes(&self) -> usize {
        LogRow::approx_bytes(self)
    }
}

/// 내보내기용 전체 컬럼 행. 원문은 없다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportRow {
    /// 작업 ID.
    pub job_id: i64,
    /// 파일 ID.
    pub source_id: i64,
    /// 줄 번호.
    pub line_number: i64,
    /// UTC 마이크로초.
    pub timestamp_utc: Option<i64>,
    /// 적용 오프셋(초).
    pub tz_offset_seconds: Option<i32>,
    /// 클라이언트 IP.
    pub client_ip: Option<String>,
    /// 메서드.
    pub method: Option<String>,
    /// 요청 대상.
    pub request_target: Option<String>,
    /// 프로토콜.
    pub protocol: Option<String>,
    /// 상태코드.
    pub status: Option<i32>,
    /// 전송 바이트.
    pub bytes_sent: Option<i64>,
    /// Referrer.
    pub referrer: Option<String>,
    /// User-Agent.
    pub user_agent: Option<String>,
    /// 확장 필드 JSON.
    pub extra_json: Option<String>,
}

impl KeyedRow for ExportRow {
    fn key(&self) -> (Option<i64>, i64, i64) {
        (self.timestamp_utc, self.source_id, self.line_number)
    }
    fn approx_bytes(&self) -> usize {
        let s = |v: &Option<String>| v.as_ref().map_or(0, String::len);
        96 + s(&self.client_ip)
            + s(&self.method)
            + s(&self.request_target)
            + s(&self.protocol)
            + s(&self.referrer)
            + s(&self.user_agent)
            + s(&self.extra_json)
    }
}

const EXPORT_COLUMNS: &str = "job_id, source_id, line_number, epoch_us(timestamp_utc), tz_offset_seconds, client_ip, method, request_target, protocol, status, bytes_sent, referrer, user_agent, extra_json";

fn read_export_row(r: &duckdb::Row<'_>) -> duckdb::Result<ExportRow> {
    Ok(ExportRow {
        job_id: r.get(0)?,
        source_id: r.get(1)?,
        line_number: r.get(2)?,
        timestamp_utc: r.get(3)?,
        tz_offset_seconds: r.get(4)?,
        client_ip: r.get(5)?,
        method: r.get(6)?,
        request_target: r.get(7)?,
        protocol: r.get(8)?,
        status: r.get(9)?,
        bytes_sent: r.get(10)?,
        referrer: r.get(11)?,
        user_agent: r.get(12)?,
        extra_json: r.get(13)?,
    })
}

/// 읽기 전용 연결. 가져오기가 진행 중인 동안 다른 스레드에서 조회할 때 쓴다.
pub struct Reader {
    conn: duckdb::Connection,
}

impl std::fmt::Debug for Reader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reader")
    }
}

impl Reader {
    pub(crate) fn new(conn: duckdb::Connection) -> Self {
        Self { conn }
    }
}

/// 조회 API. [`Store`]와 [`Reader`]가 같은 구현을 공유한다. 읽기 전용 메타데이터 조회도 여기에 둔다.
pub trait LogQuery {
    /// 연결.
    fn query_conn(&self) -> &duckdb::Connection;

    /// 프로필 정의를 읽는다.
    fn profile(&self, profile_id: i64) -> EngineResult<FormatProfile> {
        let json: String = self.query_conn().query_row(
            "SELECT definition_json FROM parser_profiles WHERE profile_id = ?",
            params![profile_id],
            |r| r.get(0),
        )?;
        Ok(FormatProfile::from_json(&json)?)
    }

    /// 등록된 파일의 경로와 식별 정보.
    fn source(&self, source_id: i64) -> EngineResult<(String, SourceIdentity)> {
        let row = self.query_conn()
            .query_row(
                "SELECT current_path, file_size, modified_unix, compression, head_hash, head_bytes, full_hash FROM sources WHERE source_id = ?",
                params![source_id],
                |r| {
                    let path: String = r.get(0)?;
                    let identity = SourceIdentity {
                        file_size: u64::try_from(r.get::<_, i64>(1)?).unwrap_or(0),
                        modified_unix: r.get(2)?,
                        compression: parse_compression(&r.get::<_, String>(3)?),
                        head_hash: r.get(4)?,
                        head_bytes: u64::try_from(r.get::<_, i64>(5)?).unwrap_or(0),
                        full_hash: r.get(6)?,
                    };
                    Ok((path, identity))
                },
            )
            .optional()?;
        row.ok_or_else(|| EngineError::Job(format!("파일 {source_id}이 등록되지 않음")))
    }

    /// 작업 정보를 읽는다.
    fn job(&self, job_id: i64) -> EngineResult<JobInfo> {
        self.query_conn()
            .query_row(
                &format!("{JOB_SELECT} WHERE job_id = ?"),
                params![job_id],
                map_job,
            )
            .optional()?
            .ok_or_else(|| EngineError::Job(format!("작업 {job_id}이 없음")))
    }

    /// 모든 작업(최신순).
    fn list_jobs(&self) -> EngineResult<Vec<JobInfo>> {
        let rows = self
            .query_conn()
            .prepare(&format!("{JOB_SELECT} ORDER BY job_id DESC"))?
            .query_map([], map_job)?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// 작업의 파일 목록(처리 순서).
    fn job_sources(&self, job_id: i64) -> EngineResult<Vec<JobSource>> {
        let rows = self.query_conn()
            .prepare(
                "SELECT js.source_id, js.ordinal, js.status, s.current_path FROM import_job_sources js JOIN sources s ON s.source_id = js.source_id WHERE js.job_id = ? ORDER BY js.ordinal",
            )?
            .query_map(params![job_id], |r| {
                Ok(JobSource {
                    source_id: r.get(0)?,
                    ordinal: r.get(1)?,
                    status: r.get(2)?,
                    path: r.get(3)?,
                })
            })?
            .collect::<Result<_, _>>()?;
        Ok(rows)
    }

    /// 작업의 최대 확정 배치 ID. 조회 범위 고정에 쓴다.
    fn max_committed_batch_id(&self, job_id: Option<i64>) -> EngineResult<i64> {
        let v: Option<i64> = match job_id {
            Some(id) => self.query_conn().query_row(
                "SELECT MAX(batch_id) FROM import_batches WHERE job_id = ?",
                duckdb::params![id],
                |r| r.get(0),
            )?,
            None => self.query_conn().query_row(
                "SELECT MAX(batch_id) FROM import_batches",
                [],
                |r| r.get(0),
            )?,
        };
        Ok(v.unwrap_or(0))
    }

    /// 한 페이지를 조회한다. 확정 배치 범위를 커서에 고정해 가져오기 중에도 페이지가 끼어들지 않게 한다.
    fn query_page(&self, req: &PageRequest) -> EngineResult<LogPage> {
        let (rows, next_cursor, approx_bytes) = self.page_generic(
            ROW_COLUMNS,
            read_row,
            &req.filter,
            req.sort,
            req.page_size,
            req.cursor.as_ref(),
            Some(MAX_PAGE_BYTES),
            MAX_PAGE_SIZE,
        )?;
        Ok(LogPage {
            rows,
            next_cursor,
            approx_bytes,
        })
    }

    /// 내보내기용 전체 컬럼 페이지. 바이트 상한은 두지 않고 행 수로만 자른다(디스크로 바로 흘려보낸다).
    fn export_page(
        &self,
        filter: &LogFilter,
        sort: SortOrder,
        page_size: u32,
        cursor: Option<&PageCursor>,
    ) -> EngineResult<(Vec<ExportRow>, Option<PageCursor>)> {
        let (rows, next, _) = self.page_generic(
            EXPORT_COLUMNS,
            read_export_row,
            filter,
            sort,
            page_size,
            cursor,
            None,
            MAX_EXPORT_PAGE_SIZE,
        )?;
        Ok((rows, next))
    }

    /// 커서 페이징 공통 구현. 확정 배치 범위를 커서에 고정해 가져오기 중에도 페이지가 끼어들지 않게 한다.
    #[doc(hidden)]
    #[allow(clippy::too_many_arguments)]
    fn page_generic<T: KeyedRow>(
        &self,
        columns: &str,
        map: fn(&duckdb::Row<'_>) -> duckdb::Result<T>,
        filter: &LogFilter,
        sort: SortOrder,
        page_size: u32,
        cursor: Option<&PageCursor>,
        byte_cap: Option<usize>,
        max_page_size: u32,
    ) -> EngineResult<(Vec<T>, Option<PageCursor>, usize)> {
        if page_size == 0 || page_size > max_page_size {
            return Err(EngineError::Query(format!(
                "page_size는 1~{max_page_size} 사이여야 함"
            )));
        }
        let hash = filter_hash(filter, sort)?;
        let (max_batch_id, segment, last_key) = match cursor {
            Some(c) => {
                if c.filter_hash != hash {
                    return Err(EngineError::Query(
                        "커서가 다른 조건으로 만들어짐".to_owned(),
                    ));
                }
                (
                    c.max_batch_id,
                    c.segment,
                    Some((c.last_ts, c.last_source_id, c.last_line)),
                )
            }
            None => (
                self.max_committed_batch_id(filter.job_id)?,
                CursorSegment::Timed,
                None,
            ),
        };
        let base = filter_sql(filter)?;
        let limit = page_size as usize + 1;
        let mut rows: Vec<T> = Vec::with_capacity(limit);
        let mut current_segment = segment;
        if current_segment == CursorSegment::Timed {
            rows.extend(fetch_timed(
                self.query_conn(),
                columns,
                map,
                &base,
                max_batch_id,
                sort,
                last_key,
                limit,
            )?);
            if rows.len() < limit {
                let remaining = limit - rows.len();
                rows.extend(fetch_null_time(
                    self.query_conn(),
                    columns,
                    map,
                    &base,
                    max_batch_id,
                    None,
                    remaining,
                )?);
                current_segment = CursorSegment::NullTime;
            }
        } else {
            let key = last_key.map(|(_, s, l)| (s, l));
            rows.extend(fetch_null_time(
                self.query_conn(),
                columns,
                map,
                &base,
                max_batch_id,
                key,
                limit,
            )?);
        }
        let has_more = rows.len() >= limit;
        rows.truncate(page_size as usize);
        let mut approx_bytes = 0;
        let mut cut = rows.len();
        for (i, row) in rows.iter().enumerate() {
            approx_bytes += row.approx_bytes();
            if byte_cap.is_some_and(|cap| approx_bytes > cap) && i > 0 {
                cut = i;
                break;
            }
        }
        let truncated_by_bytes = cut < rows.len();
        rows.truncate(cut);
        let next_cursor = if has_more || truncated_by_bytes {
            rows.last().map(|last| {
                let (ts, sid, line) = last.key();
                PageCursor {
                    filter_hash: hash,
                    max_batch_id,
                    segment: if ts.is_some() {
                        CursorSegment::Timed
                    } else {
                        current_segment
                    },
                    last_ts: ts,
                    last_source_id: sid,
                    last_line: line,
                }
            })
        } else {
            None
        };
        Ok((rows, next_cursor, approx_bytes))
    }

    /// 실행 중인 쿼리를 다른 스레드에서 중단할 수 있는 핸들.
    fn interrupt_handle(&self) -> std::sync::Arc<duckdb::InterruptHandle> {
        self.query_conn().interrupt_handle()
    }

    /// 조건에 맞는 전체 건수. 페이지 조회와 분리된 별도 요청이다.
    fn count_matching(&self, filter: &LogFilter) -> EngineResult<i64> {
        let base = filter_sql(filter)?;
        let sql = format!("SELECT COUNT(*) FROM logs WHERE {}", base.where_sql);
        let n: i64 = self
            .query_conn()
            .query_row(&sql, params_from_iter(base.params), |r| r.get(0))?;
        Ok(n)
    }

    /// 한 레코드의 상세. 같은 파일·줄에 여러 결과 버전이 있으면 `job_id`로 구분한다.
    fn detail(
        &self,
        job_id: Option<i64>,
        source_id: i64,
        line_number: i64,
    ) -> EngineResult<Option<LogDetail>> {
        let mut sql = String::from(
            "SELECT l.job_id, l.batch_id, l.source_id, s.current_path, l.line_number, epoch_us(l.timestamp_utc), l.tz_offset_seconds, l.client_ip, l.method, l.request_target, l.protocol, l.status, l.bytes_sent, l.referrer, l.user_agent, l.extra_json FROM logs l JOIN sources s ON s.source_id = l.source_id WHERE l.source_id = ? AND l.line_number = ?",
        );
        let mut params = vec![Value::BigInt(source_id), Value::BigInt(line_number)];
        if let Some(j) = job_id {
            sql.push_str(" AND l.job_id = ?");
            params.push(Value::BigInt(j));
        }
        sql.push_str(" ORDER BY l.job_id DESC LIMIT 1");
        Ok(self
            .query_conn()
            .query_row(&sql, params_from_iter(params), |r| {
                Ok(LogDetail {
                    job_id: r.get(0)?,
                    batch_id: r.get(1)?,
                    source_id: r.get(2)?,
                    source_path: r.get(3)?,
                    line_number: r.get(4)?,
                    timestamp_utc: r.get(5)?,
                    tz_offset_seconds: r.get(6)?,
                    client_ip: r.get(7)?,
                    method: r.get(8)?,
                    request_target: r.get(9)?,
                    protocol: r.get(10)?,
                    status: r.get(11)?,
                    bytes_sent: r.get(12)?,
                    referrer: r.get(13)?,
                    user_agent: r.get(14)?,
                    extra_json: r.get(15)?,
                })
            })
            .optional()?)
    }
}

#[allow(clippy::too_many_arguments)]
fn fetch_timed<T>(
    conn: &duckdb::Connection,
    columns: &str,
    map: fn(&duckdb::Row<'_>) -> duckdb::Result<T>,
    base: &SqlParts,
    max_batch_id: i64,
    sort: SortOrder,
    last_key: Option<(Option<i64>, i64, i64)>,
    limit: usize,
) -> EngineResult<Vec<T>> {
    let (cmp, dir) = match sort {
        SortOrder::TimeAsc => (">", "ASC"),
        SortOrder::TimeDesc => ("<", "DESC"),
    };
    let mut sql = format!(
        "SELECT {columns} FROM logs WHERE {} AND batch_id <= ? AND timestamp_utc IS NOT NULL",
        base.where_sql
    );
    let mut params = base.params.clone();
    params.push(Value::BigInt(max_batch_id));
    if let Some((Some(ts), sid, line)) = last_key {
        sql.push_str(&format!(
            " AND (timestamp_utc {cmp} make_timestamp(?) OR (timestamp_utc = make_timestamp(?) AND (source_id > ? OR (source_id = ? AND line_number > ?))))"
        ));
        params.extend([
            Value::BigInt(ts),
            Value::BigInt(ts),
            Value::BigInt(sid),
            Value::BigInt(sid),
            Value::BigInt(line),
        ]);
    }
    sql.push_str(&format!(
        " ORDER BY timestamp_utc {dir}, source_id ASC, line_number ASC LIMIT ?"
    ));
    params.push(Value::BigInt(limit as i64));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(params), map)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn fetch_null_time<T>(
    conn: &duckdb::Connection,
    columns: &str,
    map: fn(&duckdb::Row<'_>) -> duckdb::Result<T>,
    base: &SqlParts,
    max_batch_id: i64,
    last_key: Option<(i64, i64)>,
    limit: usize,
) -> EngineResult<Vec<T>> {
    let mut sql = format!(
        "SELECT {columns} FROM logs WHERE {} AND batch_id <= ? AND timestamp_utc IS NULL",
        base.where_sql
    );
    let mut params = base.params.clone();
    params.push(Value::BigInt(max_batch_id));
    if let Some((sid, line)) = last_key {
        sql.push_str(" AND (source_id > ? OR (source_id = ? AND line_number > ?))");
        params.extend([Value::BigInt(sid), Value::BigInt(sid), Value::BigInt(line)]);
    }
    sql.push_str(" ORDER BY source_id ASC, line_number ASC LIMIT ?");
    params.push(Value::BigInt(limit as i64));
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt
        .query_map(params_from_iter(params), map)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

impl LogQuery for Store {
    fn query_conn(&self) -> &duckdb::Connection {
        Store::conn(self)
    }
}

impl LogQuery for Reader {
    fn query_conn(&self) -> &duckdb::Connection {
        &self.conn
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::parse::LogRecord;
    use crate::store::{PendingBatch, StoreConfig};

    /// 시간이 뒤섞인 6행 + NULL 시간 2행을 두 배치로 넣는다.
    fn seeded_store() -> (Store, i64) {
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let profile_id = store
            .upsert_profile(&crate::format::presets::apache_combined())
            .unwrap();
        store
            .conn()
            .execute(
                "INSERT INTO sources (source_id, original_path, current_path, file_size, encoding, compression, head_hash, head_bytes) VALUES (1, 'a.log', 'a.log', 0, 'utf-8', 'none', '0', 0)",
                [],
            )
            .unwrap();
        let job = store
            .create_job(profile_id, &[1], None, LogKind::Access)
            .unwrap();
        let mk = |line: u64, ts: Option<i64>, status: u16| LogRecord {
            line_number: line,
            timestamp_utc: ts,
            status: Some(status),
            client_ip: Some("10.0.0.1".to_owned()),
            request_target: Some(format!("/p{line}")),
            ..LogRecord::default()
        };
        let b0 = PendingBatch {
            job_id: job.job_id,
            source_id: 1,
            batch_seq: 0,
            records: vec![
                mk(1, Some(300), 200),
                mk(2, Some(100), 500),
                mk(3, Some(200), 200),
                mk(4, None, 404),
            ],
            ..PendingBatch::default()
        };
        let b1 = PendingBatch {
            job_id: job.job_id,
            source_id: 1,
            batch_seq: 1,
            records: vec![
                mk(5, Some(100), 200),
                mk(6, Some(400), 200),
                mk(7, Some(50), 500),
                mk(8, None, 200),
            ],
            ..PendingBatch::default()
        };
        store.commit_batch(&b0).unwrap();
        store.commit_batch(&b1).unwrap();
        (store, job.job_id)
    }

    fn all_pages(store: &Store, mut req: PageRequest) -> Vec<Vec<LogRow>> {
        let mut pages = Vec::new();
        loop {
            let page = store.query_page(&req).unwrap();
            let next = page.next_cursor.clone();
            pages.push(page.rows);
            match next {
                Some(c) => req.cursor = Some(c),
                None => return pages,
            }
        }
    }

    fn req(job_id: i64, page_size: u32, sort: SortOrder) -> PageRequest {
        PageRequest {
            filter: LogFilter {
                job_id: Some(job_id),
                ..LogFilter::default()
            },
            sort,
            page_size,
            cursor: None,
        }
    }

    #[test]
    fn bookmarks_toggle_show_on_rows_and_filter() {
        let (mut store, job) = seeded_store();
        assert!(store.toggle_bookmark(1, 3).unwrap(), "처음 누르면 북마크됨");
        assert!(store.toggle_bookmark(1, 7).unwrap());
        assert!(!store.toggle_bookmark(1, 7).unwrap(), "다시 누르면 해제");
        let r = req(job, 100, SortOrder::TimeAsc);
        let rows: Vec<LogRow> = all_pages(&store, r.clone()).into_iter().flatten().collect();
        let marked: Vec<i64> = rows
            .iter()
            .filter(|x| x.bookmarked)
            .map(|x| x.line_number)
            .collect();
        assert_eq!(marked, vec![3]);
        let mut only = r.clone();
        only.filter.bookmarked_only = true;
        only.filter.status = Some(500);
        assert_eq!(
            store.count_matching(&only.filter).unwrap(),
            0,
            "다른 조건과 AND"
        );
        only.filter.status = None;
        assert_eq!(store.count_matching(&only.filter).unwrap(), 1);
        let _ = &mut store;
    }

    /// 접근 로그 파일(source 1)과 에러 로그 파일(source 2)을 서로 다른 작업으로 넣는다.
    fn kinded_store() -> (Store, i64, i64) {
        let mut store = Store::open_in_memory(&StoreConfig::default()).unwrap();
        let profile_id = store
            .upsert_profile(&crate::format::presets::apache_combined())
            .unwrap();
        for (id, path) in [(1i64, "access.log"), (2, "error.log")] {
            store
                .conn()
                .execute(
                    "INSERT INTO sources (source_id, original_path, current_path, file_size, encoding, compression, head_hash, head_bytes) VALUES (?, ?, ?, 0, 'utf-8', 'none', '0', 0)",
                    params![id, path, path],
                )
                .unwrap();
        }
        let access = store
            .create_job(profile_id, &[1], None, LogKind::Access)
            .unwrap();
        let error = store
            .create_job(profile_id, &[2], None, LogKind::Error)
            .unwrap();
        let access_rec = |line: u64| LogRecord {
            line_number: line,
            timestamp_utc: Some(100 * i64::try_from(line).unwrap_or(0)),
            status: Some(200),
            client_ip: Some("10.0.0.1".to_owned()),
            request_target: Some(format!("/p{line}")),
            ..LogRecord::default()
        };
        let error_rec = |line: u64, level: &str, message: &str| LogRecord {
            line_number: line,
            timestamp_utc: Some(100 * i64::try_from(line).unwrap_or(0)),
            client_ip: Some("10.0.2.7".to_owned()),
            extra: [
                ("level".to_owned(), level.to_owned()),
                ("message".to_owned(), message.to_owned()),
            ]
            .into_iter()
            .collect(),
            ..LogRecord::default()
        };
        store
            .commit_batch(&PendingBatch {
                job_id: access.job_id,
                source_id: 1,
                batch_seq: 0,
                records: vec![access_rec(1), access_rec(2), access_rec(3)],
                ..PendingBatch::default()
            })
            .unwrap();
        store
            .commit_batch(&PendingBatch {
                job_id: error.job_id,
                source_id: 2,
                batch_seq: 0,
                records: vec![
                    error_rec(1, "error", "open() \"/var/www/x\" failed (2: No such file)"),
                    error_rec(2, "warn", "upstream sent too big header"),
                ],
                ..PendingBatch::default()
            })
            .unwrap();
        (store, access.job_id, error.job_id)
    }

    #[test]
    fn log_kind_filter_separates_access_and_error() {
        let (store, access, error) = kinded_store();
        let kinded = |kind: Option<LogKind>| LogFilter {
            log_kind: kind,
            ..LogFilter::default()
        };
        assert_eq!(store.count_matching(&kinded(None)).unwrap(), 5, "전체");
        assert_eq!(
            store
                .count_matching(&kinded(Some(LogKind::Access)))
                .unwrap(),
            3
        );
        assert_eq!(
            store.count_matching(&kinded(Some(LogKind::Error))).unwrap(),
            2
        );
        let page = store
            .query_page(&PageRequest {
                filter: kinded(Some(LogKind::Error)),
                sort: SortOrder::TimeAsc,
                page_size: 100,
                cursor: None,
            })
            .unwrap();
        assert!(
            page.rows.iter().all(|r| r.source_id == 2),
            "에러 작업의 파일만 나온다"
        );
        assert_eq!(store.job(access).unwrap().log_kind, LogKind::Access);
        assert_eq!(store.job(error).unwrap().log_kind, LogKind::Error);
    }

    #[test]
    fn page_rows_expose_extra_json() {
        let (store, _, _) = kinded_store();
        let page = store
            .query_page(&PageRequest {
                filter: LogFilter {
                    log_kind: Some(LogKind::Error),
                    ..LogFilter::default()
                },
                sort: SortOrder::TimeAsc,
                page_size: 100,
                cursor: None,
            })
            .unwrap();
        let first = page.rows.first().expect("에러 행");
        let extra = first.extra_json.as_deref().expect("확장 필드 JSON");
        assert!(extra.contains("\"level\":\"error\""), "레벨 노출: {extra}");
        assert!(extra.contains("open()"), "메시지 노출: {extra}");
        let access = store
            .query_page(&PageRequest {
                filter: LogFilter {
                    log_kind: Some(LogKind::Access),
                    ..LogFilter::default()
                },
                sort: SortOrder::TimeAsc,
                page_size: 100,
                cursor: None,
            })
            .unwrap();
        assert!(
            access.rows.iter().all(|r| r.extra_json.is_none()),
            "확장 필드가 없으면 NULL"
        );
    }

    #[test]
    fn extra_field_search_covers_message_and_client() {
        let (store, _, _) = kinded_store();
        let cond = |field, op, value: &str| FilterExpr::Cond {
            field,
            op,
            value: value.to_owned(),
        };
        let search = |v: &str| LogFilter {
            log_kind: Some(LogKind::Error),
            // 화면의 에러 검색: 메시지(확장 필드) 또는 클라이언트 IP.
            expr: Some(FilterExpr::Or {
                items: vec![
                    cond(CondField::Extra, CondOp::Icontains, v),
                    cond(CondField::ClientIp, CondOp::Eq, v),
                ],
            }),
            ..LogFilter::default()
        };
        assert_eq!(
            store.count_matching(&search("OPEN()")).unwrap(),
            1,
            "대소문자 무시"
        );
        assert_eq!(store.count_matching(&search("upstream")).unwrap(), 1);
        assert_eq!(
            store.count_matching(&search("10.0.2.7")).unwrap(),
            2,
            "IP는 정확히 일치"
        );
        assert_eq!(store.count_matching(&search("없는문자열")).unwrap(), 0);
        let too_long = LogFilter {
            expr: Some(cond(
                CondField::Extra,
                CondOp::Icontains,
                &"x".repeat(MAX_SEARCH_BYTES + 1),
            )),
            ..LogFilter::default()
        };
        assert!(
            store.count_matching(&too_long).is_err(),
            "검색 문자열 상한을 넘으면 오류"
        );
    }

    #[test]
    fn level_and_message_conditions_target_error_fields() {
        let (store, _, _) = kinded_store();
        let cond = |field, op, value: &str| FilterExpr::Cond {
            field,
            op,
            value: value.to_owned(),
        };
        let with = |expr: FilterExpr| LogFilter {
            expr: Some(expr),
            ..LogFilter::default()
        };
        // 전체 5행 중 에러 로그 2행만 레벨을 가진다.
        assert_eq!(
            store
                .count_matching(&with(cond(CondField::Level, CondOp::Eq, "error")))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .count_matching(&with(cond(CondField::Level, CondOp::Eq, "warn")))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .count_matching(&with(cond(CondField::Level, CondOp::IsNull, "")))
                .unwrap(),
            3,
            "접근 로그 행은 확장 필드가 NULL이라 레벨이 없다"
        );
        assert_eq!(
            store
                .count_matching(&with(cond(
                    CondField::Message,
                    CondOp::Icontains,
                    "TOO BIG HEADER"
                )))
                .unwrap(),
            1,
            "메시지 부분 문자열은 대소문자를 무시한다"
        );
        assert_eq!(
            store
                .count_matching(&with(cond(
                    CondField::Message,
                    CondOp::StartsWith,
                    "open()"
                )))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .count_matching(&with(cond(
                    CondField::Message,
                    CondOp::Regex,
                    "failed \\(\\d+:"
                )))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .count_matching(&with(cond(
                    CondField::Message,
                    CondOp::Ne,
                    "upstream sent too big header"
                )))
                .unwrap(),
            1,
            "값이 없는 접근 로그 행은 잡히지 않는다"
        );
        // 크기 비교는 문자열 컬럼에 쓸 수 없고, 오류 메시지에는 SQL 식이 아니라 필드 이름이 나온다.
        let err = store
            .count_matching(&with(cond(CondField::Level, CondOp::Gt, "error")))
            .unwrap_err();
        let text = err.to_string();
        assert!(text.contains("level"), "필드 이름을 알려야 함: {text}");
        assert!(!text.contains("json_extract"), "SQL 식 노출 금지: {text}");
    }

    #[test]
    fn expr_combines_and_or_not_and_validates_types() {
        let (store, job) = seeded_store();
        let cond = |field, op, value: &str| FilterExpr::Cond {
            field,
            op,
            value: value.to_owned(),
        };
        let mut r = req(job, 100, SortOrder::TimeAsc);
        // (status >= 500) or (path == /p1 and not path endswith 3)
        r.filter.expr = Some(FilterExpr::Or {
            items: vec![
                cond(CondField::Status, CondOp::Gte, "500"),
                FilterExpr::And {
                    items: vec![
                        cond(CondField::RequestTarget, CondOp::Eq, "/p1"),
                        FilterExpr::Not {
                            item: Box::new(cond(CondField::RequestTarget, CondOp::EndsWith, "3")),
                        },
                    ],
                },
            ],
        });
        let mut lines: Vec<i64> = all_pages(&store, r.clone())
            .into_iter()
            .flatten()
            .map(|x| x.line_number)
            .collect();
        lines.sort_unstable();
        assert_eq!(lines, vec![1, 2, 7]);

        r.filter.expr = Some(cond(CondField::RequestTarget, CondOp::Icontains, "/P"));
        assert_eq!(store.count_matching(&r.filter).unwrap(), 8);
        r.filter.expr = Some(cond(CondField::ClientIp, CondOp::IsNull, ""));
        assert_eq!(store.count_matching(&r.filter).unwrap(), 0);
        r.filter.expr = Some(FilterExpr::And { items: vec![] });
        assert_eq!(store.count_matching(&r.filter).unwrap(), 8);

        r.filter.expr = Some(cond(CondField::Status, CondOp::Gte, "secret-value"));
        let err = store.query_page(&r).unwrap_err();
        assert!(matches!(err, EngineError::Query(_)));
        let text = err.to_string();
        assert!(text.contains("status"), "어느 필드인지 알려야 함: {text}");
        assert!(
            !text.contains("secret-value"),
            "입력값을 오류 메시지에 남기지 않는다: {text}"
        );
        r.filter.expr = Some(cond(CondField::Method, CondOp::Gt, "GET"));
        assert!(matches!(
            store.query_page(&r).unwrap_err(),
            EngineError::Query(_)
        ));
        r.filter.expr = Some(cond(CondField::Status, CondOp::Contains, "4"));
        assert!(matches!(
            store.query_page(&r).unwrap_err(),
            EngineError::Query(_)
        ));
        r.filter.expr = Some(cond(CondField::UserAgent, CondOp::Regex, "("));
        assert!(matches!(
            store.query_page(&r).unwrap_err(),
            EngineError::Query(_)
        ));
    }

    #[test]
    fn target_regex_filters_rows_and_rejects_bad_patterns() {
        let (store, job) = seeded_store();
        let mut r = req(job, 100, SortOrder::TimeAsc);
        r.filter.target_regex = Some("^/p[13]$".to_owned());
        let rows: Vec<LogRow> = all_pages(&store, r.clone()).into_iter().flatten().collect();
        let mut lines: Vec<i64> = rows.iter().map(|x| x.line_number).collect();
        lines.sort_unstable();
        assert_eq!(lines, vec![1, 3]);
        assert_eq!(store.count_matching(&r.filter).unwrap(), 2);
        r.filter.target_regex = Some("(?i)^/P1$".to_owned());
        assert_eq!(store.count_matching(&r.filter).unwrap(), 1);
        r.filter.target_regex = Some("(".to_owned());
        assert!(matches!(
            store.query_page(&r).unwrap_err(),
            EngineError::Query(_)
        ));
    }

    #[test]
    fn pages_cover_all_rows_in_time_order_without_duplicates_and_nulls_last() {
        let (store, job_id) = seeded_store();
        let pages = all_pages(&store, req(job_id, 3, SortOrder::TimeAsc));
        let keys: Vec<(Option<i64>, i64)> = pages
            .iter()
            .flatten()
            .map(|r| (r.timestamp_utc, r.line_number))
            .collect();
        assert_eq!(
            keys,
            vec![
                (Some(50), 7),
                (Some(100), 2),
                (Some(100), 5),
                (Some(200), 3),
                (Some(300), 1),
                (Some(400), 6),
                (None, 4),
                (None, 8)
            ]
        );
        assert_eq!(pages.len(), 3);
    }

    #[test]
    fn descending_order_keeps_nulls_last() {
        let (store, job_id) = seeded_store();
        let keys: Vec<i64> = all_pages(&store, req(job_id, 5, SortOrder::TimeDesc))
            .into_iter()
            .flatten()
            .map(|r| r.line_number)
            .collect();
        assert_eq!(keys, vec![6, 1, 3, 2, 5, 7, 4, 8]);
    }

    #[test]
    fn status_filter_is_applied() {
        let (store, job_id) = seeded_store();
        let mut r = req(job_id, 10, SortOrder::TimeAsc);
        r.filter.status = Some(500);
        let rows = store.query_page(&r).unwrap().rows;
        assert_eq!(
            rows.iter().map(|r| r.line_number).collect::<Vec<_>>(),
            vec![7, 2]
        );
        assert_eq!(store.count_matching(&r.filter).unwrap(), 2);
    }

    #[test]
    fn time_range_filter_excludes_null_time_rows() {
        let (store, job_id) = seeded_store();
        let mut r = req(job_id, 10, SortOrder::TimeAsc);
        r.filter.time_from_micros = Some(100);
        r.filter.time_to_micros = Some(300);
        let lines: Vec<i64> = store
            .query_page(&r)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.line_number)
            .collect();
        assert_eq!(lines, vec![2, 5, 3]);
    }

    #[test]
    fn cursor_from_different_filter_is_rejected() {
        let (store, job_id) = seeded_store();
        let first = store
            .query_page(&req(job_id, 2, SortOrder::TimeAsc))
            .unwrap();
        let mut other = req(job_id, 2, SortOrder::TimeDesc);
        other.cursor = first.next_cursor;
        assert!(matches!(
            store.query_page(&other),
            Err(EngineError::Query(_))
        ));
    }

    #[test]
    fn cursor_freezes_committed_batch_range_while_import_continues() {
        let (mut store, job_id) = seeded_store();
        let first = store
            .query_page(&req(job_id, 2, SortOrder::TimeAsc))
            .unwrap();
        let cursor = first.next_cursor.clone().unwrap();
        let late = PendingBatch {
            job_id,
            source_id: 1,
            batch_seq: 2,
            records: vec![LogRecord {
                line_number: 9,
                timestamp_utc: Some(150),
                ..LogRecord::default()
            }],
            ..PendingBatch::default()
        };
        store.commit_batch(&late).unwrap();
        let mut next = req(job_id, 100, SortOrder::TimeAsc);
        next.cursor = Some(cursor);
        let lines: Vec<i64> = store
            .query_page(&next)
            .unwrap()
            .rows
            .iter()
            .map(|r| r.line_number)
            .collect();
        assert!(
            !lines.contains(&9),
            "row from a batch committed after the cursor must not appear"
        );
        let fresh: Vec<i64> = store
            .query_page(&req(job_id, 100, SortOrder::TimeAsc))
            .unwrap()
            .rows
            .iter()
            .map(|r| r.line_number)
            .collect();
        assert!(fresh.contains(&9));
    }

    #[test]
    fn page_size_outside_limits_is_rejected() {
        let (store, job_id) = seeded_store();
        assert!(store
            .query_page(&req(job_id, 0, SortOrder::TimeAsc))
            .is_err());
        assert!(store
            .query_page(&req(job_id, MAX_PAGE_SIZE + 1, SortOrder::TimeAsc))
            .is_err());
    }

    #[test]
    fn detail_returns_full_record_with_source_path() {
        let (store, job_id) = seeded_store();
        let d = store.detail(Some(job_id), 1, 3).unwrap().unwrap();
        assert_eq!(d.request_target.as_deref(), Some("/p3"));
        assert_eq!(d.timestamp_utc, Some(200));
        assert!(store.detail(Some(job_id), 1, 999).unwrap().is_none());
    }
}
