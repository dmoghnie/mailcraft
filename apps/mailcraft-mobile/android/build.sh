#!/usr/bin/env bash
# MailCraft for Android: the Rust library for each phone architecture, then the app around it.
#   ANDROID_HOME   the Android SDK (with the NDK installed in it, or ANDROID_NDK_HOME set)
#   GRADLE         the gradle to run (default: `gradle` on the PATH)
# Needs `cargo install cargo-ndk` and `rustup target add aarch64-linux-android x86_64-linux-android`.
# The APK: app/build/outputs/apk/release/app-release.apk (signed with the debug key).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
: "${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    ANDROID_NDK_HOME="$(ls -d "$ANDROID_HOME"/ndk/* 2>/dev/null | sort -V | tail -1)"
    export ANDROID_NDK_HOME
fi
cd "$here/../../.."
cargo ndk -t arm64-v8a -t x86_64 -P 26 -o "$here/app/src/main/jniLibs" build --release -p mailcraft-mobile-native
cd "$here"
"${GRADLE:-gradle}" --no-daemon assembleRelease "$@"
