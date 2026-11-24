#!/usr/bin/env bash
# Fails if the built APK requests any network permission.
# Usage: scripts/check_no_internet.sh app/build/outputs/apk/debug/app-debug.apk
set -euo pipefail
apk="${1:?usage: $0 <apk>}"
aapt2="$(ls -d "${ANDROID_HOME:?set ANDROID_HOME}"/build-tools/*/aapt2 | sort -V | tail -1)"
perms="$("$aapt2" dump permissions "$apk")"
echo "$perms"
if echo "$perms" | grep -E "android.permission.(INTERNET|ACCESS_NETWORK_STATE|ACCESS_WIFI_STATE|CHANGE_NETWORK_STATE|CHANGE_WIFI_STATE)"; then
  echo "ERROR: network permission found in $apk" >&2
  exit 1
fi
echo "OK: no network permissions in $apk"
