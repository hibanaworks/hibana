#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
OUTPUT="${ROOT_DIR}/target/prefix-input-sensitivity"
PROOF_DIR="${ROOT_DIR}/proofs/lean"
mkdir -p "${OUTPUT}"
git -C "${ROOT_DIR}" rev-parse HEAD > "${OUTPUT}/revision.txt"
git -C "${ROOT_DIR}" diff HEAD --binary > "${OUTPUT}/source.patch"
cat "${ROOT_DIR}/.github/ci-tests/endpoint_conformance.lean" \
  "${ROOT_DIR}/.github/ci-tests/protocol_shapes.lean" > "${OUTPUT}/original.lean"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" proof-guard toolchain "${PROOF_DIR}"
printf 'mutation\texit\n' > "${OUTPUT}/results.tsv"

for mutation in forget-inputs choose-future; do
  case "${mutation}" in
    forget-inputs)
      # Preserve input validation, then restore the old unconstrained final
      # states. This is the specific disconnect found in the original checker.
      perl -0pe 's/if states\.isEmpty then none else some states/if states.isEmpty then none else some (shapeStates source plans word)/g == 1 or die "missing replay result\n"' \
        "${OUTPUT}/original.lean" > "${OUTPUT}/${mutation}.lean"
      expected=a_shared_entry_keeps_the_recorded_left_arm
      ;;
    choose-future)
      # Retaining every fresh choice would incorrectly fix later roll visits.
      perl -0pe 's/shapeScopeEntered scope before \|\| !shapeScopeEntered scope after \|\|/shapeScopeEntered scope before ||/g == 1 or die "missing scope-entry check\n"' \
        "${OUTPUT}/original.lean" > "${OUTPUT}/${mutation}.lean"
      expected=replay_does_not_choose_a_future_resolver_before_entry
      ;;
  esac
  status=0
  git diff --no-index -- "${OUTPUT}/original.lean" "${OUTPUT}/${mutation}.lean" \
    > "${OUTPUT}/${mutation}.patch" || status=$?
  test "${status}" = 1
  shasum -a 256 "${OUTPUT}/${mutation}.lean" > "${OUTPUT}/${mutation}.sha256"
  status=0
  (cd "${PROOF_DIR}" && lake env lean -j 1 -D Elab.async=false "${OUTPUT}/${mutation}.lean") \
    > "${OUTPUT}/${mutation}.log" 2>&1 || status=$?
  test "${status}" = 1
  grep -F 'Tactic `decide` proved that the proposition' "${OUTPUT}/${mutation}.log" > /dev/null
  grep -F "'${expected}' depends on axioms:" "${OUTPUT}/${mutation}.log" \
    | grep -F 'sorryAx'
  printf '%s\t%s\n' "${mutation}" "${status}" >> "${OUTPUT}/results.tsv"
  rm -- "${OUTPUT}/${mutation}.lean"
done

printf 'Recorded resolver input controls passed: lost history and premature future selection rejected\n'
