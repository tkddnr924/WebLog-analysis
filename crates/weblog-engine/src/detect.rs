//! 샘플 판별. 후보 프로필마다 같은 파서로 파일 선두를 미리보기해 매칭률을 비교한다.
//! 매칭률은 샘플 기준이며 서버 식별 확률이나 전체 파일 정확도가 아니다.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::error::EngineResult;
use crate::format::{presets, FormatProfile};
use crate::preview::{preview_file, PreviewConfig};

/// 후보별 판별 결과.
#[derive(Debug, Clone, Serialize)]
pub struct DetectionCandidate {
    /// 프로필 이름.
    pub profile_name: String,
    /// 정의 해시.
    pub definition_hash: String,
    /// 검사한 줄 수.
    pub lines_checked: u64,
    /// 레코드 수.
    pub records: u64,
    /// 오류 수.
    pub errors: u64,
    /// 제외 수.
    pub skipped: u64,
    /// 샘플 매칭률(레코드 / (레코드+오류)).
    pub match_rate: f64,
}

/// 파일 하나의 판별 결과. 후보는 매칭률 내림차순.
#[derive(Debug, Clone, Serialize)]
pub struct FileDetection {
    /// 경로.
    pub path: PathBuf,
    /// 후보 목록.
    pub candidates: Vec<DetectionCandidate>,
    /// 파일을 열 수 없을 때의 오류.
    pub error: Option<String>,
}

impl FileDetection {
    /// 최고 후보(레코드가 하나 이상인 경우만).
    pub fn best(&self) -> Option<&DetectionCandidate> {
        self.candidates.first().filter(|c| c.records > 0)
    }
}

/// 후보 프로필들로 파일을 판별한다.
pub fn detect_file(
    path: &Path,
    candidates: &[FormatProfile],
    cfg: &PreviewConfig,
) -> FileDetection {
    let mut out = Vec::with_capacity(candidates.len());
    for profile in candidates {
        match preview_file(path, profile, cfg) {
            Ok(p) => out.push(DetectionCandidate {
                profile_name: profile.name.clone(),
                definition_hash: profile.definition_hash().unwrap_or_default(),
                lines_checked: p.lines_checked,
                records: p.records,
                errors: p.errors,
                skipped: p.skipped,
                match_rate: p.match_rate,
            }),
            Err(e) => {
                return FileDetection {
                    path: path.to_path_buf(),
                    candidates: Vec::new(),
                    error: Some(e.to_string()),
                }
            }
        }
    }
    out.sort_by(|a, b| {
        b.match_rate
            .partial_cmp(&a.match_rate)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.records.cmp(&a.records))
    });
    FileDetection {
        path: path.to_path_buf(),
        candidates: out,
        error: None,
    }
}

/// 기본 프리셋 후보 목록(Combined를 Common보다 먼저 두어 동률일 때 더 구체적인 쪽을 고른다).
pub fn default_candidates() -> Vec<FormatProfile> {
    vec![
        presets::combined(crate::format::ServerHint::Unknown),
        presets::common(crate::format::ServerHint::Unknown),
        presets::iis_w3c(),
    ]
}

/// 여러 파일을 판별하고 최고 후보의 정의 해시로 묶는다. 폴더 전체에 하나를 강제하지 않는다.
#[derive(Debug, Clone, Serialize)]
pub struct FormatGroup {
    /// 프로필 이름.
    pub profile_name: String,
    /// 정의 해시.
    pub definition_hash: String,
    /// 이 프로필이 최고 후보인 파일들.
    pub paths: Vec<PathBuf>,
}

/// 파일별 판별 후 그룹화. 어떤 후보도 맞지 않는 파일은 `unmatched`로 돌려준다.
pub fn detect_and_group(
    paths: &[PathBuf],
    candidates: &[FormatProfile],
    cfg: &PreviewConfig,
) -> EngineResult<(Vec<FormatGroup>, Vec<FileDetection>, Vec<FileDetection>)> {
    let mut groups: Vec<FormatGroup> = Vec::new();
    let mut detections = Vec::with_capacity(paths.len());
    let mut unmatched = Vec::new();
    for path in paths {
        let d = detect_file(path, candidates, cfg);
        match d.best() {
            Some(best) => {
                if let Some(g) = groups
                    .iter_mut()
                    .find(|g| g.definition_hash == best.definition_hash)
                {
                    g.paths.push(path.clone());
                } else {
                    groups.push(FormatGroup {
                        profile_name: best.profile_name.clone(),
                        definition_hash: best.definition_hash.clone(),
                        paths: vec![path.clone()],
                    });
                }
                detections.push(d);
            }
            None => unmatched.push(d),
        }
    }
    Ok((groups, detections, unmatched))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name)
    }

    #[test]
    fn combined_fixture_is_detected_as_combined_over_common_and_w3c() {
        let d = detect_file(
            &fixture("apache_combined.log"),
            &default_candidates(),
            &PreviewConfig::default(),
        );
        assert_eq!(d.best().unwrap().profile_name, "combined");
    }

    #[test]
    fn common_fixture_prefers_common_when_combined_cannot_match() {
        let d = detect_file(
            &fixture("nginx_common.log"),
            &default_candidates(),
            &PreviewConfig::default(),
        );
        assert_eq!(d.best().unwrap().profile_name, "common");
    }

    #[test]
    fn w3c_fixture_is_detected_as_w3c() {
        let d = detect_file(
            &fixture("iis_w3c.log"),
            &default_candidates(),
            &PreviewConfig::default(),
        );
        assert_eq!(d.best().unwrap().profile_name, "iis_w3c");
    }

    #[test]
    fn unrecognized_file_has_no_best_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.log");
        std::fs::write(&p, "nothing here\nstill nothing\n").unwrap();
        let d = detect_file(&p, &default_candidates(), &PreviewConfig::default());
        assert!(d.best().is_none());
    }

    #[test]
    fn grouping_separates_files_by_best_profile_and_reports_unmatched() {
        let dir = tempfile::tempdir().unwrap();
        let junk = dir.path().join("junk.log");
        std::fs::write(&junk, "?\n").unwrap();
        let paths = vec![
            fixture("apache_combined.log"),
            fixture("apache_combined_bom_crlf.log"),
            fixture("iis_w3c.log"),
            junk,
        ];
        let (groups, detections, unmatched) =
            detect_and_group(&paths, &default_candidates(), &PreviewConfig::default()).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].profile_name, "combined");
        assert_eq!(groups[0].paths.len(), 2);
        assert_eq!(groups[1].profile_name, "iis_w3c");
        assert_eq!(detections.len(), 3);
        assert_eq!(unmatched.len(), 1);
    }

    #[test]
    fn missing_file_is_reported_in_detection_error() {
        let d = detect_file(
            Path::new("/missing/file.log"),
            &default_candidates(),
            &PreviewConfig::default(),
        );
        assert!(d.error.is_some());
        assert!(d.best().is_none());
    }
}
