//! 엔진 공통 오류 타입. 메시지에 입력 원문을 포함하지 않는다.

/// 엔진 전역 오류.
#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    /// 파일 열기·읽기 등 I/O 실패.
    #[error("I/O 오류: {0}")]
    Io(#[from] std::io::Error),
    /// 포맷 정의가 유효하지 않음.
    #[error("포맷 정의 오류: {0}")]
    Format(String),
    /// 포맷 정의로부터 만든 정규식 컴파일 실패.
    #[error("정규식 컴파일 실패: {0}")]
    Regex(#[from] regex::Error),
    /// DuckDB 저장소 오류.
    #[error("저장소 오류: {0}")]
    Store(#[from] duckdb::Error),
    /// 저장소 스키마 버전이 엔진과 맞지 않음.
    #[error("스키마 버전 불일치: 저장소 {found}, 엔진 {expected}")]
    SchemaVersion {
        /// 저장소에 기록된 버전.
        found: i64,
        /// 엔진이 기대하는 버전.
        expected: i64,
    },
    /// 작업 상태 전이·식별 오류.
    #[error("작업 오류: {0}")]
    Job(String),
    /// 조회 요청이 계약을 위반함.
    #[error("조회 요청 오류: {0}")]
    Query(String),
    /// 배치 오류 저장 한도 초과. 조용히 버리지 않고 작업을 중단한다.
    #[error("오류 저장 한도 초과: 배치 오류 {count}건이 한도 {limit}건을 넘음")]
    ErrorLimit {
        /// 발생한 오류 수.
        count: usize,
        /// 허용 한도.
        limit: usize,
    },
    /// 크기·개수 한도 초과.
    #[error("한도 초과: {0}")]
    Limit(String),
    /// 입력 파일이 등록 당시와 달라짐(크기·수정 시각·내용 해시 불일치).
    #[error("입력 파일 변경 감지(source {source_id}): {reason}")]
    SourceChanged {
        /// 파일 ID.
        source_id: i64,
        /// 불일치 항목 설명(입력 내용 없음).
        reason: String,
    },
    /// JSON 직렬화 오류.
    #[error("직렬화 오류: {0}")]
    Serde(#[from] serde_json::Error),
}

/// 엔진 결과 타입.
pub type EngineResult<T> = Result<T, EngineError>;
