#!/usr/bin/env bash
# Stage 11.A8 — cargo-mutants mutation-testing runner for the encoder
# modules.
#
# Reads `rust/.cargo/mutants.toml` for the default examined-glob set
# (small / well-tested encoders, ~1331 mutants at the Stage-11.A8
# baseline) and writes results into a per-run output directory under
# `target/mutants/`.
#
# Usage:
#
#   ./scripts/run-mutants.sh           # default subset (~30 min @ 4 jobs)
#   ./scripts/run-mutants.sh --full    # every encoder under src/symbology/
#                                       # (~12K mutants, multi-hour run)
#
# The runner is intentionally NOT wired into `scripts/ci-local.sh` —
# cargo-mutants would dominate the CI wall-clock budget. Use it as an
# on-demand quality-bar check when materially changing an encoder's
# control flow.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$HERE/.." && pwd)"
RUST_DIR="$REPO_ROOT/rust"

if ! command -v cargo-mutants >/dev/null 2>&1; then
    echo "error: cargo-mutants not installed. Install with:" >&2
    echo "  cargo install --locked cargo-mutants" >&2
    exit 1
fi

mode="default"
case "${1:-}" in
    --full)   mode="full" ;;
    --default|"") mode="default" ;;
    *)
        echo "usage: $0 [--default | --full]" >&2
        exit 2
        ;;
esac

stamp="$(date +%Y%m%dT%H%M%S)"
out_dir="$RUST_DIR/target/mutants/$stamp"
mkdir -p "$out_dir"

cd "$RUST_DIR"

if [ "$mode" = "full" ]; then
    echo "=> running cargo-mutants over all src/symbology/*.rs files"
    echo "   output: $out_dir"
    # `--no-config` bypasses the curated subset; we want every file.
    cargo mutants \
        --no-config \
        -f 'src/symbology/**/*.rs' \
        --jobs 4 \
        --no-shuffle \
        --output "$out_dir"
else
    echo "=> running cargo-mutants over the default subset"
    echo "   (rust/.cargo/mutants.toml's examine_globs list)"
    echo "   output: $out_dir"
    cargo mutants \
        --jobs 4 \
        --no-shuffle \
        --output "$out_dir"
fi

echo ""
echo "=> done. summary:"
caught=$(wc -l < "$out_dir/mutants.out/caught.txt" 2>/dev/null || echo 0)
missed=$(wc -l < "$out_dir/mutants.out/missed.txt" 2>/dev/null || echo 0)
unviable=$(wc -l < "$out_dir/mutants.out/unviable.txt" 2>/dev/null || echo 0)
timeout=$(wc -l < "$out_dir/mutants.out/timeout.txt" 2>/dev/null || echo 0)
total_viable=$((caught + missed))
if [ "$total_viable" -gt 0 ]; then
    pct=$(awk -v c="$caught" -v t="$total_viable" 'BEGIN{printf "%.1f", 100.0*c/t}')
else
    pct="n/a"
fi

echo "   caught:   $caught"
echo "   missed:   $missed"
echo "   unviable: $unviable"
echo "   timeout:  $timeout"
echo "   killed-mutant rate (caught / viable): ${pct}%"
echo ""
echo "   survivors in: $out_dir/mutants.out/missed.txt"
