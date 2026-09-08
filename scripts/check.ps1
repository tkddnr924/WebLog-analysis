# 로컬·CI 공통 검사 진입점(Windows). scripts/check.sh와 같은 순서로 실행한다.
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

Write-Host "== pnpm check (typecheck, lint, test, build)"
Push-Location apps\desktop
pnpm install --frozen-lockfile | Out-Null
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
pnpm check
if ($LASTEXITCODE -ne 0) { Pop-Location; exit $LASTEXITCODE }
Pop-Location
Write-Host "== cargo fmt --check"
cargo fmt --all -- --check
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "== cargo clippy"
cargo clippy --workspace --all-targets --locked -- -D warnings
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "== cargo test"
cargo test --workspace --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
Write-Host "== 모든 검사 통과"
