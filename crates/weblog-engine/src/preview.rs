//! 포맷 미리보기. 파일 선두를 제한적으로 읽어 같은 파서로 판별한다.
//! 원문 샘플은 반환값에 담지 않으며 파싱 결과와 집계만 돌려준다.

use std::path::Path;

use serde::Serialize;

use crate::error::EngineResult;
use crate::format::FormatProfile;
use crate::parse::{LineOutcome, LineParser, ParseErrorCode};
use crate::source::{LineContent, LineReader};

/// 미리보기 한도.
#[derive(Debug, Clone)]
pub struct PreviewConfig {
    /// 최대 줄 수.
    pub max_lines: u64,
    /// 최대 논리 바이트.
    pub max_bytes: u64,
    /// 줄 길이 상한.
    pub max_line_bytes: usize,
}

impl Default for PreviewConfig {
    fn default() -> Self {
        Self {
            max_lines: 200,
            max_bytes: 1024 * 1024,
            max_line_bytes: 64 * 1024,
        }
    }
}

/// 미리보기 결과.
#[derive(Debug, Clone, Serialize)]
pub struct PreviewResult {
    /// 검사한 줄 수.
    pub lines_checked: u64,
    /// 레코드 수.
    pub records: u64,
    /// 오류 수.
    pub errors: u64,
    /// 제외 수.
    pub skipped: u64,
    /// 레코드 / (레코드+오류). 제외 줄은 분모에 넣지 않는다. 서버 식별 확률이 아니다.
    pub match_rate: f64,
    /// 줄별 결과(원문 없음).
    pub outcomes: Vec<LineOutcome>,
    /// 한도에 걸려 중단했는지.
    pub truncated: bool,
}

/// 파일 선두를 미리보기한다.
pub fn preview_file(
    path: &Path,
    profile: &FormatProfile,
    cfg: &PreviewConfig,
) -> EngineResult<PreviewResult> {
    let mut parser = LineParser::from_profile(profile)?;
    let mut reader = LineReader::open(path, cfg.max_line_bytes)?;
    let mut outcomes = Vec::new();
    let mut truncated = false;
    loop {
        if reader.line_number() >= cfg.max_lines || reader.offset() >= cfg.max_bytes {
            truncated = true;
            break;
        }
        let Some(line) = reader.next_line()? else {
            break;
        };
        let outcome = match line.content {
            LineContent::Text(text) => parser.parse_line(line.line_number, text),
            LineContent::InvalidUtf8 => {
                LineOutcome::error(line.line_number, ParseErrorCode::InvalidUtf8, None)
            }
            LineContent::TooLong => {
                LineOutcome::error(line.line_number, ParseErrorCode::LineTooLong, None)
            }
        };
        outcomes.push(outcome);
    }
    let records = outcomes
        .iter()
        .filter(|o| matches!(o, LineOutcome::Record(_)))
        .count() as u64;
    let errors = outcomes
        .iter()
        .filter(|o| matches!(o, LineOutcome::Error { .. }))
        .count() as u64;
    let skipped = outcomes.len() as u64 - records - errors;
    let denom = records + errors;
    Ok(PreviewResult {
        lines_checked: outcomes.len() as u64,
        records,
        errors,
        skipped,
        match_rate: if denom == 0 {
            0.0
        } else {
            records as f64 / denom as f64
        },
        outcomes,
        truncated,
    })
}
