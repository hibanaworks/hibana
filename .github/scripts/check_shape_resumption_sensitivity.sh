#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
OUTPUT="${ROOT_DIR}/target/shape-resumption-sensitivity"
mkdir -p "${OUTPUT}"
cd "${ROOT_DIR}"
git rev-parse HEAD > "${OUTPUT}/revision.txt"
shasum -a 256 .github/ci-tests/endpoint_conformance.lean \
  .github/ci-tests/protocol_shapes.lean > "${OUTPUT}/source.sha256"
cat .github/ci-tests/endpoint_conformance.lean .github/ci-tests/protocol_shapes.lean \
  > "${OUTPUT}/current.lean"
# Use the same proved imports. Only the shape checker is mutated; there is no
# alternate runtime, application protocol or reduced observation inventory.
(cd proofs/lean && lake env lean -j1 "${OUTPUT}/current.lean") \
  > "${OUTPUT}/positive.log" 2>&1
(cd proofs/lean && lake env lean -j1 --run "${OUTPUT}/current.lean" --check-guarded-reentry) \
  > "${OUTPUT}/guarded-positive.log" 2>&1
: > "${OUTPUT}/mutants.tsv"
for defect in resumption context actor guarded-context; do
  cp "${OUTPUT}/current.lean" "${OUTPUT}/${defect}.lean"
  if [[ "${defect}" == resumption ]]; then
    perl -0pi -e '
      s/shapeRowsReady prepared\.dependencies points point &&\n    shapeResumptionsReady prepared\.resumptions points point/shapeRowsReady prepared.dependencies points point/ == 1 or die "missing resumption query\n";
      s/prepared\.ready points point = \(\(occurrenceCut prepared\.plan points\)\.ready point &&\n      \(occurrenceCut prepared\.plan points\)\.resumptionReady point\)/prepared.ready points point = (occurrenceCut prepared.plan points).ready point/ == 1 or die "missing query specification\n";
    ' "${OUTPUT}/${defect}.lean"
    expected='shapeStates resumedOutgoingTail'
  elif [[ "${defect}" == context ]]; then
    perl -0pi -e '
      s/inside \+\+ \(shapeRollFocuses left\)\.flatMap fun focus =>/inside ++ ((shapeRollFocuses left).filter (fun focus => match focus.context with | .hole => true | _ => false)).flatMap fun focus =>/ == 1 or die "missing contextual resumption\n";
    ' "${OUTPUT}/${defect}.lean"
    expected='shapeStates parallelRetainedRolls'
  elif [[ "${defect}" == actor ]]; then
    perl -0pi -e '
      s/(def shapeOriginalActorsMatch \(plan : SourceOccurrenceCut source\) : Bool :=)\n.*?\n\nstructure ShapePlan/$1\n  true\n\nstructure ShapePlan/s == 1 or die "missing original actor bindings\n";
    ' "${OUTPUT}/${defect}.lean"
    expected='shapeStates returnedRollEndpoint'
  else
    perl -0pi -e '
      s/\| \.roll body => ⟨\.hole, body, rfl⟩ :: \(\(shapeRollFocuses body\)\.map fun focus =>\n      ⟨\.roll focus.context, focus.body, by simp only \[RollContext.plug, focus.source_eq\]⟩\)/| .roll body => [⟨.hole, body, rfl⟩]/ == 1 or die "missing parent-roll context\n";
      s/(def shapeRollFocuses.*?\n)  \| \.route resolver left right =>.*?\n\ndef shapeAlongsidePlans/$1  | .route _ _ _ => []\n\ndef shapeAlongsidePlans/s == 1 or die "missing route context\n";
    ' "${OUTPUT}/${defect}.lean"
    expected='guarded inner-roll regression:'
  fi
  diff -u "${OUTPUT}/current.lean" "${OUTPUT}/${defect}.lean" \
    > "${OUTPUT}/${defect}.patch" || test "$?" == 1
  status=0
  if [[ "${defect}" == guarded-context ]]; then
    (cd proofs/lean && lake env lean -j1 --run "${OUTPUT}/${defect}.lean" --check-guarded-reentry) \
      > "${OUTPUT}/${defect}.log" 2>&1 || status=$?
  else
    (cd proofs/lean && lake env lean -j1 "${OUTPUT}/${defect}.lean") \
      > "${OUTPUT}/${defect}.log" 2>&1 || status=$?
  fi
  if [[ "${status}" == 0 ]] || ! grep -Fq "${expected}" "${OUTPUT}/${defect}.log"; then
    echo "The shape checker did not detect the restored ${defect} omission" >&2
    tail -35 "${OUTPUT}/${defect}.log" >&2
    exit 1
  fi
  printf '%s\t%s\n' "${defect}" "${status}" >> "${OUTPUT}/mutants.tsv"
done
shasum -a 256 --check "${OUTPUT}/source.sha256" > "${OUTPUT}/source-check.log"
rm "${OUTPUT}/current.lean" "${OUTPUT}/resumption.lean" "${OUTPUT}/context.lean" \
  "${OUTPUT}/actor.lean" "${OUTPUT}/guarded-context.lean"
echo 'Shape resumption sensitivity: all four restored checker omissions are rejected'
