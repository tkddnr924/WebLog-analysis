//! 버전이 있는 구조화된 포맷 정의. 이 정의가 유일한 기준이며 퍼즐·YAML은 이를 편집한다.

use serde::{Deserialize, Serialize};

/// 현재 포맷 정의 스키마 버전.
pub const SCHEMA_VERSION: u32 = 1;

/// 서버 힌트. 실제 포맷과 분리한다. Unknown이어도 포맷이 확인되면 파싱한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerHint {
    /// Apache HTTP Server.
    Apache,
    /// Nginx.
    Nginx,
    /// Microsoft IIS.
    Iis,
    /// 알 수 없음.
    Unknown,
}

/// 시간대 해석 정책. PC 시간대로 조용히 해석하지 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimezonePolicy {
    /// 입력에 오프셋이 있으면 사용하고, 없으면 미확정(NULL)으로 둔다.
    FromInput,
    /// 입력에 오프셋이 없을 때 이 고정 오프셋(초)을 적용한다. 있으면 입력을 우선한다.
    Fixed {
        /// UTC 기준 오프셋(초). 동쪽이 양수.
        offset_seconds: i32,
    },
    /// 입력에 오프셋이 없으면 UTC로 간주한다. W3C 확장 로그의 기본값이다.
    Utc,
}

/// 타임스탬프 문자열 형식.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TimestampFormat {
    /// Common Log Format: `10/Oct/2000:13:55:36 -0700`. 오프셋은 생략될 수 있다.
    Clf,
    /// ISO 8601 / RFC 3339. 오프셋은 생략될 수 있다.
    Iso8601,
    /// chrono strftime 형식. `%z`가 있으면 오프셋을 읽는다.
    Custom {
        /// strftime 패턴.
        pattern: String,
    },
}

/// 필드의 의미 종류. 표준 필드는 logs 타입 컬럼으로, 나머지는 확장 영역으로 간다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FieldKind {
    /// 클라이언트 IP(v4/v6). 검증 후 원문 그대로 보존한다.
    ClientIp,
    /// 타임스탬프.
    Timestamp {
        /// 문자열 형식.
        format: TimestampFormat,
    },
    /// `GET /path HTTP/1.1` 형태의 요청문. method/target/protocol로 분해한다.
    RequestLine,
    /// HTTP 메서드.
    Method,
    /// 요청 대상(경로+쿼리). 디코딩하지 않는다.
    RequestTarget,
    /// 프로토콜 버전.
    Protocol,
    /// 상태코드 100~599.
    Status,
    /// 전송 바이트 수.
    BytesSent,
    /// Referrer.
    Referrer,
    /// User-Agent.
    UserAgent,
    /// 확장 영역에 저장하는 정수.
    Integer,
    /// 확장 영역에 저장하는 문자열.
    Text,
}

/// 원문 구간을 어떻게 잘라낼지. 의미 검증은 [`FieldKind`]가 담당한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Capture {
    /// 공백이 아닌 연속 문자.
    Token,
    /// 큰따옴표로 감싼 문자열. `\"` 이스케이프를 허용한다. 따옴표는 값에서 제외한다.
    Quoted,
    /// 대괄호로 감싼 문자열. 대괄호는 값에서 제외한다.
    Bracketed,
    /// 사용자 정규식 조각. 그룹은 캡처하지 않는 형태여야 한다.
    Pattern {
        /// Rust regex 문법 패턴.
        pattern: String,
    },
}

/// 필드 정의.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldDef {
    /// 필드 이름. 확장 필드는 이 이름으로 저장된다.
    pub name: String,
    /// 의미 종류.
    pub kind: FieldKind,
    /// 추출 방식.
    pub capture: Capture,
    /// 누락값으로 간주할 문자열 목록(예: `-`). 누락은 NULL이며 0으로 바꾸지 않는다.
    #[serde(default)]
    pub missing: Vec<String>,
}

/// 퍼즐 블록.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "block", rename_all = "snake_case")]
pub enum Block {
    /// 고정 문자열.
    Literal {
        /// 문자열.
        text: String,
    },
    /// 하나 이상의 공백/탭.
    Whitespace,
    /// 필드.
    Field(FieldDef),
    /// 있어도 되고 없어도 되는 블록 묶음.
    OptionalGroup {
        /// 하위 블록.
        blocks: Vec<Block>,
    },
    /// 고급 정규식 블록. 이름 있는 캡처 `(?P<name>...)`는 확장 필드가 된다.
    Regex {
        /// Rust regex 문법 패턴.
        pattern: String,
    },
}

/// 파싱 전략.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Strategy {
    /// 블록 조립을 한 개의 정규식으로 컴파일해 한 줄씩 매칭한다.
    Blocks {
        /// 블록 목록.
        blocks: Vec<Block>,
    },
    /// IIS/W3C 확장 로그. `#Fields:` 헤더에 따라 필드를 매핑하며 도중 헤더 변경을 반영한다.
    W3c,
}

/// 포맷 프로필.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormatProfile {
    /// 정의 스키마 버전.
    pub schema_version: u32,
    /// 표시 이름.
    pub name: String,
    /// 프로필 버전. 편집 시 증가한다.
    pub version: u32,
    /// 서버 힌트.
    pub server_hint: ServerHint,
    /// 시간대 정책.
    pub timezone: TimezonePolicy,
    /// 파싱 전략.
    pub strategy: Strategy,
}

impl FormatProfile {
    /// JSON 문자열로 직렬화한다. 저장소의 정의 스냅샷에 사용한다.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// JSON 문자열에서 복원한다.
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// 정의 내용을 대표하는 해시(FNV-1a 64비트, 16진수). 같은 정의는 같은 해시를 가진다.
    pub fn definition_hash(&self) -> Result<String, serde_json::Error> {
        let json = serde_json::to_vec(self)?;
        Ok(fnv1a_hex(&json))
    }
}

/// FNV-1a 64비트 해시. 암호학적 용도가 아니라 정의 동일성 판별용이다.
pub(crate) fn fnv1a_hex(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn profile_json_roundtrip_preserves_definition() {
        let profile = crate::format::presets::apache_combined();
        let json = profile.to_json().unwrap();
        let back = FormatProfile::from_json(&json).unwrap();
        assert_eq!(profile, back);
    }

    #[test]
    fn definition_hash_changes_when_definition_changes() {
        let a = crate::format::presets::apache_combined();
        let mut b = a.clone();
        b.timezone = TimezonePolicy::Utc;
        assert_ne!(a.definition_hash().unwrap(), b.definition_hash().unwrap());
    }
}
