//! 앱 조립과 명령 등록. 핵심 로직은 `weblog-service`와 `weblog-engine`에 있다.

mod applog;
mod commands;
mod paths;

use std::sync::Arc;

use tauri::{Emitter, Manager};
use weblog_service::{Service, ServiceConfig, ServiceEvent};

/// 서비스 이벤트를 프런트엔드로 전달하는 이벤트 이름.
pub const IMPORT_EVENT: &str = "weblog://import";

/// 앱을 실행한다.
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            // 케이스·프리셋·로그는 실행 파일 옆에 둔다. 설치 폴더가 읽기 전용이면 임시 폴더로 물러난다.
            let (root, fallback_note) = paths::data_root(paths::exe_dir());
            let log_path = applog::init(&root.join("logs")).ok();
            applog::install_panic_hook();
            applog::info(&format!(
                "앱 시작 version={} root={} log={}",
                env!("CARGO_PKG_VERSION"),
                root.display(),
                log_path
                    .as_ref()
                    .map_or_else(|| "없음".to_owned(), |p| p.display().to_string())
            ));
            if let Some(note) = &fallback_note {
                applog::error(note);
            }
            let service = Arc::new(Service::new(ServiceConfig {
                profiles_dir: Some(root.join("presets")),
                cases_dir: Some(root.join("cases")),
                ..ServiceConfig::default()
            }));
            let handle = app.handle().clone();
            service.set_event_sink(move |event: ServiceEvent| {
                applog::log_event(&event);
                // 이벤트 전송 실패(창 닫힘 등)는 무시한다. 상태는 import_status로 다시 조회할 수 있다.
                let _ = handle.emit(IMPORT_EVENT, &event);
            });
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::create_case,
            commands::list_cases,
            commands::delete_case,
            commands::close_project,
            commands::current_project,
            commands::list_presets,
            commands::list_profiles,
            commands::validate_profile,
            commands::profile_from_yaml,
            commands::profile_to_yaml,
            commands::save_profile,
            commands::delete_profile,
            commands::scan_files,
            commands::preview_format,
            commands::sample_lines,
            commands::start_import,
            commands::cancel_import,
            commands::import_status,
            commands::query_page,
            commands::count_logs,
            commands::log_detail,
            commands::compute_stats,
            commands::cancel_heavy,
            commands::list_views,
            commands::save_view,
            commands::delete_view,
            commands::toggle_bookmark,
            commands::start_export,
            commands::cancel_export,
            commands::export_status,
        ])
        .run(tauri::generate_context!());
    if let Err(e) = result {
        eprintln!("앱 실행 실패: {e}");
        std::process::exit(1);
    }
}

/// 명령에서 서비스를 꺼낼 때 쓰는 별칭.
pub(crate) type ServiceState<'a> = tauri::State<'a, Arc<Service>>;
