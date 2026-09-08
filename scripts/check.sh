#!/usr/bin/env bash
# 로컬·CI 공통 검사 진입점. 프런트엔드 검사(빌드 포함)를 먼저 실행한다: Tauri crate가 dist를 필요로 한다.
set -euo pipefail
cd "$(dirname "$0")/.."

echo "== pnpm check (typecheck, lint, test, build)"
(cd apps/desktop && pnpm install --frozen-lockfile >/dev/null && pnpm check)
echo "== cargo fmt --check"
cargo fmt --all -- --check
echo "== cargo clippy"
cargo clippy --workspace --all-targets --locked -- -D warnings
echo "== cargo test"
cargo test --workspace --locked
echo "== 모든 검사 통과"
