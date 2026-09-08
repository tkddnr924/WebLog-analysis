# Claude 구현 순서

이 문서는 구현 지시다. 기술 선택은 확정되었다. 진행 상태: 1단계 완료(2026-09-07, 결과는 [docs/benchmarks.md](benchmarks.md)), 2단계 완료(2026-09-07, 결정은 [docs/decisions/0001-engine-and-recovery-choices.md](decisions/0001-engine-and-recovery-choices.md)), 3단계 완료(2026-09-08, 앱 구조는 [docs/decisions/0002-desktop-app-structure.md](decisions/0002-desktop-app-structure.md); Windows 실행 확인은 사용자 검증), 4단계 완료(2026-09-08, [docs/decisions/0003-profile-editor.md](decisions/0003-profile-editor.md)), 5단계 구현 완료(2026-09-08, [docs/decisions/0004-stats-views-export.md](decisions/0004-stats-views-export.md), 시나리오 상태는 [docs/verification.md](verification.md); Windows 패키징·100GB 측정은 사용자 검증). 사용자가 요청한 범위에서 아래 순서대로 진행한다.

## 1. 엔진 선행 실험
- Rust 도구체인·잠금 파일·최소 CLI 실험 진입점과 검사 명령을 구성한다.
- 합성 로그와 예상 필드 fixture를 만든다. Common/Combined/W3C/Custom을 포함한다.
- 스트리밍 gzip 파싱 → DuckDB 배치 적재 → 제한된 SQL 페이지 조회를 구현한다.
- 원문 미저장과 출처 추적을 처음부터 적용한다.
- Windows/RAM 16GB/HDD에서 가능한 규모까지 측정하고 docs/benchmarks.md에 장비·명령·결과·미검증 범위를 기록한다.
- 통과 기준: 작은 자료의 정확성, 바이트 상한, 저장/조회 연결이 검증되고 병목을 확인했다. 100GB 자료 부재만으로 모든 후속 구현을 막지는 않는다.

## 2. 가져오기와 복구
- 재귀·파일명 패턴 탐색, 파일 선택, 샘플 판별, 포맷 스냅샷을 연결한다.
- 배치 상태, 취소, interrupted 복구, 파일 검증, 재파싱 결과 버전을 구현한다.
- 통과 기준: 커밋 경계 장애 주입에서 재개 후 중복·누락이 없다.

## 3. Tauri·React 기본 화면
- Tauri 2 + React/TypeScript를 구성한다. lib.rs는 조립 계층으로 유지한다.
- 서버/경로 선택, 파일 목록, 포맷 미리보기, 가져오기 진행·취소, 페이지 조회를 연결한다.
- 가상 테이블, 필터, 구조화 상세, 재구성 로그와 출처를 구현한다.
- 통과 기준: 실제 Windows 앱에서 선택 → 적재 → 필터 → 상세 흐름이 동작한다.

## 4. 퍼즐·YAML 편집기
- 공통 정의 모델, 블록 편집, 키보드 조작, 초안 검증, 프리셋 저장을 구현한다.
- 통과 기준: YAML 왕복 의미 보존, 미리보기/적재 일치, 새 포맷 버전 재파싱 동작.

## 5. 제품 검증
- 기본 통계, 저장된 뷰, CSV/JSON 스트리밍 내보내기를 추가한다.
- 대표 대용량 성능, 취소·복구, Windows 패키징을 검증한다.
- README에 설치·실행·검사·저장 위치·복구·원문 미저장 제약을 명시한다.
- 통과 기준: docs/testing.md의 관련 시나리오 결과와 남은 제한이 기록되어 있다.

## 전달용 시작 요청

“CLAUDE.md와 AGENTS.md 및 연결된 명세를 읽고 1단계부터 구현해줘. 원문은 저장하지 말고 필드와 출처만 저장해. 확정한 기술을 다시 묻지 말고, 먼저 파서·DuckDB 저장/조회 경로를 검증한 뒤 UI로 진행해. 실제 실행한 검사와 성능 실험 결과, 아직 검증하지 못한 항목을 구분해서 보고해.”
