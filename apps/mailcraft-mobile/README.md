# MailCraft on phones

The Android and iOS apps: one Rust core, two native shells. How the pieces fit is in
[the architecture](../../docs/architecture.md#the-phones-appsmailcraft-mobile); what they can't
do yet is in [known gaps](../../docs/gaps.md).

| Where | What |
|---|---|
| `../../crates/mobile` | The app core (Crux): screens, actions, the view model |
| `src/` | The native library: the core and the network on their own thread; JNI and C entry points |
| `android/` | The Android app: Kotlin, Jetpack Compose |
| `ios/` | The iOS app: SwiftUI |

## Android

You need the Android SDK with the NDK, a JDK (17 or newer), Gradle, and:

```bash
cargo install cargo-ndk
rustup target add aarch64-linux-android x86_64-linux-android
```

Then:

```bash
ANDROID_HOME=~/Android/Sdk apps/mailcraft-mobile/android/build.sh
adb install apps/mailcraft-mobile/android/app/build/outputs/apk/release/app-release.apk
```

The APK has the library for phones (arm64) and for the emulator (x86_64). A local build is
signed with the debug key.

## iOS

On a Mac with Xcode and [XcodeGen](https://github.com/yonaskolb/XcodeGen):

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
apps/mailcraft-mobile/ios/build.sh
open apps/mailcraft-mobile/ios/MailCraft.xcodeproj
```

Choose a signing team in Xcode, then run it on a phone or the simulator.

## The bridge

The shells call five functions, the same on both phones, with JSON text both ways:

| Call | Does |
|---|---|
| `start(dir, secrets, offset, zone)` | Opens the mail folder and starts; `secrets` is what the keystore kept |
| `send(action)` | The person's action: `{"type": "openFolder", "account": "a1", "folder": "INBOX"}` |
| `poll(timeoutMs)` | The next message, from a thread of the shell's own: `render` (the view model), `secret` / `forget` (a password for the keystore), `notice` |
| `view()` | The view model now |
| `zone(offset, zone)` | The phone's time zone changed |

The actions are `Action` in `crates/mobile/src/lib.rs`; the view model is `ViewModel` in
`crates/mobile/src/view.rs`.
