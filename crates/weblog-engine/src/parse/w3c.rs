//! IIS/W3C 확장 로그 파서. `#Fields:` 헤더가 필드 매핑을 결정하며 파일 중간에 바뀔 수 있다.

use serde::{Deserialize, Serialize};

use crate::error::EngineResult;
use crate::format::TimezonePolicy;
use crate::parse::record::{LineOutcome, LogRecord, ParseErrorCode, SkipReason};
use crate::parse::semantic;

/// 헤더 필드 수 상한.
const MAX_FIELDS: usize = 256;

/// 재개 시 복원해야 하는 헤더 상태.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct W3cHeaderState {
    /// 현재 `#Fields` 이름 목록. 아직 없으면 비어 있다.
    pub fields: Vec<String>,
}

/// W3C 파서.
#[derive(Debug)]
pub struct W3cParser {
    state: W3cHeaderState,
    timezone: TimezonePolicy,
}

impl W3cParser {
    /// 새 파서.
    pub fn new(timezone: TimezonePolicy) -> Self {
        Self {
            state: W3cHeaderState::default(),
            timezone,
        }
    }

    /// 헤더 상태를 초기화한다.
    pub fn reset(&mut self) {
        self.state = W3cHeaderState::default();
    }

    /// 헤더 상태 JSON.
    pub fn header_state_json(&self) -> EngineResult<String> {
        Ok(serde_json::to_string(&self.state)?)
    }

    /// 헤더 상태를 복원한다.
    pub fn restore_header_state(&mut self, json: &str) -> EngineResult<()> {
        self.state = serde_json::from_str(json)?;
        Ok(())
    }

    /// 현재 헤더 필드 목록.
    pub fn fields(&self) -> &[String] {
        &self.state.fields
    }

    /// 한 줄을 파싱한다.
    pub fn parse_line(&mut self, line_number: u64, line: &str) -> LineOutcome {
        if line.trim().is_empty() {
            return LineOutcome::Skipped {
                line_number,
                reason: SkipReason::Blank,
            };
        }
        if let Some(directive) = line.strip_prefix('#') {
            return self.handle_directive(line_number, directive);
        }
        if self.state.fields.is_empty() {
            return LineOutcome::error(line_number, ParseErrorCode::HeaderMissing, None);
        }
        let tokens: Vec<&str> = line.split_ascii_whitespace().collect();
        if tokens.len() != self.state.fields.len() {
            return LineOutcome::error(line_number, ParseErrorCode::FieldCountMismatch, None);
        }
        let mut record = LogRecord {
            line_number,
            ..LogRecord::default()
        };
        let mut date = None;
        let mut time = None;
        let mut uri_stem = None;
        let mut uri_query = None;
        for (name, raw) in self.state.fields.iter().zip(tokens) {
            if raw == "-" {
                continue;
            }
            let result = match name.as_str() {
                "date" => {
                    date = Some(raw);
                    Ok(())
                }
                "time" => {
                    time = Some(raw);
                    Ok(())
                }
                "c-ip" => semantic::validate_ip(raw)
                    .map(|ip| record.client_ip = Some(ip.to_owned()))
                    .ok_or(ParseErrorCode::InvalidIp),
                "cs-method" => {
                    record.method = Some(raw.to_owned());
                    Ok(())
                }
                "cs-uri-stem" => {
                    uri_stem = Some(raw);
                    Ok(())
                }
                "cs-uri-query" => {
                    uri_query = Some(raw);
                    Ok(())
                }
                "cs-version" => {
                    record.protocol = Some(raw.to_owned());
                    Ok(())
                }
                "sc-status" => semantic::parse_status(raw)
                    .map(|s| record.status = Some(s))
                    .ok_or(ParseErrorCode::InvalidStatus),
                "sc-bytes" => semantic::parse_i64(raw)
                    .map(|b| record.bytes_sent = Some(b))
                    .ok_or(ParseErrorCode::InvalidInteger),
                "cs(Referer)" => {
                    record.referrer = Some(raw.to_owned());
                    Ok(())
                }
                "cs(User-Agent)" => {
                    record.user_agent = Some(raw.to_owned());
                    Ok(())
                }
                other => {
                    record.extra.insert(other.to_owned(), raw.to_owned());
                    Ok(())
                }
            };
            if let Err(code) = result {
                return LineOutcome::error(line_number, code, Some(name));
            }
        }
        match (date, time) {
            (Some(d), Some(t)) => {
                let Some(resolved) = semantic::resolve_w3c_datetime(d, t, self.timezone) else {
                    return LineOutcome::error(
                        line_number,
                        ParseErrorCode::InvalidTimestamp,
                        Some("date"),
                    );
                };
                record.timestamp_utc = resolved.utc_micros;
                record.tz_offset_seconds = resolved.offset_seconds;
            }
            (Some(d), None) => {
                record.extra.insert("date".to_owned(), d.to_owned());
            }
            (None, Some(t)) => {
                record.extra.insert("time".to_owned(), t.to_owned());
            }
            (None, None) => {}
        }
        record.request_target = match (uri_stem, uri_query) {
            (Some(stem), Some(query)) => Some(format!("{stem}?{query}")),
            (Some(stem), None) => Some(stem.to_owned()),
            (None, Some(query)) => Some(format!("?{query}")),
            (None, None) => None,
        };
        LineOutcome::Record(record)
    }

    fn handle_directive(&mut self, line_number: u64, directive: &str) -> LineOutcome {
        if let Some(rest) = directive.strip_prefix("Fields:") {
            let fields: Vec<String> = rest.split_ascii_whitespace().map(str::to_owned).collect();
            if fields.is_empty() || fields.len() > MAX_FIELDS {
                return LineOutcome::error(
                    line_number,
                    ParseErrorCode::InvalidDirective,
                    Some("Fields"),
                );
            }
            self.state.fields = fields;
        }
        // #Software, #Version, #Date, #Remark 등은 제외로 집계한다.
        LineOutcome::Skipped {
            line_number,
            reason: SkipReason::Directive,
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    const FIELDS: &str = "#Fields: date time s-ip cs-method cs-uri-stem cs-uri-query s-port cs-username c-ip cs(User-Agent) cs(Referer) sc-status sc-substatus sc-win32-status time-taken";
    const DATA: &str = "2024-01-02 03:04:05 10.0.0.1 GET /index.html a=1&b=2 80 - 192.168.1.5 Mozilla/5.0+(Windows) http://ref.example/ 200 0 0 15";

    fn parser() -> W3cParser {
        W3cParser::new(TimezonePolicy::Utc)
    }

    #[test]
    fn data_before_fields_header_is_header_missing_error() {
        let mut p = parser();
        assert_eq!(
            p.parse_line(1, DATA),
            LineOutcome::error(1, ParseErrorCode::HeaderMissing, None)
        );
    }

    #[test]
    fn directives_are_skipped_not_errors() {
        let mut p = parser();
        assert_eq!(
            p.parse_line(1, "#Software: Microsoft Internet Information Services 10.0"),
            LineOutcome::Skipped {
                line_number: 1,
                reason: SkipReason::Directive
            }
        );
    }

    #[test]
    fn fields_header_maps_standard_columns() {
        let mut p = parser();
        p.parse_line(1, FIELDS);
        let LineOutcome::Record(r) = p.parse_line(2, DATA) else {
            panic!("expected record");
        };
        assert_eq!(r.timestamp_utc, Some(1_704_164_645_000_000));
        assert_eq!(r.client_ip.as_deref(), Some("192.168.1.5"));
        assert_eq!(r.method.as_deref(), Some("GET"));
        assert_eq!(r.request_target.as_deref(), Some("/index.html?a=1&b=2"));
        assert_eq!(r.status, Some(200));
        assert_eq!(r.user_agent.as_deref(), Some("Mozilla/5.0+(Windows)"));
        assert_eq!(r.referrer.as_deref(), Some("http://ref.example/"));
        assert_eq!(r.extra.get("time-taken").map(String::as_str), Some("15"));
        assert!(!r.extra.contains_key("cs-username"));
    }

    #[test]
    fn header_change_mid_file_is_applied() {
        let mut p = parser();
        p.parse_line(1, FIELDS);
        p.parse_line(2, "#Fields: date time c-ip sc-status");
        let LineOutcome::Record(r) = p.parse_line(3, "2024-01-02 03:04:05 10.1.1.1 404") else {
            panic!("expected record");
        };
        assert_eq!(r.status, Some(404));
        assert_eq!(r.client_ip.as_deref(), Some("10.1.1.1"));
    }

    #[test]
    fn field_count_mismatch_is_error() {
        let mut p = parser();
        p.parse_line(1, "#Fields: date time c-ip sc-status");
        assert_eq!(
            p.parse_line(2, "2024-01-02 03:04:05 10.1.1.1"),
            LineOutcome::error(2, ParseErrorCode::FieldCountMismatch, None)
        );
    }

    #[test]
    fn header_state_roundtrips_through_json() {
        let mut p = parser();
        p.parse_line(1, "#Fields: date time c-ip sc-status");
        let json = p.header_state_json().unwrap();
        let mut q = parser();
        q.restore_header_state(&json).unwrap();
        assert_eq!(q.fields(), p.fields());
    }
}
