# 0001. 1~2단계 엔진·복구 설계 결정

상태: 채택(2026-09-07). 기술 스택(Rust/Tauri 2/React/DuckDB)은 이전에 확정되었으며 여기서는 그 안에서 내린 세부 결정만 기록한다.

## 1. 저장 스키마의 시간·키

- `logs.timestamp_utc`는 DuckDB `TIMESTAMP`(마이크로초). Appender에는 `Value::Timestamp`, 조회는 `epoch_us()`, 바인딩은 `make_timestamp(?)`로 통일했다. 시간 범위 zonemap 건너뛰기를 그대로 쓰기 위해 BIGINT 대신 TIMESTAMP를 골랐다.
- 레코드 고유 키는 `(job_id, source_id, line_number)`. PK 제약(ART 인덱스)은 만들지 않았다. 100GB에서 인덱스 메모리·쓰기 비용이 크고, 커서 페이징은 정렬 컬럼 zonemap만으로 10GB까지 수십 ms였다.
- 대안: 전역 자동 증가 키. 재개·재파싱 시 안정성이 떨어져 채택하지 않았다.

## 2. 배치 커밋과 Appender

- 배치마다 `BEGIN` → Appender(logs) → Appender(parse_errors) → `import_batches` INSERT → `import_jobs` 집계 UPDATE → `COMMIT`. 장애 주입 테스트로 Appender 행이 ROLLBACK에 함께 되돌아감을 확인했다(`store/batch.rs`).
- 배치 식별자는 `(job_id, source_id, batch_seq)` UNIQUE. 같은 배치 재커밋은 `AlreadyCommitted`로 건너뛴다.
- 배치 상한은 행 수(기본 50,000)와 바이트(32MiB) 둘 다. 병목은 커밋(전체의 약 60%)이며 파서/쓰기 스레드 분리는 3단계 이전 과제로 남긴다.

## 3. 마이그레이션은 문장 단위 autocommit

DuckDB는 한 트랜잭션에서 같은 테이블을 두 번 `ALTER`하면 커밋 시 "another transaction has altered this table"로 실패한다(v2 마이그레이션에서 재현). 그래서 DDL은 문장 단위로 실행하고 모든 문장을 `IF NOT EXISTS`로 멱등하게 둔 뒤, 성공 후에만 `schema_migrations`에 버전을 기록한다. 부분 적용 후 재실행해도 안전하다.

## 4. 작업은 프로필 하나, 파일 여러 개

- `import_jobs.profile_id` 하나에 `import_job_sources` 여러 파일. 포맷이 다른 파일은 `detect_and_group`으로 묶어 그룹마다 작업을 만든다. 파일별 프로필을 작업 안에 섞는 것보다 재파싱·전환 단위가 단순하다.
- 대안: 작업당 파일별 프로필. 결과 버전 전환이 파일 단위로 쪼개져 UI가 복잡해지므로 보류.

## 5. 결과 버전 전환

- 재파싱은 `replaces_job_id`를 가진 새 작업이며 `active = false`로 시작한다. 완료 후 `activate_job`이 새 작업을 활성화하고 대체 대상을 비활성화한다(한 트랜잭션).
- 조회는 `LogFilter.active_only`로 활성 결과만 보거나 `job_id`로 특정 버전을 본다.
- 삭제는 `delete_job_results`로만 하며 활성·실행 중 작업은 거부한다.

## 6. 파일 식별과 변경 감지

- 등록 시 크기 + 선두 64KiB SHA-256. 재개 전 빠른 검증(크기·선두 해시)을 하고, `full_verify_on_resume`이면 전체 SHA-256을 스트리밍으로 계산해 비교한다. 전체 해시는 처음 계산할 때 `sources.full_hash`에 기록한다.
- 가져오는 도중에는 배치 커밋 직전마다 크기·수정 시각을 다시 읽어 달라지면 그 배치를 커밋하지 않고 `SourceChanged`로 실패시킨다. 최초 버전은 고정된 파일을 전제로 하며, 추가 기록 중인 파일의 이어읽기는 후속 범위다.
- 수정 시각은 재개 검증에 쓰지 않는다. 복사·이동으로 바뀌기 때문이다.

## 7. 재개 위치

- 일반 파일은 `next_offset`으로 seek. gzip은 스트림을 처음부터 재생하며 `next_offset`까지 버린다(설계에서 합의한 초기 방식). 파일 사이에서 죽으면 앞 파일은 EOF에서 재개되어 0줄을 읽고 넘어간다.
- W3C 헤더 상태(`header_state_json`)는 배치마다 저장되고 재개 시 복원된다.
- `Store::open`은 running/cancelling으로 남은 작업을 interrupted로 바꾼다. 단일 소유 프로세스이므로 열 때 그런 작업이 있다는 것은 이전 실행의 비정상 종료를 뜻한다.

## 8. 동시 조회

`Store::open_reader`가 `Connection::try_clone`으로 읽기 연결을 만든다. 같은 프로세스에서 가져오기(쓰기 연결)와 조회(읽기 연결)를 다른 스레드에서 동시에 수행할 수 있으며, 조회는 커밋된 배치만 본다. 무거운 조회의 직렬화(동시 1개)는 3단계 서비스 계층에서 세마포어로 둔다.
