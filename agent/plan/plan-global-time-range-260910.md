# 기간 필터를 룰보다 상위로 (2026-09-10)

## 요청
사고 발생 시점이 특정되면 그 구간만 보고 싶다. 지금은 기간 입력이 조회·통계 각 탭 안에 있어서 탭을 옮기면 풀린다.
기간을 사이드바의 `[룰]` 아래에 두고, 룰보다 상위 조건으로 모든 탭(접근·에러의 조회·통계)에 함께 걸리게 한다.

## 설계
- 기간은 전역 상태로 둔다: 입력 문자열(`range`)과 적용된 값(`appliedRange`: UTC 마이크로초 + nonce).
- 사이드바가 기간을 소유한다. "적용"을 누르면 검증 후 전역에 반영하고, nonce가 바뀌면 열려 있는 탭이 다시 조회·집계한다.
- 각 패널의 시작·끝 입력은 없앤다. 조건 조립은 `룰 조건 → 기간 → 화면 검색` 순서로 항상 기간이 걸린다.
- 로그 종류를 바꿔도 기간은 유지한다(사고 시각은 종류와 무관).

## 작업

- [x] 동작을 검증하는 테스트를 먼저 작성한다(기간 파싱·검증, 조건에 기간이 항상 얹히는지, 잘못된 값 거부).
  근거: `src/lib/timeRange.test.ts`(빈 값=열린 끝, 한국 시간→UTC 마이크로초, 형식 오류·역순 거부, 룰 조건 위에 얹기, 룰이 들고 있던 구간 덮어쓰기) — 구현 전 실행 시 모듈 없음으로 실패 확인. `src/panels/errorFilter.test.ts`에 "사이드바 기간이 룰의 기간을 덮는다" 항목 추가.
- [x] `lib/timeRange.ts`(파싱·적용)와 전역 상태를 구현한다.
  근거: `lib/timeRange.ts`(`parseRange`/`withRange`/`rangeLabel`), `state.tsx`에 `range`(입력)·`appliedRange`(적용값+nonce)·`applyRange` 추가.
- [x] 사이드바에 기간 입력을 넣고, 조회·통계 패널의 기간 입력을 없앤다.
  근거: `panels/RulesSidebar.tsx`의 `RangeBox`(룰 헤더 아래, 시작·끝·"기간 적용"·"해제"), `styles.css`의 `.range-box` 계열. `QueryPanel`·`ErrorPanel`·`StatsPanel`·`ErrorStatsPanel`에서 from/to 입력과 상태를 제거하고 `appliedRange`를 쓰며, `appliedRange.nonce`가 바뀌면 다시 조회·집계한다. `composeFilter`/`composeErrorFilter`는 기간을 인자로 받아 항상 마지막에 얹는다(룰이 들고 있던 기간은 덮음).
- [x] 작성한 테스트를 실행하고 통과를 확인한다.
  근거: `pnpm --dir apps/desktop test` 13 files / 70 tests 통과, `pnpm typecheck` 오류 0, `pnpm lint` 오류 0(기존 useLogRows 경고 1).
- [x] 실제 화면에서 기간을 넣고 탭·로그 종류를 오갈 때 조건이 유지되는지 확인한다.
  근거: 실제 엔진 덤프로 IPC를 채운 임시 하네스에서 사이드바에 `2026-08-10T02:00`~`03:00`을 넣고 "기간 적용" 후, 통계 탭 → 에러 로그 → 조회 탭으로 이동하며 나간 모든 `compute_stats`·`query_page` 요청에 `time_from_micros=1786294800000000`, `time_to_micros=1786298400000000`이 들어 있음을 확인(하네스는 삭제). 스크린샷으로 사이드바 배치(룰 헤더 → 기간 상자 → 룰 목록)와 조회 막대에서 시작·끝 입력이 사라진 것 확인.
- [x] 위키를 갱신한다.
  근거: `README.md` 결과 화면 절에 기간 항목 추가(사이드바 위치, 룰보다 상위, 탭·종류 이동에도 유지, 해제, 역순 거부)와 조회 설명의 "룰 조건 + 기간" 문구 정리.
