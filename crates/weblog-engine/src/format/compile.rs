//! 블록 정의를 한 개의 앵커된 정규식으로 컴파일한다. 프로필별로 한 번 컴파일하고 재사용한다.

use regex::Regex;

use super::model::{Block, Capture, FieldDef};
use crate::error::{EngineError, EngineResult};

/// 컴파일된 정규식 패턴 길이 상한(바이트).
pub const MAX_PATTERN_BYTES: usize = 16 * 1024;
/// 컴파일된 정규식 내부 크기 상한.
const REGEX_SIZE_LIMIT: usize = 8 * 1024 * 1024;
/// 블록 중첩 깊이 상한.
const MAX_NESTING: usize = 8;

/// 컴파일된 블록 파서. 필드 인덱스는 `f{n}` 캡처 그룹으로 연결된다.
#[derive(Debug)]
pub struct CompiledBlocks {
    regex: Regex,
    fields: Vec<FieldDef>,
    /// 정규식 블록에서 온 이름 있는 캡처(확장 필드).
    extra_groups: Vec<String>,
}

impl CompiledBlocks {
    /// 블록 목록을 컴파일한다.
    pub fn compile(blocks: &[Block]) -> EngineResult<Self> {
        let mut pattern = String::from("^");
        let mut fields = Vec::new();
        emit_blocks(blocks, &mut pattern, &mut fields, 0)?;
        pattern.push('$');
        if pattern.len() > MAX_PATTERN_BYTES {
            return Err(EngineError::Limit(format!(
                "정규식 패턴 {}바이트가 상한 {MAX_PATTERN_BYTES}바이트를 넘음",
                pattern.len()
            )));
        }
        if fields.is_empty() {
            return Err(EngineError::Format("필드가 하나도 없음".to_owned()));
        }
        let mut seen = std::collections::HashSet::new();
        for f in &fields {
            if !seen.insert(f.name.as_str()) {
                return Err(EngineError::Format(format!("필드 이름 중복: {}", f.name)));
            }
        }
        let regex = regex::RegexBuilder::new(&pattern)
            .size_limit(REGEX_SIZE_LIMIT)
            .build()?;
        let extra_groups = regex
            .capture_names()
            .flatten()
            .filter(|n| !is_field_group(n))
            .map(str::to_owned)
            .collect();
        Ok(Self {
            regex,
            fields,
            extra_groups,
        })
    }

    /// 정규식 원문(진단용).
    pub fn pattern(&self) -> &str {
        self.regex.as_str()
    }

    /// 필드 정의 목록.
    pub fn fields(&self) -> &[FieldDef] {
        &self.fields
    }

    /// 한 줄을 매칭해 필드별 원문 구간과 정규식 블록의 확장 캡처를 돌려준다. 매칭 실패면 `None`.
    pub fn capture<'p, 'a>(&'p self, line: &'a str) -> Option<Captured<'p, 'a>> {
        let caps = self.regex.captures(line)?;
        let values = (0..self.fields.len())
            .map(|i| caps.name(&field_group(i)).map(|m| m.as_str()))
            .collect();
        let extras = self
            .extra_groups
            .iter()
            .filter_map(|name| caps.name(name).map(|m| (name.as_str(), m.as_str())))
            .collect();
        Some(Captured { values, extras })
    }
}

/// 매칭 결과. 필드 순서는 [`CompiledBlocks::fields`]와 같다.
#[derive(Debug)]
pub struct Captured<'p, 'a> {
    /// 필드별 원문 구간. 선택 그룹 안의 필드가 매칭되지 않았으면 `None`.
    pub values: Vec<Option<&'a str>>,
    /// 정규식 블록의 이름 있는 캡처.
    pub extras: Vec<(&'p str, &'a str)>,
}

fn field_group(i: usize) -> String {
    format!("f{i}")
}

fn is_field_group(name: &str) -> bool {
    name.strip_prefix('f')
        .map(|rest| !rest.is_empty() && rest.bytes().all(|b| b.is_ascii_digit()))
        .unwrap_or(false)
}

fn emit_blocks(
    blocks: &[Block],
    out: &mut String,
    fields: &mut Vec<FieldDef>,
    depth: usize,
) -> EngineResult<()> {
    if depth > MAX_NESTING {
        return Err(EngineError::Limit(format!(
            "블록 중첩 깊이가 {MAX_NESTING}를 넘음"
        )));
    }
    for block in blocks {
        match block {
            Block::Literal { text } => out.push_str(&regex::escape(text)),
            Block::Whitespace => out.push_str("[ \\t]+"),
            Block::Field(def) => {
                if def.name.is_empty() || is_field_group(&def.name) {
                    return Err(EngineError::Format(
                        "필드 이름이 비었거나 예약된 형식(f0, f1, ...)임".to_owned(),
                    ));
                }
                emit_field(def, fields.len(), out)?;
                fields.push(def.clone());
            }
            Block::OptionalGroup { blocks } => {
                out.push_str("(?:");
                emit_blocks(blocks, out, fields, depth + 1)?;
                out.push_str(")?");
            }
            Block::Regex { pattern } => {
                validate_user_pattern(pattern)?;
                out.push_str("(?:");
                out.push_str(pattern);
                out.push(')');
            }
        }
    }
    Ok(())
}

/// 구분 문자(따옴표·대괄호)는 캡처 그룹 밖에 두어 값에서 제외한다.
fn emit_field(def: &FieldDef, index: usize, out: &mut String) -> EngineResult<()> {
    let group = field_group(index);
    match &def.capture {
        Capture::Token => {
            out.push_str(&format!("(?P<{group}>[^ \\t]+)"));
        }
        Capture::Quoted => {
            out.push_str(&format!("\"(?P<{group}>(?:[^\"\\\\]|\\\\.)*)\""));
        }
        Capture::Bracketed => {
            out.push_str(&format!("\\[(?P<{group}>[^\\]]*)\\]"));
        }
        Capture::Pattern { pattern } => {
            validate_user_pattern(pattern)?;
            out.push_str(&format!("(?P<{group}>{pattern})"));
        }
    }
    Ok(())
}

fn validate_user_pattern(pattern: &str) -> EngineResult<()> {
    if pattern.len() > MAX_PATTERN_BYTES {
        return Err(EngineError::Limit(format!(
            "정규식 블록이 상한 {MAX_PATTERN_BYTES}바이트를 넘음"
        )));
    }
    // 사용자 패턴 안의 f0, f1 같은 예약 그룹 이름은 필드 매핑을 깨뜨린다.
    let tmp = regex::RegexBuilder::new(pattern)
        .size_limit(REGEX_SIZE_LIMIT)
        .build()?;
    if tmp.capture_names().flatten().any(is_field_group) {
        return Err(EngineError::Format(
            "정규식 블록에 예약된 그룹 이름(f0, f1, ...)을 쓸 수 없음".to_owned(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::format::model::FieldKind;

    fn token(name: &str) -> Block {
        Block::Field(FieldDef {
            name: name.to_owned(),
            kind: FieldKind::Text,
            capture: Capture::Token,
            missing: vec![],
        })
    }

    #[test]
    fn quoted_capture_excludes_quotes_and_keeps_escapes() {
        let blocks = vec![Block::Field(FieldDef {
            name: "ua".to_owned(),
            kind: FieldKind::UserAgent,
            capture: Capture::Quoted,
            missing: vec![],
        })];
        let compiled = CompiledBlocks::compile(&blocks).unwrap();
        let caps = compiled.capture(r#""Mozilla \"x\" 1.0""#).unwrap();
        assert_eq!(caps.values[0], Some(r#"Mozilla \"x\" 1.0"#));
    }

    #[test]
    fn bracketed_capture_excludes_brackets() {
        let blocks = vec![Block::Field(FieldDef {
            name: "ts".to_owned(),
            kind: FieldKind::Text,
            capture: Capture::Bracketed,
            missing: vec![],
        })];
        let compiled = CompiledBlocks::compile(&blocks).unwrap();
        let caps = compiled.capture("[10/Oct/2000:13:55:36 -0700]").unwrap();
        assert_eq!(caps.values[0], Some("10/Oct/2000:13:55:36 -0700"));
    }

    #[test]
    fn optional_group_field_is_none_when_absent() {
        let blocks = vec![
            token("a"),
            Block::OptionalGroup {
                blocks: vec![Block::Whitespace, token("b")],
            },
        ];
        let compiled = CompiledBlocks::compile(&blocks).unwrap();
        let caps = compiled.capture("x").unwrap();
        assert_eq!(caps.values, vec![Some("x"), None]);
    }

    #[test]
    fn regex_block_named_group_becomes_extra() {
        let blocks = vec![
            token("a"),
            Block::Whitespace,
            Block::Regex {
                pattern: "rt=(?P<response_time>[0-9.]+)".to_owned(),
            },
        ];
        let compiled = CompiledBlocks::compile(&blocks).unwrap();
        let caps = compiled.capture("x rt=0.25").unwrap();
        assert_eq!(caps.extras, vec![("response_time", "0.25")]);
    }

    #[test]
    fn duplicate_field_name_is_rejected() {
        let err = CompiledBlocks::compile(&[token("a"), token("a")]).unwrap_err();
        assert!(matches!(err, EngineError::Format(_)));
    }

    #[test]
    fn reserved_group_name_in_regex_block_is_rejected() {
        let blocks = vec![
            token("a"),
            Block::Regex {
                pattern: "(?P<f0>x)".to_owned(),
            },
        ];
        assert!(CompiledBlocks::compile(&blocks).is_err());
    }

    #[test]
    fn line_not_matching_anchored_pattern_returns_none() {
        let compiled = CompiledBlocks::compile(&[token("a")]).unwrap();
        assert!(compiled.capture("two tokens").is_none());
    }

    #[test]
    fn invalid_regex_block_is_reported_as_error_not_panic() {
        let blocks = vec![
            token("a"),
            Block::Regex {
                pattern: "(unclosed".to_owned(),
            },
        ];
        assert!(matches!(
            CompiledBlocks::compile(&blocks),
            Err(EngineError::Regex(_))
        ));
    }
}
