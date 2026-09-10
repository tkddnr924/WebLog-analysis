//! 저장 위치. 케이스·프리셋·로그는 실행 파일 옆에 둔다(포터블). 쓸 수 없으면 임시 폴더로 물러난다.

use std::path::{Path, PathBuf};

/// 실행 파일이 있는 폴더.
pub fn exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    exe.parent().map(Path::to_path_buf)
}

/// 폴더를 만들고 쓸 수 있는지 확인한다. 설치 경로가 읽기 전용일 수 있어 실제로 파일을 써 본다.
pub fn is_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".weblog-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// 데이터 루트. 실행 파일 폴더가 쓰기 가능하면 그곳, 아니면 임시 폴더 아래 `weblog`.
/// 두 번째 값은 대체 위치를 쓴 이유이며, 없으면 정상이다.
pub fn data_root(exe_dir: Option<PathBuf>) -> (PathBuf, Option<String>) {
    if let Some(dir) = exe_dir {
        if is_writable(&dir) {
            return (dir, None);
        }
        let fallback = std::env::temp_dir().join("weblog");
        return (
            fallback.clone(),
            Some(format!(
                "실행 파일 폴더에 쓸 수 없어 {}에 저장합니다. 쓰기 가능한 폴더로 옮겨 실행하세요.",
                fallback.display()
            )),
        );
    }
    let fallback = std::env::temp_dir().join("weblog");
    (
        fallback.clone(),
        Some(format!(
            "실행 파일 위치를 알 수 없어 {}에 저장합니다.",
            fallback.display()
        )),
    )
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn data_root_uses_the_executable_folder_when_writable() {
        let dir = tempfile::tempdir().unwrap();
        let (root, note) = data_root(Some(dir.path().to_path_buf()));
        assert_eq!(root, dir.path());
        assert!(note.is_none(), "정상 경로에는 안내가 없다");
    }

    #[cfg(unix)]
    #[test]
    fn data_root_falls_back_when_the_folder_is_read_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let locked = dir.path().join("ro");
        std::fs::create_dir(&locked).unwrap();
        std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o555)).unwrap();
        let (root, note) = data_root(Some(locked.clone()));
        assert_ne!(root, locked);
        assert!(root.starts_with(std::env::temp_dir()));
        assert!(note.unwrap().contains("쓸 수 없어"), "이유를 알린다");
    }

    #[test]
    fn data_root_falls_back_without_an_executable_path() {
        let (root, note) = data_root(None);
        assert!(root.starts_with(std::env::temp_dir()));
        assert!(note.is_some());
    }
}
