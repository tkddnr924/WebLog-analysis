# 상세 패널 북마크 별 버튼 스타일 깨짐 (2026-09-10)

## 증상
결과 화면에서 행을 고르면 열리는 오른쪽 상세 패널의 머리글에서 북마크 별 버튼이 넓고 큰 상자로 그려진다.

## 원인
`DetailPanel`이 `className="star big"`을 쓰는데, `big`은 시작 화면의 큰 버튼용 전역 규칙이다(`button.big { padding: 11px 32px; border-radius: 8px; }`). 선택자 우선순위가 `.star`(0,1,0) < `button.big`(0,1,1)이라 `.star`의 `padding: 0`·`border-radius: 4px`가 덮인다. `.star.big`은 폭·높이·글자 크기만 지정해 패딩이 그대로 남는다.

CSS 수정이라 단위 테스트 대신 실제 브라우저에서 버튼 상자 크기를 재서 수정 전후를 비교한다(AGENTS.md 테스트 예외 사유).

- [x] 실제 스타일로 상세 머리글을 그려 별 버튼 크기를 재고, 수정 전 값이 어긋남을 확인한다.
  근거: `src/styles.css`를 그대로 넣은 임시 페이지를 브라우저로 열어 측정 — 수정 전 상세 별 버튼 `64×28`, `padding: 11px 32px`, `border-radius: 8px`(목록의 별은 `22×22`).
- [x] 별 버튼의 큰 크기 변형을 전역 `big`과 겹치지 않는 이름으로 바꾼다.
  근거: `styles.css` `.star.big` → `.star.star-lg`(주석으로 이유 기록), `panels/DetailPanel.tsx:76`의 클래스 `star big` → `star star-lg`. `big`을 쓰는 다른 곳은 시작 화면 "폴더 선택"(`primary big`)뿐이라 영향 없음.
- [x] 같은 방법으로 수정 후 크기를 다시 재고 확인한다.
  근거: 같은 페이지 재측정 `28×28`, `padding: 0px`, `border-radius: 4px`, 배경 투명. 스크린샷으로 머리글에 노란 상자 없이 별만 보이는 것 확인. `pnpm typecheck` 오류 0, `pnpm test` 65 통과. 임시 페이지는 삭제.


## 추가 요청 (머리글 정리)

- [x] 북마크 버튼을 오른쪽으로 옮기고 버튼 테두리를 준다.
  근거: `panels/DetailPanel.tsx`에서 별과 닫기를 `.detail-actions`로 묶어 오른쪽에 배치, `styles.css`에 `.star.boxed`(테두리 `--line`, 켜지면 `--bm` 배경 + `--bm-line` 테두리) 추가. 측정: 28×28, 테두리 `1px solid rgb(225,220,211)`, 켜짐 배경 `rgb(255,243,191)`.
- [x] 닫기를 아이콘으로 바꾼다.
  근거: `닫기` 링크 → `button.icon.close`(✕, 26×26, 평소 `--muted`·호버 `--ink`). 별과 6px 간격, 머리글 오른쪽 여백 16px. 스크린샷으로 켜짐/꺼짐 두 상태 확인. `pnpm typecheck` 0, `pnpm test` 70 통과.