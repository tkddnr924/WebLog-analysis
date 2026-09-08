# WebLog 공통 개발 규칙

## 확정 범위
- Rust + Tauri 2 + React/TypeScript + 프로젝트별 영속 DuckDB.
- Windows x64 가상 인스턴스, RAM 16GB, 증설 가능한 HDD. 압축 후 100GB 이상 입력을 고려한다.
- Apache/Nginx 접근 로그, IIS W3C, 커스텀 단일행 텍스트. 일반 파일과 `.gz` 지원.
- 서버 선택 → 경로 → 재귀/파일명 탐색 → 파일별 포맷 검사·편집 → 파싱 → DB 저장 → 조회.
- 기본 프리셋, 퍼즐/YAML 편집, 테이블과 구조화 트리, 재구성 로그 제공.
- 원문은 저장하지 않는다. 파싱 필드와 출처 파일·줄 위치를 저장한다. 실패 원문도 복제하지 않는다.
- 재구성 로그는 원본과 바이트 단위로 같지 않으며 정확한 원문으로 표시하지 않는다.
- 실시간 감시, 멀티라인 에러 로그, 다중 사용자 접속은 최초 범위에서 제외한다.

## 작업 방식
- 구현 전 [설계](docs/harness-design.md)와 관련 명세를 읽는다. [구현 순서](docs/implementation-plan.md)를 따른다.
- 합의한 범위는 반복 승인 없이 진행한다. 기술 스택과 데이터 보존 정책을 변경하려면 이유와 영향을 제안한다.
- 관련 없는 리팩터링을 피하고, 의존성은 필요 이유를 기록한다. 사용자 변경을 덮어쓰지 않는다.
- 새 스킬 설치·업데이트 및 하위 에이전트 사용은 사용자 요청이 있을 때 수행한다.
- 설명은 한국어, 식별자는 영어. 완료 시 변경 내용·검사 결과·미검증 사항을 보고한다.

## 구조와 데이터 처리
- UI → Tauri 명령 → 서비스 → 파서/저장소. 핵심 엔진은 Tauri 없이 테스트 가능해야 한다.
- lib.rs는 앱 조립·명령 등록을 담당한다. 핵심 로직을 한 파일에 모으지 않는다.
- 파일 전체 또는 전체 쿼리 결과를 메모리에 수집하지 않는다. 파서·큐·응답·UI 캐시에 바이트 상한을 둔다.
- 압축 해제는 스트리밍으로 수행하고 행별 INSERT/COMMIT 대신 배치 적재를 사용한다.
- 미리보기와 실제 가져오기는 동일한 Rust 파서/정규화 코드를 사용한다.
- 성공 행·오류 위치·배치 상태·체크포인트는 하나의 트랜잭션으로 확정한다.
- 같은 작업의 재시도는 중복을 만들지 않는다. 다른 위치에 반복된 로그는 제거하지 않는다.
- 취소 시 미커밋 배치를 롤백하고 확정 배치를 보존한다. gzip 재개는 초기에는 재생·건너뛰기를 허용한다.
- DB는 단일 소유 프로세스에서 관리한다. HDD 기준 단일 쓰기 큐와 제한된 쿼리 실행을 사용한다.
- 필터/정렬은 DB에서 수행한다. 필요한 컬럼과 제한된 페이지를 반환하고 큰 OFFSET을 기본 방식으로 삼지 않는다.
- 내보내기도 스트리밍한다. LIMIT이나 DuckDB memory_limit를 앱 전체 작업량/메모리 상한으로 오해하지 않는다.
- 필드값·원문 샘플을 진단 로그나 오류 메시지에 몰래 저장하지 않는다. 샘플은 미리보기 동안만 제한적으로 유지한다.

## 스킬 사용
아래 파일은 프로젝트에 설치되어 있다. 도구의 자동 발견 여부와 무관하게 관련 작업에서 해당 SKILL.md를 읽고 필요한 상대 참조를 따라간다.
- Rust: `.agents/skills/rust-best-practices/SKILL.md`
- 비동기/취소: `.agents/skills/rust-async-patterns/SKILL.md`
- 테스트: `.agents/skills/rust-testing/SKILL.md`
- Tauri: `.agents/skills/tauri-v2/SKILL.md`
- 새 스킬 검색: `.agents/skills/find-skills/SKILL.md`
- UI 가이드라인 검토: `.agents/skills/web-design-guidelines/SKILL.md`
- 프런트엔드 디자인: `.agents/skills/frontend-design/SKILL.md`
- UI 시각 디자인: `.agents/skills/design-taste-frontend/SKILL.md`

사용자가 합의한 프로젝트별 보완:
- 제품 코드에서 unwrap/expect로 입력·I/O 오류를 종료시키지 않는다. Result와 명시적 오류를 사용한다.
- Tauri 스킬의 모든 로직을 lib.rs에 둔다는 지침 대신 위 모듈 분리 규칙을 적용한다.
- IPC 구조체의 필드 이름은 serde 규약으로 명시한다. 중첩 필드 자동 변환을 가정하지 않는다.
- 파서·복구·버그 수정은 테스트를 먼저 작성한다. 단순 문서·스타일 수정에 불필요한 테스트를 추가하지 않는다.
- 커버리지 80~100%를 일괄 강제하지 않는다. 데이터 무결성과 실패 시나리오 검증을 우선한다.
- 스킬의 예제 버전·API는 도입 시 공식 문서와 실제 빌드로 검증한다. 스킬 원문은 수정하지 않는다.

## 검증
1단계(엔진·CLI)까지 구현되어 있다. 공통 검사 진입점은 `scripts/check.sh` / `scripts/check.ps1`이며 React 검사는 3단계에서 추가한다.
- Rust: `cargo fmt --all -- --check`
- Rust: `cargo clippy --workspace --all-targets --locked -- -D warnings`
- Rust: `cargo test --workspace --locked`
- React: 타입 검사, 린트, 관련 테스트, 프로덕션 빌드.
- Windows x64 Tauri 빌드와 실제 파일 선택·IPC·DB 연동 확인.
- 로컬/CI 공통 검사 진입점을 만들고 실행법을 README에 문서화한다. 지원 feature 조합만 검사한다.
- [테스트 기준](docs/testing.md)에 따라 정확성·복구·메모리·디스크 사용을 측정한다.
- 실행하지 않은 대용량 검사나 Windows 검사를 통과했다고 보고하지 않는다.
