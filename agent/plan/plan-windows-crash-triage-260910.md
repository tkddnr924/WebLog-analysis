# Windows 크래시(0.1.3) 원인 좁히기 (2026-09-10)

## 증거 (`agent/ref/Report.wer`, 커밋 금지)
- `EventType=APPCRASH`, 앱 `Weblog-analysis.exe 0.1.3.0`, 예외 코드 `c0000005`(접근 위반).
- 오류 모듈 `MSVCP140.dll 14.34.31931.0`(+0x13008). 이 DLL은 시스템(`C:\Windows\SYSTEM32`)에 설치된 VC++ 재배포 패키지이며 2022년 11월(VS 17.4) 판이다. GitHub Actions 러너는 그보다 새 툴셋으로 빌드하므로 DuckDB(C++)가 요구하는 런타임보다 낮다.
- 크래시 시각(FILETIME 변환) 2026-09-10 14:42:39.5 KST. 로그 마지막 줄 `앱 시작 ... 14:42:32` → **시작 7초 뒤**다. 가져오기는 시작조차 하지 않았다.
- 로드된 모듈에 `explorerframe.dll`, `StructuredQuery.dll`, `Windows.FileExplorer.Common.dll`, `thumbcache.dll`, `NetworkExplorer.dll`, OneDrive `FileSyncShell64.dll`이 있다. 폴더 선택 대화상자가 열린 상태에서 죽었다는 뜻이다.
- `EmbeddedBrowserWebView.dll`(WebView2 152)만 있고 DuckDB는 실행 파일에 정적으로 들어가므로 별도 모듈이 없다.

## 판단
2.1GB 파싱 중 크래시가 아니라 **폴더 선택 대화상자 단계**에서 죽었다. 후보는 두 가지이고 지금 증거로는 못 가른다.
1. 우리 실행 파일이 동적으로 쓰는 `MSVCP140.dll`이 빌드 툴셋보다 낮다(사용자 PC 재배포 패키지가 오래됨).
2. 대화상자에 끼어드는 셸 확장(OneDrive 등)이 같은 DLL 안에서 죽었다.

대응: (1)은 CRT 정적 링크로 원천 제거하고(재배포 패키지 자체가 필요 없어진다), 가르지 못하는 부분은 **단계 로그**를 넣어 다음 크래시 리포트에서 바로 구분한다.

## 작업

- [x] 동작을 검증하는 테스트를 먼저 작성한다(단계 로그 한 줄 형식·길이 상한, 폴더 선택 전후로 단계 로그가 순서대로 나가는지).
  근거: `apps/desktop/src-tauri/src/applog.rs`의 `ui_steps_are_marked_and_capped`(구현 전 FAILED 확인). 순서 검증은 임시 브라우저 하네스로 실제 UI의 IPC 호출을 기록해 확인했다.
- [x] `applog`에 단계 로그를 만들고 `log_step` 명령을 추가해 등록한다.
  근거: `applog::step_message`/`step`(`UI ` 표시, 200자 상한), `commands::log_step`, `lib.rs` 명령 등록.
- [x] 프런트엔드에서 폴더 선택 전후·파싱 시작에 단계 로그를 보낸다.
  근거: `api.ts`의 `logStep`(실패 무시), `StartPanel.tsx`의 `pickFolder`(열기/완료/취소/실패)와 `start`(파싱 시작 버튼).
- [x] 오래 걸리는 명령에 시작 로그를 넣는다.
  근거: `commands.rs`의 `scan_files`(탐색 시작/완료·실패), `preview_format`(미리보기 시작), `sample_lines`(샘플 읽기 시작), `create_case`(케이스 생성 시작 → 기존 완료 로그 앞).
- [x] Windows 빌드에서 CRT를 정적 링크하고 릴리스 워크플로에 확인 단계를 넣는다.
  근거: `.cargo/config.toml`(`x86_64-pc-windows-msvc`에 `-C target-feature=+crt-static`; cc 크레이트가 C++ 쪽도 `/MT`로 컴파일), `.github/workflows/release.yml`의 "Check no MSVC runtime dependency"(실행 파일에 `MSVCP140`/`VCRUNTIME140` 문자열이 있으면 실패). **Windows 빌드·링크 성공은 미검증**(macOS에서는 이 설정이 적용되지 않는다).
- [x] 작성한 테스트를 실행하고 통과를 확인한다.
  근거: `cargo test -p weblog-desktop` 10 passed / 0 failed, `cargo clippy --workspace --all-targets` 경고 0, `cargo fmt --all`, `pnpm --dir apps/desktop typecheck` 0, `pnpm --dir apps/desktop test` 65 passed. 임시 하네스에서 "폴더 선택" 클릭 시 호출 순서 `log_step :: 폴더 선택 대화상자 열기` → `plugin:dialog|open` → `log_step :: 폴더 선택 완료` → `scan_files` 확인(하네스는 삭제).
- [x] 위키를 갱신한다.
  근거: `docs/architecture.md` 저장 위치·실행 로그 절에 화면 단계 로그 항목과 정적 CRT 결정 추가.
- [x] `agent/ref/`를 `.gitignore`에 넣는다.
  근거: `.gitignore`에 `/agent/ref` 추가, `git check-ignore -v agent/ref/Report.wer` → `.gitignore:14:/agent/ref`.
