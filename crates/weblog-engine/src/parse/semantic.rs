//! 정규식 매칭 뒤의 의미 검증·정규화. 매칭만 성공했다고 유효한 IP·날짜로 간주하지 않는다.

use chrono::{DateTime, FixedOffset, NaiveDateTime};

use crate::format::{TimestampFormat, TimezonePolicy};

/// 해석된 타임스탬프.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedTime {
    /// UTC 마이크로초. 시간대 미확정이면 `None`.
    pub utc_micros: Option<i64>,
    /// 적용한 오프셋(초).
    pub offset_seconds: Option<i32>,
}

/// IP 문자열을 검증한다. 유효하면 원문을 그대로 돌려준다.
pub fn validate_ip(raw: &str) -> Option<&str> {
    raw.parse::<std::net::IpAddr>().ok().map(|_| raw)
}

/// 정수를 파싱한다.
pub fn parse_i64(raw: &str) -> Option<i64> {
    raw.parse::<i64>().ok()
}

/// 상태코드를 파싱한다. 100~599만 유효하다.
pub fn parse_status(raw: &str) -> Option<u16> {
    raw.parse::<u16>().ok().filter(|s| (100..=599).contains(s))
}

/// 요청문을 method/target/protocol로 분해한다.
/// 형식이 어긋나면 손실 없이 전체를 target으로 둔다.
pub fn split_request_line(raw: &str) -> (Option<String>, Option<String>, Option<String>) {
    let mut parts = raw.splitn(3, ' ');
    let a = parts.next().unwrap_or_default();
    let b = parts.next();
    let c = parts.next();
    match (b, c) {
        (Some(target), Some(proto)) if is_method(a) && proto.starts_with("HTTP/") => (
            Some(a.to_owned()),
            Some(target.to_owned()),
            Some(proto.to_owned()),
        ),
        (Some(target), None) if is_method(a) => (Some(a.to_owned()), Some(target.to_owned()), None),
        _ => (None, Some(raw.to_owned()), None),
    }
}

fn is_method(token: &str) -> bool {
    !token.is_empty()
        && token
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b == b'-' || b == b'_')
}

/// 타임스탬프를 해석한다. 오프셋이 입력에 없으면 정책을 적용한다.
pub fn resolve_timestamp(
    raw: &str,
    format: &TimestampFormat,
    policy: TimezonePolicy,
) -> Option<ResolvedTime> {
    match format {
        TimestampFormat::Clf => {
            if let Ok(dt) = DateTime::parse_from_str(raw, "%d/%b/%Y:%H:%M:%S %z") {
                return Some(from_fixed(dt));
            }
            let naive = NaiveDateTime::parse_from_str(raw, "%d/%b/%Y:%H:%M:%S").ok()?;
            Some(apply_policy(naive, policy))
        }
        TimestampFormat::Iso8601 => {
            if let Ok(dt) = DateTime::parse_from_rfc3339(raw) {
                return Some(from_fixed(dt));
            }
            let naive = NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M:%S%.f")
                .or_else(|_| NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f"))
                .ok()?;
            Some(apply_policy(naive, policy))
        }
        TimestampFormat::Custom { pattern } => {
            if pattern.contains("%z") || pattern.contains("%:z") {
                let dt = DateTime::parse_from_str(raw, pattern).ok()?;
                return Some(from_fixed(dt));
            }
            let naive = NaiveDateTime::parse_from_str(raw, pattern).ok()?;
            Some(apply_policy(naive, policy))
        }
    }
}

/// W3C의 `date` + `time` 필드(UTC 기본)를 해석한다.
pub fn resolve_w3c_datetime(
    date: &str,
    time: &str,
    policy: TimezonePolicy,
) -> Option<ResolvedTime> {
    let combined = format!("{date} {time}");
    let naive = NaiveDateTime::parse_from_str(&combined, "%Y-%m-%d %H:%M:%S%.f").ok()?;
    Some(apply_policy(naive, policy))
}

fn from_fixed(dt: DateTime<FixedOffset>) -> ResolvedTime {
    ResolvedTime {
        utc_micros: Some(dt.timestamp_micros()),
        offset_seconds: Some(dt.offset().local_minus_utc()),
    }
}

fn apply_policy(naive: NaiveDateTime, policy: TimezonePolicy) -> ResolvedTime {
    let offset_seconds = match policy {
        TimezonePolicy::FromInput => {
            return ResolvedTime {
                utc_micros: None,
                offset_seconds: None,
            }
        }
        TimezonePolicy::Fixed { offset_seconds } => offset_seconds,
        TimezonePolicy::Utc => 0,
    };
    let micros = naive.and_utc().timestamp_micros() - i64::from(offset_seconds) * 1_000_000;
    ResolvedTime {
        utc_micros: Some(micros),
        offset_seconds: Some(offset_seconds),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn clf_timestamp_with_offset_converts_to_utc() {
        let t = resolve_timestamp(
            "10/Oct/2000:13:55:36 -0700",
            &TimestampFormat::Clf,
            TimezonePolicy::FromInput,
        )
        .unwrap();
        // 2000-10-10T20:55:36Z
        assert_eq!(t.utc_micros, Some(971_211_336_000_000));
        assert_eq!(t.offset_seconds, Some(-25_200));
    }

    #[test]
    fn clf_timestamp_without_offset_is_unresolved_under_from_input_policy() {
        let t = resolve_timestamp(
            "10/Oct/2000:13:55:36",
            &TimestampFormat::Clf,
            TimezonePolicy::FromInput,
        )
        .unwrap();
        assert_eq!(t.utc_micros, None);
    }

    #[test]
    fn clf_timestamp_without_offset_uses_fixed_policy() {
        let t = resolve_timestamp(
            "10/Oct/2000:13:55:36",
            &TimestampFormat::Clf,
            TimezonePolicy::Fixed {
                offset_seconds: 32_400,
            },
        )
        .unwrap();
        // 13:55:36 KST = 04:55:36Z
        assert_eq!(t.utc_micros, Some(971_153_736_000_000));
        assert_eq!(t.offset_seconds, Some(32_400));
    }

    #[test]
    fn input_offset_wins_over_fixed_policy() {
        let t = resolve_timestamp(
            "10/Oct/2000:13:55:36 +0000",
            &TimestampFormat::Clf,
            TimezonePolicy::Fixed {
                offset_seconds: 32_400,
            },
        )
        .unwrap();
        assert_eq!(t.offset_seconds, Some(0));
    }

    #[test]
    fn invalid_clf_timestamp_is_none() {
        assert!(resolve_timestamp(
            "32/Oct/2000:13:55:36 -0700",
            &TimestampFormat::Clf,
            TimezonePolicy::Utc
        )
        .is_none());
    }

    #[test]
    fn w3c_datetime_is_utc_by_default() {
        let t = resolve_w3c_datetime("2024-01-02", "03:04:05", TimezonePolicy::Utc).unwrap();
        assert_eq!(t.utc_micros, Some(1_704_164_645_000_000));
    }

    #[test]
    fn ipv6_is_valid_and_preserved() {
        assert_eq!(validate_ip("2001:db8::1"), Some("2001:db8::1"));
    }

    #[test]
    fn non_ip_is_rejected() {
        assert_eq!(validate_ip("999.1.1.1"), None);
    }

    #[test]
    fn status_outside_range_is_rejected() {
        assert_eq!(parse_status("600"), None);
        assert_eq!(parse_status("200"), Some(200));
    }

    #[test]
    fn request_line_splits_into_three_parts() {
        let (m, t, p) = split_request_line("GET /a?b=c%20d HTTP/1.1");
        assert_eq!(m.as_deref(), Some("GET"));
        assert_eq!(t.as_deref(), Some("/a?b=c%20d"));
        assert_eq!(p.as_deref(), Some("HTTP/1.1"));
    }

    #[test]
    fn malformed_request_line_keeps_whole_text_as_target() {
        let (m, t, p) = split_request_line("\\x16\\x03 garbage with spaces");
        assert_eq!(m, None);
        assert_eq!(t.as_deref(), Some("\\x16\\x03 garbage with spaces"));
        assert_eq!(p, None);
    }
}
