#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../../../.." && pwd)"
cd "$ROOT"
LEAN_BIN="${LEAN_BIN:-lean}"
"$LEAN_BIN" --version | bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" version
evidence="$(mktemp -d /tmp/hibana-dispatch-subtree.XXXXXX)"
trap 'rm -r -- "$evidence"' EXIT
for model in Dispatch OfferLinks; do
  case "$model" in
    Dispatch) claims=14 ;;
    OfferLinks) claims=7 ;;
  esac
  if ! "$LEAN_BIN" "proofs/checks/image/dispatch-subtree/$model.lean" > "$evidence/$model.log" 2>&1; then
    cat "$evidence/$model.log" >&2
    exit 1
  fi
  cat "$evidence/$model.log"
  bash "$ROOT/.github/scripts/lib/supplemental_proofs_tool.sh" lean "$evidence/$model.log" "$claims"
done
cargo +"${TOOLCHAIN:-stable}" run --quiet --locked -p hibana-ci-tests --bin dispatch-model
bash .github/scripts/check_smt2.sh proofs/checks/image/dispatch-subtree/Dispatch.smt2
bash .github/scripts/check_smt2.sh proofs/checks/image/dispatch-subtree/OfferLinks.smt2
bash .github/scripts/check_smt2.sh proofs/checks/image/dispatch-subtree/TargetPath.smt2
