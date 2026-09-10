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

/// 앱이 만드는 모든 파일의 위치. 실행 파일 폴더에는 `cases` 하나만 생긴다.
pub struct Layout {
    /// 케이스 DB(`<이름>-<시각>.duckdb`)를 두는 폴더.
    pub cases: PathBuf,
    /// 실행·크래시 로그 폴더.
    pub logs: PathBuf,
    /// 사용자 프리셋 폴더.
    pub presets: PathBuf,
}

/// 데이터 루트 아래 배치. 로그·프리셋도 `cases` 안에 둬서 폴더 하나만 남긴다.
pub fn layout(root: &Path) -> Layout {
    let cases = root.join("cases");
    Layout {
        logs: cases.join("logs"),
        presets: cases.join("presets"),
        cases,
    }
}

/// WebView2 캐시 위치. 앱 폴더와 AppData를 더럽히지 않도록 임시 폴더에 둔다.
pub fn webview_cache_dir() -> PathBuf {
    std::env::temp_dir().join("weblog-webview")
}

/// 종료할 때 WebView2 캐시를 지운다. 파일이 잠겨 있으면 남을 수 있어 실패는 무시한다.
pub fn remove_webview_cache(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
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

    #[test]
    fn everything_lives_under_the_cases_folder() {
        let root = Path::new("/opt/weblog");
        let l = layout(root);
        assert_eq!(l.cases, root.join("cases"));
        for p in [&l.logs, &l.presets] {
            assert!(p.starts_with(&l.cases), "{} 는 cases 밖이다", p.display());
        }
    }

    #[test]
    fn webview_cache_stays_out_of_the_app_folder_and_is_removable() {
        let cache = webview_cache_dir();
        assert!(cache.starts_with(std::env::temp_dir()));
        let dir = tempfile::tempdir().unwrap();
        let victim = dir.path().join("EBWebView");
        std::fs::create_dir_all(victim.join("Default")).unwrap();
        std::fs::write(victim.join("Default").join("cookies"), b"x").unwrap();
        remove_webview_cache(&victim);
        assert!(!victim.exists(), "종료 시 캐시를 지운다");
    }
}
