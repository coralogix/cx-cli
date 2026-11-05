# Mobile SDKs

Mobile SDK APIs change between releases. **Fetch the platform's install page before writing code**: with `cx`, `cx docs fetch <path>`; without it, open `https://coralogix.com/docs/<path without .md>/`. Use the package's latest version from its registry, not a version remembered from training data.

| Platform | Package | Docs path |
|---|---|---|
| React Native (≥ 0.73) | npm `@coralogix/react-native-plugin` (replaces the older `@coralogix/react-native-sdk`) | `user-guides/rum/sdk-installation/react-native/react-native-plugin.md` |
| Flutter (≥ 3.27) | pub `cx_flutter_plugin` (`flutter pub add cx_flutter_plugin`) | `user-guides/rum/sdk-installation/flutter/mobile.md` |
| iOS / tvOS (iOS 13+) | Swift Package `https://github.com/coralogix/cx-ios-sdk` (product `Coralogix`; `SessionReplay` for replay), or CocoaPods `pod 'Coralogix'` | `user-guides/rum/sdk-installation/apple/ios.md` |
| Android (minSdk 24) | Gradle `com.coralogix:android-sdk` | `user-guides/rum/sdk-installation/android.md` |

Every platform takes the same core values as the browser: `application`, `version`, `environment`, the RUM `public_key`, and the region as `coralogixDomain` (or the platform's equivalent enum — check the docs page).

## Where to init and where the key lives

| Platform | Init | Key and config |
|---|---|---|
| React Native | `await CoralogixRum.init({...})` as early as possible in the app entry (`index.js` / `App.tsx`, or the root `_layout.tsx` with Expo Router). `init` is async. | Use the project's existing config mechanism: `react-native-config` (`.env`), Expo `EXPO_PUBLIC_*` env vars or `app.config.*` `extra`. `version` from `app.json`/`package.json` version or the native build number. |
| Flutter | `CxFlutterPlugin.initSdk(...)` in `main()` after `WidgetsFlutterBinding.ensureInitialized()` (check the docs for the exact call) | `--dart-define=CORALOGIX_RUM_PUBLIC_KEY=...` read with `String.fromEnvironment`. Android release builds also need the `INTERNET` permission in the **main** `AndroidManifest.xml`. |
| iOS | In `application(_:didFinishLaunchingWithOptions:)` or the SwiftUI `App` `init()` | An `.xcconfig` value surfaced through `Info.plist`; `version` from `CFBundleShortVersionString`. |
| Android | In the `Application` subclass's `onCreate()` (create one and register it in the manifest if missing) | `buildConfigField` from `local.properties` / Gradle property / env var; `version` from `versionName`. API 24–25 need core library desugaring. |

## React Native with Expo

`@coralogix/react-native-plugin` contains native code (an iOS podspec and an Android Gradle module) and ships no Expo config plugin, so it **does not run in Expo Go**. The app needs a development build: `npx expo install @coralogix/react-native-plugin`, then `npx expo prebuild` (or EAS Build) and `npx expo run:ios` / `run:android`. Tell the user this before installing, since it changes how they run the app. Plain bare React Native apps need `cd ios && pod install` after installing.

## Session Replay on mobile

Opt-in, as on web. iOS needs the separate `SessionReplay` product/pod; check each platform's docs page for its replay and masking options and default to masking all text and inputs.

## Build check

- React Native: `npx expo export --platform android` (Expo) or `npx react-native bundle --platform android --dev false --entry-file index.js --bundle-output /tmp/rn.bundle` confirms the JS side bundles; a native build (`run:android`, Xcode) needs the user's toolchain.
- Flutter: `flutter build apk --debug` (or `flutter analyze` without an Android SDK).
- iOS: `xcodebuild -scheme <scheme> -destination 'generic/platform=iOS Simulator' build`.
- Android: `./gradlew assembleDebug`.

Source maps / symbol uploads for each platform: `references/source-maps.md` → "Mobile".
