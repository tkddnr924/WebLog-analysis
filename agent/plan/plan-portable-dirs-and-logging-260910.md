# 식별자 정리 · 포터블 저장 위치 · 크래시 로깅 (2026-09-10)

## 배경

- 0.1.2 Windows 실행 파일에서 2.1GB Apache 로그로 `파싱 시작`을 누르면 **프로세스가 즉시 종료**된다(멈춤이 아니라 터짐). 화면·메시지가 남지 않고 앱에 로그 기능이 없어 원인을 특정할 수 없다.
- 같은 크기(12,000,000줄 / 2,132,919,402바이트) 합성 로그로 mac에서 서비스 계층 전체(create_case → start_import → 폴링)를 돌린 결과는 정상이다: 프리셋 24.4초, 화면이 만드는 정의 22.8초, 피크 RSS 473MB, 폴링 지연 10ms 미만. 즉 파일 크기 자체나 백엔드 잠금 문제는 아니다.
- 번들 식별자에 회사 이름이 들어가 있어 저장소에서 지워야 한다. 새 식별자는 회사와 무관한 값으로 둔다.
- 케이스 DB는 앱 데이터 폴더 대신 실행 파일 옆에 만들어야 한다.

## 작업

- [x] 실행 파일 기준 저장 위치와 로그 파일 동작을 검증하는 테스트를 먼저 작성한다(실행 파일 폴더 계산, 쓰기 불가 시 대체 위치, 로그 줄 형식, 로그 파일 회전, 이벤트 로그 내용).
  근거: `apps/desktop/src-tauri/src/paths.rs`(`data_root_uses_the_executable_folder_when_writable`, `data_root_falls_back_when_the_folder_is_read_only`, `data_root_falls_back_without_an_executable_path`), `applog.rs`(`line_has_time_level_and_message`, `init_rotates_a_large_file_and_appends_after`, `failed_import_is_logged_as_an_error_with_counts`, `progress_message_reports_how_far_the_import_got`).
- [x] 번들 식별자를 회사와 무관한 값으로 바꾸고 저장소 문서에서 해당 문자열을 제거한다.
  근거: `apps/desktop/src-tauri/tauri.conf.json:5` → `com.weblog.analysis`, `README.md` 저장 위치 표에서 이전 식별자 경로 삭제. 저장소 전체 문자열 검색 결과 남은 곳 없음(git 이력·기존 릴리스 자산은 별도 조치 필요).
- [x] 케이스·프리셋·로그 디렉터리를 실행 파일 옆(`cases/`, `presets/`, `logs/`)으로 옮긴다. 쓰기가 불가능하면 임시 폴더로 물러나고 로그에 남긴다.
  근거: `apps/desktop/src-tauri/src/paths.rs`(`exe_dir`, `is_writable`, `data_root`), `lib.rs:20-41`에서 `profiles_dir`/`cases_dir`를 `<실행 파일 폴더>/presets`·`/cases`로 설정. 화면 알림 대신 로그 기록으로 처리(대체 위치 사용 시 ERROR 한 줄).
- [x] 파일 로깅을 추가한다: 앱 시작, 케이스 생성, 가져오기 요청·시작·진행·종료, 패닉 훅. 로그 원문·필드 값은 남기지 않는다.
  근거: `applog.rs`(파일 싱크·회전·패닉 훅·이벤트 포맷), `lib.rs`(시작 로그·패닉 훅 설치·이벤트 싱크), `commands.rs`(케이스 생성, 가져오기 요청 시 파일 수·총 바이트·첫 경로, 시작/실패 결과).
- [x] 작성한 테스트를 실행하고 통과를 확인한다.
  근거: `cargo test -p weblog-desktop` 7 passed / 0 failed.
- [x] 실제 앱을 실행해 로그 파일이 실행 파일 옆에 생기는지 확인한다.
  근거: `cargo run -p weblog-desktop` 실행 후 `target/debug/logs/weblog.log`에 `2026-09-10 12:32:55 KST [INFO] 앱 시작 version=0.1.2 root=.../target/debug log=.../target/debug/logs/weblog.log` 기록 확인.
- [x] 위키에 저장 위치와 로그 파일 사용법(문제 보고 시 첨부할 파일)을 반영한다.
  근거: `docs/architecture.md`에 "저장 위치와 실행 로그" 절 추가(포터블 경로, 대체 위치, 로그 항목·회전·패닉 기록), `README.md` 설치·실행 요약 표에 프리셋·케이스·실행 로그 위치 갱신.

## 남은 확인(사용자 환경)

- Windows 이벤트 뷰어 > Windows 로그 > 응용 프로그램에서 종료 시각의 "응용 프로그램 오류" 항목: 예외 코드와 오류 모듈 이름.
- 위 로깅을 넣은 빌드로 재현했을 때 마지막 로그 줄(어느 단계에서 끊겼는지).
