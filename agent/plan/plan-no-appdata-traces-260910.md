# AppData 흔적 제거와 로그 위치 정리 (2026-09-10)

## 배경
- Windows에서 `C:\Users\<사용자>\AppData\Local\com.weblog.analysis\`가 생긴다. Tauri가 WebView2 사용자 데이터 폴더를 지정하지 않으면 `LocalData/<identifier>`로 강제하기 때문이다(`tauri-2.11.5/src/manager/webview.rs:534-551`). 설정 파일의 `dataDirectory`는 상대 경로만 받고 결국 `local_data_dir` 아래로 풀리므로 설정으로는 못 옮긴다.
- 로그가 `cases` 밖(`<실행 파일 폴더>/logs/weblog.log`)에 있다.

## 목표
프로그램을 실행해도 실행 파일 폴더의 `cases` 말고는 어디에도 파일을 남기지 않는다.
- `cases/<이름>-<시각>.duckdb` — 파싱 데이터
- `cases/logs/weblog.log` — 실행·크래시 로그
- `cases/presets/*.yaml` — 사용자 프리셋(저장할 때만)
- WebView2 캐시는 임시 폴더(`%TEMP%\weblog-webview`)에 두고 종료 시 지운다. 앱 폴더도 AppData도 아니다.

## 작업

- [x] 동작을 검증하는 테스트를 먼저 작성한다(`cases` 아래로 모이는 경로 배치, WebView2 캐시 위치가 앱 폴더 밖이며 정리 함수가 실제로 지우는지).
  근거: `apps/desktop/src-tauri/src/paths.rs`의 `everything_lives_under_the_cases_folder`, `webview_cache_stays_out_of_the_app_folder_and_is_removable`. 회귀 확인: `layout`의 로그 경로를 예전처럼 `root/logs`로 되돌리자 `everything_lives_under_the_cases_folder` FAILED, 되돌리니 통과.
- [x] `paths.rs`에 경로 배치와 WebView2 캐시 경로·정리를 구현한다.
  근거: `paths.rs`의 `Layout`/`layout()`(cases, cases/logs, cases/presets), `webview_cache_dir()`(임시 폴더 `weblog-webview`), `remove_webview_cache()`.
- [x] `lib.rs`에서 창을 설정 대신 코드로 만들며 `data_directory`를 지정하고, 종료 시 캐시를 지운다. 로그·프리셋·케이스 경로를 새 배치로 바꾼다.
  근거: `apps/desktop/src-tauri/tauri.conf.json` 창에 `"create": false`, `lib.rs`에서 `WebviewWindowBuilder::from_config(..).data_directory(webview_cache)`로 창 생성, `Builder::build(..)` + `RunEvent::Exit`에서 `remove_webview_cache`, `ServiceConfig.cases_dir`/`profiles_dir`와 `applog::init`을 `layout()` 값으로 교체.
- [x] 작성한 테스트를 실행하고 통과를 확인한다.
  근거: `cargo test -p weblog-desktop` 9 passed / 0 failed, `cargo clippy --workspace --all-targets` 경고 0, `cargo fmt --all` 적용.
- [x] 실제 실행으로 파일 배치를 확인한다.
  근거: `cargo run -p weblog-desktop` 실행 후 실행 파일 폴더(`target/debug`)에 생긴 것은 `cases`뿐이고 내부는 `cases/logs/weblog.log`. 로그 첫 줄 `앱 시작 version=0.1.3 cases=.../target/debug/cases log=.../cases/logs/weblog.log webview=/var/folders/.../T/weblog-webview`, 다음 줄 `창 생성 label=main 표시=true`(설정 자동 생성을 끈 뒤에도 창이 실제로 만들어져 보인다). 종료 경로는 임시 스캐폴드(`WEBLOG_SMOKE_EXIT_MS` → `handle.exit(0)`)로 확인: 로그에 `앱 종료`가 남고 `/var/folders/.../T/weblog-webview`가 사라졌다. 스캐폴드는 제거했다. Windows에서 `%LOCALAPPDATA%\com.weblog.analysis`가 더는 생기지 않는지는 다음 릴리스 빌드로 확인해야 한다(미검증).
- [x] 위키(README 저장 위치, `docs/architecture.md`)를 갱신한다.
  근거: `README.md` 설치·실행 요약 표(케이스·로그·프리셋 경로)와 그 아래 단락(폴더 하나·WebView2 캐시·레지스트리 미사용), `docs/architecture.md` 저장 위치와 실행 로그 절.
