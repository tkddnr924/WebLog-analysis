# PLAN · 결과 화면 작업 탭 (260908)

검토 결과(`agent/plan/plan-code-quality-review-260908.md`) 높음 2번 수정.

문제: `api.ts`의 `resumeJob`/`listJobs`/`activateJob`/`deleteJobResults`/`verifySource` 호출부가 없고,
`state.tsx:115`가 존재하지 않는 "결과 화면의 작업 탭"을 안내했다(`ResultsPanel`의 탭은 조회·통계뿐).
엔진·서비스·IPC까지 구현된 중단 작업 복구를 사용자가 쓸 수 없었다.

수정 방향: 결과 화면에 작업 탭을 추가한다. 동작 허용 규칙은 Rust 엔진 규칙과 같게 유지하고, 그 판정만 순수 함수로 분리해 테스트한다.

- [x] 동작 허용 규칙을 검증하는 테스트를 먼저 작성한다.
  근거: `apps/desktop/src/lib/jobs.test.ts` — 8개 상태 전체에 대해 재개 가능 집합이 `["cancelled","failed","interrupted"]`인지, 활성화가 완료된 비활성 결과에만 제안되는지, 실행 중 작업과 활성 결과가 삭제 불가인지 검증. 작성 직후 실행하면 `lib/jobs.ts`가 없어 실패(모듈 해석 오류).
- [x] 규칙 모듈을 구현한다.
  근거: `apps/desktop/src/lib/jobs.ts` — `jobStatusLabel`, `SOURCE_STATUS_LABELS`, `jobActions`. `jobActions`는 `JobStatus::is_resumable`(`store/mod.rs:94`), `Store::activate_job`(`store/mod.rs:472-482`), `Store::delete_job_results`(`store/mod.rs:508-524`)의 조건을 그대로 반영한다. `pnpm vitest run src/lib/jobs.test.ts` 4개 통과.
- [x] 작업 탭 화면을 구현하고 결과 화면에 연결한다.
  근거: `apps/desktop/src/panels/JobsPanel.tsx` 신규 — 작업 카드(상태·활성·복구 대상 배지, 결과 버전, 재파싱 대상, 확정 건수, 실패 사유), 동작 버튼(재개 / 전체 검증 후 재개 / 활성화 / 이 작업만 조회 / 결과 삭제), 파일별 검증·전체 검증·위치 지정(파일 선택 대화상자로 재연결). 가져오기 진행 중이면 변경 동작을 비활성화한다. `panels/ResultsPanel.tsx:8-14,32`에 `jobs` 탭 추가, `styles.css:416-436`에 작업 탭 스타일(`.badge` 기본 스타일 포함) 추가.
- [x] 실제 화면에서 동작을 확인한다.
  근거: vite dev(5173)에 임시 mock IPC 페이지(`apps/desktop/mock.html`, 확인 후 삭제)를 띄워 헤드리스 브라우저로 확인. 작업 카드 3개 렌더, 배지 `완료`/`활성 결과`/`비정상 종료`/`이번 실행에서 복구 대상`/`실패` 표시, 동작 버튼이 상태별로 정확히 노출(활성 완료 작업은 `이 작업만 조회`만, 비정상 종료·실패 작업은 재개·전체 검증 후 재개·결과 삭제 포함). 파일 "검증" 클릭 시 불일치 사유가 파일 행과 상단 알림에 표시. "이 작업만 조회" 클릭 후 조회 탭의 `query_page` 인자에 `"job_id":2`가 담기는 것을 확인. 스크린샷으로 레이아웃 확인.
- [x] 검사를 실행한다.
  근거: `pnpm check`(typecheck → eslint → Vitest 11파일 51개 → 프로덕션 빌드) 통과. eslint 경고는 기존 `QueryPanel.tsx:136` 1건뿐. Rust 측 변경 없음(1번 수정의 검사 결과 유지).
- [x] 위키에 반영한다.
  근거: `README.md:54,70,145`에 작업 탭 동작·복구 안내를 기록하고, `docs/verification.md`의 코드 품질 검토 표에서 높음 2건을 해결로 갱신했다.

미검증: Windows x64 Tauri 번들에서의 실제 파일 선택 대화상자(위치 지정)와 재개 동작은 사용자 검증 영역이다. 이번 확인은 macOS 헤드리스 브라우저 + mock IPC 기준이다.
