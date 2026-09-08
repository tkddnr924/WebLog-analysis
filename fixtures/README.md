# fixtures

합성 로그와 예상 결과. 실제 서비스 로그를 넣지 않는다.

| 파일 | 프로필 | 확인 항목 |
|---|---|---|
| apache_combined.log | `apache_combined` 프리셋 | IPv6, 쿼리 문자열, 인용 이스케이프, 누락값, 깨진 요청문, 빈 줄, 잘못된 IP/상태/날짜, 다른 줄의 동일 로그 |
| apache_combined_bom_crlf.log | `apache_combined` | BOM, CRLF |
| nginx_common.log | `common` | Common 형식, remote_user, 추가 필드로 인한 불일치 |
| iis_w3c.log | `iis_w3c` | 지시문 제외, 표준 필드 매핑, 헤더 변경, 필드 수 불일치, IPv6 |
| iis_w3c_noheader.log | `iis_w3c` | 헤더 이전 데이터 오류 |
| custom_pipe.log + custom_pipe.profile.json | 사용자 정의 | 파이프 구분자, 고정 시간대 정책, 입력 오프셋 우선, 선택 그룹 + 정규식 블록 |

`*.expected.json`은 줄별 `LineOutcome`의 직렬화 결과다. 손으로 계산해 작성했으며 파서 출력으로 자동 생성하지 않았다.
