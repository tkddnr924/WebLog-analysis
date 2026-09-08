# 검증 기록

[docs/testing.md](testing.md)의 시나리오별 상태. 자동 = 저장소의 테스트가 검사한다. 재현 = 스크립트·CLI로 확인했다(수치는 [benchmarks.md](benchmarks.md)). 사용자 = Windows/HDD 환경에서 사용자가 검증한다. 미실행 = 아직 하지 않았다.

## 필수 정확성 시나리오

| 시나리오 | 상태 | 근거 |
|---|---|---|
| Apache/Nginx Common·Combined, IIS W3C 헤더 변경, Custom, Unknown+인식 가능 포맷 | 자동 | `tests/fixtures.rs`(손으로 쓴 예상 결과 6종), `detect.rs` 테스트 |
| IPv4/IPv6, 쿼리 문자열, 인용 문자열, 누락값, UTC 변환, 시간대 미확정 | 자동 | `parse/semantic.rs`, `parse/blocks.rs`, fixture `apache_combined`, `custom_pipe` |
| 빈 줄·주석·깨진 줄·CRLF/LF·BOM·인코딩 오류·긴 줄·gzip 손상 | 자동(gzip 손상은 미실행) | `source/reader.rs`, `importer.rs`; 잘린 gzip 파일 입력은 별도 테스트 없음 — 디코더 오류가 I/O 오류로 작업 실패 처리됨 |
| 미리보기와 실제 DB 결과 일치, 퍼즐/YAML 의미 보존, 재구성 표시 구분 | 자동 | `tests/fixtures.rs`(필드별 비교), `format/yaml.rs`, `reconstruct.rs`, 화면의 재구성 라벨 |
| 다른 위치의 동일 로그는 두 행, 동일 작업 재시도는 한 번만 저장 | 자동 | `store/batch.rs`, `importer.rs` |
| 원문/raw_line 테이블·실패 원문·샘플 영속 캐시 없음, 예외 메시지 입력 누출 없음 | 자동 + 재현 | `schema.rs` raw 컬럼 검사, `tests/fixtures.rs` 누출 검사, 서비스 미리보기 페이로드 검사, 1GB DB 바이트 검색 |
| 파일 경로·줄 번호 추적, 원본 없는 조회·재구성, 원본 없는 재파싱의 명시적 실패 | 자동(재파싱 실패는 미실행) | `detail`은 파일 경로·줄·배치를 돌려준다; 원본이 없을 때 재파싱은 `SourceIdentity::read`의 I/O 오류로 실패하지만 전용 테스트는 없음 |

## 통합·장애 시나리오

| 시나리오 | 상태 | 근거 |
|---|---|---|
| 커밋 전후 취소와 강제 종료, 재개 후 중복·누락 검사 | 자동 + 재현 | `tests/recovery.rs`(panic으로 종료 흉내), `scripts/recovery-demo.sh`(kill -9, 200만 줄 합계 일치) |
| 실패행·주석을 포함한 체크포인트, gzip 재생, IIS 헤더 상태 복원 | 자동 | `tests/recovery.rs` |
| 파일 이동·변경·삭제, 디스크 부족, DB 쓰기 실패, 연결 중단 | 일부 자동 | 이동(재연결+검증), 변경(가져오는 중·재개 전) 자동. 디스크 부족·DB 쓰기 실패는 `commit_batch_with_hook` 장애 주입으로 롤백만 검증. 실제 디스크 부족은 미실행 |
| Appender flush와 트랜잭션 경계 장애 주입 | 자동 | `store/batch.rs` |
| 가져오기 중 페이지 이동, NULL 시간, 같은 시간의 많은 로그, 필터 변경 시 오래된 응답 폐기 | 자동 | `store/query.rs`, `tests/recovery.rs`(동시 조회 커서 고정), `src/lib/pages.test.ts` |
| Windows 파일 선택·긴 경로·한글 경로·IPC·Tauri 번들 실행 | 사용자 | macOS dev 실행만 확인. `pnpm tauri build`(NSIS/MSI)는 미실행 |

## 성능 실험

| 항목 | 상태 | 근거 |
|---|---|---|
| GUI 없이 압축 읽기 → 파싱 → 저장 → 조회 경로 | 재현 | `weblog` CLI, `scripts/bench-run.sh` |
| 1GB → 10GB → 100GB | 10GB까지 재현, 100GB 미실행 | benchmarks.md. 100GB는 사용자 환경에서 |
| 다양한 고유값과 뒤섞인 시간 | 재현 | 합성 생성기(고유 IP 20만~100만, 경로 5만~20만, 5% 지연 기록) |
| RSS·처리량·DB 크기·임시 공간·페이지 지연 | 재현(임시 공간 최고치는 미측정) | benchmarks.md |
| 쿼리 세트(시간 범위, 상태, IP, 경로, 시간 집계, 넓은 문자열 검색, 재구성 상세, 스트리밍 내보내기) | 재현 | benchmarks.md 5단계 항목 |
| 실행 계획과 cold/warm | 부분 | 새 프로세스 실행으로 DuckDB 버퍼 cold 조건만. EXPLAIN은 기록하지 않음 |
| 무거운 작업 직렬화와 취소 응답 | 자동 | 서비스의 heavy 잠금, `cancel_heavy`(DuckDB interrupt), 내보내기 취소 테스트 |

## 개발 검사

- `scripts/check.sh` / `check.ps1`: pnpm(타입·린트·Vitest·빌드) + cargo fmt/clippy/test. Windows에서 PowerShell 스크립트 실행은 사용자 검증.
