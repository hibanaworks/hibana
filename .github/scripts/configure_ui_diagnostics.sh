#!/usr/bin/env bash

readonly HIBANA_UI_DIAGNOSTIC_COLUMNS=200
HIBANA_UI_TTY_ROWS=""
HIBANA_UI_TTY_COLUMNS=""

hibana_pin_ui_diagnostic_width() {
  local size
  if ! size="$(stty size 2>/dev/null </dev/tty)"; then
    return 0
  fi
  read -r HIBANA_UI_TTY_ROWS HIBANA_UI_TTY_COLUMNS <<<"${size}"
  if ! [[ "${HIBANA_UI_TTY_ROWS}" =~ ^[0-9]+$ \
    && "${HIBANA_UI_TTY_COLUMNS}" =~ ^[1-9][0-9]*$ ]]; then
    echo "UI diagnostic gate could not read the controlling terminal size" >&2
    return 1
  fi
  if ! stty cols "${HIBANA_UI_DIAGNOSTIC_COLUMNS}" </dev/tty; then
    echo "UI diagnostic gate could not pin the controlling terminal width" >&2
    return 1
  fi
}

hibana_restore_ui_diagnostic_width() {
  if [[ -z "${HIBANA_UI_TTY_ROWS}" ]]; then
    return 0
  fi
  stty rows "${HIBANA_UI_TTY_ROWS}" cols "${HIBANA_UI_TTY_COLUMNS}" </dev/tty
  HIBANA_UI_TTY_ROWS=""
  HIBANA_UI_TTY_COLUMNS=""
}
