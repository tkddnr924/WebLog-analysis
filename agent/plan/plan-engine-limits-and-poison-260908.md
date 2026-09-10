# PLAN · 탐색 오류 상한 · YAML 상한 · 락 포이즈닝 (260908)

검토 결과(`agent/plan/plan-code-quality-review-260908.md`)의 중간 항목 3건을 함께 수정한다. 서로 겹치지 않는 Rust 계층 결함이다.

1. `crates/weblog-engine/src/source/scan.rs` — 항목(`entries`)에는 `max_entries` 상한이 있지만 `errors`에는 상한이 없다.
   읽을 수 없는 디렉터리가 많은 트리(권한 없는 하위 디렉터리, 네트워크 공유)에서 오류 목록이 계속 자란다.
2. `crates/weblog-engine/src/format/library.rs` — 프리셋 YAML을 `read_to_string`으로 전부 읽은 뒤에야 `yaml.rs`의 상한을 검사한다. 상한이 사후 검사여서 의미가 없다.
3. `crates/weblog-service/src/service.rs` — `with_store`가 `try_lock` 실패를 원인 구분 없이 `ImportRunning`으로 바꾼다.
   가져오기 스레드가 락을 쥔 채 패닉하면 이후 모든 쓰기 명령이 "가져오기 진행 중"이라고 잘못 보고한다.

수정 방향:
- 오류 목록도 `max_entries`를 상한으로 쓰고, 상한에 걸리면 `errors_truncated`로 알린다(탐색 자체는 계속한다. `truncated`는 항목 상한 의미를 유지).
- YAML은 읽기 전에 파일 크기를 상한과 비교한다. 상한 상수를 `format::yaml`에서 공개한다.
- `with_store`는 `TryLockError::Poisoned`를 복구해 진행하고(다른 잠금 헬퍼와 같은 방식), `WouldBlock`만 `ImportRunning`으로 보고한다.

- [x] 요청 동작을 검증하는 테스트를 먼저 작성한다.
  근거: `crates/weblog-engine/src/source/scan.rs:372-399` `scan_errors_stop_at_the_entry_limit`(unix 전용, 권한 없는 디렉터리 6개 + `max_entries: 2` → 오류 ≤ 2, `errors_truncated` 참, `truncated` 거짓, `directories_visited == 7`), `crates/weblog-engine/src/format/library.rs:186-201` `oversized_yaml_is_rejected_without_reading_the_file`, `crates/weblog-service/src/service.rs:1293-1323` `poisoned_store_lock_is_recovered_not_reported_as_import_running`. 수정 전 실행 결과: 엔진 두 테스트는 `errors_truncated` 필드 없음·`MAX_YAML_BYTES` 비공개로 컴파일 실패(E0609/E0603), 서비스 테스트는 `save_view`가 `ImportRunning`을 돌려주어 `expect`에서 패닉.
- [x] 세 지점을 구현한다(스캔 오류 상한과 DTO·CLI·프런트 타입 반영 포함).
  근거: `source/scan.rs:105-112`에 `push_error` 헬퍼를 두고 오류 지점 7곳을 모두 통과시켜 `max_entries` 상한을 적용했고, `ScanResult.errors_truncated`(73-75행)와 `ScanOptions.max_entries` 문서를 갱신했다. 상한에 걸려도 탐색은 계속한다(`truncated`와 의미 분리). `format/yaml.rs:6-7` `MAX_YAML_BYTES`를 `pub`으로 바꾸고 `format/library.rs:60-70` `read_profile`이 `fs::metadata` 크기를 먼저 비교하며 `list`/`load`가 이를 쓴다. `weblog-service/src/service.rs:803-817` `with_store`가 `TryLockError::Poisoned`를 `into_inner()`로 복구하고 `WouldBlock`만 `ImportRunning`으로 보고한다. 전달 경로: `dto.rs:113-114`, `service.rs:420`, `weblog-cli/src/main.rs:626`, `apps/desktop/src/types.ts:73`, `panels/StartPanel.tsx:479-481`(오류 목록 요약에 "상한에 걸려 일부만 표시").
- [x] 작성한 테스트를 실행하고 회귀를 확인한다.
  근거: 세 테스트 모두 통과. 회귀 — `cargo test --workspace --locked` 155개 통과(engine 129, recovery 13, fixtures 4, service 9), `cargo clippy --workspace --all-targets --locked -- -D warnings` 경고 0, `cargo fmt --all -- --check` 통과, `pnpm check`(typecheck·lint·Vitest 52개·프로덕션 빌드) 통과.
- [x] 위키(`docs/verification.md` 코드 품질 검토 표)에서 해당 3건을 해결로 갱신한다.
  근거: `docs/verification.md:52-54`를 해결로 갱신하고 `README.md:74`에 탐색 항목·오류 상한과 프리셋 YAML 상한 동작을 기록했다.

## 추가 작업

- [x] AGENTS.md에 추가된 "코드 주석은 영어로 짧게" 규칙을 이번 세션에서 작성한 주석에 적용한다.
  근거: 세션 중 추가·수정한 주석만 영어로 바꿨다 — `lib/jobs.ts`, `panels/JobsPanel.tsx:1,25`, `panels/ErrorBoundary.tsx:1,5,7,37`, `lib/escapes.ts:13,29`, `lib/escapes.test.ts:35`, `importer.rs:110-111,162`, `service.rs:1242,1305`, `commands.rs:157`, `source/scan.rs:105`, `format/library.rs:60`, `format/yaml.rs:6`, `styles.css:416,438-439`. 기존 파일의 한국어 주석은 이번 변경과 무관해 손대지 않았다(일괄 전환은 별도 작업 제안).
