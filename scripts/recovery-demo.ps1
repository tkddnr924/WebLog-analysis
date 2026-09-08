# 복구 흐름 재현(Windows). scripts/recovery-demo.sh와 같은 순서. 이 스크립트는 아직 Windows에서 실행 검증되지 않았다.
# 사용: powershell -ExecutionPolicy Bypass -File scripts\recovery-demo.ps1 [-Lines 2000000]
param([int64]$Lines = 2000000)
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")
cargo build --release --locked | Out-Null
$B = ".\target\release\weblog.exe"
New-Item -ItemType Directory -Force bench-data | Out-Null
$Log = "bench-data\recovery_$Lines.log"
$Db = "bench-data\recovery.duckdb"
Remove-Item -ErrorAction SilentlyContinue $Db, "$Db.wal"
if (-not (Test-Path $Log)) { & $B gen --format combined --lines $Lines --seed 21 --out $Log | Out-Null }

Write-Host "== 1. 가져오기 시작 후 3초 뒤 강제 종료"
$p = Start-Process -FilePath $B -ArgumentList "import --db $Db --format apache_combined --batch-rows 20000 --progress $Log" -PassThru -NoNewWindow -RedirectStandardError bench-data\recovery_progress.txt
Start-Sleep -Seconds 3
Stop-Process -Id $p.Id -Force
Start-Sleep -Seconds 1
Get-Content bench-data\recovery_progress.txt -Tail 1

Write-Host "== 2. 작업 상태(interrupted여야 함)"
$jobs = & $B jobs --db $Db | ConvertFrom-Json
$jobs[0].job | Select-Object job_id, status, committed_records, committed_errors | Format-List
$Job = $jobs[0].job.job_id

Write-Host "== 3. 재개"
$s = (& $B resume --db $Db --job $Job --batch-rows 20000 | ConvertFrom-Json).summary
$s | Select-Object status, resumed, records, errors, skipped | Format-List
Write-Host "resumed_from_line:" $s.sources[0].resumed_from_line

Write-Host "== 4. 중복·누락 검사"
$total = (Get-Content $Log | Measure-Object -Line).Lines
$j = ((& $B jobs --db $Db | ConvertFrom-Json)[0]).job
$sum = $j.committed_records + $j.committed_errors + $j.committed_skipped
Write-Host "원본 줄 수: $total  저장 레코드+오류+제외: $sum " ($(if ($sum -eq $total) { "OK" } else { "MISMATCH" }))

Write-Host "== 5. 재파싱(nginx_combined) → 활성화 전환"
& $B import --db $Db --format nginx_combined --replaces-job $Job $Log | Out-Null
$New = ((& $B jobs --db $Db | ConvertFrom-Json)[0]).job.job_id
& $B activate --db $Db --job $New | Out-Null
(& $B jobs --db $Db | ConvertFrom-Json) | ForEach-Object { "{0} {1} active={2}" -f $_.job.job_id, $_.job.status, $_.job.active }

Write-Host "== 6. 파일 검증(전체 해시)"
& $B verify --db $Db --source 1 --full
Write-Host "== 7. 이전 결과 삭제"
& $B delete-results --db $Db --job $Job
& $B stats --db $Db
Write-Host "== done"
