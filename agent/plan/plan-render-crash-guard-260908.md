# PLAN · 렌더 크래시 차단 (260908)

검토 결과(`agent/plan/plan-code-quality-review-260908.md`) 중간 항목 수정:
"`\xHH` 이스케이프가 많은 요청 대상에서 배열 스프레드가 인자 수 상한을 넘길 수 있고 ErrorBoundary가 없어 화면 전체가 크래시한다".

확인한 사실: 2번 작업 중 mock IPC가 `query_page`로 `null`을 돌려주자 예외가 콘솔에도 남지 않고 React 트리 전체가 사라져 흰 화면이 됐다.
즉 어떤 패널의 렌더 예외든 앱 전체를 날린다.

수정 방향:
1. `lib/escapes.ts`의 `bytes.push(...enc.encode(...))`와 `String.fromCharCode(...bytes)` 스프레드를 없앤다(요청 대상은 줄 길이 상한 64KiB까지 가능).
2. 렌더 예외를 화면 단위로 가두는 ErrorBoundary를 넣어 헤더·알림을 살리고 사용자가 다시 시도하거나 다른 화면으로 갈 수 있게 한다.

- [x] 요청 동작을 검증하는 테스트를 먼저 작성한다.
  근거: `apps/desktop/src/lib/escapes.test.ts:34-42` — 이스케이프 하나 뒤에 끊기지 않는 평문 64KiB(한 번에 넘기는 원소가 가장 많아지는 형태)를 디코드해 바이트 수·텍스트 길이·`printable` 결과를 확인한다. **이 테스트는 수정 전에도 통과했다.** 스프레드 인자 수를 실측하니 이 런타임은 30만 원소까지 견디고 100만 원소에서 `RangeError`가 났다. 요청 대상은 줄 길이 상한 64KiB로 묶여 있어 검토에서 매긴 "중간" 심각도는 과대평가였다. 테스트는 그 상한 계약을 고정하는 회귀 검사로 남긴다. ErrorBoundary는 렌더 예외 처리라 단위 테스트 대상이 아니며 실제 화면에서 예외를 주입해 확인했다.
- [x] `escapes.ts`의 스프레드를 제거한다(도달 불가 catch 경로 정리 포함).
  근거: `apps/desktop/src/lib/escapes.ts:9-37` — `bytes.push(...enc.encode(slice))`를 바이트 루프로 바꿨고, `printable`은 `TextDecoder("utf-8", { fatal: false })`가 예외를 던지지 않아 도달할 수 없던 `try/catch`와 `String.fromCharCode(...bytes)` 대체 경로를 제거했다. 엔진 스택 상한 의존이 사라졌다.
- [x] ErrorBoundary를 만들고 화면에 적용한다.
  근거: `apps/desktop/src/panels/ErrorBoundary.tsx` 신규 — `getDerivedStateFromError`로 메시지·스택을 잡고 `resetKey`가 바뀌면 예외 상태를 지운다. 대체 화면은 화면 이름, 데이터가 남아 있다는 안내, 오류 메시지, 접힌 스택, "다시 시도" 버튼으로 구성한다. `componentDidCatch`는 코드 위치 정보만 콘솔에 남기고 로그 원문은 다루지 않는다. `App.tsx:10,71-77`에서 `<main>` 안쪽만 감싸 헤더·알림을 살리고 `resetKey={stage}`로 화면 전환 시 복구한다. 스타일은 `styles.css:438-444`.
- [x] 테스트를 실행하고, 실제 화면에서 렌더 예외를 주입해 대체 화면과 복구 동작을 확인한다.
  근거: `pnpm check`(typecheck → eslint → Vitest 11파일 52개 → 프로덕션 빌드) 통과, eslint 경고는 기존 `QueryPanel.tsx:136` 1건뿐. 실제 화면 확인은 vite dev + 임시 mock 페이지(`apps/desktop/mock-crash.html`, `query_page`가 규약을 어긴 `null` 반환, 확인 후 삭제)로 수행 — "결과 보기"를 누르면 이전에는 흰 화면이었으나 이제 헤더가 유지되고(`.topbar` 존재) 대체 화면에 "결과 화면을 표시하지 못했습니다"와 `Cannot read properties of null (reading 'rows')`가 표시된다. "새 로그 가져오기"로 화면을 바꾸면 예외 상태가 지워져 시작 화면이 정상 렌더된다(`.render-error` 사라짐). 오류 상자 높이는 186px(뷰포트 768px)로 화면을 채우지 않는다.
- [x] 위키(`docs/verification.md` 코드 품질 검토 표)에서 해당 항목을 해결로 갱신한다.
  근거: `docs/verification.md`의 해당 행을 해결로 바꾸고, 스프레드 위험이 64KiB 상한 안에서는 재현되지 않아 심각도를 정정한다는 내용을 함께 기록했다.
