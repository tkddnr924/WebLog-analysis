#!/usr/bin/env bash
# 복구 흐름 재현: 가져오기 도중 프로세스를 강제 종료(kill -9) → 재개 → 중복·누락 검사 → 재파싱·전환 → 검증.
# 사용: scripts/recovery-demo.sh [lines]   (기본 2,000,000줄 ≈ 350MB)
set -euo pipefail
cd "$(dirname "$0")/.."
LINES="${1:-2000000}"
cargo build --release --locked >/dev/null
B=./target/release/weblog
mkdir -p bench-data
LOG=bench-data/recovery_${LINES}.log
DB=bench-data/recovery.duckdb
rm -f "$DB" "$DB.wal"
[ -f "$LOG" ] || $B gen --format combined --lines "$LINES" --seed 21 --out "$LOG" >/dev/null

echo "== 1. 가져오기 시작 후 3초 뒤 강제 종료"
$B import --db "$DB" --format apache_combined --batch-rows 20000 --progress "$LOG" > /dev/null 2> bench-data/recovery_progress.txt &
PID=$!
sleep 3
kill -9 "$PID" || true
wait "$PID" 2>/dev/null || true
tail -1 bench-data/recovery_progress.txt

echo "== 2. 작업 상태(열 때 interrupted로 표시되어야 함)"
$B jobs --db "$DB" | python3 -c 'import sys,json; j=json.load(sys.stdin)[0]["job"]; print({k:j[k] for k in ("job_id","status","committed_records","committed_errors")})'
JOB=$($B jobs --db "$DB" | python3 -c 'import sys,json; print(json.load(sys.stdin)[0]["job"]["job_id"])')

echo "== 3. 재개"
$B resume --db "$DB" --job "$JOB" --batch-rows 20000 | python3 -c 'import sys,json; s=json.load(sys.stdin)["summary"]; print({k:s[k] for k in ("status","resumed","records","errors","skipped")}); print("resumed_from_line:", s["sources"][0]["resumed_from_line"])'

echo "== 4. 중복·누락 검사: (source_id, line_number) 고유 여부와 원본 줄 수 비교"
$B stats --db "$DB" | python3 -c 'import sys,json; print(json.load(sys.stdin)["tables"])'
python3 - "$DB" "$LOG" <<'PY'
import sys, subprocess, json
db, log = sys.argv[1], sys.argv[2]
total_lines = sum(1 for _ in open(log, 'rb'))
out = subprocess.run(["./target/release/weblog", "stats", "--db", db], capture_output=True, text=True, check=True).stdout
t = json.loads(out)["tables"]
print("원본 줄 수:", total_lines, " 저장 레코드+오류+제외:", end=" ")
jobs = subprocess.run(["./target/release/weblog", "jobs", "--db", db], capture_output=True, text=True, check=True).stdout
j = json.loads(jobs)[0]["job"]
s = j["committed_records"] + j["committed_errors"] + j["committed_skipped"]
print(s, "OK" if s == total_lines else "MISMATCH")
PY

echo "== 5. 같은 파일을 편집된 프로필(nginx_combined: 서버 힌트만 다름)로 재파싱 → 비활성 버전 → 활성화 전환"
$B import --db "$DB" --format nginx_combined --replaces-job "$JOB" "$LOG" | python3 -c 'import sys,json; s=json.load(sys.stdin)["summary"]; print({k:s[k] for k in ("job_id","status","records","errors")})'
NEW=$($B jobs --db "$DB" | python3 -c 'import sys,json; print(json.load(sys.stdin)[0]["job"]["job_id"])')
$B activate --db "$DB" --job "$NEW" | python3 -c 'import sys,json; d=json.load(sys.stdin); print("activated", d["activated"], "active=", d["job"]["active"])'
$B jobs --db "$DB" | python3 -c 'import sys,json; print([(j["job"]["job_id"], j["job"]["status"], j["job"]["active"]) for j in json.load(sys.stdin)])'

echo "== 6. 파일 검증(빠른 검사 → 전체 해시)"
$B verify --db "$DB" --source 1 --full | python3 -c 'import sys,json; d=json.load(sys.stdin); print({k:d[k] for k in ("matches","reason","full_checked")})'
echo "== 7. 이전 결과 삭제"
$B delete-results --db "$DB" --job "$JOB"
$B stats --db "$DB" | python3 -c 'import sys,json; print(json.load(sys.stdin)["tables"])'
echo "== done"
