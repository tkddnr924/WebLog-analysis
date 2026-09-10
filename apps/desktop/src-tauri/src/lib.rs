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
    // WebView2 캐시는 임시 폴더에 두고 종료할 때 지운다. 지정하지 않으면 Tauri가
    // `%LOCALAPPDATA%\<identifier>`를 강제해 AppData에 흔적이 남는다.
    let webview_cache = paths::webview_cache_dir();
    let cache_for_setup = webview_cache.clone();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(move |app| {
            // 파일은 실행 파일 옆 `cases` 하나에만 남긴다. 폴더가 읽기 전용이면 임시 폴더로 물러난다.
            let (root, fallback_note) = paths::data_root(paths::exe_dir());
            let dirs = paths::layout(&root);
            let log_path = applog::init(&dirs.logs).ok();
            applog::install_panic_hook();
            applog::info(&format!(
                "앱 시작 version={} cases={} log={} webview={}",
                env!("CARGO_PKG_VERSION"),
                dirs.cases.display(),
                log_path
                    .as_ref()
                    .map_or_else(|| "없음".to_owned(), |p| p.display().to_string()),
                cache_for_setup.display()
            ));
            if let Some(note) = &fallback_note {
                applog::error(note);
            }
            // 창은 설정(create=false) 대신 여기서 만든다. WebView2 데이터 폴더를 지정하려면
            // 빌더가 필요하다.
            for window in &app.config().app.windows.clone() {
                let built = tauri::WebviewWindowBuilder::from_config(app.handle(), window)?
                    .data_directory(cache_for_setup.clone())
                    .build()?;
                applog::info(&format!(
                    "창 생성 label={} 표시={}",
                    window.label,
                    built.is_visible().unwrap_or(false)
                ));
            }
            let service = Arc::new(Service::new(ServiceConfig {
                profiles_dir: Some(dirs.presets),
                cases_dir: Some(dirs.cases),
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
            commands::log_step,
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
        .build(tauri::generate_context!());
    match app {
        Ok(app) => app.run(move |_handle, event| {
            if matches!(event, tauri::RunEvent::Exit) {
                applog::info("앱 종료");
                paths::remove_webview_cache(&webview_cache);
            }
        }),
        Err(e) => {
            applog::error(&format!("앱 실행 실패: {e}"));
            eprintln!("앱 실행 실패: {e}");
            std::process::exit(1);
        }
    }
}

/// 명령에서 서비스를 꺼낼 때 쓰는 별칭.
pub(crate) type ServiceState<'a> = tauri::State<'a, Arc<Service>>;
