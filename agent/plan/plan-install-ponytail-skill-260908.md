# PLAN · ponytail 스킬 설치 (260908)

사용자 요청: `npx skills add https://github.com/dietrichgebert/ponytail --skill ponytail` 실행.
제품 코드 변경이 없는 스킬 설치·문서 갱신 작업이므로 테스트 코드를 추가하지 않는다.
대신 설치 결과 파일 확인과 스킬 목록 문서 반영으로 검증한다.

- [x] 요청한 설치 명령을 실행한다.
  근거: `npx --yes skills add https://github.com/dietrichgebert/ponytail --skill ponytail` 실행. 출력: `Installed 1 skill` / `.agents/skills/ponytail` / `symlinked: Claude Code`, 위험도 평가 Gen Safe · Socket 0 alerts · Snyk Low Risk.
- [x] 설치 결과 파일과 링크를 확인한다.
  근거: `.agents/skills/ponytail/SKILL.md` 존재(120줄, frontmatter `name: ponytail`, `argument-hint: [lite|full|ultra]`, MIT), `.claude/skills/ponytail` 심링크 생성, `skills-lock.json` 변경 확인.
- [x] 스킬 내용을 읽고 적용 범위를 파악한다.
  근거: SKILL.md 전문 확인. 코딩 작업에서 사다리(YAGNI → 기존 코드 재사용 → stdlib → 플랫폼 기능 → 기존 의존성 → 한 줄 → 최소 구현) 적용, 기본 강도 full, 입력 검증·오류 처리·보안·접근성은 단순화 대상 아님, 비단순 로직은 최소 검사 1개 유지.
- [x] 설치된 스킬 목록을 AGENTS.md에 반영한다.
  근거: `AGENTS.md` 「스킬 사용」 목록에 `- 최소 구현·YAGNI: \`.agents/skills/ponytail/SKILL.md\`` 추가(38줄).

미검증: 없음. 스킬은 문서만 제공하며 실행 코드가 없어 별도 동작 검사가 불필요하다.
