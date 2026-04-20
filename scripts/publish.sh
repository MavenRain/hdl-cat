#!/usr/bin/env bash
#
# Publish every hdl-cat workspace crate to crates.io in topological order.
#
# By default the script is a *plan*: it prints the cargo invocations it
# would run without touching the network.  Pass --execute to actually
# publish.  Use --from <crate> to resume after a partial publish.
#
# Pre-flight (`--verify`) runs the workspace verification gate from
# CLAUDE.md (clippy with -D warnings, then cargo test --all and
# cargo test --doc --all) before any publish step.
#
# Usage:
#   scripts/publish.sh                       # dry-plan (default)
#   scripts/publish.sh --verify              # dry-plan + run verification gate
#   scripts/publish.sh --execute             # publish everything
#   scripts/publish.sh --execute --verify    # verify then publish
#   scripts/publish.sh --execute --from hdl-cat-circom
#                                            # resume mid-list
#   scripts/publish.sh --execute -- --allow-dirty
#                                            # forward extra args to cargo

set -euo pipefail

EXECUTE=0
VERIFY=0
FROM=""
SLEEP_SECONDS=15
EXTRA_ARGS=()

usage() {
  cat <<'USAGE'
Usage: scripts/publish.sh [--execute] [--verify] [--from <crate>]
                          [--sleep <secs>] [-- <extra cargo args>]

Options:
  --execute        Actually publish.  Without this, the script only prints
                   the cargo publish invocations it would run.
  --verify         Run the CLAUDE.md verification gate (clippy with
                   -D warnings, then cargo test --all and
                   cargo test --doc --all) before any publish step.
  --from <crate>   Resume from <crate>, skipping all earlier crates in the
                   topological order.  Useful when a partial publish failed.
  --sleep <secs>   Seconds to wait after each successful publish so the
                   crates.io index updates before the next dependent crate
                   is uploaded (default: 15).
  -- <args...>     Extra positional args forwarded to every cargo publish
                   invocation (e.g. --allow-dirty, --no-verify, --token X).
  -h, --help       Show this help and exit.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --execute)
      EXECUTE=1
      shift
      ;;
    --verify)
      VERIFY=1
      shift
      ;;
    --from)
      if [[ $# -lt 2 ]]; then
        echo "Error: --from requires a crate name" >&2
        exit 2
      fi
      FROM="$2"
      shift 2
      ;;
    --sleep)
      if [[ $# -lt 2 ]]; then
        echo "Error: --sleep requires a value in seconds" >&2
        exit 2
      fi
      SLEEP_SECONDS="$2"
      shift 2
      ;;
    --)
      shift
      while [[ $# -gt 0 ]]; do
        EXTRA_ARGS+=("$1")
        shift
      done
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Error: unknown flag: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

# Topological order: each crate depends only on those listed above it.
CRATES=(
  hdl-cat-error
  hdl-cat-bits
  hdl-cat-kind
  hdl-cat-signal
  hdl-cat-ir
  hdl-cat-circuit
  hdl-cat-sync
  hdl-cat-sim
  hdl-cat-verilog
  hdl-cat-circom
  hdl-cat-std
  hdl-cat-macros
  hdl-cat
)

# Validate --from
if [[ -n "$FROM" ]]; then
  found=0
  for c in "${CRATES[@]}"; do
    if [[ "$c" == "$FROM" ]]; then
      found=1
      break
    fi
  done
  if [[ $found -eq 0 ]]; then
    echo "Error: --from $FROM is not a workspace crate." >&2
    echo "Known crates: ${CRATES[*]}" >&2
    exit 2
  fi
fi

# Anchor at the workspace root so cargo finds the right Cargo.toml.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$WORKSPACE_ROOT"

if [[ ! -f Cargo.toml ]]; then
  echo "Error: $WORKSPACE_ROOT does not contain a Cargo.toml." >&2
  exit 1
fi

mode_label="dry-plan"
if [[ $EXECUTE -eq 1 ]]; then
  mode_label="EXECUTE"
fi

echo "==> hdl-cat publish ($mode_label)"
echo "    workspace root: $WORKSPACE_ROOT"
if [[ -n "$FROM" ]]; then
  echo "    starting from: $FROM"
fi
if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
  echo "    extra cargo args: ${EXTRA_ARGS[*]}"
fi
echo

if [[ $VERIFY -eq 1 ]]; then
  echo "==> Verification gate"
  echo "    cargo clippy --all-targets --all-features (-D warnings)"
  RUSTFLAGS="-D warnings" cargo clippy --all-targets --all-features
  echo "    cargo test --all"
  cargo test --all
  echo "    cargo test --doc --all"
  cargo test --doc --all
  echo "==> Verification gate passed"
  echo
fi

# Walk the topo order, skipping until --from matches.
skipping=0
if [[ -n "$FROM" ]]; then
  skipping=1
fi

for crate in "${CRATES[@]}"; do
  if [[ $skipping -eq 1 ]]; then
    if [[ "$crate" == "$FROM" ]]; then
      skipping=0
    else
      echo "--  skip $crate (before --from $FROM)"
      continue
    fi
  fi

  cmd=(cargo publish -p "$crate" --locked)
  if [[ ${#EXTRA_ARGS[@]} -gt 0 ]]; then
    cmd+=("${EXTRA_ARGS[@]}")
  fi

  if [[ $EXECUTE -eq 1 ]]; then
    echo "==> Publishing $crate"
    echo "    ${cmd[*]}"
    "${cmd[@]}"
    echo "==> $crate published; sleeping ${SLEEP_SECONDS}s for crates.io index"
    sleep "$SLEEP_SECONDS"
    echo
  else
    echo "[dry-plan] ${cmd[*]}"
  fi
done

if [[ $EXECUTE -eq 1 ]]; then
  echo "==> All crates published."
else
  echo
  echo "==> Dry-plan complete.  Re-run with --execute to publish for real."
fi
