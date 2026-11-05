#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
source "$root/.github/scripts/repo_rustflags.sh"
hibana_disable_repo_tests_cfg
unset CARGO_TARGET_DIR
mkdir -p "$root/target/choreofix-regressions" "$root/target/choreofix-source-cache"
out="$(mktemp -d "$root/target/choreofix-regressions/run.XXXXXXXX")"
trap 'status=$?; printf "%s\n" "$status" > "$out/exit-status.txt"' EXIT
printf 'Consumer regression inputs: %s\n' "$out"

# These are immutable test inputs, never alternative development checkouts or
# operational consumer dependencies. Normal consumers keep their public pins.
archive_source() {
    local name="$1" url="$2" revision="$3"
    local cache="$root/target/choreofix-source-cache/$name.git"
    if [[ ! -d "$cache" ]]; then git init --bare --quiet "$cache"; fi
    if ! git --git-dir="$cache" cat-file -e "$revision^{commit}" 2>/dev/null; then
        git --git-dir="$cache" fetch --quiet --depth 1 "$url" "$revision"
    fi
    test "$(git --git-dir="$cache" rev-parse "$revision^{commit}")" = "$revision"
    mkdir "$out/$name"
    git --git-dir="$cache" archive "$revision" | tar -xf - -C "$out/$name"
    printf '%s %s %s\n' "$name" "$revision" "$url" >> "$out/input-revisions.txt"
}
archive_source consumer https://github.com/hibanaworks/choreofix.git 757732035326e9776d8c9931356bd702233724b1
archive_source epf https://github.com/hibanaworks/hibana-epf.git 2cf37f34bff65cd8ed06a1ab30480b9842fc7792
archive_source wasi https://github.com/hibanaworks/hibana-wasip1-runtime.git 82253a5f89246baa081a5c6581f09e9c0cbb918e
read -r mode kind guest_revision guest_path < <(git --git-dir="$root/target/choreofix-source-cache/consumer.git" ls-tree 757732035326e9776d8c9931356bd702233724b1 vendor/hibana-wash)
test "$mode:$kind:$guest_path" = '160000:commit:vendor/hibana-wash'
archive_source wash https://github.com/hibanaworks/hibana-wash.git "$guest_revision"
rmdir "$out/consumer/vendor/hibana-wash"
mv "$out/wash" "$out/consumer/vendor/hibana-wash"

fixture="$out/consumer"
mkdir -p "$fixture/.cargo" "$fixture/tools/runner/src/bin/consumer_core"
cat > "$fixture/.cargo/config.toml" <<CONFIG
[patch."https://github.com/hibanaworks/hibana"]
hibana = { path = "$root" }
[patch."https://github.com/hibanaworks/hibana-epf"]
hibana-epf = { path = "$out/epf" }
[patch."https://github.com/hibanaworks/hibana-wasip1-runtime"]
hibana-wasip1-runtime = { path = "$out/wasi" }
CONFIG
# Only fixture packaging changes. Actual production global/localside Rust is
# extracted unchanged; the extra binary reuses the original qualifier helpers.
printf '\n[workspace]\n' >> "$fixture/Cargo.toml"
for manifest in tools/runner/Cargo.toml tools/inputs/Cargo.toml; do
    printf '\n[workspace]\n' >> "$fixture/$manifest"
done
cp "$root/.github/ci-tests/choreofix_regressions.rs" "$fixture/tools/runner/src/bin/consumer_core/regressions.rs"
cp "$fixture/tools/runner/src/qualification.rs" "$fixture/tools/runner/src/bin/core-regressions.rs"
cat >> "$fixture/tools/runner/src/bin/core-regressions.rs" <<'RUST'

#[path = "../evidence.rs"]
mod evidence;
#[path = "consumer_core/regressions.rs"]
mod core_regressions;
fn main() -> Result<()> { core_regressions::run() }
RUST

export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 RUST_TEST_THREADS=1
export PYTHONHASHSEED=0 PYTHONDONTWRITEBYTECODE=1
cd "$fixture"
for package in app runner inputs wash; do
    case "$package" in
        app) manifest=Cargo.toml ;;
        runner) manifest=tools/runner/Cargo.toml ;;
        inputs) manifest=tools/inputs/Cargo.toml ;;
        wash) manifest=vendor/hibana-wash/Cargo.toml ;;
    esac
    cargo +stable metadata --manifest-path "$manifest" --format-version 1 \
        > "$out/$package-metadata.json" 2> "$out/$package-resolution.log"
done
cargo +stable run --locked --manifest-path tools/inputs/Cargo.toml -- restore \
    > "$out/restore-inputs.log" 2>&1
"${CHOREOFIX_PYTHON:-python3}" -m venv "$fixture/.venv"
"$fixture/.venv/bin/python" -m pip install --requirement requirements.lock > "$out/python-install.log" 2>&1
export PYO3_PYTHON="$fixture/.venv/bin/python"
cargo +stable run --locked --manifest-path tools/runner/Cargo.toml --bin core-regressions -- "$root" "$out/evidence" \
    > "$out/regressions.log" 2>&1
