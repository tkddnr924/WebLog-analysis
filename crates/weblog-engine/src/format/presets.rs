//! 기본 프리셋. Apache/Nginx Common·Combined, IIS W3C.

use super::model::{
    Block, Capture, FieldDef, FieldKind, FormatProfile, ServerHint, Strategy, TimestampFormat,
    TimezonePolicy, SCHEMA_VERSION,
};

fn field(name: &str, kind: FieldKind, capture: Capture) -> Block {
    Block::Field(FieldDef {
        name: name.to_owned(),
        kind,
        capture,
        missing: vec!["-".to_owned()],
    })
}

fn common_blocks() -> Vec<Block> {
    vec![
        field("client_ip", FieldKind::ClientIp, Capture::Token),
        Block::Whitespace,
        field("ident", FieldKind::Text, Capture::Token),
        Block::Whitespace,
        field("remote_user", FieldKind::Text, Capture::Token),
        Block::Whitespace,
        field(
            "timestamp",
            FieldKind::Timestamp {
                format: TimestampFormat::Clf,
            },
            Capture::Bracketed,
        ),
        Block::Whitespace,
        field("request", FieldKind::RequestLine, Capture::Quoted),
        Block::Whitespace,
        field("status", FieldKind::Status, Capture::Token),
        Block::Whitespace,
        field("bytes_sent", FieldKind::BytesSent, Capture::Token),
    ]
}

/// Apache/Nginx Common Log Format.
pub fn common(server_hint: ServerHint) -> FormatProfile {
    FormatProfile {
        schema_version: SCHEMA_VERSION,
        name: "common".to_owned(),
        version: 1,
        server_hint,
        timezone: TimezonePolicy::FromInput,
        strategy: Strategy::Blocks {
            blocks: common_blocks(),
        },
    }
}

/// Apache/Nginx Combined Log Format(Common + referrer + user agent).
pub fn combined(server_hint: ServerHint) -> FormatProfile {
    let mut blocks = common_blocks();
    blocks.extend([
        Block::Whitespace,
        field("referrer", FieldKind::Referrer, Capture::Quoted),
        Block::Whitespace,
        field("user_agent", FieldKind::UserAgent, Capture::Quoted),
    ]);
    FormatProfile {
        schema_version: SCHEMA_VERSION,
        name: "combined".to_owned(),
        version: 1,
        server_hint,
        timezone: TimezonePolicy::FromInput,
        strategy: Strategy::Blocks { blocks },
    }
}

/// Apache Combined 프리셋.
pub fn apache_combined() -> FormatProfile {
    combined(ServerHint::Apache)
}

/// Nginx Combined 프리셋(Nginx 기본 `combined` 형식과 동일).
pub fn nginx_combined() -> FormatProfile {
    combined(ServerHint::Nginx)
}

/// IIS W3C 확장 로그 프리셋. 헤더 기반이며 시간은 UTC로 기록된다.
pub fn iis_w3c() -> FormatProfile {
    FormatProfile {
        schema_version: SCHEMA_VERSION,
        name: "iis_w3c".to_owned(),
        version: 1,
        server_hint: ServerHint::Iis,
        timezone: TimezonePolicy::Utc,
        strategy: Strategy::W3c,
    }
}

/// 이름으로 프리셋을 찾는다. `common`, `combined`, `apache_combined`, `nginx_combined`, `iis_w3c`.
pub fn by_name(name: &str) -> Option<FormatProfile> {
    match name {
        "common" => Some(common(ServerHint::Unknown)),
        "combined" => Some(combined(ServerHint::Unknown)),
        "apache_combined" => Some(apache_combined()),
        "nginx_combined" => Some(nginx_combined()),
        "iis_w3c" | "w3c" => Some(iis_w3c()),
        _ => None,
    }
}

/// 내장 프리셋 전체(이름, 정의).
pub fn builtin() -> Vec<(&'static str, FormatProfile)> {
    PRESET_NAMES
        .iter()
        .filter_map(|n| by_name(n).map(|p| (*n, p)))
        .collect()
}

/// 프리셋 이름 목록.
pub const PRESET_NAMES: &[&str] = &[
    "common",
    "combined",
    "apache_combined",
    "nginx_combined",
    "iis_w3c",
];
