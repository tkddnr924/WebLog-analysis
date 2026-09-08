//! 정규화된 레코드와 줄 처리 결과.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 정규화된 로그 레코드. 원문은 담지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct LogRecord {
    /// 파일 내 논리 줄 번호(1부터).
    pub line_number: u64,
    /// UTC 타임스탬프(마이크로초). 시간대 미확정이면 `None`.
    pub timestamp_utc: Option<i64>,
    /// 해석에 사용한 UTC 오프셋(초). 정책·입력 어디에서 왔든 실제 적용값을 보존한다.
    pub tz_offset_seconds: Option<i32>,
    /// 클라이언트 IP 원문(검증됨).
    pub client_ip: Option<String>,
    /// HTTP 메서드.
    pub method: Option<String>,
    /// 요청 대상(경로+쿼리, 디코딩하지 않음).
    pub request_target: Option<String>,
    /// 프로토콜.
    pub protocol: Option<String>,
    /// 상태코드.
    pub status: Option<u16>,
    /// 전송 바이트. 누락은 `None`이며 0으로 바꾸지 않는다.
    pub bytes_sent: Option<i64>,
    /// Referrer.
    pub referrer: Option<String>,
    /// User-Agent.
    pub user_agent: Option<String>,
    /// 확장 필드(사용자 정의·비표준 W3C 필드).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub extra: BTreeMap<String, String>,
}

impl LogRecord {
    /// 레코드가 차지하는 대략적 바이트 수. 배치 바이트 상한 계산에 쓴다.
    pub fn approx_bytes(&self) -> usize {
        let s = |v: &Option<String>| v.as_ref().map_or(0, String::len);
        64 + s(&self.client_ip)
            + s(&self.method)
            + s(&self.request_target)
            + s(&self.protocol)
            + s(&self.referrer)
            + s(&self.user_agent)
            + self
                .extra
                .iter()
                .map(|(k, v)| k.len() + v.len() + 8)
                .sum::<usize>()
    }
}

/// 파싱 실패 코드. 입력 내용을 포함하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParseErrorCode {
    /// 포맷 정규식에 매칭되지 않음.
    NoMatch,
    /// UTF-8이 아닌 바이트 포함. 손실 변환하지 않고 실패로 기록한다.
    InvalidUtf8,
    /// 줄 길이가 상한을 넘음.
    LineTooLong,
    /// IP 형식이 아님.
    InvalidIp,
    /// 타임스탬프 형식이 아님.
    InvalidTimestamp,
    /// 정수 형식이 아님.
    InvalidInteger,
    /// 상태코드 범위(100~599) 밖.
    InvalidStatus,
    /// W3C: `#Fields` 헤더 이전에 데이터 줄이 나옴.
    HeaderMissing,
    /// W3C: 필드 수가 헤더와 다름.
    FieldCountMismatch,
    /// W3C: 헤더 지시문 형식 오류.
    InvalidDirective,
}

impl ParseErrorCode {
    /// 저장용 문자열 코드.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoMatch => "no_match",
            Self::InvalidUtf8 => "invalid_utf8",
            Self::LineTooLong => "line_too_long",
            Self::InvalidIp => "invalid_ip",
            Self::InvalidTimestamp => "invalid_timestamp",
            Self::InvalidInteger => "invalid_integer",
            Self::InvalidStatus => "invalid_status",
            Self::HeaderMissing => "header_missing",
            Self::FieldCountMismatch => "field_count_mismatch",
            Self::InvalidDirective => "invalid_directive",
        }
    }
}

/// 제외 사유. 실패로 집계하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkipReason {
    /// 빈 줄.
    Blank,
    /// 주석·헤더 지시문.
    Directive,
    /// 앞 레코드에 이어지는 줄(스택 트레이스 등). 멀티라인은 지원하지 않으므로 건너뛴다.
    Continuation,
}

/// 한 줄의 처리 결과.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LineOutcome {
    /// 정상 레코드.
    Record(LogRecord),
    /// 실패. 위치와 코드만 남긴다.
    Error {
        /// 줄 번호.
        line_number: u64,
        /// 오류 코드.
        code: ParseErrorCode,
        /// 오류 대상 필드 이름 등 입력 내용이 아닌 부가 설명.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<String>,
    },
    /// 제외.
    Skipped {
        /// 줄 번호.
        line_number: u64,
        /// 사유.
        reason: SkipReason,
    },
}

impl LineOutcome {
    /// 실패 결과를 만든다.
    pub fn error(line_number: u64, code: ParseErrorCode, field: Option<&str>) -> Self {
        Self::Error {
            line_number,
            code,
            field: field.map(str::to_owned),
        }
    }
}
