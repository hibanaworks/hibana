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
for defect in resumption context actor guarded-context selected-expansion continuation-position resolver-origin; do
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
    # The entry-owner check also protects the concrete return example. Keep
    # checking this predicate's own general contract when it is made vacuous.
    expected="'shapeOriginalActorsMatch_checks_each_occurrence' depends on axioms: [propext, sorryAx]"
  elif [[ "${defect}" == guarded-context ]]; then
    perl -0pi -e '
      s/\| \.roll body => ⟨\.hole, body, rfl⟩ :: \(\(shapeRollFocuses body\)\.map fun focus =>\n      ⟨\.roll focus.context, focus.body, by simp only \[RollContext.plug, focus.source_eq\]⟩\)/| .roll body => [⟨.hole, body, rfl⟩]/ == 1 or die "missing parent-roll context\n";
      s/(def shapeRollFocuses.*?\n)  \| \.route resolver left right =>.*?\n\n-- Every subtree/$1  | .route _ _ _ => []\n\n-- Every subtree/s == 1 or die "missing route context\n";
    ' "${OUTPUT}/${defect}.lean"
    expected='guarded inner-roll regression:'
  elif [[ "${defect}" == continuation-position ]]; then
    perl -0pi -e '
      s/\(shapeContinuationCuts tail\.1\)/([⟨RollContext.hole, tail.1, rfl⟩] : List (Σ context : RollContext, { focus : Source \/\/ context.plug focus = tail.1 }))/ == 1 or die "missing continuation insertion sites\n";
    ' "${OUTPUT}/${defect}.lean"
    expected='continuation-cut regression:'
  elif [[ "${defect}" == resolver-origin ]]; then
    perl -0pi -e '
      s/shapeResolverEntered plan scope id (before|after)/shapeScopeEntered scope $1/g == 4 or die "missing resolver origin checks\n";
    ' "${OUTPUT}/${defect}.lean"
    expected='continuation-cut regression:'
  else
    perl -0pi -e '
      s/(  \| some original =>\n)(    match expandedAt : normalizeExpansion)/$1    if (normalize expanded).isNone then [] else\n$2/ == 1 or die "missing selected expansion normalization\n";
    ' "${OUTPUT}/${defect}.lean"
    expected='selected-expansion regression:'
  fi
  diff -u "${OUTPUT}/current.lean" "${OUTPUT}/${defect}.lean" \
    > "${OUTPUT}/${defect}.patch" || test "$?" == 1
  status=0
  if [[ "${defect}" == guarded-context || "${defect}" == selected-expansion ||
        "${defect}" == continuation-position || "${defect}" == resolver-origin ]]; then
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

# Ignore the source entry holders in an isolated model copy. The general query
# proofs still compile; the two-resource executable regression must detect the
# weakened interpretation. Keep a separate Lake output directory for the copy.
owner_copy="$(mktemp -d "${OUTPUT}/owner-model.XXXXXX")"
mkdir -p "${owner_copy}/proofs" "${owner_copy}/target/lean-proof"
cp -R proofs/lean "${owner_copy}/proofs/lean"
cp "${OUTPUT}/current.lean" "${owner_copy}/target/lean-proof/ProtocolShapesGenerated.lean"
owner_source="proofs/lean/Hibana/SourceOccurrenceProgress.lean"
shasum -a 256 "${owner_source}" > "${OUTPUT}/owner-original.sha256"
git diff HEAD -- proofs/lean .github/ci-tests/endpoint_conformance.lean \
  .github/ci-tests/protocol_shapes.lean > "${OUTPUT}/owner-baseline.patch"
perl -0pi -e '
  s/(def sourceOwnerBefore.*?: Nat :=)\n.*?\n\ndef SourceUnfolding.guardsCompatible/$1\n  key.role\n\ndef SourceUnfolding.guardsCompatible/s == 1 or die "missing source entry ownership fold\n";
' "${owner_copy}/${owner_source}"
diff -u "${owner_source}" "${owner_copy}/${owner_source}" \
  > "${OUTPUT}/owner-context.patch" || test "$?" == 1
shasum -a 256 "${owner_copy}/${owner_source}" > "${OUTPUT}/owner-mutant.sha256"
bash "${ROOT_DIR}/.github/scripts/lib/lean_audit_tool.sh" build-serial \
  "${owner_copy}/proofs/lean" --native Hibana.SourceOccurrenceTrace \
  > "${OUTPUT}/owner-build.log" 2>&1
(cd "${owner_copy}/proofs/lean" && lake build protocolShapes) \
  >> "${OUTPUT}/owner-build.log" 2>&1
status=0
"${owner_copy}/proofs/lean/.lake/build/bin/protocol-shapes" --check-guarded-reentry \
  > "${OUTPUT}/owner-context.log" 2>&1 || status=$?
if [[ "${status}" == 0 ]] || ! grep -Fq 'retained-owner regression:' "${OUTPUT}/owner-context.log"; then
  echo 'The source checker did not detect omitted entry ownership' >&2
  cat "${OUTPUT}/owner-context.log" >&2
  exit 1
fi
printf 'owner-context\t%s\n' "${status}" >> "${OUTPUT}/mutants.tsv"
shasum -a 256 --check "${OUTPUT}/owner-original.sha256" > "${OUTPUT}/owner-source-check.log"
rm -r "${owner_copy}"
shasum -a 256 --check "${OUTPUT}/source.sha256" > "${OUTPUT}/source-check.log"
rm "${OUTPUT}/current.lean" "${OUTPUT}/resumption.lean" "${OUTPUT}/context.lean" \
  "${OUTPUT}/actor.lean" "${OUTPUT}/guarded-context.lean" "${OUTPUT}/selected-expansion.lean" \
  "${OUTPUT}/continuation-position.lean" "${OUTPUT}/resolver-origin.lean"
echo 'Shape resumption sensitivity: all eight restored checker omissions are rejected'
