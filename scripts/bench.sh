#!/usr/bin/env bash
# 선행 실험 벤치마크. 합성 로그 생성 → 가져오기 → 조회 세트. 결과는 stdout에 JSON 라인으로 남긴다.
# 사용: scripts/bench.sh <lines> [gzip]   예) scripts/bench.sh 6000000 gzip
set -euo pipefail
cd "$(dirname "$0")/.."
LINES="${1:-1000000}"
GZ="${2:-}"
mkdir -p bench-data
cargo build --release --locked >/dev/null
B=./target/release/weblog
NAME="bench_${LINES}"
LOG="bench-data/${NAME}.log"
[ -n "$GZ" ] && LOG="${LOG}.gz"
DB="bench-data/${NAME}${GZ:+_gz}.duckdb"
rm -f "$DB" "$DB.wal"

if [ ! -f "$LOG" ]; then
  echo "== gen $LOG"
  $B gen --format combined --lines "$LINES" --seed 7 --unique-ips 200000 --unique-paths 50000 ${GZ:+--gzip} --out "$LOG"
fi

echo "== import"
if [[ "$(uname)" == "Darwin" ]]; then
  /usr/bin/time -l $B import --db "$DB" --format apache_combined "$LOG" 2> "bench-data/${NAME}${GZ:+_gz}.time.txt"
  grep -E 'maximum resident|real' "bench-data/${NAME}${GZ:+_gz}.time.txt" || true
else
  $B import --db "$DB" --format apache_combined "$LOG"
fi

echo "== query set (cold: 새 프로세스)"
$B query --db "$DB" --page-size 200 --pages 3
$B query --db "$DB" --page-size 200 --pages 3 --desc
$B query --db "$DB" --status 500 --page-size 200 --pages 3 --count
$B query --db "$DB" --status-class 4 --page-size 200 --pages 3
$B query --db "$DB" --from-micros 1704070800000000 --to-micros 1704074400000000 --page-size 200 --pages 3 --count
$B query --db "$DB" --target-contains "/api/v1/users/8" --page-size 200 --pages 3 --count
$B stats --db "$DB"
