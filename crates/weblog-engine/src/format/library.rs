//! 사용자 프리셋 저장소. 디렉터리 하나에 `<이름>.yaml`로 저장한다. 내장 프리셋과 이름이 겹칠 수 없다.

use std::path::{Path, PathBuf};

use serde::Serialize;

use super::model::FormatProfile;
use super::presets;
use super::validate::is_valid_profile_name;
use super::yaml;
use crate::error::{EngineError, EngineResult};

/// 저장된 프로필.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct StoredProfile {
    /// 이름(파일명과 같다).
    pub name: String,
    /// 파일 경로.
    pub path: PathBuf,
    /// 정의.
    pub profile: FormatProfile,
}

/// 목록 결과: 읽은 프로필과 읽지 못한 파일.
#[derive(Debug, Clone, Default)]
pub struct ProfileListing {
    /// 읽은 프로필(이름순).
    pub profiles: Vec<StoredProfile>,
    /// 읽지 못한 파일과 사유.
    pub errors: Vec<(PathBuf, String)>,
}

/// 사용자 프리셋 디렉터리.
#[derive(Debug, Clone)]
pub struct ProfileLibrary {
    dir: PathBuf,
}

impl ProfileLibrary {
    /// 디렉터리를 지정한다. 없으면 저장 시 만든다.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// 디렉터리.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn path_for(&self, name: &str) -> EngineResult<PathBuf> {
        if !is_valid_profile_name(name) {
            return Err(EngineError::Format(format!(
                "프로필 이름으로 쓸 수 없음: {name}"
            )));
        }
        Ok(self.dir.join(format!("{name}.yaml")))
    }

    /// 저장된 프로필 목록(이름순). 읽을 수 없는 파일은 건너뛰고 오류 목록으로 돌려준다.
    pub fn list(&self) -> EngineResult<ProfileListing> {
        let mut out = Vec::new();
        let mut errors = Vec::new();
        let read = match std::fs::read_dir(&self.dir) {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(ProfileListing::default())
            }
            Err(e) => return Err(e.into()),
        };
        for entry in read {
            let path = entry?.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            match std::fs::read_to_string(&path)
                .map_err(EngineError::from)
                .and_then(|t| yaml::from_yaml(&t))
            {
                Ok(profile) => out.push(StoredProfile {
                    name: stem.to_owned(),
                    path: path.clone(),
                    profile,
                }),
                Err(e) => errors.push((path, e.to_string())),
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(ProfileListing {
            profiles: out,
            errors,
        })
    }

    /// 이름으로 읽는다.
    pub fn load(&self, name: &str) -> EngineResult<Option<FormatProfile>> {
        let path = self.path_for(name)?;
        match std::fs::read_to_string(&path) {
            Ok(text) => Ok(Some(yaml::from_yaml(&text)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// 검증 후 저장한다. 파일 이름은 정의의 `name`이다. 내장 프리셋 이름은 거부한다.
    pub fn save(&self, profile: &FormatProfile) -> EngineResult<PathBuf> {
        profile.ensure_valid()?;
        if presets::by_name(&profile.name).is_some() {
            return Err(EngineError::Format(format!(
                "'{}'은 내장 프리셋 이름이라 저장할 수 없음. 다른 이름을 쓴다",
                profile.name
            )));
        }
        let path = self.path_for(&profile.name)?;
        std::fs::create_dir_all(&self.dir)?;
        let text = yaml::to_yaml(profile)?;
        // 임시 파일에 쓴 뒤 교체해 부분 기록을 남기지 않는다.
        let tmp = path.with_extension("yaml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(&tmp, &path)?;
        Ok(path)
    }

    /// 삭제한다. 없으면 false.
    pub fn delete(&self, name: &str) -> EngineResult<bool> {
        let path = self.path_for(name)?;
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn save_list_load_delete_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let lib = ProfileLibrary::new(dir.path().join("presets"));
        assert!(
            lib.list().unwrap().profiles.is_empty(),
            "missing dir is empty, not an error"
        );
        let mut p = presets::apache_combined();
        p.name = "my_apache".to_owned();
        p.version = 3;
        lib.save(&p).unwrap();
        let listing = lib.list().unwrap();
        assert!(listing.errors.is_empty());
        assert_eq!(listing.profiles.len(), 1);
        assert_eq!(listing.profiles[0].profile, p);
        assert_eq!(lib.load("my_apache").unwrap().unwrap().version, 3);
        assert!(lib.delete("my_apache").unwrap());
        assert!(!lib.delete("my_apache").unwrap());
        assert!(lib.load("my_apache").unwrap().is_none());
    }

    #[test]
    fn builtin_names_and_unsafe_names_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let lib = ProfileLibrary::new(dir.path());
        assert!(lib.save(&presets::apache_combined()).is_err());
        let mut p = presets::apache_combined();
        p.name = "../escape".to_owned();
        assert!(lib.save(&p).is_err());
        assert!(lib.load("../escape").is_err());
    }

    #[test]
    fn corrupt_file_is_reported_not_fatal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("broken.yaml"), "name: [").unwrap();
        let lib = ProfileLibrary::new(dir.path());
        let listing = lib.list().unwrap();
        assert!(listing.profiles.is_empty());
        assert_eq!(listing.errors.len(), 1);
    }
}
