# 조각 색과 항목 잇기 (2026-09-10)

## 요청
1. 에러 로그 상세에서 `client:`와 그 값이 떨어져 보여 다른 값처럼 읽힌다. 이어지게 표현한다.
2. 라벨 칩이 검정·회색·흰 계열인 것들이 있다. 전부 색감 있는 색으로 바꾼다.

## 설계
- 잇기: 조각은 그대로 두고(끌어 옮기기 유지) 같은 항목(`key:` + 값들)의 배경만 이어 붙인다. 음수 여백으로 사이를 없애고 모서리는 덩어리 양 끝에만 준다. 라벨은 첫 조각 위에만 나온다.
- 색: 엔진 필드 종류(`kind`)로 기본색을 주되, 같은 종류를 여러 뜻으로 쓰는 `text`·`integer`는 라벨 무리(`client`·`time`·`request`·`response`·`server`·`misc`)로 색조를 갈라 준다. 라벨 id는 `%h`·`$remote_addr`처럼 CSS에 못 쓰는 문자가 있어 무리를 쓴다. 회색은 값이 아닌 "무시"에만 남긴다.

CSS·표시 변경이라 단위 테스트 대신 실제 화면 렌더 측정으로 확인한다(AGENTS.md 테스트 예외 사유).

## 작업

- [x] 같은 항목의 조각을 이어 붙인다.
  근거: `panels/StartPanel.tsx`의 `Puzzle`에서 `tails` 정보로 `tail-cont`(이어지는 값)·`run-end`(덩어리 끝) 클래스를 붙이고, `styles.css`에 `.chunk.absorbed.tail-cont { margin-left: -5px }`와 양 끝 모서리 규칙 추가.
- [x] 칩 색을 전부 색감 있는 값으로 바꾼다.
  근거: `styles.css`의 색 블록 재작성 — 종류별 기본색 13개(client_ip 초록, timestamp 파랑, request_line·method 청록, request_target 쪽빛, protocol 보라, status 호박, bytes_sent 황토, referrer 장미, user_agent 자주, integer·text 라일락, ignore 연회색)와 무리별 보정 8개(`.grp-client.kind-text` 풀색, `.grp-time.*` 청록, `.grp-request.kind-text` 하늘, `.grp-response.*` 주황·호박, `.grp-server.*` 자주). `StartPanel`의 칩·조각에 `grp-<무리>` 클래스 추가.
- [x] 실제 화면으로 확인한다.
  근거: 실제 StartPanel을 띄운 임시 하네스(스캔·샘플·미리보기 IPC 모의)에서
  · 색: 기본 팔레트 16개 칩이 모두 유채색으로 계산됨(예 클라이언트 IP `rgb(74,162,135)`, 로그인 사용자 `rgb(123,168,92)`, 요청 시각 `rgb(95,146,205)`, 경로 `rgb(127,134,210)`, 상태코드 `rgb(208,160,82)`, 서버 이름 `rgb(176,122,168)`, 에러 팔레트의 레벨 `rgb(201,112,79)`), 회색은 "무시"(`rgb(169,155,176)`, 점선)만 남음. 스크린샷 확인.
  · 잇기: nginx 에러 줄에서 `client:`/`161.35.117.252,` 조각이 `tail-key`/`tail-cont run-end`로 붙고 배경 간격 0.3px, 모서리 `4px 0 0 4px` + `0 4px 4px 0`, 라벨은 첫 조각에만("클라이언트 IP"). `request:`처럼 값이 여러 조각인 항목도 마지막 조각에만 `run-end`가 붙는 것을 확인.
  · `pnpm typecheck` 0, `pnpm test` 70 통과, `pnpm lint` 오류 0. 하네스는 삭제.
  · 미검증: 마지막 잇기 화면 스크린샷은 브라우저 캡처가 응답하지 않아 남기지 못했다(치수·클래스는 위와 같이 측정으로 확인).
