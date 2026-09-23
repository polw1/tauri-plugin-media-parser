#!/bin/sh
# Runs the Android test suites on a connected emulator, booting `ANDROID_AVD_NAME`
# when none is running. Cross-compiling only links the suites; MediaCodec is an
# operating-system API, so the executables have to run on Android.
set -eu

workspace_root=$(CDPATH= cd -- "$(dirname -- "$0")/../../.." && pwd)
sdk_root=${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}
if [ -z "$sdk_root" ]; then
   echo "ANDROID_HOME or ANDROID_SDK_ROOT must point to the Android SDK" >&2
   exit 1
fi

adb="$sdk_root/platform-tools/adb"
emulator="$sdk_root/emulator/emulator"
ndk_root=${ANDROID_NDK_LATEST_HOME:-${ANDROID_NDK_HOME:-}}
if [ -z "$ndk_root" ]; then
   ndk_root=$(find "$sdk_root/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)
fi
if [ -z "$ndk_root" ] || [ ! -d "$ndk_root" ]; then
   echo "ANDROID_NDK_LATEST_HOME, ANDROID_NDK_HOME, or an installed SDK NDK is required" >&2
   exit 1
fi

for required_command in awk basename cargo date flock grep jq mktemp sed sort tail timeout tr; do
   if ! command -v "$required_command" >/dev/null 2>&1; then
      echo "$required_command is required" >&2
      exit 1
   fi
done
if [ ! -x "$adb" ]; then
   echo "Android adb is not executable at $adb" >&2
   exit 1
fi

# The NDK ships `x86_64-linux-android24-clang` (24 is `minSdk`) and `llvm-ar`, not
# the unversioned names cc-rs probes for, so both are named in full.
toolchain="$ndk_root/toolchains/llvm/prebuilt/linux-x86_64/bin"
export CC_x86_64_linux_android="$toolchain/x86_64-linux-android24-clang"
export AR_x86_64_linux_android="$toolchain/llvm-ar"
export CARGO_TARGET_X86_64_LINUX_ANDROID_LINKER="$toolchain/x86_64-linux-android24-clang"
if [ ! -x "$CC_x86_64_linux_android" ] || [ ! -x "$AR_x86_64_linux_android" ]; then
   echo "The API 24 x86_64 NDK compiler and archiver are required in $toolchain" >&2
   exit 1
fi

test_timeout=${ANDROID_TEST_TIMEOUT_SECONDS:-300}
case "$test_timeout" in
   ''|*[!0-9]*|0)
      echo "ANDROID_TEST_TIMEOUT_SECONDS must be a positive integer" >&2
      exit 1
      ;;
esac
lock_timeout=${ANDROID_EMULATOR_LOCK_TIMEOUT_SECONDS:-30}
case "$lock_timeout" in
   ''|*[!0-9]*|0)
      echo "ANDROID_EMULATOR_LOCK_TIMEOUT_SECONDS must be a positive integer" >&2
      exit 1
      ;;
esac

emulator_log=$(mktemp)
emulator_pid=
emulator_serial=
build_output=
run_log=
remote_dir=
remote_identity_file=
stop_remote_test() {
   if [ -z "$remote_identity_file" ] || [ -z "$emulator_serial" ]; then
      return
   fi

   remote_identity=
   if remote_identity=$(timeout 5 "$adb" -s "$emulator_serial" shell \
      "if [ -f $remote_identity_file ]; then cat $remote_identity_file; fi" 2>/dev/null); then
      remote_identity=$(printf '%s' "$remote_identity" | tr -d '\r')
   else
      remote_identity=
   fi
   remote_pid=$(printf '%s\n' "$remote_identity" |
      awk 'NF == 2 && $1 ~ /^[0-9]+$/ && $2 ~ /^[0-9]+$/ { print $1 }')
   remote_start=$(printf '%s\n' "$remote_identity" |
      awk 'NF == 2 && $1 ~ /^[0-9]+$/ && $2 ~ /^[0-9]+$/ { print $2 }')
   case "$remote_pid" in
      ''|*[!0-9]*) return ;;
   esac
   case "$remote_start" in
      ''|*[!0-9]*) return ;;
   esac

   current_start=
   if current_start=$(timeout 5 "$adb" -s "$emulator_serial" shell \
      "if [ -r /proc/$remote_pid/stat ]; then cut -d ' ' -f 22 /proc/$remote_pid/stat; fi" 2>/dev/null); then
      current_start=$(printf '%s' "$current_start" | tr -d '\r\n')
   else
      current_start=
   fi
   if [ "$current_start" != "$remote_start" ]; then
      return
   fi

   timeout 10 "$adb" -s "$emulator_serial" shell \
      "current_start=\$(cut -d ' ' -f 22 /proc/$remote_pid/stat 2>/dev/null); if [ \"\$current_start\" = $remote_start ]; then kill $remote_pid 2>/dev/null || true; stop_attempt=0; while [ \$stop_attempt -lt 8 ]; do current_start=\$(cut -d ' ' -f 22 /proc/$remote_pid/stat 2>/dev/null); [ \"\$current_start\" = $remote_start ] || break; stop_attempt=\$((stop_attempt + 1)); sleep 1; done; current_start=\$(cut -d ' ' -f 22 /proc/$remote_pid/stat 2>/dev/null); if [ \"\$current_start\" = $remote_start ]; then kill -9 $remote_pid 2>/dev/null || true; fi; fi; rm -f $remote_identity_file" \
      >/dev/null 2>&1 || true
}
cleanup() {
   trap - EXIT INT TERM
   if [ -n "$remote_dir" ] && [ -n "$emulator_serial" ]; then
      stop_remote_test
      timeout 10 "$adb" -s "$emulator_serial" shell rm -rf "$remote_dir" >/dev/null 2>&1 || true
   fi
   if [ -n "$emulator_pid" ]; then
      kill "$emulator_pid" 2>/dev/null || true

      stop_attempt=0
      while kill -0 "$emulator_pid" 2>/dev/null && [ "$stop_attempt" -lt 10 ]; do
         stop_attempt=$((stop_attempt + 1))
         sleep 1
      done
      if kill -0 "$emulator_pid" 2>/dev/null; then
         kill -KILL "$emulator_pid" 2>/dev/null || true
      fi
      wait "$emulator_pid" 2>/dev/null || true
   fi
   if [ -n "$build_output" ]; then
      rm -f "$build_output"
   fi
   if [ -n "$run_log" ]; then
      rm -f "$run_log"
   fi
   rm -f "$emulator_log"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

exec 9>/tmp/tauri-media-parser-android.lock
if ! flock -w "$lock_timeout" 9; then
   echo "Timed out waiting ${lock_timeout}s for the Android media-parser test lock" >&2
   exit 1
fi

preferred_port=${EMULATOR_PORT:-}
case "$preferred_port" in
   *[!0-9]*)
      echo "EMULATOR_PORT must be numeric" >&2
      exit 1
      ;;
esac
if [ -n "$preferred_port" ] &&
   { [ "$preferred_port" -lt 5554 ] || [ "$preferred_port" -gt 5682 ] || [ $((preferred_port % 2)) -ne 0 ]; }; then
   echo "EMULATOR_PORT must be an even port from 5554 through 5682" >&2
   exit 1
fi

if ! adb_devices=$(timeout 10 "$adb" devices); then
   echo "Unable to list Android devices" >&2
   exit 1
fi
if [ -n "$preferred_port" ]; then
   emulator_serial="emulator-$preferred_port"
else
   emulator_serial=$(printf '%s\n' "$adb_devices" |
      awk '$1 ~ /^emulator-[0-9]+$/ && $2 == "device" { sub(/^emulator-/, "", $1); print $1 }' |
      sort -n | sed -n '1s/^/emulator-/p')
fi

if [ -z "$emulator_serial" ]; then
   emulator_port=5554
   while printf '%s\n' "$adb_devices" | awk '{ print $1 }' |
      grep -qx "emulator-$emulator_port"; do
      emulator_port=$((emulator_port + 2))
      if [ "$emulator_port" -gt 5682 ]; then
         echo "No free Android emulator console port is available from 5554 through 5682" >&2
         exit 1
      fi
   done
   emulator_serial="emulator-$emulator_port"
   avd_name=${ANDROID_AVD_NAME:-Tablet_API_35}
   if [ ! -x "$emulator" ]; then
      echo "Android emulator is not executable at $emulator" >&2
      exit 1
   fi
   "$emulator" "@$avd_name" \
      -port "$emulator_port" \
      -no-window -no-audio -no-boot-anim -no-snapshot \
      -gpu swiftshader_indirect >"$emulator_log" 2>&1 &
   emulator_pid=$!
fi

attempt=0
while :; do
   device_state=
   if device_state=$(timeout 5 "$adb" -s "$emulator_serial" get-state 2>/dev/null) &&
      [ "$(printf '%s' "$device_state" | tr -d '\r')" = device ]; then
      break
   fi
   if [ -n "$emulator_pid" ] && ! kill -0 "$emulator_pid" 2>/dev/null; then
      cat "$emulator_log" >&2
      echo "Android emulator $emulator_serial exited before connecting" >&2
      exit 1
   fi
   attempt=$((attempt + 1))
   if [ "$attempt" -ge 60 ]; then
      [ -z "$emulator_pid" ] || cat "$emulator_log" >&2
      echo "Android emulator $emulator_serial did not connect" >&2
      exit 1
   fi
   sleep 1
done

attempt=0
while :; do
   boot_completed=
   if boot_completed=$(timeout 5 "$adb" -s "$emulator_serial" shell getprop sys.boot_completed 2>/dev/null) &&
      [ "$(printf '%s' "$boot_completed" | tr -d '\r')" = 1 ]; then
      break
   fi
   if [ -n "$emulator_pid" ] && ! kill -0 "$emulator_pid" 2>/dev/null; then
      cat "$emulator_log" >&2
      echo "Android emulator $emulator_serial exited before finishing boot" >&2
      exit 1
   fi
   attempt=$((attempt + 1))
   if [ "$attempt" -ge 60 ]; then
      [ -z "$emulator_pid" ] || cat "$emulator_log" >&2
      echo "Android emulator $emulator_serial did not finish booting" >&2
      exit 1
   fi
   sleep 1
done

if ! device_api=$(timeout 10 "$adb" -s "$emulator_serial" shell getprop ro.build.version.sdk); then
   echo "Unable to read the Android API level from $emulator_serial" >&2
   exit 1
fi
device_api=$(printf '%s' "$device_api" | tr -d '\r')
case "$device_api" in
   ''|*[!0-9]*)
      echo "Android emulator $emulator_serial reported invalid API level: ${device_api:-empty}" >&2
      exit 1
      ;;
esac
if [ "$device_api" -lt 24 ]; then
   echo "Android emulator $emulator_serial uses API $device_api; API 24 or newer is required" >&2
   exit 1
fi
if ! device_abis=$(timeout 10 "$adb" -s "$emulator_serial" shell getprop ro.product.cpu.abilist); then
   echo "Unable to read Android ABIs from $emulator_serial" >&2
   exit 1
fi
device_abis=$(printf '%s' "$device_abis" | tr -d '\r')
case ",$device_abis," in
   *,x86_64,*) ;;
   *)
      echo "Android emulator $emulator_serial does not support x86_64 (ABIs: ${device_abis:-empty})" >&2
      exit 1
      ;;
esac

build_output=$(mktemp)

remote_dir="/data/local/tmp/media_parser_$$_$(date +%s)"
remote_fixtures="$remote_dir/fixtures"
remote_identity_file="$remote_dir/test.identity"
timeout 10 "$adb" -s "$emulator_serial" shell mkdir -p "$remote_fixtures"
timeout 60 "$adb" -s "$emulator_serial" push "$workspace_root/crates/media-parser/tests/fixtures/." "$remote_fixtures/" >/dev/null

cargo test \
   --manifest-path "$workspace_root/Cargo.toml" \
   -p media-parser \
   --target x86_64-linux-android \
   --no-default-features \
   --features thumbnails,android-mediacodec \
   --lib --test android_mediacodec --test mp4_thumbnails \
   --no-run \
   --message-format=json-render-diagnostics >"$build_output"

run_test() {
   target_name=$1
   if ! jq_output=$(jq -r --arg target_name "$target_name" \
      'select(.target.name == $target_name and .executable != null) | .executable' \
      "$build_output"); then
      echo "Unable to read Cargo build output for $target_name" >&2
      exit 1
   fi
   test_binary=$(printf '%s\n' "$jq_output" | tail -n 1)
   if [ -z "$test_binary" ]; then
      echo "Cargo did not report an executable for $target_name" >&2
      exit 1
   fi

   remote_binary="$remote_dir/$(basename "$test_binary")"
   timeout 60 "$adb" -s "$emulator_serial" push "$test_binary" "$remote_binary" >/dev/null
   timeout 10 "$adb" -s "$emulator_serial" shell chmod 755 "$remote_binary"
   # `adb shell` reports the shell's exit status, not the test binary's, so the
   # status is echoed and read back from the captured output.
   run_log=$(mktemp)
   adb_status=0
   timeout "$test_timeout" "$adb" -s "$emulator_serial" shell \
      "sh -c 'test_pid=\$\$; test_start=\$(cut -d \" \" -f 22 /proc/\$\$/stat) || exit 125; printf \"%s %s\\n\" \"\$test_pid\" \"\$test_start\" >$remote_identity_file; exec env MEDIA_PARSER_TEST_FIXTURES=$remote_fixtures $remote_binary --test-threads=1' & test_pid=\$!; wait \"\$test_pid\"; test_status=\$?; rm -f $remote_identity_file; printf '\nEXIT=%s\n' \"\$test_status\"" \
      >"$run_log" 2>&1 || adb_status=$?
   if [ "$adb_status" -ne 0 ]; then
      stop_remote_test
      cat "$run_log"
      echo "$target_name could not complete through adb (status $adb_status)" >&2
      exit "$adb_status"
   fi
   cat "$run_log"
   status=$(tr -d '\r' <"$run_log" | sed -n 's/^EXIT=//p' | tail -n 1)
   rm -f "$run_log"
   run_log=
   case "$status" in
      ''|*[!0-9]*)
         echo "$target_name returned an invalid device status: ${status:-missing}" >&2
         exit 1
         ;;
   esac
   if [ "$status" -gt 255 ]; then
      echo "$target_name returned an invalid device status: $status" >&2
      exit 1
   fi
   if [ "$status" != 0 ]; then
      echo "$target_name failed on the emulator with status ${status:-unknown}" >&2
      exit "$status"
   fi
}

run_test media_parser
run_test android_mediacodec
run_test mp4_thumbnails
