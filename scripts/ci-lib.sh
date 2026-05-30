#!/usr/bin/env bash
# Shared helpers for local CI scripts. Source this from each ci-*.sh.

set -euo pipefail

# Find the repo root from the script that sourced us (works regardless of cwd).
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[1]:-$0}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Print a section banner.
section() {
    printf '\n\033[1;34m==> %s\033[0m\n' "$*"
}

# Print success status.
ok() {
    printf '\033[1;32m✓\033[0m %s\n' "$*"
}

# Print informational message.
info() {
    printf '\033[1;36mi\033[0m %s\n' "$*"
}

# Print a soft warning (does not exit).
warn() {
    printf '\033[1;33m!\033[0m %s\n' "$*" 1>&2
}

# Print a hard error and exit.
die() {
    printf '\033[1;31m✗\033[0m %s\n' "$*" 1>&2
    exit 1
}

# Run a command; if it fails, surface a clear error.
run() {
    printf '\033[2m$ %s\033[0m\n' "$*"
    "$@"
}

# Run a tool through `mise exec` when mise is on PATH, so we get the same
# pinned toolchain CI users have. Falls back to a bare exec otherwise so
# people without mise are not blocked.
mexec() {
    if command -v mise >/dev/null 2>&1; then
        mise exec -- "$@"
    else
        "$@"
    fi
}

# Require that a command is available. Mise-aware: if `mise` is on PATH
# but the tool isn't, ask mise whether the tool resolves through its
# shim layer — that catches the common case where the user invoked the
# script directly (no `mise exec`) but mise can still find the pinned
# binary. Falls back to a hard error with one actionable hint.
# Compute a path relative to the current working directory in a
# portable way (Linux GNU coreutils ships `realpath --relative-to=.`
# but macOS / BSD realpath does NOT). Falls back to a pure-Bash
# substring trim if the absolute path begins with `$PWD/`, otherwise
# returns the absolute path unchanged.
relpath_to_cwd() {
    local abs="$1"
    # Resolve to absolute first if input is relative.
    case "$abs" in
        /*) ;;
        *) abs="$(cd "$(dirname "$abs")" 2>/dev/null && pwd)/$(basename "$abs")" ;;
    esac
    local cwd="$PWD"
    case "$abs" in
        "$cwd"/*) printf '%s\n' "./${abs#"$cwd"/}" ;;
        "$cwd") printf '.\n' ;;
        *) printf '%s\n' "$abs" ;;
    esac
}

require() {
    local cmd="$1"
    local hint="${2:-}"
    if command -v "$cmd" >/dev/null 2>&1; then
        return 0
    fi
    if command -v mise >/dev/null 2>&1 && mise which "$cmd" >/dev/null 2>&1; then
        # mise can resolve the tool but the user invoked the script
        # without `mise exec`. Prefer the user-invoked top-level
        # entry script (set by ci-local.sh's CI_ENTRY_SCRIPT export)
        # so the hint matches what the user actually typed; fall
        # back to the nested script if no top-level was set.
        local entry
        entry="$(relpath_to_cwd "${CI_ENTRY_SCRIPT:-${BASH_SOURCE[1]:-$0}}")"
        die "missing $cmd on PATH but mise resolves it. Rerun via: mise exec -- $entry"
    fi
    if [ -n "$hint" ]; then
        die "missing required tool: $cmd (hint: $hint)"
    else
        die "missing required tool: $cmd"
    fi
}

export REPO_ROOT
