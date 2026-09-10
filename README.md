# WebLog-analysis

대용량 웹 서버 접근 로그(Apache/Nginx, IIS W3C, 커스텀 단일행)를 파싱해 DuckDB에 저장하고 조회하는 데스크톱 도구. 5단계까지 구현되어 있다(파서·저장·조회 엔진, 탐색·판별·복구, Tauri/React 화면, 퍼즐식 포맷 확인, 통계·저장된 뷰·스트리밍 내보내기). 시나리오별 검증 상태는 [docs/verification.md](docs/verification.md)에 있다. 설계·규칙은 [CLAUDE.md](CLAUDE.md), [AGENTS.md](AGENTS.md), [프로젝트 위키](docs/README.md)를 따른다.

## 구성

```
crates/weblog-engine   파서·리더·저장소·조회 엔진 (Tauri 의존 없음)
crates/weblog-service  애플리케이션 서비스: 프로젝트, 백그라운드 가져오기, 취소, 진행 이벤트, 읽기 연결 풀 (Tauri 의존 없음)
crates/weblog-cli      실험 CLI: gen / scan / preview / import / resume / query / detail / jobs / activate / delete-results / verify / stats
apps/desktop           Tauri 2 + React/TypeScript 앱. src-tauri/src/lib.rs는 조립·명령 등록만 담당
fixtures/              합성 로그와 손으로 작성한 예상 결과
scripts/check.sh|ps1   로컬·CI 공통 검사 진입점
scripts/bench.sh, bench-run.sh   벤치마크
scripts/recovery-demo.sh|ps1     강제 종료 → 재개 → 중복·누락 검사 → 재파싱 전환 재현
docs/benchmarks.md     측정 결과와 미검증 범위
docs/decisions/        세부 설계 결정과 대안
```

엔진 모듈: `format`(정의 모델·프리셋·블록→정규식 컴파일) → `parse`(블록 파서, W3C 파서, 의미 검증) / `source`(스트리밍 gzip, 줄 길이 상한, 오프셋, 재귀 탐색, 파일 식별) → `store`(마이그레이션, 배치 트랜잭션, 커서 페이지 조회, 작업·결과 버전, 읽기 연결) ← `importer`(파이프라인, 취소, 재개, 변경 감지) / `preview` / `detect`(샘플 판별·그룹화).

## 요구 사항

- Rust stable(개발 시 1.97.1), `rustfmt`, `clippy`. `rust-toolchain.toml`이 채널을 고정한다.
- Node 22 이상과 pnpm(개발 시 Node 26.7, pnpm 11). `apps/desktop`에서 `pnpm install`.
- Windows에서 Tauri 빌드: Visual Studio C++ 빌드 도구와 WebView2 런타임(Windows 10/11에는 보통 설치되어 있음).
- DuckDB는 `duckdb` crate의 `bundled` 기능으로 소스 빌드된다. 첫 빌드는 수 분(M1 기준 약 2분 20초) 걸린다.
- Windows x64 빌드는 MSVC 도구체인이 필요하다. 이 저장소는 아직 Windows에서 검증되지 않았다(아래 "미검증" 참조).

## 검사

```bash
scripts/check.sh
```

```powershell
scripts\check.ps1
```

내용: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets --locked -- -D warnings`, `cargo test --workspace --locked`, 그리고 `apps/desktop`에서 `pnpm check`(타입 검사, ESLint, Vitest, 프로덕션 빌드). Tauri crate는 프런트엔드 `dist`가 있어야 컴파일되므로 `pnpm build`가 먼저 실행된다.

## 데스크톱 앱

```bash
cd apps/desktop
pnpm install
pnpm tauri dev        # 개발 실행(Vite + Rust 디버그 빌드)
pnpm tauri build      # 배포 번들(Windows: NSIS/MSI, macOS: DMG)
```

화면은 세 화면으로 진행한다. 결과 화면에서 실제 분석 작업을 한다.
1. 시작: 서버 종류(Apache/Nginx/IIS/모름)를 고르고 "폴더 선택"을 누르면 접근 로그 패턴과 에러 로그 패턴으로 동시에 탐색한다(패턴은 패널 아래에서 바꿀 수 있다). 찾은 종류만 "Access Log N / Error Log N" 탭으로 나온다. 접근 로그는 파일마다 선두 200줄로 포맷을 판별하고, 첫 파일의 첫 레코드 한 줄을 조각(공백 기준, "…"·[…]는 한 조각, 날짜+시각은 한 조각)으로 나눠 색으로 하이라이트한 뒤 조각마다 라벨을 자동으로 붙인다. 라벨 어휘는 서버·로그 종류별 실제 변수(Nginx `$remote_addr`…, Apache `%h`…, 에러 로그의 시각·레벨·프로세스·메시지)이며 사람이 읽는 이름으로 보이고 변수명은 툴팁에 있다. 아래 팔레트("기본", 접힌 "그 외")의 라벨을 조각에 끌어 놓거나 조각끼리 끌어 놓아 바꾸면 즉시 다시 파싱해 "선두 N줄 모두 파싱됨/오류 N"을 보여준다. 에러 로그는 메시지 뒤의 client/server/request/upstream/host/referrer(Nginx) 또는 referer(Apache)를 정규식 블록으로 나눠 저장하고, 시간대가 없어 한국 시간(UTC+9)으로 간주한다. 화면의 모든 시각 표시와 입력은 한국 시간으로 고정되며 저장은 UTC다(통계 일 단위 버킷도 한국 시간 자정 기준). "파싱 시작"(상단 요약 막대)은 접근 로그와 에러 로그를 차례로 파싱하며, 열린 프로젝트가 없을 때 실행 파일 옆 `cases/<로그 폴더 이름>-<시각>.duckdb`를 자동으로 만들어 열고 바로 가져오기를 시작한다. 저장 위치를 묻지 않는다.
2. 파싱 중: 진행(읽은 줄·확정 레코드·배치)과 취소. 끝나면 "결과 보기".
3. 결과: 최상위에서 `접근 로그` / `에러 로그`를 고르고, 각 종류 안에 왼쪽 "룰" 사이드바 + `조회`·`통계` 탭이 있다. 룰 목록은 종류별로 다르며(접근 룰은 상태·메서드·경로·UA, 에러 룰은 레벨·메시지·IP) 사용자 룰은 저장할 때의 종류에 붙는다. 룰은 YARA풍 텍스트로 쓴다(`src/lib/yara.ts` 파서, 테스트 있음).
   ```
   rule sql_injection_success {
     meta:
       name = "SQL Injection · 성공 응답"
       description = "서명이 있는데 2xx로 응답한 요청"
     strings:
       $union = /union[\s+]+select/ nocase
       $quote = "%27"
     condition:
       status in 200..299 and any of them
   }
   ```
   접근 룰 필드는 status·bytes·method·ip·path·protocol·referrer·ua, 에러 룰 필드는 level·message(msg)·ip다(에러 필드는 문자열 연산만). 연산은 `== != > >= < <=`, `in a..b`, `contains/icontains/startswith/endswith/matches`, `is null`, `and/or/not`, `any of them`, `all of ($a*)`. `$id`만 쓰면 접근 룰은 path, 에러 룰은 message에 적용된다. 파서는 엔진의 조건식(`LogFilter.expr`: and/or/not/cond 트리, RE2 정규식은 바인딩 전 검증)으로 컴파일하며, 에러 룰의 level·message는 `extra_json`에서 꺼낸다. 접근 기본 룰 15개(북마크, 전체 요청, 정상 응답, SQL Injection, XSS, 경로 탐색, 명령 주입, 취약점 스캐너 경로, SQL Injection·성공 응답, sqlmap, 스캐너 도구 UA, Log4Shell, 파일 포함·SSRF, 웹셸 업로드·실행, 봇·스크립트 UA)와 에러 기본 룰 12개(북마크, 전체 에러, 심각(crit·alert·emerg), 오류(error), 경고(warn), 파일 없음, 권한 거부, 업스트림 연결 실패, 요청 본문 초과, SSL 핸드쉐이크 오류, PHP·FastCGI 실패, 디렉터리 색인 금지)는 내장 원문이며 ✎로 열어 보고 복사본으로 저장할 수 있다. "+"는 큰 편집기(줄 번호, 실시간 파싱 오류 위치, 문법 안내)를 열고, 사용자 룰은 저장된 뷰에 원문과 함께 저장된다.
   - 접근 로그 · 조회: 룰 조건 + 기간(한국 시간)·빠른 검색(경로·IP·리퍼러·UA)·정렬. 가상 스크롤 표, 커서 기반 추가 로드, UI 캐시 8MiB 상한. 행을 고르면 구조화 필드와 "재구성 로그"를 오른쪽에 보여준다. 현재 조건은 룰로 저장하거나 CSV/JSON Lines로 스트리밍 내보낼 수 있다(백그라운드, 취소 가능).
   - 접근 로그 · 통계: 시간별 요청 수, 상태코드·메서드 분포, 상위 N IP(최초·마지막 탐지 포함)·요청 대상.
   - 에러 로그 · 조회: 컬럼이 시간·레벨·클라이언트·메시지·출처다. 레벨과 메시지는 파싱 정의가 `extra_json`에 남긴 값에서 꺼내며 서버마다 키가 달라도(`level`, `client`, `error_code`, `message`) 화면에서 흡수한다. 룰 조건 + 기간·검색·정렬을 쓰고, 검색어는 메시지(대소문자 무시)나 클라이언트 IP 중 하나만 맞아도 걸린다.
   - 에러 로그 · 통계: 레벨 분포, 상위 메시지, 상위 N 클라이언트 IP, 시간축. 상태코드·메서드·요청 대상은 에러 로그에 없으므로 계산하지 않는다.
   - 두 종류 모두 전체를 훑는 통계·건수는 무거운 조회라 실행 중 중단할 수 있다. 구분 기준은 가져오기 작업에 기록한 `log_kind`다.
   - 북마크: 별은 파일·줄로만 저장하므로 두 종류가 같은 표를 쓴다. 어느 쪽에서 달아도 같은 저장소에 남고, 각 종류의 "북마크" 룰은 자기 종류의 행만 보여준다.

포맷 확인용 원문 샘플(`sample_lines` 명령, 최대 20줄·256KiB)은 화면에만 잠시 보여주며 저장하지 않는다.

상한: 탐색은 파일 항목과 오류 목록에 각각 같은 상한(`max_entries`, 기본 10만)을 쓰고, 넘으면 `truncated`·`errors_truncated`로 알린다(오류 상한에 걸려도 탐색은 계속한다). 사용자 프리셋 YAML은 읽기 전에 파일 크기를 256KiB 상한과 비교해 거부한다.

IPC 규약: 명령 인자와 구조체 필드는 모두 snake_case로 고정한다(`#[tauri::command(rename_all = "snake_case")]`, `#[serde(rename_all = "snake_case")]`). 진행 이벤트는 `weblog://import`로 250ms 이상 간격으로 보내고, UI는 2초 폴링으로 이벤트 유실을 보완한다. 권한은 `src-tauri/capabilities/default.json`에서 핵심 IPC, 이벤트 수신, 파일/폴더 선택 대화상자만 허용한다.

## CLI 사용

```bash
cargo build --release --locked
B=./target/release/weblog

# 합성 로그 생성 (combined | common | w3c | custom-pipe, --gzip 가능)
$B gen --format combined --lines 1000000 --out bench-data/a.log
$B gen --format w3c --lines 100000 --gzip --out bench-data/b.log.gz

# 미리보기(선두 N줄, 같은 파서 사용, 원문은 출력하지 않음)
$B preview --format apache_combined --lines 20 bench-data/a.log

# 가져오기 (프리셋 이름 또는 프로필 JSON 경로)
$B import --db bench-data/a.duckdb --format apache_combined bench-data/a.log
$B import --db bench-data/a.duckdb --format iis_w3c bench-data/b.log.gz
$B import --db bench-data/a.duckdb --format fixtures/custom_pipe.profile.json some.log

# 페이지 조회 지연 측정 (커서 기반, 페이지 크기 상한 1000)
$B query --db bench-data/a.duckdb --status 500 --page-size 200 --pages 3 --count

# 상세 + 재구성 로그 (원문 복원이 아님)
$B detail --db bench-data/a.duckdb --source 1 --line 42

# 저장소 통계
$B stats --db bench-data/a.duckdb

# 경로 재귀 탐색 + 파일별 포맷 후보 판별(선두 200줄 샘플, 같은 파서 사용)
$B scan /var/log/nginx --include 'access.log*' --exclude '*.bak' --detect

# 작업 목록과 파일 상태
$B jobs --db bench-data/a.duckdb

# 중단(interrupted/cancelled/failed)된 작업 재개. 마지막 확정 배치 다음 줄부터 이어간다.
$B resume --db bench-data/a.duckdb --job 1 [--full-verify]

# 재파싱: 이전 작업을 대체하는 새 결과 버전(비활성) → 완료 후 명시적 전환 → 이전 결과 삭제
$B import --db bench-data/a.duckdb --format my.profile.json --replaces-job 1 bench-data/a.log
$B activate --db bench-data/a.duckdb --job 2
$B delete-results --db bench-data/a.duckdb --job 1

# 파일 검증(빠른: 크기+선두 64KiB 해시, --full: 전체 SHA-256). 이동한 파일은 --relink로 재연결.
$B verify --db bench-data/a.duckdb --source 1 --full [--relink /new/path/a.log]
```

`import`/`resume` 실행 중 stdin에 `q`를 입력하면 현재 배치까지 확정하고 취소한다(상태 cancelling → cancelled). 취소한 작업은 `resume`으로 이어갈 수 있다.

## 복구 동작

- 배치마다 성공 행·오류 위치·다음 읽기 오프셋·W3C 헤더 상태가 한 트랜잭션으로 커밋된다.
- DB를 열 때 running/cancelling으로 남은 작업은 interrupted로 바뀐다(단일 소유 프로세스 전제).
- `resume`은 파일 식별(크기·선두 해시, 옵션으로 전체 해시)을 검증한 뒤 마지막 확정 오프셋에서 이어간다. 일반 파일은 seek, gzip은 처음부터 재생하며 건너뛴다.
- 가져오는 도중 파일 크기·수정 시각이 바뀌면 그 배치를 커밋하지 않고 `SourceChanged`로 실패한다(확정 배치는 보존, 재개 가능).
- 재현: `scripts/recovery-demo.sh` (macOS/Linux, `kill -9` 사용) 또는 `scripts/recovery-demo.ps1` (Windows, `Stop-Process -Force`; 아직 Windows에서 실행 검증되지 않음).

프리셋: `common`, `combined`, `apache_combined`, `nginx_combined`, `iis_w3c`. `--format`에는 프리셋 이름, JSON 파일, YAML 파일을 줄 수 있다. 커스텀 프로필 예시는 [fixtures/custom_pipe.profile.yaml](fixtures/custom_pipe.profile.yaml)(같은 정의의 JSON도 있음). `weblog profile validate <spec>` / `weblog profile to-yaml <spec>`로 검증·변환한다.

## 설치와 실행 요약

| 항목 | 위치·명령 |
|---|---|
| 설치(개발) | Rust stable + Node/pnpm 설치 후 `apps/desktop`에서 `pnpm install` |
| 실행(개발) | `apps/desktop`에서 `pnpm tauri dev` |
| 배포 번들 | `apps/desktop`에서 `pnpm tauri build` → Windows NSIS/MSI, macOS DMG (Windows 번들은 아직 만들어 보지 않음) |
| 검사 | `scripts/check.sh` 또는 `scripts\check.ps1` |
| 프로젝트 DB | 사용자가 고른 `.duckdb` 파일 하나(+ 잠시 `.wal`). 원본 로그와 같은 볼륨에 두지 않아도 된다 |
| 케이스 DB | 실행 파일 옆 `cases/<로그 폴더 이름>-<YYYYMMDD-HHMMSS>.duckdb` |
| 실행 로그 | `cases/logs/weblog.log`(4MiB를 넘으면 `weblog.prev.log`로 밀어냄). 문제를 보고할 때 이 파일을 첨부한다 |
| 사용자 프리셋 | `cases/presets/<이름>.yaml`(저장할 때만 생긴다) |
| 내보내기 | 사용자가 고른 경로. 부분 결과(취소)도 파일로 남고 화면에 표시된다 |
| 복구 | 앱을 다시 열면 중단된 작업을 알림으로 알려 준다. 재개·활성화·결과 삭제·소스 검증은 CLI(`weblog resume` / `jobs` / `activate` / `delete-results` / `verify`)로 한다 |

앱이 만드는 파일은 실행 파일 옆 `cases` 폴더 하나뿐이다. WebView2 캐시만 임시 폴더(`%TEMP%\weblog-webview`)에 두고 종료할 때 지운다. 창을 코드에서 만들며 이 경로를 지정하므로 `%LOCALAPPDATA%`에는 아무것도 만들지 않는다(지정하지 않으면 Tauri가 `%LOCALAPPDATA%\<identifier>`를 강제한다). 레지스트리도 쓰지 않는다.

실행 파일이 있는 폴더에 쓸 수 없으면(예: Program Files) 임시 폴더 아래 `weblog/`를 대신 쓰고 그 사실을 로그에 남긴다. 쓰기 가능한 폴더에 두고 실행하는 것을 권한다.

## 저장 위치와 보존 정책

- DB는 `--db`로 지정한 단일 DuckDB 파일이다. 같은 이름의 `.wal`이 잠시 생길 수 있다.
- **원문은 저장하지 않는다.** `logs`에는 파싱 필드와 `source_id`/`line_number`만 있고, 실패 행도 `parse_errors`에 위치와 오류 코드만 남는다. 스키마 검사 테스트가 raw 컬럼 부재를 확인한다.
- 상세 화면의 "재구성 로그"는 저장 필드로 만든 텍스트이며 원본과 바이트 단위로 같지 않다.
- 같은 파일을 다시 가져오면 새 `result_version`의 작업이 생긴다. 같은 작업 안에서 같은 배치를 재시도해도 중복이 생기지 않는다.
- 재파싱은 `replaces_job_id`를 가진 새 작업이며 비활성으로 시작한다. `activate`로 전환하기 전까지 `active_only` 조회는 이전 결과를 본다. 활성·실행 중 작업은 삭제할 수 없다.
- 작업 하나는 프로필 하나를 쓴다. 포맷이 다른 파일은 `scan --detect`의 그룹마다 작업을 만든다.

## 미검증

- Windows x64 빌드·실행·번들(`pnpm tauri dev/build`), 한글/긴 경로, HDD 환경 측정, `recovery-demo.ps1` 실행(사용자 검증 대상). 앱은 macOS dev 모드로만 실행해 보았다.
- 실제 디스크 부족 상황, 잘린 gzip 입력, 원본이 없는 상태의 재파싱 실패 메시지(`docs/verification.md` 참조).
- 압축 후 100GB 규모. 현재 측정은 [docs/benchmarks.md](docs/benchmarks.md) 참조.
- 추가 기록 중인 파일의 이어읽기(최초 범위 밖).
