#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
cd "$root"
mkdir -p "$root/target/pending-acceptance-sensitivity"
out="$(mktemp -d "$root/target/pending-acceptance-sensitivity/run.XXXXXXXX")"
printf 'Sensitivity evidence: %s\n' "$out"
source=proofs/lean/Hibana/SourceResumptionProgress.lean
regression=proofs/lean/Hibana/PendingAcceptance.lean
shasum -a 256 "$source" "$regression" > "$out/source.sha256"
bash .github/scripts/lib/lean_audit_tool.sh build-serial "$root/proofs/lean" \
  --native Hibana.PendingAcceptance > "$out/build.log" 2>&1
(cd proofs/lean && lake env lean -j1 Hibana/PendingAcceptance.lean) \
  > "$out/positive.log" 2>&1
for defect in acceptance untracked-body; do
  copy="$out/$defect"
  mkdir -p "$copy/proofs"
  cp -R proofs/lean "$copy/proofs/lean"
  if [[ "$defect" == acceptance ]]; then
    perl -0pi -e 's/\| \.accepted _ => false/| .accepted event => scope.inBody event/ == 1 or die "missing acceptance boundary\n"' "$copy/$source"
    expected=unpublished_acceptance_does_not_enter_the_senders_roll
  else
    perl -0pi -e 's/\| \.committed action => scope.inBody action.event/| .committed action => scope.inBody action.event \&\& false/ == 1 or die "missing committed body boundary\n"' "$copy/$source"
    expected=a_committed_entry_still_waits_for_its_remaining_body
  fi
  diff -u "$source" "$copy/$source" > "$out/$defect.patch" || test "$?" = 1
  shasum -a 256 "$copy/$source" "$copy/$regression" > "$out/$defect-source.sha256"
  bash .github/scripts/lib/lean_audit_tool.sh build-serial "$copy/proofs/lean" \
    --native Hibana.SourceResumptionProgress > "$out/$defect-build.log" 2>&1
  status=0
  (cd "$copy/proofs/lean" && lake env lean -j1 Hibana/PendingAcceptance.lean) \
    > "$out/$defect.log" 2>&1 || status=$?
  test "$status" = 1
  grep -Fq 'Tactic `decide` proved that the proposition' "$out/$defect.log"
  grep -Fq 'is false' "$out/$defect.log"
  grep -Fq "$expected" "$out/$defect.log"
  printf 'defect=%s\nexit=%s\nfailed_claim=%s\n' "$defect" "$status" "$expected" \
    > "$out/$defect-result.txt"
  rm -r -- "$copy"
done
shasum -a 256 --check "$out/source.sha256" > "$out/source-check.log"
echo 'Pending acceptance sensitivity: physical-acceptance entry and missing committed-body guards are detected'
