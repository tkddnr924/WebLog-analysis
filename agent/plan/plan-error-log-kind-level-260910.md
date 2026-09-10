# 에러 로그를 접근 로그와 같은 레벨로 (2026-09-10)

## 요구

- 결과 화면의 최상위 구분은 `접근 로그` / `에러 로그`이고, 각 로그 종류 안에 `조회`·`통계` 탭과 `룰` 사이드바가 있다.
- 에러 로그도 룰(YARA풍)로 조회·통계를 돌린다. 룰이 보는 필드는 레벨·메시지·클라이언트다.
- 두 종류가 공유하는 것은 북마크뿐이다(파일·줄 기준 전역 표).

## 설계 결정

- 조건식에 `level`·`message` 컬럼을 추가한다. 두 값은 `logs.extra_json`에서 `json_extract_string(extra_json, '$.level'|'$.message')`로 꺼낸다. DuckDB bundled 빌드에 JSON 함수가 포함되어 있음을 확인했다(`SELECT json_extract_string('{"level":"error"}','$.level')` → `error`).
- 통계는 `filter.log_kind`로 갈라진다. 에러 로그면 상태코드·메서드·요청 대상 집계를 건너뛰고 레벨 분포와 상위 메시지를 채운다. 접근 로그 결과는 지금과 같다.
- 사용자 룰(저장된 뷰)은 저장된 필터의 `log_kind`로 어느 사이드바에 보일지 정한다.

## 작업

- [x] 엔진 동작을 검증하는 테스트를 먼저 작성한다(level·message 조건 필터, 에러 통계의 레벨 분포·상위 메시지, 접근 통계 불변).
  근거: `crates/weblog-engine/src/store/query.rs`의 `level_and_message_conditions_target_error_fields`, `store/stats.rs`의 `error_stats_group_levels_and_messages`·`access_stats_leave_error_aggregates_empty`. 구현 전 `cargo test -p weblog-engine --lib` 컴파일 실패 12건(E0599 `CondField::Level` 없음, E0609 `levels`/`top_messages` 없음)으로 확인.
- [x] 엔진을 구현한다: `CondField::Level`/`Message`, `StatsResult.levels`/`top_messages`, 로그 종류별 집계 분기.
  근거: `query.rs`(`Level`→`json_extract_string(extra_json,'$.level')`, `Message`→`'$.message'`, 오류 메시지에는 SQL 식 대신 필드 이름 노출), `stats.rs`(에러면 status·methods·top_targets 생략, levels·top_messages 집계). `cargo test --workspace` 170 passed / 0 failed, `cargo clippy --workspace --all-targets` 경고 0.
- [x] 프런트엔드 동작을 검증하는 테스트를 먼저 작성한다(에러 기본 룰 컴파일·매칭, 룰 목록의 종류별 분리, level/message 파싱).
  근거: `apps/desktop/src/lib/rules.test.ts`(에러 룰이 실제 nginx·apache 문구를 잡고 평범한 줄은 배제, `rulesFor` 종류 분리, 기본 필드), `lib/yara.test.ts`(level/message 파싱·숫자 연산 거부·라벨), `panels/errorFilter.test.ts`(룰 조건 + 검색 AND). 구현 전 실패 확인.
- [x] 프런트엔드를 구현한다: 최상위 `접근 로그`/`에러 로그` + 각 종류의 `조회`·`통계` 탭, 종류별 룰 사이드바, 에러 기본 룰, 에러 통계 화면.
  근거: `panels/ResultsPanel.tsx`(kind 스위치 → 탭 → 패널), `RulesSidebar.tsx`(kind별 목록·선택 상태·저장 시 log_kind), `lib/rules.ts`(에러 기본 룰 원문 11개 + 북마크, `rulesFor`, `defaultRuleField`), `lib/yara.ts`(level/message 필드·에러 템플릿), `panels/ErrorPanel.tsx`(룰 조건 반영), `panels/ErrorStatsPanel.tsx`(신규), `panels/StatsPanel.tsx`·`QueryPanel.tsx`(접근 룰만 사용), `styles.css`(.kind-switch).
- [x] 작성한 테스트를 실행하고 통과를 확인한다.
  근거: `cargo test --workspace` 170 passed / 0 failed, `pnpm --dir apps/desktop typecheck` 오류 0, `pnpm --dir apps/desktop test` 12 files / 65 tests 통과.
- [x] 실제 데이터로 화면을 확인한다(접근·에러 각각 조회·통계·룰 적용, 북마크 공유).
  근거: 실제 케이스 DB(접근 500행 / 에러 120행)에서 뽑은 조회 행·통계 JSON으로 IPC를 대신 채운 임시 하네스에서 (1) 최상위 `접근 로그`/`에러 로그` 알약 + 하위 `조회`·`통계` 탭, (2) 종류별 룰 목록(접근 15 / 에러 12), (3) 에러 조회 컬럼(시간·레벨·클라이언트·메시지·출처)과 에러 통계(레벨 분포 error 54·crit 24·notice 22·warn 20, 상위 메시지 4종, 상위 IP 표, 시간축), (4) 접근 조회·통계 기존 동작 유지 확인. 화면이 만드는 에러 룰 필터 JSON을 그대로 엔진에 넣어 건수 확인: 전체 120, 오류 54, 심각 24, 경고 20, 파일 없음 29, 업스트림 연결 실패 56, 요청 본문 초과 35, 북마크 1(에러 화면에서 단 별). 이 과정에서 "PHP·FastCGI 실패"가 `upstream sent too big header`까지 잡는 것을 발견해 해당 문구를 업스트림 룰로 옮기고 테스트로 고정했다. 임시 하네스·예제·생성 테스트는 삭제했다.
- [x] 위키에 화면 구조와 에러 룰·통계 항목을 반영한다.
  근거: `README.md` 결과 화면 절(종류 → 탭 구조, 접근·에러 룰 필드와 기본 룰 목록, 종류별 조회·통계 항목, 북마크 공유, 케이스 경로 문구 수정), `docs/data-model.md` 조회 계약(level·message 컬럼, 통계 분기).
