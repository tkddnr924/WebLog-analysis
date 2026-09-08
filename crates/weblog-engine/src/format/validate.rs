//! 포맷 정의 검증. 잘못된 정의는 적용하지 않고 문제 목록을 돌려준다.

use serde::Serialize;

use super::compile::{CompiledBlocks, MAX_PATTERN_BYTES};
use super::model::{
    Block, Capture, FieldKind, FormatProfile, Strategy, TimestampFormat, TimezonePolicy,
    SCHEMA_VERSION,
};
use crate::error::{EngineError, EngineResult};

/// 이름 길이 상한.
const MAX_NAME_LEN: usize = 64;
/// 블록 수 상한.
const MAX_BLOCKS: usize = 256;

/// 검증 문제 하나. `path`는 편집기가 위치를 표시하는 데 쓴다(예: `blocks[3].name`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ValidationIssue {
    /// 정의 안의 위치.
    pub path: String,
    /// 설명.
    pub message: String,
}

fn issue(path: impl Into<String>, message: impl Into<String>) -> ValidationIssue {
    ValidationIssue {
        path: path.into(),
        message: message.into(),
    }
}

/// 필드 이름 규칙: 영문자·밑줄로 시작, 영문자·숫자·밑줄·하이픈.
pub fn is_valid_field_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() || c == '_' => {}
        _ => return false,
    }
    name.len() <= MAX_NAME_LEN && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// 프로필 이름 규칙: 파일명으로 쓸 수 있어야 한다.
pub fn is_valid_profile_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
        && !name.starts_with('.')
}

/// chrono strftime 패턴이 해석 가능한지.
pub fn is_valid_strftime(pattern: &str) -> bool {
    use chrono::format::{Item, StrftimeItems};
    !pattern.is_empty() && !StrftimeItems::new(pattern).any(|i| matches!(i, Item::Error))
}

impl FormatProfile {
    /// 정의를 검증한다. 비어 있으면 유효하다.
    pub fn validate(&self) -> Vec<ValidationIssue> {
        let mut out = Vec::new();
        if self.schema_version != SCHEMA_VERSION {
            out.push(issue(
                "schema_version",
                format!("지원하는 스키마 버전은 {SCHEMA_VERSION}"),
            ));
        }
        if !is_valid_profile_name(&self.name) {
            out.push(issue(
                "name",
                "이름은 1~64자의 영문자·숫자·밑줄·하이픈·마침표여야 함",
            ));
        }
        if self.version == 0 {
            out.push(issue("version", "버전은 1 이상"));
        }
        if let TimezonePolicy::Fixed { offset_seconds } = self.timezone {
            if !(-18 * 3600..=18 * 3600).contains(&offset_seconds) {
                out.push(issue("timezone.offset_seconds", "오프셋은 ±18시간 이내"));
            }
        }
        match &self.strategy {
            Strategy::W3c => {}
            Strategy::Blocks { blocks } => {
                if blocks.is_empty() {
                    out.push(issue("blocks", "블록이 하나도 없음"));
                }
                let mut names = Vec::new();
                let mut count = 0usize;
                validate_blocks(blocks, "blocks", &mut out, &mut names, &mut count, 0);
                if count > MAX_BLOCKS {
                    out.push(issue(
                        "blocks",
                        format!("블록 수가 상한 {MAX_BLOCKS}를 넘음"),
                    ));
                }
                if names.is_empty() {
                    out.push(issue("blocks", "필드 블록이 하나 이상 필요함"));
                }
                // 개별 검사를 통과했으면 실제 컴파일로 남은 문제를 잡는다.
                if out.is_empty() {
                    if let Err(e) = CompiledBlocks::compile(blocks) {
                        out.push(issue("blocks", e.to_string()));
                    }
                }
            }
        }
        out
    }

    /// 유효하지 않으면 첫 문제를 오류로 돌려준다.
    pub fn ensure_valid(&self) -> EngineResult<()> {
        let issues = self.validate();
        match issues.first() {
            None => Ok(()),
            Some(first) => Err(EngineError::Format(format!(
                "{} — {}{}",
                first.path,
                first.message,
                if issues.len() > 1 {
                    format!(" (외 {}건)", issues.len() - 1)
                } else {
                    String::new()
                }
            ))),
        }
    }
}

fn validate_blocks(
    blocks: &[Block],
    path: &str,
    out: &mut Vec<ValidationIssue>,
    names: &mut Vec<String>,
    count: &mut usize,
    depth: usize,
) {
    if depth > 8 {
        out.push(issue(path, "블록 중첩이 너무 깊음"));
        return;
    }
    for (i, block) in blocks.iter().enumerate() {
        *count += 1;
        let p = format!("{path}[{i}]");
        match block {
            Block::Literal { text } => {
                if text.is_empty() {
                    out.push(issue(format!("{p}.text"), "고정 문자열이 비어 있음"));
                }
            }
            Block::Whitespace => {}
            Block::Field(def) => {
                if !is_valid_field_name(&def.name) {
                    out.push(issue(
                        format!("{p}.name"),
                        "필드 이름은 영문자/밑줄로 시작하고 영문자·숫자·밑줄·하이픈만 허용",
                    ));
                } else if names.iter().any(|n| n == &def.name) {
                    out.push(issue(
                        format!("{p}.name"),
                        format!("필드 이름 중복: {}", def.name),
                    ));
                } else {
                    names.push(def.name.clone());
                }
                if let Capture::Pattern { pattern } = &def.capture {
                    check_pattern(pattern, &format!("{p}.capture.pattern"), out);
                }
                if let FieldKind::Timestamp {
                    format: TimestampFormat::Custom { pattern },
                } = &def.kind
                {
                    if !is_valid_strftime(pattern) {
                        out.push(issue(
                            format!("{p}.kind.format.pattern"),
                            "strftime 패턴을 해석할 수 없음",
                        ));
                    }
                }
            }
            Block::OptionalGroup { blocks } => {
                if blocks.is_empty() {
                    out.push(issue(format!("{p}.blocks"), "선택 그룹이 비어 있음"));
                }
                validate_blocks(blocks, &format!("{p}.blocks"), out, names, count, depth + 1);
            }
            Block::Regex { pattern } => check_pattern(pattern, &format!("{p}.pattern"), out),
        }
    }
}

fn check_pattern(pattern: &str, path: &str, out: &mut Vec<ValidationIssue>) {
    if pattern.is_empty() {
        out.push(issue(path, "정규식이 비어 있음"));
        return;
    }
    if pattern.len() > MAX_PATTERN_BYTES {
        out.push(issue(
            path,
            format!("정규식이 상한 {MAX_PATTERN_BYTES}바이트를 넘음"),
        ));
        return;
    }
    if let Err(e) = regex::Regex::new(pattern) {
        // regex 오류 메시지는 패턴 원문을 포함하지만 패턴은 사용자 정의이지 로그 원문이 아니다.
        out.push(issue(path, format!("정규식 오류: {e}")));
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::format::model::FieldDef;
    use crate::format::presets;

    fn custom(blocks: Vec<Block>) -> FormatProfile {
        FormatProfile {
            schema_version: SCHEMA_VERSION,
            name: "custom".to_owned(),
            version: 1,
            server_hint: crate::format::ServerHint::Unknown,
            timezone: TimezonePolicy::Utc,
            strategy: Strategy::Blocks { blocks },
        }
    }

    fn field(name: &str, kind: FieldKind, capture: Capture) -> Block {
        Block::Field(FieldDef {
            name: name.to_owned(),
            kind,
            capture,
            missing: vec![],
        })
    }

    #[test]
    fn builtin_presets_are_valid() {
        for name in presets::PRESET_NAMES {
            let p = presets::by_name(name).unwrap();
            assert!(p.validate().is_empty(), "{name}: {:?}", p.validate());
        }
    }

    #[test]
    fn duplicate_and_invalid_field_names_are_reported_with_paths() {
        let p = custom(vec![
            field("a", FieldKind::Text, Capture::Token),
            Block::Whitespace,
            field("a", FieldKind::Text, Capture::Token),
            field("1bad", FieldKind::Text, Capture::Token),
        ]);
        let issues = p.validate();
        assert_eq!(issues[0].path, "blocks[2].name");
        assert_eq!(issues[1].path, "blocks[3].name");
    }

    #[test]
    fn invalid_regex_and_strftime_are_reported() {
        let p = custom(vec![field(
            "ts",
            FieldKind::Timestamp {
                format: TimestampFormat::Custom {
                    pattern: "%Q".to_owned(),
                },
            },
            Capture::Pattern {
                pattern: "(unclosed".to_owned(),
            },
        )]);
        let paths: Vec<String> = p.validate().into_iter().map(|i| i.path).collect();
        assert!(paths.contains(&"blocks[0].capture.pattern".to_owned()));
        assert!(paths.contains(&"blocks[0].kind.format.pattern".to_owned()));
    }

    #[test]
    fn empty_blocks_and_missing_fields_are_rejected() {
        assert!(!custom(vec![]).validate().is_empty());
        let no_field = custom(vec![Block::Literal {
            text: "x".to_owned(),
        }]);
        assert!(no_field
            .validate()
            .iter()
            .any(|i| i.message.contains("필드 블록")));
        assert!(no_field.ensure_valid().is_err());
    }

    #[test]
    fn profile_name_rules() {
        assert!(is_valid_profile_name("my-nginx_v2"));
        assert!(!is_valid_profile_name("../etc"));
        assert!(!is_valid_profile_name("a b"));
        assert!(!is_valid_profile_name(""));
    }
}
