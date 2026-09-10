# PLAN · 개발 코드 품질 검토 (260908)

사용자 요청: 현재 개발된 코드에서 미사용 코드 / 중복 코드 / 예외처리 미흡 / 에러 발생 위험 / 메모리 관리 미흡 코드를 검토한다.
검토(진단) 작업이며 기능 변경이 없다. 따라서 테스트 코드를 먼저 작성하지 않고, 기존 검사 명령 실행과 정적 근거(파일·라인) 수집으로 검증한다.
수정이 필요한 항목은 이 PLAN의 결과를 근거로 별도 PLAN에서 처리한다.

- [x] 검토 대상 코드 범위와 규모를 확인한다.
  근거: Rust 제품·테스트 코드 12,604줄(`crates/weblog-{engine,service,cli}`, `apps/desktop/src-tauri`), 프런트엔드 5,864줄(`apps/desktop/src`). 최대 파일은 `crates/weblog-service/src/service.rs` 1,521줄, `crates/weblog-engine/src/store/query.rs` 1,340줄.
- [x] 기존 검사(cargo fmt/clippy/test, 프런트엔드 typecheck/lint/test)를 실행해 현재 경고 상태를 기준선으로 남긴다.
  근거: `cargo clippy --workspace --all-targets --locked -- -D warnings` 통과(경고 0). `cargo test --workspace --locked` 통과(150 tests: engine 126, recovery 11, fixtures 4, service 9). `pnpm typecheck` 통과, `pnpm test` 통과(10 files / 47 tests), `pnpm lint` 경고 1건 — `apps/desktop/src/panels/QueryPanel.tsx:136` `react-hooks/incompatible-library`.
- [x] 엔진 파서·포맷(`crates/weblog-engine/src/{parse,format,detect,preview,reconstruct}`)을 5개 항목 기준으로 검토한다.
  근거: 하위 에이전트 검토 후 직접 재확인. 확인된 결함 — `format/library.rs:79-81,101-102`(전체 파일 read_to_string 뒤에야 `yaml.rs:17`의 `MAX_YAML_BYTES` 검사), `reconstruct.rs:147`(호출마다 `Regex::new` 재컴파일, 실패는 `unwrap_or(false)`로 무시), `parse/w3c.rs:47-49`(체크포인트 복원 시 `MAX_FIELDS` 재검증 없음), 미사용 pub — `parse/blocks.rs:25` `pattern()` 및 그가 감싸는 `format/compile.rs:63`(호출부 0). 제품 코드에 unwrap/expect/panic 없음(테스트 모듈에만 존재, clippy `unwrap_used` 유지).
- [x] 엔진 저장·조회·가져오기(`crates/weblog-engine/src/{store,source,importer,export}`)를 검토한다.
  근거: 확인된 결함 — `source/scan.rs`의 `errors` Vec 상한 없음(push 지점 108,128,139,151,166,206,216; `entries`만 199에서 `max_entries` 검사), `store/schema.rs:178` `assert_no_raw_columns`가 테스트(230)에서만 호출되고 런타임 경로에 미연결, `SELECT COALESCE(MAX(...))+1` 3중 중복(`store/mod.rs:556`, `store/batch.rs:161`, `store/views.rs:127`), 상태 집계 SQL 중복(`store/query.rs` vs `store/stats.rs`), `store/query.rs:225` 오류 메시지에 사용자 조건값 노출, `store/mod.rs:246` `Store::path()` 호출부 0, `source/scan.rs:194` 비UTF-8 파일명이 오류 없이 `filtered_out` 처리. 트랜잭션 경계·멱등성·커서 페이지네이션·스트리밍 내보내기는 문제 없음.
- [x] 서비스·Tauri·CLI(`crates/weblog-service`, `apps/desktop/src-tauri`, `crates/weblog-cli`)를 검토한다.
  근거: 확인된 결함 — `apps/desktop/src-tauri/src/commands.rs:149-157`의 `start_import`/`resume_job`이 동기 명령(메인 스레드 실행)인데 `crates/weblog-service/src/service.rs:745`가 `job_id_rx.recv()`로 첫 진행 통지까지 블로킹하고, 그 통지는 `crates/weblog-engine/src/importer.rs:507`의 `commit()` 이후에만 발생한다(기본 배치 50,000행/32MB — `importer.rs:36-37`) → 대용량 첫 배치 동안 UI 정지. `service.rs:806-811` `with_store`가 `try_lock` 실패를 원인 구분 없이 `ImportRunning`으로 변환(포이즈닝을 "가져오기 중"으로 오표시). `service.rs:797-801` `Service::job`은 명령 등록·CLI 사용 없이 테스트(1224,1306)에서만 호출. `commands.rs:71-112`의 `current_project`/`list_profiles`/`save_profile`/`delete_profile`도 동기 명령이며 파일 I/O·리더 획득을 메인 스레드에서 수행.
- [x] 프런트엔드(`apps/desktop/src`)를 검토한다.
  근거: 확인된 결함 — `api.ts:56,59-63`의 `resumeJob`/`listJobs`/`activateJob`/`deleteJobResults`/`verifySource` 호출부 0이고 `state.tsx:115`는 존재하지 않는 "결과 화면의 작업 탭"을 안내(`panels/ResultsPanel.tsx:7-12` 탭은 query/stats뿐) → 중단 작업 재개·소스 검증 UI 부재. `lib/escapes.ts:15,19`의 `bytes.push(...enc.encode(...))`는 최대 64KiB(`importer.rs:38`) 요청 대상에서 인자 수 상한을 넘길 수 있고 `App.tsx`에 ErrorBoundary가 없어 트리 전체 크래시로 이어짐. `panels/StatsPanel.tsx:98-130`은 요청 순번 폐기가 없어 룰 연속 변경 시 오래된 통계로 덮어씀(QueryPanel은 요청 ID로 폐기). 미사용 export — `lib/puzzle.ts:19,324,676`, `lib/paths.ts:8`, `lib/format.ts:23`. 표기 불일치 — `lib/format.ts:6`(KiB) vs `panels/ByteValue.tsx:4`(KB).
- [x] 크레이트 경계의 미사용 공개 API와 중복 로직을 기계적으로 확인한다.
  근거: `grep`으로 제품 코드 전역에 bare `unwrap()`/`expect(`/`panic!`/`todo!` 없음 확인(모두 `#[cfg(test)]` 또는 `unwrap_or*`/포이즈닝 복구 `unwrap_or_else(|e| e.into_inner())`). 미사용 pub 확정: `Store::path`, `Service::job`(테스트 전용), `BlocksParser::pattern`+`CompiledBlocks::pattern`, `assert_no_raw_columns`(테스트 전용). 프런트 미사용 export 확정: `joinPath`·`templatesFor`(테스트 전용), `GROUP_LABELS`·`PRESET_LABELS`·`DISPLAY_TZ_LABEL`(참조 0).
- [x] 검토 결과를 심각도·근거(파일:라인)와 함께 보고한다.
  근거: 높음 2건(가져오기 시작 시 UI 정지, 중단 작업 복구 UI 미연결), 중간 6건, 낮음 10건으로 정리해 사용자에게 보고. 수정은 별도 PLAN에서 진행.
- [x] 위키 등재 대상(반복 참고할 결론)이 있으면 관련 문서에 반영한다.
  근거: `docs/verification.md`의 「개발 검사」 아래에 「코드 품질 검토(2026-09-08)」 섹션을 추가해 실행한 검사 결과와 미해결 결함 요약·근거 위치를 기록했다.

미검증: Windows x64 실행 환경에서의 UI 정지 재현, 100GB급 입력에서의 메모리 상한 실측은 이번 검토 범위 밖(정적 근거와 기존 벤치마크만 사용).
