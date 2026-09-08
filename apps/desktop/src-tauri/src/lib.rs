//! 앱 조립과 명령 등록. 핵심 로직은 `weblog-service`와 `weblog-engine`에 있다.

mod commands;

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
            // 사용자 프리셋은 앱 설정 디렉터리 아래 presets/ 에 YAML로 둔다.
            let profiles_dir = app.path().app_config_dir().ok().map(|d| d.join("presets"));
            // 케이스 DB는 앱 데이터 디렉터리 아래 cases/ 에 자동 생성한다. 사용자에게 위치를 묻지 않는다.
            let cases_dir = app.path().app_data_dir().ok().map(|d| d.join("cases"));
            let service = Arc::new(Service::new(ServiceConfig {
                profiles_dir,
                cases_dir,
                ..ServiceConfig::default()
            }));
            let handle = app.handle().clone();
            service.set_event_sink(move |event: ServiceEvent| {
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
            commands::resume_job,
            commands::cancel_import,
            commands::import_status,
            commands::list_jobs,
            commands::activate_job,
            commands::delete_job_results,
            commands::verify_source,
            commands::query_page,
            commands::count_logs,
            commands::status_histogram,
            commands::log_detail,
            commands::compute_stats,
            commands::cancel_heavy,
            commands::list_views,
            commands::save_view,
            commands::delete_view,
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
