# 에러 로그 전용 조회 화면 (2026-09-10)

## 배경

- 결과 화면의 `조회`·`통계` 탭은 접근 로그 컬럼(시간·IP·메서드·대상·상태·바이트)만 렌더한다(`apps/desktop/src/panels/QueryPanel.tsx:174-179`).
- 에러 로그도 같은 `logs` 테이블에 저장된다. 에러 전용 필드(level·pid_tid·connection·message 등)는 `extra_json`에 들어간다(`crates/weblog-engine/src/store/batch.rs:165-191`).
- 현재 저장소에는 "이 작업이 접근 로그인지 에러 로그인지"를 나타내는 값이 없다. 프로필 이름 추정은 취약하므로 작업(job)에 종류를 명시한다.
- 북마크는 `(source_id, line_number)` 전역 키라 두 화면이 자동으로 공유된다(`crates/weblog-engine/src/store/views.rs:81-93`).

## 설계 결정

- `import_jobs.log_kind`(access|error) 컬럼을 추가하고 가져오기 시작 시 UI가 지정한다. 조회는 `LogFilter.log_kind`로 갈라진다.
- 목록에서 레벨·메시지를 보여야 하므로 `LogRow`에 `extra_json`을 추가한다(접근 로그는 대부분 NULL이라 응답 증가가 작다).
- 에러 로그 검색은 조건식의 `extra` 컬럼(`CondField::Extra`, icontains)과 `client_ip`를 OR로 묶는다. 별도 필터 필드를 두면 엔진이 AND로 합쳐 교집합이 되므로 조건식 하나로 처리한다.
- 에러 탭은 룰 사이드바를 쓰지 않는다. 공유하는 것은 북마크뿐이며 `북마크만 보기` 토글을 자체 필터 바에 둔다.
- 통계 탭과 기존 조회 탭은 `log_kind=access`로 고정해 에러 행이 섞이지 않게 한다.

## 작업

- [x] 백엔드 동작을 검증하는 Rust 테스트를 먼저 작성한다(로그 종류 필터 분리, 페이지 행의 extra_json 노출, 확장 필드 검색, 기존 저장소 마이그레이션 시 프로필 이름으로 종류 백필).
  근거: `crates/weblog-engine/src/store/query.rs`의 `kinded_store`/`log_kind_filter_separates_access_and_error`/`page_rows_expose_extra_json`/`extra_field_search_covers_message_and_client`, `crates/weblog-engine/src/store/schema.rs`의 `v3_store_upgrades_to_v4_backfilling_log_kind`. 구현 전 `cargo test -p weblog-engine --lib store::` 실행 결과 컴파일 실패(E0433 `LogKind` 없음 등 23건)로 미구현 확인.
- [x] 백엔드를 구현한다: 스키마 v4(`import_jobs.log_kind` + 기존 데이터 백필), `LogKind` 타입, `LogFilter.log_kind`, `CondField::Extra`, `LogRow.extra_json`, `ImportRequest`/`StartImportRequest`에 종류 전달.
  근거: `store/mod.rs`(LogKind 정의, `JobInfo.log_kind`, `create_job(.., log_kind)`, JOB_SELECT/map_job), `store/schema.rs`(SCHEMA_VERSION 4, V1에 `log_kind` 컬럼, V4 ALTER+백필 UPDATE 2건), `store/query.rs`(`LogFilter.log_kind` → `job_id IN (SELECT ... coalesce(log_kind,'access') = ?)`, `CondField::Extra` → `extra_json`, `LogRow.extra_json`+ROW_COLUMNS), `importer.rs`(`ImportRequest.log_kind` → create_job), `weblog-service/src/dto.rs`·`service.rs`(StartImportRequest.log_kind, serde 기본값 access), `weblog-cli/src/main.rs`(`--log-kind` 인자). `cargo fmt --all -- --check` 통과.
- [x] 프런트엔드 동작을 검증하는 테스트를 먼저 작성한다(에러 행 필드 추출, 에러 조회 필터 구성).
  근거: `apps/desktop/src/lib/errorRows.test.ts`(nginx·apache 키, 표준 client_ip 우선, 깨진 JSON, message 없는 정의의 요약, 레벨 색), `apps/desktop/src/panels/errorFilter.test.ts`(log_kind·active_only 고정, 검색어 → extra/client_ip OR 조건식, 북마크 토글, 시각 오류). 구현 전 실행 결과 `Test Files 2 failed | 10 passed`(모듈 없음)로 실패 확인.
- [x] 프런트엔드를 구현한다: 결과 화면에 `에러 로그` 탭 추가, 에러 전용 목록(시간·레벨·클라이언트·메시지·출처)과 북마크 공유, 기존 조회·통계는 접근 로그로 고정, 가져오기 요청에 종류 전달.
  근거: 신규 `panels/ErrorPanel.tsx`, `panels/errorFilter.ts`, `panels/useLogRows.ts`(QueryPanel의 커서 페이징·캐시 상한·북마크 토글을 공유 훅으로 추출), `lib/errorRows.ts`. 변경 `panels/ResultsPanel.tsx`(탭 3개, 에러 탭에서 룰 사이드바 숨김), `panels/QueryPanel.tsx`·`panels/StatsPanel.tsx`(`log_kind: "access"` 고정), `panels/StartPanel.tsx`(가져오기 요청에 `log_kind`), `types.ts`(LogKind 정의 이관, `log_kind`, `LogRow.extra_json`, CondField `extra`), `lib/yara.ts`(요약 라벨), `styles.css`(`.vhead.verr`/`.vrow.verr` 컬럼).
- [x] 작성한 테스트를 실행하고 통과를 확인한다(`cargo test --workspace`, `pnpm --dir apps/desktop test`, `pnpm --dir apps/desktop typecheck`).
  근거: `cargo test --workspace` 160 passed / 0 failed(engine lib 134, fixtures 4, recovery 9, service 13). `pnpm typecheck` 오류 0, `pnpm lint` 오류 0(경고 1건은 기존 `useVirtualizer` 경고가 훅으로 옮겨간 것), `pnpm test` 12 files / 56 tests 통과.
- [x] 실제 앱을 띄워 에러 로그 탭과 북마크 공유를 육안으로 확인한다.
  근거: CLI로 실제 가져오기(`weblog import --format combined access.log` 500행, `--format error-profile.json --log-kind error error.log` 120행) 후 (1) 엔진 조회 확인 — 임시 예제로 `log_kind` 분리(access 500 / error 120)와 북마크 토글 후 에러 행 `bookmarked=true`, `bookmarked_only` 1건, (2) 화면 확인 — Vite dev 서버 + IPC를 실제 조회 결과로 대신 채운 임시 하네스에서 `에러 로그` 탭이 시간·레벨·클라이언트·메시지·출처 컬럼과 레벨 색(crit/error 적색, warn 황색, notice 회색)으로 60행을 렌더, 별을 누르면 ★로 바뀌고 "북마크만" 조건에서 그 행만 남음, 행 선택 시 기존 상세 패널이 레벨·메시지·연결 번호 등을 표시, `조회` 탭은 기존 컬럼과 룰 사이드바 유지, (3) 검색 경로 확인 — 화면이 만드는 필터 JSON을 그대로 엔진에 넣어 "No such file" 29건, "10.0.2.162" 30건. 임시 하네스·예제·생성 테스트는 확인 후 삭제했다(스크린샷은 저장소에 남기지 않음).
- [x] 위키에 에러 로그 조회 화면과 북마크 공유 동작을 반영한다.
  근거: `README.md` 결과 화면 절(탭 3개, 에러 로그 컬럼·검색·북마크 공유), `docs/data-model.md`(import_jobs.log_kind, bookmarks 표, 조회 계약 3줄), `docs/testing.md`(로그 종류 분리·마이그레이션 백필·북마크 공유 시나리오).
