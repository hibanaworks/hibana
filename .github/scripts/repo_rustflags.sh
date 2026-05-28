#!/usr/bin/env bash

hibana_enable_repo_tests_cfg() {
  case " ${RUSTFLAGS:-} " in
    *" --cfg hibana_repo_tests "*) ;;
    *) export RUSTFLAGS="${RUSTFLAGS:+${RUSTFLAGS} }--cfg hibana_repo_tests" ;;
  esac
}
