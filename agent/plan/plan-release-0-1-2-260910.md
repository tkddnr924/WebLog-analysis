# 릴리스 0.1.2 (2026-09-10)

현재 작업 트리를 커밋·푸시하고 태그 `v0.1.2`로 Windows 실행 파일을 GitHub Release에 올린다.
기능 변경이 없는 릴리스 작업이므로 새 테스트를 추가하지 않고, 기존 프런트엔드·Rust 테스트 실행으로 대체한다(AGENTS.md 예외 사유 기록).

- [x] 저장소 상태·릴리스 워크플로·기존 태그 확인
  근거: `git status`(변경 44, 미추적 7), `.github/workflows/release.yml`(태그 `v*` 푸시 시 Windows x64 `--no-bundle` 빌드 → `out/Weblog-analysis.exe` 업로드), 기존 태그 `v0.1.0`, `v0.1.1`. GitHub API 확인 결과 v0.1.1 릴리스에 `Weblog-analysis.exe` 자산 존재.
- [x] 앱 버전을 0.1.2로 상향
  근거: `apps/desktop/package.json:4`, `apps/desktop/src-tauri/tauri.conf.json:4`, `apps/desktop/src-tauri/Cargo.toml:3`, `Cargo.lock`(weblog-desktop) 모두 `0.1.2`.
- [x] 도구별 로컬 스킬 링크(`.claude/`)를 무시 목록에 추가
  근거: `.gitignore:11-12`에 `/.claude` 추가. `.agents/skills/ponytail/SKILL.md`와 `agent/plan/*.md`는 추적 대상으로 커밋.
- [x] 기존 테스트 실행으로 릴리스 전 검증
  근거: `pnpm typecheck && pnpm test`(apps/desktop) → 10 files / 46 tests 통과. `cargo test --workspace` → 전 크레이트 통과(엔진·서비스·데스크톱, 실패 0).
- [x] 커밋·푸시 및 태그 `v0.1.2` 푸시
  근거: 커밋 `b26d65f`, `git push origin main`(683e95c..b26d65f), `git push origin v0.1.2`(new tag).
- [x] 릴리스 워크플로 성공과 `.exe` 자산 확인
  근거: 실행 https://github.com/tkddnr924/WebLog-analysis/actions/runs/34426396654 의 Release 단계까지 완료, GitHub API로 릴리스 `v0.1.2`(draft=false) 자산 `Weblog-analysis.exe` 35,846,144 바이트 업로드 확인.
- [ ] (후속 검토) 태그 릴리스 빌드 시간 단축
  미완료: 태그 ref로 만든 Actions 캐시는 다른 태그에서 복원되지 않아 매 릴리스가 DuckDB(bundled) 콜드 빌드다(v0.1.0 32분, v0.1.1 26분, v0.1.2 rust-cache 복원 3초=미스). 대안: main 푸시에도 워크플로를 돌려 기본 브랜치 캐시를 적재하거나 릴리스 프로필 `lto`를 thin으로 낮춘다.
