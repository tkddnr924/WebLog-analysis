# Claude 구현 진입점

이 프로젝트의 앱 구현은 Claude가 담당한다. 코딩을 시작하기 전에 아래 문서를 읽고 확정된 범위대로 진행한다.

1. [AGENTS.md](AGENTS.md): 공통 개발 규칙과 설치된 스킬의 명시적 경로.
2. [프로젝트 위키](docs/README.md): 프로젝트 범위·아키텍처·명세의 목차.
3. [구현 순서](docs/implementation-plan.md): 첫 작업과 단계별 완료 기준.
4. [하네스 설계](docs/harness-design.md): 대용량 저장·조회·취소·복구 흐름.
5. [데이터 명세](docs/data-model.md), [포맷 명세](docs/parser-format.md), [검증 기준](docs/testing.md).

AGENTS.md의 규칙을 이 파일에 복제하지 않는다. 관련 스킬은 `.agents/skills/`의 SKILL.md를 직접 읽어 사용한다. Claude가 이 경로를 자동 발견한다고 가정하지 않는다.

원문 미저장, Rust/Tauri 2/React/TypeScript/DuckDB, Windows x64/RAM 16GB/증설 가능한 HDD, 압축 후 100GB 이상이라는 조건은 확정이다. 같은 결정을 다시 묻지 않는다.

최초 작업은 GUI 독립 Rust 파서와 저장·조회 선행 실험이다. 결과를 바탕으로 배치·메모리 설정을 정하고 UI를 구현한다. 한 번에 전체 제품을 만들거나 근거 없는 처리 속도를 약속하지 않는다.
