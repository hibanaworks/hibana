#!/usr/bin/env bash

hibana_enable_repo_tests_cfg() {
  case " ${RUSTFLAGS:-} " in
    *" --cfg hibana_repo_tests "*) ;;
    *) export RUSTFLAGS="${RUSTFLAGS:+${RUSTFLAGS} }--cfg hibana_repo_tests" ;;
  esac
}

hibana_disable_repo_tests_cfg() {
  local tokens=()
  local filtered=()
  local token
  local idx
  read -r -a tokens <<< "${RUSTFLAGS:-}"
  for (( idx = 0; idx < ${#tokens[@]}; idx += 1 )); do
    token="${tokens[idx]}"
    if [[ "${token}" == "--cfg" && "${tokens[idx + 1]:-}" == "hibana_repo_tests" ]]; then
      idx=$((idx + 1))
      continue
    fi
    if [[ "${token}" == "--cfg=hibana_repo_tests" || "${token}" == "hibana_repo_tests" ]]; then
      continue
    fi
    filtered+=("${token}")
  done
  if (( ${#filtered[@]} == 0 )); then
    unset RUSTFLAGS
  else
    export RUSTFLAGS="${filtered[*]}"
  fi
  unset CARGO_ENCODED_RUSTFLAGS
}
