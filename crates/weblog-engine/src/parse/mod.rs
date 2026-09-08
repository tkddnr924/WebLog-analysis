//! 한 줄 파서. 정의를 실행 계획으로 컴파일하고 줄마다 [`LineOutcome`]을 낸다.
//! 미리보기와 실제 가져오기가 같은 코드를 쓴다.

pub mod blocks;
pub mod record;
pub mod semantic;
pub mod w3c;

use crate::error::{EngineError, EngineResult};
use crate::format::{FormatProfile, Strategy};
pub use record::{LineOutcome, LogRecord, ParseErrorCode, SkipReason};

/// 프로필로부터 만든 줄 파서. W3C는 헤더 상태를 가진다.
#[derive(Debug)]
pub enum LineParser {
    /// 블록 정규식 파서.
    Blocks(blocks::BlocksParser),
    /// W3C 확장 로그 파서.
    W3c(w3c::W3cParser),
}

impl LineParser {
    /// 프로필을 컴파일한다.
    pub fn from_profile(profile: &FormatProfile) -> EngineResult<Self> {
        if profile.schema_version != crate::format::SCHEMA_VERSION {
            return Err(EngineError::Format(format!(
                "지원하지 않는 정의 스키마 버전 {}",
                profile.schema_version
            )));
        }
        Ok(match &profile.strategy {
            Strategy::Blocks { blocks } => {
                Self::Blocks(blocks::BlocksParser::new(blocks, profile.timezone)?)
            }
            Strategy::W3c => Self::W3c(w3c::W3cParser::new(profile.timezone)),
        })
    }

    /// 한 줄을 파싱한다. `line`은 개행이 제거된 문자열이다.
    pub fn parse_line(&mut self, line_number: u64, line: &str) -> LineOutcome {
        match self {
            Self::Blocks(p) => p.parse_line(line_number, line),
            Self::W3c(p) => p.parse_line(line_number, line),
        }
    }

    /// 재개를 위해 보존해야 하는 헤더 상태(JSON). 상태가 없는 파서는 `None`.
    pub fn header_state_json(&self) -> EngineResult<Option<String>> {
        match self {
            Self::Blocks(_) => Ok(None),
            Self::W3c(p) => p.header_state_json().map(Some),
        }
    }

    /// 체크포인트의 헤더 상태를 복원한다.
    pub fn restore_header_state(&mut self, json: Option<&str>) -> EngineResult<()> {
        match (self, json) {
            (Self::W3c(p), Some(json)) => p.restore_header_state(json),
            (Self::W3c(p), None) => {
                p.reset();
                Ok(())
            }
            (Self::Blocks(_), _) => Ok(()),
        }
    }
}
