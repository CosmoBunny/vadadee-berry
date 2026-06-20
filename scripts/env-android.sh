# Source this before Android builds:
#   source scripts/env-android.sh

if [[ -z "${ANDROID_SDK_ROOT:-}" && -z "${ANDROID_HOME:-}" ]]; then
    for candidate in "$HOME/Android/Sdk" "/opt/android-sdk"; do
        if [[ -d "$candidate" ]]; then
            export ANDROID_SDK_ROOT="$candidate"
            break
        fi
    done
fi

if [[ -z "${ANDROID_SDK_ROOT:-}" && -z "${ANDROID_HOME:-}" ]]; then
    echo "Set ANDROID_SDK_ROOT (or ANDROID_HOME) to your Android SDK path." >&2
    return 1 2>/dev/null || exit 1
fi

_sdk="${ANDROID_SDK_ROOT:-$ANDROID_HOME}"

if [[ "${ANDROID_NDK_ROOT:-}" == *$'\n'* ]]; then
    echo "ANDROID_NDK_ROOT contains a newline; unsetting broken value." >&2
    unset ANDROID_NDK_ROOT
fi

_resolve_ndk() {
    local ndk=""

    if [[ -n "${ANDROID_NDK_VERSION:-}" ]]; then
        ndk="$_sdk/ndk/$ANDROID_NDK_VERSION"
    elif [[ -n "${NDK_HOME:-}" && -d "$NDK_HOME" && -f "$NDK_HOME/source.properties" ]]; then
        ndk="$NDK_HOME"
    elif [[ -n "${ANDROID_NDK_ROOT:-}" && -d "$ANDROID_NDK_ROOT" && -f "$ANDROID_NDK_ROOT/source.properties" ]]; then
        ndk="$ANDROID_NDK_ROOT"
    elif [[ -d "$_sdk/ndk" ]]; then
        ndk="$(find "$_sdk/ndk" -mindepth 1 -maxdepth 1 -type d -printf '%f\n' 2>/dev/null | sort -V | tail -1)"
        ndk="$_sdk/ndk/$ndk"
    fi

    if [[ -z "$ndk" || ! -f "$ndk/source.properties" ]]; then
        echo "Could not find a valid NDK under $_sdk/ndk" >&2
        return 1 2>/dev/null || exit 1
    fi

    export ANDROID_NDK_ROOT="$ndk"
    export ANDROID_NDK_HOME="$ndk"
    export NDK_HOME="$ndk"
}

_resolve_ndk

echo "ANDROID_SDK_ROOT=${ANDROID_SDK_ROOT:-$ANDROID_HOME}"
echo "ANDROID_NDK_ROOT=$ANDROID_NDK_ROOT"