#!/usr/bin/env bash
# Local quality gate: the same checks CI runs (.github/workflows/ci.yml), minus the Docker build.
# The pre-push hook (.githooks/pre-push) runs this; nothing is pushed unless it passes.
#
#   scripts/check.sh           full gate
#   scripts/check.sh --fast    skip Playwright (for quick local iterations; the hook never uses it)
#   scripts/check.sh --live    also run the real-network checks (Yahoo) before a deploy
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
fast=false
live=false
for arg in "$@"; do
  case "$arg" in
    --fast) fast=true ;;
    --live) live=true ;;
    *) echo "unknown option: $arg" >&2; exit 2 ;;
  esac
done

step() { printf '\n\033[1;34m==> %s\033[0m\n' "$*"; }

step "Rust format"
cargo fmt --all -- --check
step "Rust clippy (warnings are errors)"
cargo clippy --workspace --all-targets --all-features -- -D warnings
step "Rust tests"
cargo test --workspace --all-features
if command -v cargo-audit >/dev/null 2>&1; then
  step "Dependency audit"
  cargo audit --deny warnings
else
  echo "cargo-audit not installed; CI still runs it (cargo install cargo-audit to run locally)"
fi

# The desktop app (GPUI) is its own workspace with its own toolchain (rust-toolchain.toml).
# GPUI needs a display stack: this runs on macOS here, and CI runs it on a macOS runner.
if [ "$(uname)" = Darwin ]; then
  step "Desktop app (GPUI): format, clippy, tests"
  (cd apps/desktop && cargo fmt -- --check && cargo clippy --all-targets -- -D warnings && cargo test)
fi

cd apps/web
[ -d node_modules ] || { step "Install web dependencies"; npm ci; }
step "Web lint (oxlint, warnings are errors)"
npm run lint
step "Web typecheck + build"
npm run build
step "Web unit tests"
npm run test
if [ "$fast" = false ]; then
  step "Browser, accessibility and performance (Playwright, mocked market data)"
  npx playwright test
fi
if [ "$live" = true ]; then
  step "Live: Rust provider + stream against real Yahoo"
  (cd ../.. && cargo test --workspace -- --ignored live)
  step "Live: browser against the real API"
  npm run test:e2e:live
fi

printf '\n\033[1;32mAll checks passed.\033[0m\n'
