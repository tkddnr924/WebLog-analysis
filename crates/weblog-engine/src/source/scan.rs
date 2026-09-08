//! 재귀 파일 탐색. 파일명 패턴은 후보 선정에만 쓰고 포맷은 내용으로 검사한다.
//! 심볼릭 링크는 기본적으로 따라가지 않으며 권한 오류는 항목별로 보고한다.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::reader::{Compression, StatSnapshot};

/// 탐색 옵션.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    /// 하위 디렉터리 재귀.
    pub recursive: bool,
    /// 최대 깊이(루트 = 0). `None`이면 제한 없음.
    pub max_depth: Option<usize>,
    /// 포함 파일명 패턴(`*`, `?` 지원). 비어 있으면 모두 포함.
    pub include: Vec<String>,
    /// 제외 파일명 패턴. 포함보다 우선한다.
    pub exclude: Vec<String>,
    /// 심볼릭 링크 추적.
    pub follow_symlinks: bool,
    /// 최대 항목 수. 넘으면 `truncated`로 표시하고 중단한다.
    pub max_entries: usize,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            recursive: true,
            max_depth: None,
            include: Vec::new(),
            exclude: Vec::new(),
            follow_symlinks: false,
            max_entries: 100_000,
        }
    }
}

/// 탐색된 파일.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanEntry {
    /// 경로.
    pub path: PathBuf,
    /// 크기.
    pub file_size: u64,
    /// 수정 시각(Unix 초).
    pub modified_unix: Option<i64>,
    /// 내용으로 판별한 압축 방식. 읽을 수 없으면 `None`.
    pub compression: Option<Compression>,
    /// 루트 기준 깊이.
    pub depth: usize,
}

/// 항목별 오류(권한 등). 탐색 전체를 중단하지 않는다.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScanError {
    /// 경로.
    pub path: PathBuf,
    /// 오류 설명(OS 메시지).
    pub message: String,
}

/// 탐색 결과.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanResult {
    /// 파일 목록(경로 정렬).
    pub entries: Vec<ScanEntry>,
    /// 오류 목록.
    pub errors: Vec<ScanError>,
    /// 최대 항목 수에 걸려 중단됐는지.
    pub truncated: bool,
    /// 살펴본 디렉터리 수.
    pub directories_visited: usize,
    /// 패턴에 걸러진 파일 수.
    pub filtered_out: usize,
}

/// `*`(임의 길이), `?`(한 글자) 와일드카드 매칭. 대소문자를 구분한다.
pub fn wildcard_match(pattern: &str, name: &str) -> bool {
    fn go(p: &[char], n: &[char]) -> bool {
        match (p.first(), n.first()) {
            (None, None) => true,
            (Some('*'), _) => go(&p[1..], n) || (!n.is_empty() && go(p, &n[1..])),
            (Some('?'), Some(_)) => go(&p[1..], &n[1..]),
            (Some(a), Some(b)) if a == b => go(&p[1..], &n[1..]),
            _ => false,
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let n: Vec<char> = name.chars().collect();
    go(&p, &n)
}

fn name_selected(name: &str, opts: &ScanOptions) -> bool {
    if opts.exclude.iter().any(|p| wildcard_match(p, name)) {
        return false;
    }
    opts.include.is_empty() || opts.include.iter().any(|p| wildcard_match(p, name))
}

/// 루트 아래를 탐색한다. 루트가 파일이면 그 파일 하나를 검사한다.
pub fn scan_directory(root: &Path, opts: &ScanOptions) -> ScanResult {
    let mut result = ScanResult::default();
    let root_meta = match std::fs::symlink_metadata(root) {
        Ok(m) => m,
        Err(e) => {
            result.errors.push(ScanError {
                path: root.to_path_buf(),
                message: e.to_string(),
            });
            return result;
        }
    };
    if root_meta.is_file() {
        push_file(root, 0, opts, &mut result);
        return result;
    }
    let mut stack: Vec<(PathBuf, usize)> = vec![(root.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        if result.truncated {
            break;
        }
        result.directories_visited += 1;
        let read = match std::fs::read_dir(&dir) {
            Ok(r) => r,
            Err(e) => {
                result.errors.push(ScanError {
                    path: dir.clone(),
                    message: e.to_string(),
                });
                continue;
            }
        };
        let mut children: Vec<PathBuf> = Vec::new();
        for entry in read {
            match entry {
                Ok(e) => children.push(e.path()),
                Err(e) => result.errors.push(ScanError {
                    path: dir.clone(),
                    message: e.to_string(),
                }),
            }
        }
        // 스택은 LIFO이므로 역순으로 넣어 정렬된 순서로 방문한다.
        children.sort();
        for child in children.into_iter().rev() {
            let meta = match std::fs::symlink_metadata(&child) {
                Ok(m) => m,
                Err(e) => {
                    result.errors.push(ScanError {
                        path: child.clone(),
                        message: e.to_string(),
                    });
                    continue;
                }
            };
            let file_type = meta.file_type();
            let (is_dir, is_file) = if file_type.is_symlink() {
                if !opts.follow_symlinks {
                    continue;
                }
                match std::fs::metadata(&child) {
                    Ok(m) => (m.is_dir(), m.is_file()),
                    Err(e) => {
                        result.errors.push(ScanError {
                            path: child.clone(),
                            message: e.to_string(),
                        });
                        continue;
                    }
                }
            } else {
                (file_type.is_dir(), file_type.is_file())
            };
            if is_dir {
                let next_depth = depth + 1;
                if opts.recursive && opts.max_depth.is_none_or(|m| next_depth <= m) {
                    stack.push((child, next_depth));
                }
            } else if is_file {
                push_file(&child, depth + 1, opts, &mut result);
                if result.truncated {
                    break;
                }
            }
        }
    }
    result.entries.sort_by(|a, b| a.path.cmp(&b.path));
    result
}

fn push_file(path: &Path, depth: usize, opts: &ScanOptions, result: &mut ScanResult) {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !name_selected(name, opts) {
        result.filtered_out += 1;
        return;
    }
    if result.entries.len() >= opts.max_entries {
        result.truncated = true;
        return;
    }
    let stat = match StatSnapshot::read(path) {
        Ok(s) => s,
        Err(e) => {
            result.errors.push(ScanError {
                path: path.to_path_buf(),
                message: e.to_string(),
            });
            return;
        }
    };
    let compression = match Compression::detect(path) {
        Ok(c) => Some(c),
        Err(e) => {
            result.errors.push(ScanError {
                path: path.to_path_buf(),
                message: e.to_string(),
            });
            None
        }
    };
    result.entries.push(ScanEntry {
        path: path.to_path_buf(),
        file_size: stat.file_size,
        modified_unix: stat.modified_unix,
        compression,
        depth,
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn tree() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let r = dir.path();
        std::fs::create_dir_all(r.join("a/b")).unwrap();
        std::fs::create_dir_all(r.join("skip")).unwrap();
        std::fs::write(r.join("access.log"), b"x\n").unwrap();
        std::fs::write(r.join("a/access.log.1"), b"y\n").unwrap();
        std::fs::write(r.join("a/b/error.log"), b"z\n").unwrap();
        std::fs::write(r.join("a/b/access.log.gz"), [0x1f, 0x8b, 0x08, 0x00]).unwrap();
        std::fs::write(r.join("skip/access.log"), b"s\n").unwrap();
        dir
    }

    fn names(result: &ScanResult, root: &Path) -> Vec<String> {
        result
            .entries
            .iter()
            .map(|e| {
                e.path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect()
    }

    #[test]
    fn wildcard_matches_star_and_question() {
        assert!(wildcard_match("access.log*", "access.log.1"));
        assert!(wildcard_match("*.gz", "a.log.gz"));
        assert!(wildcard_match("u_ex??????.log", "u_ex240102.log"));
        assert!(!wildcard_match("*.gz", "a.log"));
        assert!(!wildcard_match("access.log", "access.log.1"));
    }

    #[test]
    fn recursive_scan_lists_all_files_sorted_with_depth_and_compression() {
        let dir = tree();
        let r = scan_directory(dir.path(), &ScanOptions::default());
        assert_eq!(
            names(&r, dir.path()),
            vec![
                "a/access.log.1",
                "a/b/access.log.gz",
                "a/b/error.log",
                "access.log",
                "skip/access.log"
            ]
        );
        let gz = r
            .entries
            .iter()
            .find(|e| e.path.ends_with("access.log.gz"))
            .unwrap();
        assert_eq!(gz.compression, Some(Compression::Gzip));
        assert_eq!(gz.depth, 3);
        assert!(r.errors.is_empty());
    }

    #[test]
    fn include_and_exclude_patterns_filter_by_file_name_only() {
        let dir = tree();
        let opts = ScanOptions {
            include: vec!["access.log*".to_owned()],
            exclude: vec!["*.gz".to_owned()],
            ..ScanOptions::default()
        };
        let r = scan_directory(dir.path(), &opts);
        assert_eq!(
            names(&r, dir.path()),
            vec!["a/access.log.1", "access.log", "skip/access.log"]
        );
        assert_eq!(r.filtered_out, 2);
    }

    #[test]
    fn non_recursive_scan_stays_at_root() {
        let dir = tree();
        let opts = ScanOptions {
            recursive: false,
            ..ScanOptions::default()
        };
        assert_eq!(
            names(&scan_directory(dir.path(), &opts), dir.path()),
            vec!["access.log"]
        );
    }

    #[test]
    fn max_depth_limits_descent() {
        let dir = tree();
        let opts = ScanOptions {
            max_depth: Some(1),
            ..ScanOptions::default()
        };
        assert_eq!(
            names(&scan_directory(dir.path(), &opts), dir.path()),
            vec!["a/access.log.1", "access.log", "skip/access.log"]
        );
    }

    #[test]
    fn max_entries_marks_truncated() {
        let dir = tree();
        let opts = ScanOptions {
            max_entries: 2,
            ..ScanOptions::default()
        };
        let r = scan_directory(dir.path(), &opts);
        assert!(r.truncated);
        assert_eq!(r.entries.len(), 2);
    }

    #[cfg(unix)]
    #[test]
    fn symlinks_are_skipped_unless_followed() {
        let dir = tree();
        std::os::unix::fs::symlink(dir.path().join("a"), dir.path().join("link")).unwrap();
        let default = scan_directory(dir.path(), &ScanOptions::default());
        assert!(!names(&default, dir.path())
            .iter()
            .any(|n| n.starts_with("link/")));
        let follow = scan_directory(
            dir.path(),
            &ScanOptions {
                follow_symlinks: true,
                ..ScanOptions::default()
            },
        );
        assert!(names(&follow, dir.path())
            .iter()
            .any(|n| n.starts_with("link/")));
    }

    #[test]
    fn missing_root_is_reported_as_error_not_panic() {
        let r = scan_directory(
            Path::new("/definitely/missing/dir"),
            &ScanOptions::default(),
        );
        assert_eq!(r.errors.len(), 1);
        assert!(r.entries.is_empty());
    }

    #[test]
    fn scanning_a_single_file_returns_that_file() {
        let dir = tree();
        let r = scan_directory(&dir.path().join("access.log"), &ScanOptions::default());
        assert_eq!(r.entries.len(), 1);
    }
}
