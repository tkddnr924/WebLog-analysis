# 검증 기록

[docs/testing.md](testing.md)의 시나리오별 상태. 자동 = 저장소의 테스트가 검사한다. 재현 = 스크립트·CLI로 확인했다(수치는 [benchmarks.md](benchmarks.md)). 사용자 = Windows/HDD 환경에서 사용자가 검증한다. 미실행 = 아직 하지 않았다.

## 필수 정확성 시나리오

| 시나리오 | 상태 | 근거 |
|---|---|---|
| Apache/Nginx Common·Combined, IIS W3C 헤더 변경, Custom, Unknown+인식 가능 포맷 | 자동 | `tests/fixtures.rs`(손으로 쓴 예상 결과 6종), `detect.rs` 테스트 |
| IPv4/IPv6, 쿼리 문자열, 인용 문자열, 누락값, UTC 변환, 시간대 미확정 | 자동 | `parse/semantic.rs`, `parse/blocks.rs`, fixture `apache_combined`, `custom_pipe` |
| 빈 줄·주석·깨진 줄·CRLF/LF·BOM·인코딩 오류·긴 줄·gzip 손상 | 자동(gzip 손상은 미실행) | `source/reader.rs`, `importer.rs`; 잘린 gzip 파일 입력은 별도 테스트 없음 — 디코더 오류가 I/O 오류로 작업 실패 처리됨 |
| 미리보기와 실제 DB 결과 일치, 퍼즐/YAML 의미 보존, 재구성 표시 구분 | 자동 | `tests/fixtures.rs`(필드별 비교), `format/yaml.rs`, `reconstruct.rs`, 화면의 재구성 라벨 |
| 다른 위치의 동일 로그는 두 행, 동일 작업 재시도는 한 번만 저장 | 자동 | `store/batch.rs`, `importer.rs` |
| 원문/raw_line 테이블·실패 원문·샘플 영속 캐시 없음, 예외 메시지 입력 누출 없음 | 자동 + 재현 | `schema.rs` raw 컬럼 검사, `tests/fixtures.rs` 누출 검사, 서비스 미리보기 페이로드 검사, 1GB DB 바이트 검색 |
| 파일 경로·줄 번호 추적, 원본 없는 조회·재구성, 원본 없는 재파싱의 명시적 실패 | 자동(재파싱 실패는 미실행) | `detail`은 파일 경로·줄·배치를 돌려준다; 원본이 없을 때 재파싱은 `SourceIdentity::read`의 I/O 오류로 실패하지만 전용 테스트는 없음 |

## 통합·장애 시나리오

| 시나리오 | 상태 | 근거 |
|---|---|---|
| 커밋 전후 취소와 강제 종료, 재개 후 중복·누락 검사 | 자동 + 재현 | `tests/recovery.rs`(panic으로 종료 흉내), `scripts/recovery-demo.sh`(kill -9, 200만 줄 합계 일치) |
| 실패행·주석을 포함한 체크포인트, gzip 재생, IIS 헤더 상태 복원 | 자동 | `tests/recovery.rs` |
| 파일 이동·변경·삭제, 디스크 부족, DB 쓰기 실패, 연결 중단 | 일부 자동 | 이동(재연결+검증), 변경(가져오는 중·재개 전) 자동. 디스크 부족·DB 쓰기 실패는 `commit_batch_with_hook` 장애 주입으로 롤백만 검증. 실제 디스크 부족은 미실행 |
| Appender flush와 트랜잭션 경계 장애 주입 | 자동 | `store/batch.rs` |
| 가져오기 중 페이지 이동, NULL 시간, 같은 시간의 많은 로그, 필터 변경 시 오래된 응답 폐기 | 자동 | `store/query.rs`, `tests/recovery.rs`(동시 조회 커서 고정), `src/lib/pages.test.ts` |
| Windows 파일 선택·긴 경로·한글 경로·IPC·Tauri 번들 실행 | 사용자 | macOS dev 실행만 확인. `pnpm tauri build`(NSIS/MSI)는 미실행 |

## 성능 실험

| 항목 | 상태 | 근거 |
|---|---|---|
| GUI 없이 압축 읽기 → 파싱 → 저장 → 조회 경로 | 재현 | `weblog` CLI, `scripts/bench-run.sh` |
| 1GB → 10GB → 100GB | 10GB까지 재현, 100GB 미실행 | benchmarks.md. 100GB는 사용자 환경에서 |
| 다양한 고유값과 뒤섞인 시간 | 재현 | 합성 생성기(고유 IP 20만~100만, 경로 5만~20만, 5% 지연 기록) |
| RSS·처리량·DB 크기·임시 공간·페이지 지연 | 재현(임시 공간 최고치는 미측정) | benchmarks.md |
| 쿼리 세트(시간 범위, 상태, IP, 경로, 시간 집계, 넓은 문자열 검색, 재구성 상세, 스트리밍 내보내기) | 재현 | benchmarks.md 5단계 항목 |
| 실행 계획과 cold/warm | 부분 | 새 프로세스 실행으로 DuckDB 버퍼 cold 조건만. EXPLAIN은 기록하지 않음 |
| 무거운 작업 직렬화와 취소 응답 | 자동 | 서비스의 heavy 잠금, `cancel_heavy`(DuckDB interrupt), 내보내기 취소 테스트 |

## 개발 검사

- `scripts/check.sh` / `check.ps1`: pnpm(타입·린트·Vitest·빌드) + cargo fmt/clippy/test. Windows에서 PowerShell 스크립트 실행은 사용자 검증.

## 코드 품질 검토 (2026-09-08)

정적 검토와 기존 검사 실행으로 확인한 결함이다. 미해결 항목은 수정 전까지 알려진 제약이다. 상세 근거는 `agent/plan/plan-code-quality-review-260908.md`.

| 심각도 | 결함 | 상태 | 위치 |
|---|---|---|---|
| 높음 | `start_import`/`resume_job`이 동기 Tauri 명령이고 첫 배치 커밋(기본 5만 행·32MB)까지 호출자를 블로킹해 그동안 UI가 정지한다 | 해결(2026-09-08) — 작업 생성 직후 진행 통지 + 명령 비동기화. 근거는 `agent/plan/plan-import-start-nonblocking-260908.md` | `crates/weblog-engine/src/importer.rs:162-169`, `apps/desktop/src-tauri/src/commands.rs:157-175` |
| 높음 | 중단 작업 재개·활성화·결과 삭제·소스 검증 IPC가 UI에 연결되어 있지 않고, 안내 문구는 존재하지 않는 "작업 탭"을 가리킨다 | 해결(2026-09-08) — 사용자 결정으로 작업 탭을 쓰지 않는다. 탭·프런트 래퍼·해당 Tauri 명령(`resume_job`, `list_jobs`, `activate_job`, `delete_job_results`, `verify_source`)을 모두 제거하고, 안내 문구는 CLI 재개를 가리키게 고쳤다. 복구 기능 자체는 엔진·서비스와 CLI(`weblog resume`/`jobs`/`activate`/`delete-results`/`verify`)에 남는다 | `apps/desktop/src/state.tsx:115`, `apps/desktop/src/panels/ResultsPanel.tsx`, `apps/desktop/src-tauri/src/lib.rs:52-57` |
| 중간 | 탐색 오류 목록에 상한이 없어 권한 오류가 많은 트리에서 계속 증가한다 | 해결(2026-09-08) — 오류도 `max_entries` 상한을 쓰고 `errors_truncated`로 알린다. 근거는 `agent/plan/plan-engine-limits-and-poison-260908.md` | `crates/weblog-engine/src/source/scan.rs:105-112` |
| 중간 | 사용자 프리셋 YAML을 상한 검사 전에 전부 메모리로 읽는다 | 해결(2026-09-08) — 읽기 전에 파일 크기를 `yaml::MAX_YAML_BYTES`와 비교한다 | `crates/weblog-engine/src/format/library.rs:60-70` |
| 중간 | `with_store`가 뮤텍스 포이즈닝을 "가져오기 진행 중"으로 잘못 보고한다 | 해결(2026-09-08) — `Poisoned`는 복구해 진행하고 `WouldBlock`만 `ImportRunning`으로 보고한다 | `crates/weblog-service/src/service.rs:803-817` |
| 중간 | 원문 컬럼 금지 검사가 테스트에서만 호출되고 런타임 가드가 아니다 | 해결(2026-09-08) — `Store::open`/`open_in_memory`가 마이그레이션 직후 검사한다. 근거는 `agent/plan/plan-cleanup-and-guards-260908.md` | `crates/weblog-engine/src/store/mod.rs:184-186,203-204` |
| 중간 | 렌더 예외가 나면 ErrorBoundary가 없어 화면 전체가 흰 화면이 된다(`\xHH` 이스케이프 디코드의 배열 스프레드도 스택 상한에 의존했다) | 해결(2026-09-08) — 화면 단위 ErrorBoundary 추가, 스프레드 제거. 스프레드 위험은 요청 대상 상한 64KiB 안에서는 재현되지 않아 심각도 과대평가였다. 근거는 `agent/plan/plan-render-crash-guard-260908.md` | `apps/desktop/src/panels/ErrorBoundary.tsx`, `apps/desktop/src/App.tsx:71-77`, `apps/desktop/src/lib/escapes.ts:9-37` |
| 중간 | 통계 화면이 오래된 응답을 폐기하지 않아 룰을 빠르게 바꾸면 이전 결과가 표시될 수 있다 | 해결(2026-09-08) — 요청 순번으로 늦은 응답을 버린다 | `apps/desktop/src/panels/StatsPanel.tsx:97-128` |
| 낮음 | 미사용 공개 API: `Store::path`, `Service::job`, `BlocksParser::pattern`, `assert_no_raw_columns`, 프런트 `joinPath`·`templatesFor`·`GROUP_LABELS`·`PRESET_LABELS`·`DISPLAY_TZ_LABEL` | 해결(2026-09-08) — `assert_no_raw_columns`는 런타임 가드로 승격, 나머지는 제거(`Service::job`은 작업 탭에서도 쓰지 않아 삭제하고 테스트는 `list_jobs` 조회로 바꿨다. 이전 표에 "작업 탭에서 사용"이라 적은 서술은 오류였다) | `store/mod.rs`, `parse/blocks.rs`, `format/compile.rs`, `weblog-service/src/service.rs`, `lib/paths.ts`, `lib/puzzle.ts`, `lib/format.ts` |
| 낮음 | 중복 로직: ID 생성 `MAX+1` 3곳, 상태 집계 SQL 2곳, 바이트 단위 표기 KiB/KB 불일치 | 해결(2026-09-08) — `store::next_id` 공유 함수로 통합, 죽은 `status_histogram` 경로(엔진 트레이트·서비스·명령) 제거, 바이트 표기를 KiB 계열로 통일, 중복이던 `jobStatusLabel`을 `lib/format.ts` 하나만 남겼다 | `store/mod.rs:564-572`, `store/batch.rs:161`, `store/views.rs:127`, `panels/ByteValue.tsx:4`, `lib/format.ts:62-76` |
| 낮음 | 조건 값 오류 메시지에 사용자 입력값을 포함한다 | 해결(2026-09-08) — 컬럼 이름만 남기고 입력값을 뺐다 | `crates/weblog-engine/src/store/query.rs:225` |

검사 결과(2026-09-08, 수정 후 재실행): `cargo clippy --workspace --all-targets --locked -- -D warnings` 경고 0, `cargo fmt --all -- --check` 통과, `cargo test --workspace --locked` 156개 통과, `pnpm typecheck`·`pnpm build` 통과, Vitest 46개 통과. Vitest 수가 줄어든 것은 삭제한 함수(`joinPath`, `templatesFor`)와 작업 탭(`lib/jobs.ts`) 테스트를 함께 지웠기 때문이다. `pnpm lint` 경고 1건은 그대로다(`QueryPanel.tsx:136` TanStack Virtual의 `useVirtualizer`는 메모이제이션할 수 없다는 `react-hooks/incompatible-library` 경고).

## 가져오기 처리량 개선 (2026-09-09)

파싱 줄당 할당 제거 → 파싱·커밋 파이프라인 → 배치 기본값 상향의 3단계로 같은 입력 기준 23.82s → 12.28s(252k → 489k줄/초)로 줄였다. 수치와 설정별 비교는 `docs/benchmarks.md`의 "가져오기 처리량 개선", 작업 기록은 `agent/plan/plan-import-throughput-260909.md`.

| 항목 | 상태 | 근거 |
|---|---|---|
| 저장 결과 동일성 | 재현 | 개선 전 DB와 개선 후 DB(플레인·gzip)의 `weblog analyze --top-n 20` 출력이 완전히 일치. 레코드 5,964,046 / 오류 30,009 / 제외 5,945 동일 |
| 재개·중복 없음·취소 | 자동 | `tests/recovery.rs` 13개. 파이프라인 때문에 취소·강제 종료 시점의 확정 배치가 1~2개 많아질 수 있어 "정확히 N행" 단언을 배치 경계·범위 단언으로 바꿨다 |
| 메모리 | 재현 | 피크 RSS 985MiB → 640MiB(기본 설정). 배치 200,000행은 10.74s로 더 빠르지만 1,309MiB를 써서 기본값으로 삼지 않았다 |
| Windows·HDD | 미검증 | 측정은 Mac SSD 기준. HDD에서는 커밋 비중이 더 커질 수 있다 |

검사 결과(2026-09-09): `cargo test --workspace --locked` 156개 통과(연속 2회), `cargo clippy --workspace --all-targets --locked -- -D warnings` 경고 0, `cargo fmt --all -- --check` 통과. 프런트엔드는 변경이 없어 재검사하지 않았다.
