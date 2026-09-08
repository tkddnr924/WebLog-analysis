//! 재구성 로그. 저장 필드로 텍스트를 만든다. 원문과 바이트 단위로 같지 않다.
//! 블록 정의가 있으면 그 순서대로 복원하고, 없으면 Combined 유사 표준 텍스트를 만든다.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::format::validate::is_valid_strftime;
use crate::format::{Block, Capture, FieldKind, FormatProfile, Strategy, TimestampFormat};
use crate::store::LogDetail;

/// 재구성 결과. 화면에는 반드시 "재구성"으로 표시한다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Reconstructed {
    /// 텍스트.
    pub text: String,
    /// 항상 true. 원문 복원이 아님을 호출자가 잊지 않게 한다.
    pub is_reconstruction: bool,
    /// 사용한 템플릿: `blocks`(정의 순서) 또는 `standard`(Combined 유사).
    pub template: &'static str,
    /// 정규식 블록·W3C처럼 역변환 템플릿이 없는 부분이 있으면 false.
    pub complete: bool,
}

fn or_dash(v: &Option<String>) -> &str {
    v.as_deref().unwrap_or("-")
}

fn zoned(micros: i64, offset: Option<i32>) -> Option<chrono::DateTime<chrono::FixedOffset>> {
    let dt = chrono::DateTime::from_timestamp_micros(micros)?;
    let tz = chrono::FixedOffset::east_opt(offset.unwrap_or(0))?;
    Some(dt.with_timezone(&tz))
}

fn format_time(micros: Option<i64>, offset: Option<i32>, format: &TimestampFormat) -> String {
    let Some(us) = micros else {
        return "-".to_owned();
    };
    let Some(dt) = zoned(us, offset) else {
        return "-".to_owned();
    };
    match format {
        TimestampFormat::Clf => dt.format("%d/%b/%Y:%H:%M:%S %z").to_string(),
        TimestampFormat::Iso8601 => dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false),
        TimestampFormat::Custom { pattern } => {
            if is_valid_strftime(pattern) {
                dt.format(pattern).to_string()
            } else {
                dt.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
            }
        }
    }
}

/// 상세 레코드를 Combined 유사 텍스트로 재구성한다. 누락값은 `-`로 표시하며 추측하지 않는다.
pub fn combined_like(detail: &LogDetail) -> Reconstructed {
    let request = match (&detail.method, &detail.request_target, &detail.protocol) {
        (None, None, None) => "-".to_owned(),
        (m, t, p) => format!("{} {} {}", or_dash(m), or_dash(t), or_dash(p)),
    };
    let status = detail
        .status
        .map_or_else(|| "-".to_owned(), |s| s.to_string());
    let bytes = detail
        .bytes_sent
        .map_or_else(|| "-".to_owned(), |b| b.to_string());
    let text = format!(
        "{} - - [{}] \"{}\" {} {} \"{}\" \"{}\"",
        or_dash(&detail.client_ip),
        format_time(
            detail.timestamp_utc,
            detail.tz_offset_seconds,
            &TimestampFormat::Clf
        ),
        request,
        status,
        bytes,
        or_dash(&detail.referrer),
        or_dash(&detail.user_agent),
    );
    Reconstructed {
        text,
        is_reconstruction: true,
        template: "standard",
        complete: true,
    }
}

/// 프로필의 블록 순서대로 재구성한다. W3C나 정규식 블록은 역변환할 수 없어 표준 텍스트로 대체하거나 `…`로 표시한다.
pub fn from_profile(
    profile: &FormatProfile,
    detail: &LogDetail,
    extra: &BTreeMap<String, String>,
) -> Reconstructed {
    let Strategy::Blocks { blocks } = &profile.strategy else {
        let mut r = combined_like(detail);
        r.complete = false;
        return r;
    };
    let mut text = String::new();
    let mut complete = true;
    emit_blocks(blocks, detail, extra, &mut text, &mut complete);
    Reconstructed {
        text,
        is_reconstruction: true,
        template: "blocks",
        complete,
    }
}

/// 필드 값. 누락이면 `None`.
fn field_value(
    kind: &FieldKind,
    name: &str,
    detail: &LogDetail,
    extra: &BTreeMap<String, String>,
) -> Option<String> {
    match kind {
        FieldKind::ClientIp => detail.client_ip.clone(),
        FieldKind::Timestamp { format } => detail
            .timestamp_utc
            .map(|_| format_time(detail.timestamp_utc, detail.tz_offset_seconds, format)),
        FieldKind::RequestLine => {
            match (&detail.method, &detail.request_target, &detail.protocol) {
                (None, None, None) => None,
                (None, Some(t), None) => Some(t.clone()),
                (m, t, p) => Some(format!("{} {} {}", or_dash(m), or_dash(t), or_dash(p))),
            }
        }
        FieldKind::Method => detail.method.clone(),
        FieldKind::RequestTarget => detail.request_target.clone(),
        FieldKind::Protocol => detail.protocol.clone(),
        FieldKind::Status => detail.status.map(|s| s.to_string()),
        FieldKind::BytesSent => detail.bytes_sent.map(|b| b.to_string()),
        FieldKind::Referrer => detail.referrer.clone(),
        FieldKind::UserAgent => detail.user_agent.clone(),
        FieldKind::Integer | FieldKind::Text => extra.get(name).cloned(),
    }
}

fn group_has_value(blocks: &[Block], detail: &LogDetail, extra: &BTreeMap<String, String>) -> bool {
    blocks.iter().any(|b| match b {
        Block::Field(def) => field_value(&def.kind, &def.name, detail, extra).is_some(),
        Block::OptionalGroup { blocks } => group_has_value(blocks, detail, extra),
        // 정규식 블록은 이름 있는 캡처가 확장 필드에 남아 있을 때만 값이 있는 것으로 본다.
        Block::Regex { pattern } => regex::Regex::new(pattern)
            .map(|re| re.capture_names().flatten().any(|n| extra.contains_key(n)))
            .unwrap_or(false),
        _ => false,
    })
}

fn emit_blocks(
    blocks: &[Block],
    detail: &LogDetail,
    extra: &BTreeMap<String, String>,
    out: &mut String,
    complete: &mut bool,
) {
    for block in blocks {
        match block {
            Block::Literal { text } => out.push_str(text),
            Block::Whitespace => out.push(' '),
            Block::Field(def) => {
                let value = field_value(&def.kind, &def.name, detail, extra).unwrap_or_else(|| {
                    def.missing
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "-".to_owned())
                });
                match &def.capture {
                    Capture::Quoted => {
                        out.push('"');
                        out.push_str(&value);
                        out.push('"');
                    }
                    Capture::Bracketed => {
                        out.push('[');
                        out.push_str(&value);
                        out.push(']');
                    }
                    Capture::Token | Capture::Pattern { .. } => out.push_str(&value),
                }
            }
            Block::OptionalGroup { blocks } => {
                if group_has_value(blocks, detail, extra) {
                    emit_blocks(blocks, detail, extra, out, complete);
                }
            }
            Block::Regex { .. } => {
                // 임의 정규식은 역변환 템플릿이 없다. 이름 있는 캡처 값만 나열한다.
                *complete = false;
                out.push('…');
            }
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::format::presets;

    fn detail() -> LogDetail {
        LogDetail {
            job_id: 1,
            batch_id: 1,
            source_id: 1,
            source_path: "x.log".to_owned(),
            line_number: 1,
            timestamp_utc: Some(971_211_336_000_000),
            tz_offset_seconds: Some(-25_200),
            client_ip: Some("127.0.0.1".to_owned()),
            method: Some("GET".to_owned()),
            request_target: Some("/a?b=1".to_owned()),
            protocol: Some("HTTP/1.0".to_owned()),
            status: Some(200),
            bytes_sent: None,
            referrer: None,
            user_agent: Some("UA".to_owned()),
            extra_json: None,
        }
    }

    #[test]
    fn standard_reconstruction_uses_stored_offset_and_marks_missing_values() {
        let r = combined_like(&detail());
        assert!(r.is_reconstruction);
        assert_eq!(
            r.text,
            "127.0.0.1 - - [10/Oct/2000:13:55:36 -0700] \"GET /a?b=1 HTTP/1.0\" 200 - \"-\" \"UA\""
        );
    }

    #[test]
    fn combined_preset_reconstruction_follows_block_order() {
        let mut extra = BTreeMap::new();
        extra.insert("remote_user".to_owned(), "frank".to_owned());
        let r = from_profile(&presets::apache_combined(), &detail(), &extra);
        assert_eq!(r.template, "blocks");
        assert!(r.complete);
        assert_eq!(
            r.text,
            "127.0.0.1 - frank [10/Oct/2000:13:55:36 -0700] \"GET /a?b=1 HTTP/1.0\" 200 - \"-\" \"UA\""
        );
    }

    #[test]
    fn custom_pipe_reconstruction_skips_empty_optional_group_and_marks_regex_incomplete() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let profile = FormatProfile::from_json(
            &std::fs::read_to_string(dir.join("custom_pipe.profile.json")).unwrap(),
        )
        .unwrap();
        let mut d = detail();
        d.tz_offset_seconds = Some(32_400);
        let r = from_profile(&profile, &d, &BTreeMap::new());
        assert_eq!(
            r.text,
            "2000-10-11T05:55:36+09:00|127.0.0.1|GET|/a?b=1|200|-|UA"
        );
        assert!(
            r.complete,
            "optional group without values is skipped, so no regex placeholder"
        );
        let mut extra = BTreeMap::new();
        extra.insert("response_time_ms".to_owned(), "12".to_owned());
        let r2 = from_profile(&profile, &d, &extra);
        assert!(r2.text.ends_with("|UA|…"));
        assert!(!r2.complete);
    }

    #[test]
    fn w3c_profile_falls_back_to_standard_text() {
        let r = from_profile(&presets::iis_w3c(), &detail(), &BTreeMap::new());
        assert_eq!(r.template, "standard");
        assert!(!r.complete);
    }
}
