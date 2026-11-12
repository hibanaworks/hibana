#!/usr/bin/env bash
# Fingerprint the executed population and compiler environment for reuse.
hibana_protocol_compiler_environment() {
  perl -e 'for my $key (sort grep {
    /\A(?:RUSTFLAGS|CARGO_ENCODED_RUSTFLAGS|RUSTC(?:_.*)?|CARGO_BUILD_(?:TARGET|RUSTFLAGS)|CARGO_PROFILE_.*)\z/
  } keys %ENV) { print "$key\0$ENV{$key}\0" }' | shasum -a 256
}

hibana_protocol_population_inputs() (
  cd "$1"
  git ls-files --cached --others --exclude-standard -z -- \
    src Cargo.toml Cargo.lock build.rs .cargo .github/ci-tests .github/scripts \
    ':(glob)proofs/lean/**/*.lean' proofs/lean/lean-toolchain \
    proofs/lean/lakefile.toml proofs/lean/lake-manifest.json |
    perl -0777 -ne 'print "$_\0" for sort grep { -f $_ } split /\0/' |
    xargs -0 shasum -a 256 --
)
