#!/usr/bin/env bash
set -euo pipefail
unset HIBANA_PROTOCOL_SHAPE_OUTPUT_ROOT
cd "$(dirname "$0")/../.."
root="$PWD"
output="$root/target/resolver-observation-sensitivity"
mkdir -p "$output"
git rev-parse HEAD > "$output/revision.txt"
for path in select send provenance; do
  case "$path" in
    select) catalog=2 ;;
    send) catalog=14 ;;
    provenance) catalog=2 ;;
  esac
  bash .github/scripts/check_protocol_shapes.sh --check --contracts "$catalog" 1 \
    > "$output/$path-positive.log" 2>&1
  mutant="$output/$path"
  test ! -e "$mutant"
  git clone --quiet --shared --no-checkout "$root" "$mutant"
  git ls-files --cached --others --exclude-standard -z |
    perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
    rsync -a --from0 --files-from=- ./ "$mutant/"
  git -C "$mutant" add -A
  git -C "$mutant" diff --cached --binary HEAD > "$output/$path-source.patch"
  if [[ "$path" == provenance ]]; then
    changed="src/endpoint/kernel/core/send_ops/route_evidence.rs"
    perl -0pi -e 's/delta\.route_is_fresh\(idx\)\s*\&\& self\.is_route_controller\(scope_id\)/true/g == 1 or die "missing resolver provenance check\n"' "$mutant/$changed"
  else
    changed="src/endpoint/kernel/core/decision_resolver/impls/$path.rs"
    perl -0pi -e 's/\s*self\.emit_dynamic_resolver_success_audit\(\s*(?:offer_lane|lane),\s*scope_id,\s*resolver\.resolver_id\(\),\s*resolution,\s*\);//g == 1 or die "missing resolver observation\n"' "$mutant/$changed"
  fi
  test "$(git -C "$mutant" diff --name-only)" = "$changed"
  git -C "$mutant" diff --binary > "$output/$path-mutation.patch"
  status=0
  (cd "$mutant"; bash .github/scripts/check_protocol_shapes.sh --contracts "$catalog" 1) \
    > "$output/$path-mutant.log" 2>&1 || status=$?
  test "$status" != 0
  grep -Eq 'offering a generated branch must record each resolver call|generated typed operations must preserve resolver observations|a resolver selection must name the actor that actually decided' \
    "$output/$path-mutant.log"
  printf 'mutant_exit=%s\n' "$status" > "$output/$path-result.txt"
  mkdir -p "$output/$path-inputs"
  rsync -a --exclude=target \
    "$mutant/target/protocol-shapes/contracts/range-$catalog-count-1/" \
    "$output/$path-inputs/"
  find "$mutant/target/protocol-shapes" -type f -perm -100 \
    -exec shasum -a 256 -- {} + > "$output/$path-binaries.sha256"
  rm -r -- "$mutant"
done
echo "Generated executions detect both missing resolver observations and false resolver provenance"
