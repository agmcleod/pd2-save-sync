#!/usr/bin/env bash
set -Eeuo pipefail

CONFIG_FILE="${XDG_CONFIG_HOME:-$HOME/.config}/pd2-save-sync/steamdeck-launcher.conf"
STEAM_WRAPPER=false

if [[ "${1:-}" == "--steam-wrapper" ]]; then
    STEAM_WRAPPER=true
    shift
elif [[ $# -gt 0 && "${1:0:1}" != "-" ]]; then
    CONFIG_FILE="$1"
    shift
fi

if [[ ! -f "$CONFIG_FILE" ]]; then
    printf 'Launcher configuration not found: %s\n' "$CONFIG_FILE" >&2
    printf 'Copy scripts/steamdeck-launcher.conf.example and edit it first.\n' >&2
    exit 2
fi

# shellcheck source=/dev/null
source "$CONFIG_FILE"

: "${ONEDRIVE_BIN:=$HOME/.local/bin/onedrive}"
: "${SYNC_BIN:?SYNC_BIN must be set in the launcher configuration}"
: "${SYNC_CONFIG:?SYNC_CONFIG must be set in the launcher configuration}"
ONEDRIVE_ARGS=("${ONEDRIVE_ARGS[@]:---synchronize}")

if [[ "$STEAM_WRAPPER" == true ]]; then
    if (( $# == 0 )); then
        printf 'Steam wrapper mode requires a game command.\n' >&2
        exit 2
    fi
    GAME_COMMAND=("$@")
else
    : "${GAME_COMMAND:?GAME_COMMAND must be set in the launcher configuration}"
fi

run_onedrive() {
    printf '[launcher] Synchronizing OneDrive...\n'
    "$ONEDRIVE_BIN" "${ONEDRIVE_ARGS[@]}"
}

run_pd2_sync() {
    printf '[launcher] Synchronizing configured save/filter pairs...\n'
    "$SYNC_BIN" --config "$SYNC_CONFIG" sync-all
}

printf '[launcher] Running pre-game synchronization.\n'
run_onedrive
run_pd2_sync

printf '[launcher] Starting Project Diablo 2.\n'
if "${GAME_COMMAND[@]}"; then
    game_status=0
else
    game_status=$?
fi

printf '[launcher] Game exited with status %d; running post-game synchronization.\n' "$game_status"
post_status=0
run_pd2_sync || post_status=$?
onedrive_status=0
run_onedrive || onedrive_status=$?
if (( post_status == 0 )); then
    post_status=$onedrive_status
fi

if (( post_status != 0 )); then
    printf '[launcher] Post-game synchronization failed with status %d.\n' "$post_status" >&2
fi

exit "$game_status"
