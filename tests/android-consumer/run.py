#!/usr/bin/env python3
"""Build a minified Tauri consumer and verify two launches on a running device."""
import argparse
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--serial", help="adb serial; optional when exactly one device is attached")
parser.add_argument("--target", choices=["aarch64", "x86_64"], default="x86_64")
phase = parser.add_mutually_exclusive_group()
phase.add_argument("--build-only", action="store_true", help="build and sign the APK without a device")
phase.add_argument("--skip-build", action="store_true", help="install the APK signed by an earlier build")
args = parser.parse_args()
root = Path(__file__).resolve().parent
generated = root / "src-tauri/gen/android"
abi = "arm64" if args.target == "aarch64" else "x86_64"
signed = generated / f"consumer-{abi}-release.apk"
sdk = Path(os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT") or Path.home() / "Android/Sdk")

if not args.skip_build:
    subprocess.run(["npm", "ci"], cwd=root, check=True)
    subprocess.run(["npm", "run", "tauri", "--", "android", "init", "--ci"], cwd=root, check=True)
    app = generated / "app"
    if 'getByName("release") {\n            isMinifyEnabled = true' not in (app / "build.gradle.kts").read_text():
        raise SystemExit("Generated release must enable R8; inspect CLI template before continuing")
    # Only the rules the plugin distributes may keep its class.
    if "mediaparser" in (app / "proguard-rules.pro").read_text():
        raise SystemExit("Generated app rules mention the plugin; the test would not prove its consumer rules")
    subprocess.run(["npm", "run", "tauri", "--", "android", "build", "--apk", "--target", args.target, "--ci", "--verbose", "--", "--locked"], cwd=root, check=True)
    mapping = app / "build/outputs/mapping/universalRelease/mapping.txt"
    if not mapping.is_file() or not mapping.stat().st_size:
        raise SystemExit("R8 mapping absent: cannot count this build as minified")
    print("R8 mapping:", mapping, flush=True)
    apks = list((app / "build/outputs/apk/universal/release").glob("*-release-unsigned.apk"))
    if len(apks) != 1:
        raise SystemExit(f"Expected one {abi} unsigned release APK, found {apks}")
    build_tools = max(
        (path for path in (sdk / "build-tools").iterdir() if re.fullmatch(r"\d+(\.\d+)*", path.name)),
        key=lambda path: tuple(int(p) for p in path.name.split(".")),
    )
    keystore = generated / "consumer-test.jks"
    if not keystore.exists():
        subprocess.run(["keytool", "-genkeypair", "-keystore", str(keystore), "-storepass", "android", "-keypass", "android", "-alias", "consumer", "-dname", "CN=Local test only", "-keyalg", "RSA", "-validity", "3650"], check=True)
    subprocess.run([str(build_tools / "apksigner"), "sign", "--ks", str(keystore), "--ks-pass", "pass:android", "--out", str(signed), str(apks[0])], check=True)
    subprocess.run([str(build_tools / "apksigner"), "verify", str(signed)], check=True)
    if args.build_only:
        print("Built", signed)
        raise SystemExit(0)

if not signed.is_file():
    raise SystemExit(f"{signed} is missing; build it first")
adb = [shutil.which("adb") or str(sdk / "platform-tools/adb")]
if args.serial:
    adb += ["-s", args.serial]
package = "com.plugin.nativejpegconsumer"
subprocess.run(adb + ["install", "-r", str(signed)], check=True, timeout=180)
for launch in range(2):
    subprocess.run(adb + ["shell", "am", "force-stop", package], check=True, timeout=20)
    subprocess.run(adb + ["shell", "am", "start", "-n", package + "/.MainActivity"], check=True, timeout=20)
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        pid = subprocess.run(adb + ["shell", "pidof", package], capture_output=True, text=True, timeout=10).stdout.strip()
        if pid:
            logs = subprocess.run(adb + ["logcat", "-d", "--pid=" + pid, "-s", "NativeJpegTest:I", "AndroidRuntime:E", "*:S"], capture_output=True, text=True, check=True, timeout=10).stdout
            if "FAIL native JPEG consumer" in logs or "FATAL EXCEPTION" in logs:
                raise SystemExit(logs)
            if "PASS native JPEG consumer" in logs:
                print(f"Launch {launch + 1}: {logs.strip()}", flush=True)
                break
        time.sleep(1)
    else:
        raise SystemExit("Timed out awaiting consumer result; inspect adb logcat")
subprocess.run(adb + ["shell", "am", "force-stop", package], check=True, timeout=20)
print("PASS minified consumer, initial launch and process restart")
