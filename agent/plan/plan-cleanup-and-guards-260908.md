# PLAN · 남은 검토 항목 5건 (260908)

`docs/verification.md` 코드 품질 검토 표의 남은 항목을 모두 처리한다.

1. 원문 컬럼 금지 검사(`store/schema.rs:178` `assert_no_raw_columns`)가 테스트에서만 호출된다 → `Store::open`/`open_in_memory`의 런타임 가드로 승격.
2. 통계 화면이 오래된 응답을 폐기하지 않는다(`StatsPanel.tsx:98-130`) → 요청 순번으로 폐기(조회 화면과 같은 방식).
3. 미사용 공개 API 제거: `Store::path`, `BlocksParser::pattern`+`CompiledBlocks::pattern`, `Service::job`(테스트 전용), 프런트 `joinPath`·`templatesFor`·`GROUP_LABELS`·`PRESET_LABELS`·`DISPLAY_TZ_LABEL`.
4. 중복 로직 정리: ID 생성 `SELECT COALESCE(MAX())+1` 3곳 공유, 상태 집계 SQL 2곳, 바이트 단위 표기 KiB/KB 불일치.
   - 상태 집계 중복은 `LogQuery::status_histogram`이 프런트에서 호출되지 않는 죽은 경로이므로(엔진→서비스→명령까지 등록되어 있으나 UI 호출 0건, 배치 범위도 고정하지 않아 통계와 값이 다를 수 있음) 통합 대신 제거한다.
5. 조건 값 오류 메시지에 사용자 입력값이 들어간다(`store/query.rs:225`) → 값 제거.

- [x] 요청 동작을 검증하는 테스트를 먼저 작성한다.
  근거: `crates/weblog-engine/src/store/mod.rs:775-791` `opening_a_store_with_a_raw_column_fails`(raw 컬럼이 든 DB를 다시 열면 `EngineError::Format`), `crates/weblog-engine/src/store/query.rs:1159-1167`(숫자 필드에 문자열 → 메시지에 `status`는 있고 입력값 `secret-value`는 없음). 수정 전 실행 결과: 전자는 `unwrap_err`에서 패닉(열기가 성공), 후자는 `expr_combines_and_or_not_and_validates_types` 실패. 통계 stale 폐기는 화면 동작이라 mock IPC로 확인했고, 미사용 제거·중복 통합은 동작 변경이 아니라 기존 테스트·컴파일로 검증했다.
- [x] 1번: 마이그레이션 직후 `assert_no_raw_columns`를 호출한다.
  근거: `store/mod.rs:184-186`(`Store::open`), `:203-204`(`open_in_memory`). 원문 미저장 계약이 테스트가 아니라 실행 경로에서 지켜진다.
- [x] 5번: 오류 메시지에서 입력값을 뺀다.
  근거: `store/query.rs:225` — `"{col} 비교 값은 정수여야 함: {value}"` → `"{col} 비교 값은 정수여야 함"`.
- [x] 3번: 미사용 공개 API와 프런트 export를 제거한다.
  근거: 제거 — `Store::path`(`store/mod.rs`), `BlocksParser::pattern`(`parse/blocks.rs`), `CompiledBlocks::pattern`(`format/compile.rs`), `Service::job`(`weblog-service/src/service.rs`, 테스트는 `job_of` 헬퍼가 `list_jobs`로 조회), 프런트 `joinPath`(`lib/paths.ts`), `templatesFor`·`GROUP_LABELS`·`PRESET_LABELS`(`lib/puzzle.ts`), `DISPLAY_TZ_LABEL`(`lib/format.ts`). 삭제한 함수의 테스트 블록(`paths.test.ts`, `puzzle.test.ts`)도 함께 지웠다. `assert_no_raw_columns`는 1번에서 런타임 가드로 승격했으므로 유지.
- [x] 4번: 중복 로직을 정리한다.
  근거: `store/mod.rs:564-572`에 `pub(crate) fn next_id(conn, table, column)`를 두고 `Store::next_id`, `store/batch.rs:161`(batch_id), `store/views.rs:127`(view_id)이 공유한다. 상태 집계 중복은 죽은 경로 제거로 해소 — `LogQuery::status_histogram`, `Service::status_histogram`, `commands::status_histogram`, `lib.rs`의 핸들러 등록을 모두 삭제(프런트 호출 0건, 배치 범위를 고정하지 않아 통계와 값이 어긋날 수 있었다). 바이트 표기는 `panels/ByteValue.tsx:4`의 단위를 `B/KiB/MiB/GiB/TiB`로 바꿔 `formatBytes`와 통일했고(`lib/bytes.test.ts` 기대값 갱신), 중복이던 `format.ts`의 `jobStatusLabel`을 지우고 `lib/jobs.ts` 하나로 모아 `ImportingPanel`이 이를 쓰게 했다(라벨은 `진행 중`, `비정상 종료(복구 가능)`으로 통일).
- [x] 2번: `StatsPanel`에 요청 순번 폐기를 넣는다.
  근거: `panels/StatsPanel.tsx:97-128` — `requestRef`로 요청 번호를 매기고 응답·오류·로딩 해제를 모두 최신 요청에서만 반영한다.
- [x] 테스트·검사를 실행하고 통계 화면 stale 폐기를 실제 화면에서 확인한다.
  근거: `cargo test --workspace --locked` 156개 통과(engine 130, recovery 13, fixtures 4, service 9), `cargo clippy --workspace --all-targets --locked -- -D warnings` 경고 0, `cargo fmt --all -- --check` 통과, `pnpm check`(typecheck·lint·Vitest 50개·빌드) 통과. stale 확인은 임시 mock 페이지(`apps/desktop/mock-stats.html`, 첫 `compute_stats`만 1.2초 지연 후 `total: 111`, 이후 즉시 `total: 222`; 확인 후 삭제)로 수행 — 통계 탭에서 룰을 120ms 간격으로 연달아 바꾼 뒤 2.2초 대기했을 때 요약 줄이 `전체 222행`으로 남았다. 늦게 도착한 111 응답이 최신 화면을 덮어쓰지 않는다.
- [x] 위키를 갱신한다.
  근거: `docs/verification.md:55,57-60`을 해결로 갱신하고, 같은 표에서 `Service::job`이 작업 탭에서 쓰인다고 적었던 서술을 오류로 정정했다. `README.md:147`의 복구 안내 문구를 새 라벨(`비정상 종료(복구 가능)`)에 맞췄다.
