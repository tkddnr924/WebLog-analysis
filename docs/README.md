# WebLog 위키

프로젝트의 범위, 설계, 구현 상태와 검증 기록을 관리하는 Markdown 위키다. 공통 개발 지침은 [AGENTS.md](../AGENTS.md), 설치·실행·사용법은 [저장소 README](../README.md)에 있다.

## 문서 목차

| 문서 | 관리하는 내용 |
|---|---|
| [프로젝트 개요](project-overview.md) | 확정 기술 스택, 대상 환경, 입력·기능 범위, 보존 정책 |
| [아키텍처와 데이터 처리](architecture.md) | 계층 경계, 메모리·쿼리 상한, 저장·취소·복구 제약 |
| [하네스 설계](harness-design.md) | 전체 흐름과 대용량 처리 상세 설계 |
| [파서 포맷](parser-format.md) | 포맷 정의, 프리셋, 블록·YAML, 정규화 |
| [데이터 모델](data-model.md) | 스키마, 출처, 작업·배치·체크포인트, 결과 버전 |
| [구현 순서와 진행 상태](implementation-plan.md) | 단계별 구현 범위, 완료 기준, 진행 상태 |
| [검증 기준](testing.md) | 검사 명령, 정확성·장애·성능 시나리오 |
| [검증 기록](verification.md) | 시나리오별 검증 결과와 미실행 항목 |
| [벤치마크](benchmarks.md) | 측정 환경, 실행 명령, 성능 결과와 한계 |

## 설계 결정

- [0001 · 엔진과 복구](decisions/0001-engine-and-recovery-choices.md)
- [0002 · 데스크톱 앱 구조](decisions/0002-desktop-app-structure.md)
- [0003 · 프로필 편집기](decisions/0003-profile-editor.md)
- [0004 · 통계·저장된 뷰·내보내기](decisions/0004-stats-views-export.md)

## 읽는 순서

프로젝트 개요 → 아키텍처 → 작업과 관련된 상세 명세·설계 결정 → 구현 순서 → 검증 기준 순으로 확인한다. 실제 검증 여부는 검증 기록과 벤치마크에서 확인한다.

## 갱신 방법

기존 주제는 위 목차의 담당 문서를 수정한다. 새로운 주제는 `docs/`에 Markdown 문서로 추가하고 이 목차와 관련 문서에 상대 링크를 연결한다. 설계 결정은 `decisions/`의 번호 순서를 이어 기록한다. 구현 완료 여부는 구현 순서에, 실행한 검사와 미검증 항목은 검증 기록에, 성능 측정값은 벤치마크에 기록한다.
