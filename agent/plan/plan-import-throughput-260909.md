# PLAN · 가져오기 처리량 개선 (260909)

## 기준값 (같은 Mac, release, `bench-data/combined_1g.log` 600만 줄 / 1,015MiB)

`weblog import` 1회: 전체 23.82s · 파싱 9.69s(41%) · 커밋 13.46s(56%) · 252k줄/초 · 피크 RSS 985MiB.
`docs/benchmarks.md:43`의 관찰(커밋이 병목, 파이프라인이 순차)과 일치한다.

## 목표와 범위

1. 파싱 경로의 줄당 할당 제거(정규식 캡처 이름 조회·`format!` 제거). 파싱 41% 구간을 줄인다.
2. 파싱과 커밋을 겹친다(파서 스레드 → 제한 큐 1개 → 쓰기 스레드). 순차 합계 대신 max(파싱, 커밋)에 가까워진다.
3. 동작·저장 결과는 바꾸지 않는다. 기존 테스트(엔진 130 · fixtures 4 · recovery 13)가 계약이다.

비범위: 파일 여러 개를 동시에 파싱(파일 간 병렬), `LogRecord`의 필드별 `String` 할당 제거, DuckDB 설정 변경.

- [x] 파이프라인의 새 동작을 기존 계약 테스트로 확인한다.
  근거: 새 동작은 "취소·강제 종료 시점이 배치 1~2개 뒤로 밀린다"뿐이고 계약(배치 경계 유지·중복 없음·재개 지점)은 그대로다. 그래서 새 테스트를 만들지 않고 `tests/recovery.rs`의 "정확히 N행" 단언을 배치 경계·범위 단언으로 바꿨다(`recovery.rs:135-152, 166-179, 275-279, 401-415, 482-493`). 입력이 너무 작으면 파서가 첫 커밋보다 먼저 파일을 끝내 취소가 늦게 도착하므로 해당 4개 테스트의 입력을 2,000줄로 키웠다. 쓰기 스레드 오류 전파는 `importer.rs`의 기존 테스트(`error_limit_aborts_job_as_failed_and_keeps_committed_batches`, `file_appended_during_import_aborts_before_committing_the_batch`)가 그대로 검증한다.
- [x] 1번: `CompiledBlocks`가 캡처 그룹 번호를 미리 계산하고 `captures_read`로 재사용 버퍼에 매칭한다.
  근거: `format/compile.rs:17-122`(`field_groups`·`extra_groups`에 그룹 번호 저장, `match_buf`/`match_line`/`field_value`/`extras`, `MatchBuf`), `parse/blocks.rs:8-71`(파서가 `MatchBuf`를 보유). 줄당 `format!("f{i}")`·이름 조회·`Captures`·벡터 2개 할당이 사라졌다.
- [x] 1번 측정: 23.82s → 19.65s, 파싱 9.69s → 6.37s(-34%), 252k → 305k줄/초. 레코드·오류·제외 건수 동일.
- [x] 2번: `import_source`를 파서 스레드 + 쓰기 스레드로 나눈다.
  근거: `importer.rs:306-510`. `std::thread::scope`로 쓰기 스레드가 `&mut Store`를 잠시 소유하고, 용량 1의 `sync_channel`로 배치를 넘기며 `CommitReport` 채널로 확정 결과를 돌려받는다. 진행 통지·집계는 파서 스레드가 보고를 받아 처리하므로 콜백은 여전히 호출자 스레드에서 실행된다. 커밋 오류는 join 결과로, 파싱 오류는 `parse_error`로 전파한다. 취소 시 큐에 남은 배치는 커밋하고 확정 배치를 보존한다.
- [x] 2번 측정: 19.65s → 15.35s(391k줄/초), 피크 RSS 985MiB → 481MiB.
- [x] 추가: 배치 기본값을 100,000행 / 48MiB로 올렸다(`importer.rs:37-38`, CLI `--batch-rows`/`--batch-bytes` 기본값도 맞췄다).
  근거: 파이프라인 적용 후 배치별 측정(memory_limit 1GB) 50,000행 14.92s/341MiB, 100,000행 13.36s/477MiB, 200,000행 10.74s/1,309MiB. 200,000행은 가장 빠르지만 피크 RSS가 개선 전보다 커져 기본값으로 삼지 않았다. 최종 기본 설정 측정: 플레인 12.28s(489k줄/초, RSS 640MiB), gzip 12.35s.
- [x] 검사와 결과 동일성 확인.
  근거: `cargo test --workspace --locked` 156개 통과(연속 2회 실행해 파이프라인 타이밍 흔들림 없음 확인), `cargo clippy --workspace --all-targets --locked -- -D warnings` 경고 0, `cargo fmt --all -- --check` 통과. 프런트엔드 파일은 변경이 없어 재검사하지 않았다. 저장 결과 동일성은 개선 전 DB와 개선 후 DB(플레인·gzip)의 `weblog analyze --top-n 20` 출력 완전 일치로 확인했다(건수·상태·메서드 분포·상위 20 IP·경로·시간축).
- [x] 위키 갱신: `docs/benchmarks.md:51-73`(개선 전후 표·변경 내용·배치별 비교·남은 병목), `docs/verification.md:64-75`(측정 기록과 미검증 항목).
