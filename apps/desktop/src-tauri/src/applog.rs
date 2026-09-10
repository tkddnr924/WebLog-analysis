//! 파일 로그. 크래시 추적용이라 마지막 줄이 어디까지 진행했는지 알려 주는 것이 목적이다.
//! 로그 원문·필드 값은 절대 남기지 않는다. 남기는 것은 단계, 경로, 건수, 오류 메시지다.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use std::time::{SystemTime, UNIX_EPOCH};

use weblog_service::ServiceEvent;

/// 화면과 같은 한국 시간으로 남긴다.
const KST_OFFSET_SECONDS: i64 = 9 * 3600;
const ROTATE_BYTES: u64 = 4 * 1024 * 1024;

struct Sink {
    file: Mutex<File>,
    path: PathBuf,
}

static SINK: OnceLock<Sink> = OnceLock::new();

/// 한 줄 형식: `시각 [레벨] 메시지`. 시각은 로컬 시간이다.
pub fn format_line(now: &str, level: &str, message: &str) -> String {
    format!("{now} [{level}] {message}\n")
}

fn now_text() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    chrono::DateTime::from_timestamp(secs + KST_OFFSET_SECONDS, 0).map_or_else(
        || "시각 없음".to_owned(),
        |t| t.format("%Y-%m-%d %H:%M:%S KST").to_string(),
    )
}

/// `<dir>/weblog.log`를 연다. 이미 초기화됐으면 기존 경로를 돌려준다.
pub fn init(dir: &Path) -> std::io::Result<PathBuf> {
    if let Some(sink) = SINK.get() {
        return Ok(sink.path.clone());
    }
    std::fs::create_dir_all(dir)?;
    let path = dir.join("weblog.log");
    if std::fs::metadata(&path).is_ok_and(|m| m.len() > ROTATE_BYTES) {
        let _ = std::fs::rename(&path, dir.join("weblog.prev.log"));
    }
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let _ = SINK.set(Sink {
        file: Mutex::new(file),
        path: path.clone(),
    });
    Ok(path)
}

/// 한 줄 기록. 초기화 전이거나 쓰기에 실패하면 조용히 버린다(로깅 때문에 앱이 죽지 않게).
pub fn write(level: &str, message: &str) {
    let Some(sink) = SINK.get() else { return };
    let line = format_line(&now_text(), level, message);
    if let Ok(mut f) = sink.file.lock() {
        let _ = f.write_all(line.as_bytes());
        let _ = f.flush();
    }
}

/// 정보 기록.
pub fn info(message: &str) {
    write("INFO", message);
}

/// 오류 기록.
pub fn error(message: &str) {
    write("ERROR", message);
}

/// 패닉을 파일에 남긴다. 릴리스 빌드는 `panic = "abort"`라 훅 실행 뒤 프로세스가 곧바로 끝난다.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map_or_else(|| "위치 없음".to_owned(), ToString::to_string);
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "메시지 없음".to_owned());
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("이름 없음").to_owned();
        write("PANIC", &format!("thread={name} at {location}: {payload}"));
        previous(info);
    }));
}

/// 진행 로그 최소 간격(초). 크래시 위치만 알면 되므로 촘촘히 남길 필요가 없다.
const PROGRESS_INTERVAL_SECS: u64 = 1;
static LAST_PROGRESS: AtomicU64 = AtomicU64::new(0);

/// 이벤트를 남길 (레벨, 메시지)로 만든다. 남길 것이 없으면 `None`.
pub fn event_message(event: &ServiceEvent) -> Option<(&'static str, String)> {
    match event {
        ServiceEvent::ImportProgress(p) => Some((
            "INFO",
            format!(
                "가져오기 진행 job={} source={} lines={} records={} batches={} elapsed={:.0}s",
                p.job_id,
                p.source_id,
                p.lines_read,
                p.committed_records,
                p.committed_batches,
                p.elapsed_secs
            ),
        )),
        ServiceEvent::ImportFinished(f) => {
            let counts = f.summary.as_ref().map_or_else(
                || "요약 없음".to_owned(),
                |s| {
                    format!(
                        "records={} errors={} skipped={} elapsed={:.1}s",
                        s.records, s.errors, s.skipped, s.elapsed_secs
                    )
                },
            );
            Some(match &f.error {
                Some(e) => (
                    "ERROR",
                    format!("가져오기 실패 job={} {counts} error={e}", f.job_id),
                ),
                None => (
                    "INFO",
                    format!(
                        "가져오기 종료 job={} status={} {counts}",
                        f.job_id, f.status
                    ),
                ),
            })
        }
        ServiceEvent::ExportProgress(_) => None,
        ServiceEvent::ExportFinished(e) => Some(match &e.error {
            Some(err) => (
                "ERROR",
                format!("내보내기 실패 out={} error={err}", e.out_path),
            ),
            None => ("INFO", format!("내보내기 종료 out={}", e.out_path)),
        }),
    }
}

/// 서비스 이벤트를 남긴다. 진행은 1초에 한 줄로 줄이고 종료·실패는 항상 남긴다.
pub fn log_event(event: &ServiceEvent) {
    if matches!(event, ServiceEvent::ImportProgress(_)) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        if now.saturating_sub(LAST_PROGRESS.load(Ordering::Relaxed)) < PROGRESS_INTERVAL_SECS {
            return;
        }
        LAST_PROGRESS.store(now, Ordering::Relaxed);
    }
    if let Some((level, message)) = event_message(event) {
        write(level, &message);
    }
}

/// 화면 단계 로그의 최대 글자 수. 로그 원문이 흘러들어도 파일을 채우지 않게 자른다.
const STEP_MAX_CHARS: usize = 200;

/// 화면이 보낸 단계를 한 줄로 만든다. `UI` 표시를 붙이고 길이를 제한한다.
pub fn step_message(step: &str) -> String {
    let trimmed = step.trim();
    if trimmed.chars().count() <= STEP_MAX_CHARS {
        return format!("UI {trimmed}");
    }
    let cut: String = trimmed.chars().take(STEP_MAX_CHARS).collect();
    format!("UI {cut}…")
}

/// 화면 단계 기록. 대화상자처럼 Rust 명령이 관여하지 않는 구간을 크래시 로그에 남긴다.
pub fn step(step: &str) {
    write("INFO", &step_message(step));
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn line_has_time_level_and_message() {
        assert_eq!(
            format_line("2026-09-10 12:00:00", "INFO", "가져오기 시작 files=1"),
            "2026-09-10 12:00:00 [INFO] 가져오기 시작 files=1\n"
        );
    }

    #[test]
    fn ui_steps_are_marked_and_capped() {
        assert_eq!(step_message("폴더 선택 열기"), "UI 폴더 선택 열기");
        let long = step_message(&"가".repeat(500));
        assert!(
            long.chars().count() <= STEP_MAX_CHARS + "UI …".chars().count(),
            "긴 단계는 잘라 낸다: {}",
            long.chars().count()
        );
        assert!(long.ends_with('…'), "잘렸음을 표시한다: {long}");
    }

    #[test]
    fn init_rotates_a_large_file_and_appends_after() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("weblog.log");
        std::fs::write(&path, vec![b'x'; (ROTATE_BYTES + 1) as usize]).unwrap();
        let opened = init(dir.path()).unwrap();
        assert_eq!(opened, path);
        assert!(dir.path().join("weblog.prev.log").exists(), "밀어낸 파일");
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 0, "새 파일로 시작");
        write("INFO", "기록 확인");
        let body = std::fs::read_to_string(&path).unwrap();
        assert!(
            body.contains("[INFO] 기록 확인"),
            "실제 파일에 남는다: {body}"
        );
    }

    #[test]
    fn failed_import_is_logged_as_an_error_with_counts() {
        let event = ServiceEvent::ImportFinished(weblog_service::ImportFinishedView {
            job_id: 7,
            status: "failed".to_owned(),
            summary: None,
            error: Some("디스크 부족".to_owned()),
        });
        let (level, message) = event_message(&event).expect("남길 내용");
        assert_eq!(level, "ERROR");
        assert!(message.contains("job=7"), "{message}");
        assert!(message.contains("디스크 부족"), "{message}");
    }

    #[test]
    fn progress_message_reports_how_far_the_import_got() {
        let event = ServiceEvent::ImportProgress(weblog_service::ImportProgressView {
            job_id: 3,
            source_id: 1,
            lines_read: 1_200_000,
            committed_records: 1_190_000,
            committed_batches: 12,
            elapsed_secs: 4.2,
            cancel_requested: false,
            resumed: false,
        });
        let (level, message) = event_message(&event).expect("남길 내용");
        assert_eq!(level, "INFO");
        assert!(message.contains("lines=1200000"), "{message}");
        assert!(message.contains("records=1190000"), "{message}");
    }
}
