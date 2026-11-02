#!/usr/bin/env bash
set -euo pipefail
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
OUTPUT="${ROOT_DIR}/target/reentry-component-sensitivity"
MUTANT="${OUTPUT}/source"
INPUT="${OUTPUT}/input"
if [[ -e "${MUTANT}" ]]; then
  echo "Previous sensitivity source remains: ${MUTANT}" >&2
  exit 1
fi
mkdir -p "${OUTPUT}"
cd "${ROOT_DIR}"
git rev-parse HEAD > "${OUTPUT}/revision.txt"
rustc +"${TOOLCHAIN:-stable}" --version > "${OUTPUT}/compiler.txt"
shasum -a 256 src/g/source/ownership/actors/reentry.rs \
  .github/ci-tests/protocol_shape_interactions.rs > "${OUTPUT}/source.sha256"

# Keep the actual grammar-generated uninformed source as the counterexample.
# This checks compiler refusal, not a second runtime or a localside workaround.
cargo +"${TOOLCHAIN:-stable}" run --quiet --locked --manifest-path .github/ci-tests/Cargo.toml \
  --bin protocol-shapes -- "${ROOT_DIR}" "${INPUT}" --interactions 4 12439 1
cargo +"${TOOLCHAIN:-stable}" run --quiet --manifest-path "${INPUT}/Cargo.toml" \
  --target-dir "${OUTPUT}/diagnose-target" --bin diagnose > "${INPUT}/diagnostics.tsv"
cargo +"${TOOLCHAIN:-stable}" run --quiet --locked --manifest-path .github/ci-tests/Cargo.toml \
  --bin protocol-shapes -- "${ROOT_DIR}" "${INPUT}" --interactions 4 12439 1 "${INPUT}/diagnostics.tsv"
fixture="src/bin/reentry_12439.rs"
test -f "${INPUT}/rejected/${fixture}"
cargo +"${TOOLCHAIN:-stable}" generate-lockfile --offline --manifest-path "${INPUT}/rejected/Cargo.toml"
status=0
cargo +"${TOOLCHAIN:-stable}" build --locked --manifest-path "${INPUT}/rejected/Cargo.toml" \
  --target-dir "${OUTPUT}/positive-target" --bin reentry_12439 > "${OUTPUT}/positive.log" 2>&1 || status=$?
if [[ "${status}" != 101 ]] || ! grep -Fq "rolled reentry requires permission" "${OUTPUT}/positive.log"; then
  echo 'Current compiler did not reject the generated uninformed parent visit' >&2
  cat "${OUTPUT}/positive.log" >&2
  exit 1
fi

git clone --quiet --shared --no-checkout "${ROOT_DIR}" "${MUTANT}"
git ls-files --cached --others --exclude-standard -z |
  perl -0ne 'print "$_\0" for grep { -f $_ } split /\0/' |
  rsync -a --from0 --files-from=- ./ "${MUTANT}/"
git -C "${MUTANT}" add -A
git -C "${MUTANT}" diff --cached --binary HEAD > "${OUTPUT}/current-source.patch"
perl -0pi -e '
  my $start = index($_, "    const fn origins(");
  my $end = index($_, "    const fn permitted_origins(", $start);
  $start >= 0 && $end > $start or die "missing origin checker\n";
  my $body = substr($_, $start, $end - $start);
  $body =~ s/        let mut connected = \[false; u8::MAX as usize \+ 1\];\n// == 1
    or die "missing component workspace\n";
  $body =~ s/Ok\(true\) => \{.*?\n                    \}\n                    Ok\(false\)/Ok(true) => needed[event] = 3,\n                    Ok(false)/s == 1
    or die "missing resource seeds\n";
  $body =~ s/        let mut changed = true;.*?        self\.permitted_origins/        self.permitted_origins/s == 1
    or die "missing component closure\n";
  substr($_, $start, $end - $start) = $body;
' "${MUTANT}/src/g/source/ownership/actors/reentry.rs"
git -C "${MUTANT}" diff --binary > "${OUTPUT}/mutation.patch"
cp -R "${INPUT}/rejected" "${OUTPUT}/mutant-input"
SENSITIVITY_ROOT="${ROOT_DIR}" SENSITIVITY_MUTANT="${MUTANT}" perl -0pi -e '
  s/\Q$ENV{SENSITIVITY_ROOT}\E/$ENV{SENSITIVITY_MUTANT}/g == 1 or die "missing core dependency\n";
' "${OUTPUT}/mutant-input/Cargo.toml"
cmp "${INPUT}/rejected/${fixture}" "${OUTPUT}/mutant-input/${fixture}"
cargo +"${TOOLCHAIN:-stable}" build --locked --manifest-path "${OUTPUT}/mutant-input/Cargo.toml" \
  --target-dir "${OUTPUT}/mutant-target" --bin reentry_12439 > "${OUTPUT}/mutant.log" 2>&1
shasum -a 256 --check "${OUTPUT}/source.sha256" > "${OUTPUT}/source-check.log"
printf 'current=compile_refused\nomitted_component_closure=compiled\nfixture=byte_identical_grammar_source\n' > "${OUTPUT}/result.txt"
rm -r -- "${MUTANT}" "${OUTPUT}/mutant-target" "${OUTPUT}/positive-target" "${OUTPUT}/diagnose-target"
echo 'Reentry component sensitivity: current source rejects; omitted closure wrongly compiles the identical generated source'
