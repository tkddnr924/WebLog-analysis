# 릴리스 0.1.3 (2026-09-10)

에러 로그 화면·이식 가능한 저장 위치·크래시 로깅 변경을 커밋·푸시하고 태그 `v0.1.3`으로 Windows 실행 파일을 GitHub Release에 올린다.
기능 변경은 각 작업 PLAN(`plan-error-log-kind-level-260910.md`, `plan-portable-dirs-and-logging-260910.md`)에서 테스트 우선으로 검증했다. 이 릴리스 작업 자체는 버전 문자열 상향과 배포뿐이라 새 테스트를 추가하지 않고 기존 검사 실행으로 대체한다(AGENTS.md 예외 사유).

- [x] 버전 문자열을 0.1.3으로 올린다.
  근거: `apps/desktop/package.json:4`, `apps/desktop/src-tauri/tauri.conf.json:4`, `apps/desktop/src-tauri/Cargo.toml:3`, `Cargo.lock:4898`. 소스·문서에 남은 `0.1.2` 하드코딩 없음(grep 확인).
- [x] 릴리스 전 검사를 실행한다.
  근거: `cargo test --workspace` 170 passed / 0 failed, `cargo fmt --all -- --check` 통과, `cargo clippy --workspace --all-targets` 경고 0, `pnpm --dir apps/desktop typecheck` 오류 0, `pnpm --dir apps/desktop test` 12 files / 65 tests 통과.
- [x] 커밋·푸시하고 태그를 만든다.
  근거: 커밋 `9b6112f`, `git push origin main`(cadb65e..9b6112f), 태그 `v0.1.3` 푸시(new tag).
- [x] 릴리스 워크플로 성공과 `.exe` 자산을 확인한다.
  근거: 실행 https://github.com/tkddnr924/WebLog-analysis/actions/runs/34439493734 success(약 24분), 릴리스 `v0.1.3`(draft=false) 자산 `Weblog-analysis.exe` 35,883,008바이트 게시.
