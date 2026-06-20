#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

export ANDROID_NDK_VERSION="${ANDROID_NDK_VERSION:-29.0.14033849}"

# shellcheck source=env-android.sh
source "$ROOT/scripts/env-android.sh"

PROFILE="${ANDROID_PROFILE:-release}"
TARGETS="${ANDROID_TARGETS:-arm64-v8a}"

echo "Building libvadadee_berry.so (${PROFILE}, no desktop features)..."
cargo ndk -t "${TARGETS}" -o app/src/main/jniLibs build \
    --"${PROFILE}" \
    --lib \
    --no-default-features

echo "Building APK..."
./gradlew assembleDebug

echo "APK: app/build/outputs/apk/debug/app-debug.apk"