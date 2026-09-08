#!/usr/bin/env bash
# 1GB(플레인/gzip) + 10GB 플레인 가져오기와 조회 세트를 순차 실행하고 결과를 bench-data/results.txt에 남긴다.
set -uo pipefail
cd "$(dirname "$0")/.."
B=./target/release/weblog
OUT=bench-data/results.txt
: > "$OUT"
log() { echo "$@" | tee -a "$OUT"; }
run_q() { local db=$1; shift; log "--- query $*"; $B query --db "$db" "$@" | python3 -c 'import sys,json; d=json.load(sys.stdin); print({k:d.get(k) for k in ("page_latency_ms","rows_returned","count","count_latency_ms","has_more")})' | tee -a "$OUT"; }
imp() { local db=$1 file=$2 tag=$3; shift 3; rm -f "$db" "$db.wal"; log "=== import $tag: $file $*"; /usr/bin/time -l $B import --db "$db" --format apache_combined "$@" "$file" 2> "bench-data/$tag.time.txt" | tee -a "$OUT"; grep -E 'real|maximum resident' "bench-data/$tag.time.txt" | tee -a "$OUT"; }

log "=== gen"
$B gen --format combined --lines 6000000 --seed 7 --unique-ips 200000 --unique-paths 50000 --out bench-data/combined_1g.log | tee -a "$OUT"
$B gen --format combined --lines 6000000 --seed 7 --unique-ips 200000 --unique-paths 50000 --gzip --out bench-data/combined_1g.log.gz | tee -a "$OUT"
$B gen --format combined --lines 60000000 --seed 11 --unique-ips 1000000 --unique-paths 200000 --out bench-data/combined_10g.log | tee -a "$OUT"

imp bench-data/plain.duckdb bench-data/combined_1g.log plain_1g
imp bench-data/gz.duckdb bench-data/combined_1g.log.gz gz_1g
imp bench-data/limited2.duckdb bench-data/combined_1g.log limited_1g --memory-limit 512MB --threads 2 --batch-rows 20000
imp bench-data/plain10.duckdb bench-data/combined_10g.log plain_10g --memory-limit 2GB --threads 4

for db in bench-data/plain.duckdb bench-data/plain10.duckdb; do
  log "=== query set: $db (각 줄 새 프로세스)"
  run_q "$db" --page-size 200 --pages 3
  run_q "$db" --page-size 200 --pages 3 --desc
  run_q "$db" --status 500 --page-size 200 --pages 3 --count
  run_q "$db" --status-class 4 --page-size 200 --pages 3
  run_q "$db" --from-micros 1704070800000000 --to-micros 1704074400000000 --page-size 200 --pages 3 --count
  run_q "$db" --target-contains /api/v1/users/8 --page-size 200 --pages 3 --count
  run_q "$db" --ip 91.142.229.39 --page-size 200 --pages 1 --count
  run_q "$db" --method POST --status 404 --page-size 200 --pages 3 --count
  run_q "$db" --page-size 1000 --pages 5
  run_q "$db" --memory-limit 512MB --threads 2 --page-size 200 --pages 3 --desc
  run_q "$db" --memory-limit 512MB --threads 2 --target-contains /api/v1/users/8 --page-size 200 --pages 3 --count
  log "--- stats"; $B stats --db "$db" | tee -a "$OUT"
done
log "=== done"
