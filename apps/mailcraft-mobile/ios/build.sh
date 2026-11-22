#!/usr/bin/env bash
# MailCraft for iOS: the Rust library for iPhones and the simulator, as one XCFramework the
# Xcode project (project.yml, made with XcodeGen) links. Runs on a Mac with Xcode, after
# `rustup target add aarch64-apple-ios aarch64-apple-ios-sim`.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
cd "$here/../../.."
for t in aarch64-apple-ios aarch64-apple-ios-sim; do
    cargo build --release --target "$t" -p mailcraft-mobile-native
done
rm -rf "$here/MailCraftCore.xcframework"
xcodebuild -create-xcframework \
    -library target/aarch64-apple-ios/release/libmailcraftcore.a -headers "$here/MailCraftCore/include" \
    -library target/aarch64-apple-ios-sim/release/libmailcraftcore.a -headers "$here/MailCraftCore/include" \
    -output "$here/MailCraftCore.xcframework"
cd "$here"
xcodegen generate
echo "Open MailCraft.xcodeproj in Xcode, choose a team for signing, and run."
