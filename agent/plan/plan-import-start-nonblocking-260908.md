# PLAN · 가져오기 시작 시 UI 정지 제거 (260908)

검토 결과(`agent/plan/plan-code-quality-review-260908.md`) 높음 1번 수정.

문제: `apps/desktop/src-tauri/src/commands.rs:149-157`의 `start_import`/`resume_job`이 동기 Tauri 명령이라 메인 스레드에서 실행되고,
`crates/weblog-service/src/service.rs:745`가 `job_id_rx.recv()`로 첫 진행 통지까지 기다리며, 그 통지는 `crates/weblog-engine/src/importer.rs:507`의
`commit()` 이후에만 발생한다(기본 배치 50,000행/32MiB). 결과적으로 첫 배치 파싱·커밋이 끝날 때까지 앱 전체가 응답하지 않는다.

수정 방향:
1. 엔진: `run_import`이 작업 생성 직후 진행 통지를 한 번 보내 `job_id`를 커밋 전에 알린다(재개 경로는 서비스가 이미 즉시 통지).
2. Tauri: 리더 획득·파일 I/O·스레드 대기가 있는 명령을 `async fn` + `blocking()`(spawn_blocking)으로 바꿔 메인 스레드에서 빼낸다.
   대상: `start_import`, `resume_job`, `current_project`, `close_project`, `list_profiles`, `save_profile`, `delete_profile`.
   유지(메모리 읽기·짧은 잠금만): `list_presets`, `validate_profile`, `profile_from_yaml`, `profile_to_yaml`, `import_status`, `export_status`, `cancel_*`.

- [x] 요청 동작을 검증하는 테스트를 먼저 작성한다.
  근거: `crates/weblog-engine/src/importer.rs:601-622` `first_progress_reports_job_id_before_any_batch_commit`(첫 통지의 `job_id`가 확정되고 건수 0, 통지 2회), `crates/weblog-service/src/service.rs:1231-1259` `start_import_returns_job_id_before_first_batch_commit`(배치 경계를 파일 끝으로 두고 반환 직후 `committed_batches == 0`, `finished.is_none()`). 수정 전 실행 결과 두 테스트 모두 실패 — 엔진 `left: (3, 1) right: (0, 0)`, 서비스 `left: 1 right: 0`.
- [x] 엔진에서 작업 생성 직후 진행 통지를 보내고 `Progress` 문서를 갱신한다.
  근거: `crates/weblog-engine/src/importer.rs:162-169`에서 `create_job` 직후 건수 0의 `Progress`를 통지. `Progress` 문서 주석(110-111행) 갱신. `tests/recovery.rs`의 장애 주입 훅은 `committed_batches >= 1`에서만 반응하므로 영향 없음.
- [x] Tauri 명령을 비동기+블로킹 풀로 옮긴다.
  근거: `apps/desktop/src-tauri/src/commands.rs`의 `close_project`(66-70), `current_project`(72-76), `list_profiles`(83-87), `save_profile`(107-114), `delete_profile`(116-120), `start_import`(157-165), `resume_job`(167-175)을 `async fn` + `blocking()`으로 변경. 메모리 읽기·짧은 잠금만 하는 `list_presets`·`validate_profile`·`profile_from_yaml`·`profile_to_yaml`·`import_status`·`export_status`·`cancel_*`는 동기 유지.
- [x] 작성한 테스트를 실행하고 통과를 확인한다.
  근거: `cargo test -p weblog-engine first_progress`, `cargo test -p weblog-service start_import_returns` 통과. 회귀 — `cargo test --workspace --locked` 152개 통과(engine 127, recovery 12, fixtures 4, service 9), `cargo clippy --workspace --all-targets --locked -- -D warnings` 경고 0, `cargo fmt --all -- --check` 통과.
- [x] 위키(`docs/verification.md`의 코드 품질 검토 표)에서 해당 항목을 해결로 갱신한다.
  근거: `docs/verification.md:50`을 "해결(2026-09-08)"로 갱신하고 근거 PLAN 경로와 수정 위치(`importer.rs:162-169`, `commands.rs:157-175`)를 기록했다. 표에 상태 열을 추가해 미해결 항목과 구분했다.
