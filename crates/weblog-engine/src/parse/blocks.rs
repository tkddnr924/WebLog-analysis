//! 블록 정의 기반 줄 파서. 정규식 매칭 뒤 필드 종류별로 의미 검증을 수행한다.

use crate::error::EngineResult;
use crate::format::{Block, CompiledBlocks, FieldDef, FieldKind, TimezonePolicy};
use crate::parse::record::{LineOutcome, LogRecord, ParseErrorCode, SkipReason};
use crate::parse::semantic;

/// 블록 파서.
#[derive(Debug)]
pub struct BlocksParser {
    compiled: CompiledBlocks,
    timezone: TimezonePolicy,
}

impl BlocksParser {
    /// 블록을 컴파일한다.
    pub fn new(blocks: &[Block], timezone: TimezonePolicy) -> EngineResult<Self> {
        Ok(Self {
            compiled: CompiledBlocks::compile(blocks)?,
            timezone,
        })
    }

    /// 컴파일된 정규식(진단용).
    pub fn pattern(&self) -> &str {
        self.compiled.pattern()
    }

    /// 한 줄을 파싱한다.
    pub fn parse_line(&self, line_number: u64, line: &str) -> LineOutcome {
        if line.trim().is_empty() {
            return LineOutcome::Skipped {
                line_number,
                reason: SkipReason::Blank,
            };
        }
        let Some(captured) = self.compiled.capture(line) else {
            if is_continuation(line) {
                return LineOutcome::Skipped {
                    line_number,
                    reason: SkipReason::Continuation,
                };
            }
            return LineOutcome::error(line_number, ParseErrorCode::NoMatch, None);
        };
        let mut record = LogRecord {
            line_number,
            ..LogRecord::default()
        };
        for (def, value) in self.compiled.fields().iter().zip(captured.values) {
            let Some(raw) = value else { continue };
            if def.missing.iter().any(|m| m == raw) {
                continue;
            }
            if let Err(code) = apply_field(&mut record, def, raw, self.timezone) {
                return LineOutcome::error(line_number, code, Some(&def.name));
            }
        }
        for (name, value) in captured.extras {
            record.extra.insert(name.to_owned(), value.to_owned());
        }
        LineOutcome::Record(record)
    }
}

/// 포맷에 맞지 않는 줄이 앞 레코드의 이어지는 줄인지. PHP·Java·Python 스택 트레이스와 들여쓴 줄을 본다.
/// 로그 줄은 시각이나 IP로 시작하므로, 공백·`#`·`at `·`Stack trace`·`thrown`·`Caused by`·`Traceback`로 시작하면 이어지는 줄로 본다.
fn is_continuation(line: &str) -> bool {
    let t = line.trim_end();
    t.starts_with(' ')
        || t.starts_with('\t')
        || t.starts_with('#')
        || t.starts_with("at ")
        || t.starts_with("Stack trace")
        || t.starts_with("thrown")
        || t.starts_with("Caused by")
        || t.starts_with("Traceback")
        || t.starts_with("...")
}

fn apply_field(
    record: &mut LogRecord,
    def: &FieldDef,
    raw: &str,
    timezone: TimezonePolicy,
) -> Result<(), ParseErrorCode> {
    match &def.kind {
        FieldKind::ClientIp => {
            let ip = semantic::validate_ip(raw).ok_or(ParseErrorCode::InvalidIp)?;
            record.client_ip = Some(ip.to_owned());
        }
        FieldKind::Timestamp { format } => {
            let t = semantic::resolve_timestamp(raw, format, timezone)
                .ok_or(ParseErrorCode::InvalidTimestamp)?;
            record.timestamp_utc = t.utc_micros;
            record.tz_offset_seconds = t.offset_seconds;
        }
        FieldKind::RequestLine => {
            let (m, t, p) = semantic::split_request_line(raw);
            record.method = m;
            record.request_target = t;
            record.protocol = p;
        }
        FieldKind::Method => record.method = Some(raw.to_owned()),
        FieldKind::RequestTarget => record.request_target = Some(raw.to_owned()),
        FieldKind::Protocol => record.protocol = Some(raw.to_owned()),
        FieldKind::Status => {
            record.status = Some(semantic::parse_status(raw).ok_or(ParseErrorCode::InvalidStatus)?);
        }
        FieldKind::BytesSent => {
            record.bytes_sent =
                Some(semantic::parse_i64(raw).ok_or(ParseErrorCode::InvalidInteger)?);
        }
        FieldKind::Referrer => record.referrer = Some(raw.to_owned()),
        FieldKind::UserAgent => record.user_agent = Some(raw.to_owned()),
        FieldKind::Integer => {
            semantic::parse_i64(raw).ok_or(ParseErrorCode::InvalidInteger)?;
            record.extra.insert(def.name.clone(), raw.to_owned());
        }
        FieldKind::Text => {
            record.extra.insert(def.name.clone(), raw.to_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;
    use crate::format::presets;
    use crate::format::Strategy;

    fn combined_parser() -> BlocksParser {
        let profile = presets::apache_combined();
        let Strategy::Blocks { blocks } = &profile.strategy else {
            panic!("combined preset must be block based");
        };
        BlocksParser::new(blocks, profile.timezone).unwrap()
    }

    const LINE: &str = r#"127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] "GET /apache_pb.gif?x=1 HTTP/1.0" 200 2326 "http://www.example.com/start.html" "Mozilla/4.08 [en] (Win98; I ;Nav)""#;

    #[test]
    fn combined_line_parses_all_standard_fields() {
        let outcome = combined_parser().parse_line(1, LINE);
        let LineOutcome::Record(r) = outcome else {
            panic!("expected record, got {outcome:?}");
        };
        assert_eq!(r.client_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(r.timestamp_utc, Some(971_211_336_000_000));
        assert_eq!(r.method.as_deref(), Some("GET"));
        assert_eq!(r.request_target.as_deref(), Some("/apache_pb.gif?x=1"));
        assert_eq!(r.protocol.as_deref(), Some("HTTP/1.0"));
        assert_eq!(r.status, Some(200));
        assert_eq!(r.bytes_sent, Some(2326));
        assert_eq!(
            r.referrer.as_deref(),
            Some("http://www.example.com/start.html")
        );
        assert_eq!(
            r.user_agent.as_deref(),
            Some("Mozilla/4.08 [en] (Win98; I ;Nav)")
        );
        assert_eq!(
            r.extra.get("remote_user").map(String::as_str),
            Some("frank")
        );
        assert!(
            !r.extra.contains_key("ident"),
            "'-' must be treated as missing"
        );
    }

    #[test]
    fn missing_bytes_is_none_not_zero() {
        let line = r#"127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] "GET / HTTP/1.0" 304 - "-" "-""#;
        let LineOutcome::Record(r) = combined_parser().parse_line(1, line) else {
            panic!("expected record");
        };
        assert_eq!(r.bytes_sent, None);
        assert_eq!(r.referrer, None);
        assert_eq!(r.user_agent, None);
    }

    #[test]
    fn invalid_ip_is_error_with_field_name_and_no_input() {
        let line = LINE.replacen("127.0.0.1", "300.1.1.1", 1);
        let outcome = combined_parser().parse_line(7, &line);
        assert_eq!(
            outcome,
            LineOutcome::error(7, ParseErrorCode::InvalidIp, Some("client_ip"))
        );
    }

    #[test]
    fn blank_line_is_skipped() {
        assert_eq!(
            combined_parser().parse_line(3, "   "),
            LineOutcome::Skipped {
                line_number: 3,
                reason: SkipReason::Blank
            }
        );
    }

    #[test]
    fn garbage_line_is_no_match() {
        assert_eq!(
            combined_parser().parse_line(4, "not a log line"),
            LineOutcome::error(4, ParseErrorCode::NoMatch, None)
        );
    }

    #[test]
    fn ipv6_client_is_accepted() {
        let line = LINE.replacen("127.0.0.1", "2001:db8::ff00:42:8329", 1);
        let LineOutcome::Record(r) = combined_parser().parse_line(1, &line) else {
            panic!("expected record");
        };
        assert_eq!(r.client_ip.as_deref(), Some("2001:db8::ff00:42:8329"));
    }

    #[test]
    fn stack_trace_lines_are_skipped_as_continuation_not_errors() {
        let parser = combined_parser();
        for line in [
            "#0 /var/www/html/app/Http/Controllers/GpController.php(1220): app\\Models\\Company->detailInfo()",
            "    at java.base/java.lang.Thread.run(Thread.java:833)",
            "Stack trace:",
            "thrown in /var/www/html/index.php on line 12",
            "Caused by: java.io.IOException",
            "Traceback (most recent call last):",
        ] {
            assert!(
                matches!(
                    parser.parse_line(1, line),
                    LineOutcome::Skipped {
                        reason: SkipReason::Continuation,
                        ..
                    }
                ),
                "{line}"
            );
        }
        assert!(matches!(
            parser.parse_line(1, "garbage that is not a log line"),
            LineOutcome::Error { .. }
        ));
    }
}
