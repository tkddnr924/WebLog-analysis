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
    /// Capture group number per field, same order as `fields`.
    field_groups: Vec<usize>,
    /// Named captures from regex blocks (extra fields), with their group numbers.
    extra_groups: Vec<(String, usize)>,
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
        // Resolve group names once. The hot path then indexes groups by number.
        let mut field_groups = vec![usize::MAX; fields.len()];
        let mut extra_groups = Vec::new();
        for (group, name) in regex.capture_names().enumerate() {
            let Some(name) = name else { continue };
            match field_index(name) {
                Some(i) if i < field_groups.len() => field_groups[i] = group,
                Some(_) => {}
                None => extra_groups.push((name.to_owned(), group)),
            }
        }
        if let Some(i) = field_groups.iter().position(|g| *g == usize::MAX) {
            return Err(EngineError::Format(format!(
                "필드 {}의 캡처 그룹을 찾을 수 없음",
                fields[i].name
            )));
        }
        Ok(Self {
            regex,
            fields,
            field_groups,
            extra_groups,
        })
    }

    /// 필드 정의 목록.
    pub fn fields(&self) -> &[FieldDef] {
        &self.fields
    }

    /// Reusable match buffer. Allocated once per parser, not per line.
    pub fn match_buf(&self) -> MatchBuf {
        MatchBuf(self.regex.capture_locations())
    }

    /// 한 줄을 매칭한다. 성공하면 `buf`에 그룹 위치가 담긴다. 줄당 할당이 없다.
    pub fn match_line(&self, line: &str, buf: &mut MatchBuf) -> bool {
        self.regex.captures_read(&mut buf.0, line).is_some()
    }

    /// 매칭 뒤 `i`번째 필드의 원문. 선택 그룹 안에서 매칭되지 않았으면 `None`.
    pub fn field_value<'a>(&self, i: usize, line: &'a str, buf: &MatchBuf) -> Option<&'a str> {
        let group = *self.field_groups.get(i)?;
        buf.0.get(group).map(|(s, e)| &line[s..e])
    }

    /// 매칭 뒤 정규식 블록의 이름 있는 캡처.
    pub fn extras<'p, 'a>(
        &'p self,
        line: &'a str,
        buf: &'p MatchBuf,
    ) -> impl Iterator<Item = (&'p str, &'a str)> + use<'p, 'a> {
        self.extra_groups.iter().filter_map(move |(name, group)| {
            buf.0.get(*group).map(|(s, e)| (name.as_str(), &line[s..e]))
        })
    }
}

/// 재사용 매칭 버퍼.
#[derive(Debug)]
pub struct MatchBuf(regex::CaptureLocations);

/// `f{n}` 그룹 이름의 필드 번호.
fn field_index(name: &str) -> Option<usize> {
    let rest = name.strip_prefix('f')?;
    if rest.is_empty() || !rest.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    rest.parse().ok()
}

fn field_group(i: usize) -> String {
    format!("f{i}")
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
                if def.name.is_empty() || field_index(&def.name).is_some() {
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
    if tmp
        .capture_names()
        .flatten()
        .any(|n| field_index(n).is_some())
    {
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

    /// 한 줄을 매칭해 필드별 원문을 모은다. 테스트 편의용.
    fn values(compiled: &CompiledBlocks, line: &str) -> Option<Vec<Option<String>>> {
        let mut buf = compiled.match_buf();
        if !compiled.match_line(line, &mut buf) {
            return None;
        }
        Some(
            (0..compiled.fields().len())
                .map(|i| compiled.field_value(i, line, &buf).map(str::to_owned))
                .collect(),
        )
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
        let vals = values(&compiled, r#""Mozilla \"x\" 1.0""#).unwrap();
        assert_eq!(vals[0].as_deref(), Some(r#"Mozilla \"x\" 1.0"#));
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
        let vals = values(&compiled, "[10/Oct/2000:13:55:36 -0700]").unwrap();
        assert_eq!(vals[0].as_deref(), Some("10/Oct/2000:13:55:36 -0700"));
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
        assert_eq!(
            values(&compiled, "x").unwrap(),
            vec![Some("x".to_owned()), None]
        );
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
        let mut buf = compiled.match_buf();
        assert!(compiled.match_line("x rt=0.25", &mut buf));
        let extras: Vec<_> = compiled.extras("x rt=0.25", &buf).collect();
        assert_eq!(extras, vec![("response_time", "0.25")]);
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
        assert!(values(&compiled, "two tokens").is_none());
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
