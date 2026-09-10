//! 포맷 정의의 YAML 표현. 퍼즐과 YAML은 같은 정의를 편집하며 의미가 같으면 같은 정의 해시를 가진다.

use super::model::FormatProfile;
use crate::error::{EngineError, EngineResult};

/// YAML input byte cap. Callers check file size before reading.
pub const MAX_YAML_BYTES: usize = 256 * 1024;

/// 정의를 YAML 문자열로.
pub fn to_yaml(profile: &FormatProfile) -> EngineResult<String> {
    serde_norway::to_string(profile)
        .map_err(|e| EngineError::Format(format!("YAML 직렬화 실패: {e}")))
}

/// YAML 문자열을 정의로 읽고 검증한다. 잘못된 정의는 오류다.
pub fn from_yaml(yaml: &str) -> EngineResult<FormatProfile> {
    if yaml.len() > MAX_YAML_BYTES {
        return Err(EngineError::Limit(format!(
            "YAML이 상한 {MAX_YAML_BYTES}바이트를 넘음"
        )));
    }
    let profile: FormatProfile = serde_norway::from_str(yaml)
        .map_err(|e| EngineError::Format(format!("YAML 해석 실패: {e}")))?;
    profile.ensure_valid()?;
    Ok(profile)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use crate::format::presets;

    #[test]
    fn presets_roundtrip_through_yaml_with_identical_definition_hash() {
        for name in presets::PRESET_NAMES {
            let p = presets::by_name(name).unwrap();
            let yaml = to_yaml(&p).unwrap();
            let back = from_yaml(&yaml).unwrap();
            assert_eq!(p, back, "{name}");
            assert_eq!(
                p.definition_hash().unwrap(),
                back.definition_hash().unwrap()
            );
        }
    }

    #[test]
    fn hand_written_yaml_matches_json_fixture_profile() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let json = std::fs::read_to_string(dir.join("custom_pipe.profile.json")).unwrap();
        let yaml = std::fs::read_to_string(dir.join("custom_pipe.profile.yaml")).unwrap();
        let from_json = FormatProfile::from_json(&json).unwrap();
        let from_yaml = from_yaml(&yaml).unwrap();
        assert_eq!(from_json, from_yaml);
    }

    #[test]
    fn invalid_yaml_and_invalid_definition_are_rejected() {
        assert!(matches!(
            from_yaml("name: [unclosed"),
            Err(EngineError::Format(_))
        ));
        let mut p = presets::apache_combined();
        p.name = "bad name".to_owned();
        let yaml = to_yaml(&p).unwrap();
        assert!(matches!(from_yaml(&yaml), Err(EngineError::Format(_))));
    }

    #[test]
    fn yaml_comments_are_not_preserved_but_meaning_is() {
        let p = presets::common(crate::format::ServerHint::Nginx);
        let yaml = format!("# 주석은 보존하지 않는다\n{}", to_yaml(&p).unwrap());
        let back = from_yaml(&yaml).unwrap();
        assert_eq!(
            back.definition_hash().unwrap(),
            p.definition_hash().unwrap()
        );
        assert!(!to_yaml(&back).unwrap().contains("주석"));
    }
}
