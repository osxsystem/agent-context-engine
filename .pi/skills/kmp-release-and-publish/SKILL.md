---
name: kmp-release-and-publish
description: Use when shipping a Kotlin Multiplatform app or library, covering Android release builds/Play Store (R8 with shared code), iOS archiving/TestFlight/App Store (privacy manifest, dSYMs), publishing a KMP library to Maven Central, setting up CI (GitHub Actions runner split, konan caching), or choosing which Gradle task runs which tests.
---

# KMP Release & Publish

Ship KMP apps to the stores, libraries to Maven Central, and set up CI that doesn't burn macOS minutes. Full walkthroughs with YAML/Gradle: [reference.md](reference.md).

## Android app

A KMP Android app is a normal Android app, built with `./gradlew :androidApp:bundleRelease` and standard signing, and Play Store can't tell the difference. The KMP-specific part is **R8**: shared-module code is shrunk too, so reflection users (kotlinx.serialization, Ktor, Koin, Room) in `commonMain` need keep rules. Always QA a minified build; read `missing_rules.txt` on failure. Library modules on the new AGP-KMP plugin publish consumer rules via `androidLibrary.optimization.consumerKeepRules` (the old `consumerProguardFiles` doesn't apply).

## iOS app

Archive from Xcode as usual. The build phase transparently runs the Gradle framework build, and the embedded framework is signed with the app's certificate (no separate provisioning). KMP-specific:

- **Privacy manifest** (`PrivacyInfo.xcprivacy`) covering the shared framework's required-reason API usage, a real App Store rejection risk for KMP apps.
- Upload the shared framework's **dSYMs** so crashes symbolicate to Kotlin lines.
- Bitcode is dead (Xcode 14+), so there's nothing to configure.
- Release archive triggers `linkReleaseFramework*`, noticeably slower than debug, so budget CI time.

## Library → Maven Central (2026 route)

OSSRH is gone; use the **Central Portal** (central.sonatype.com) with the **vanniktech `com.vanniktech.maven.publish`** plugin: namespace verification, GPG key (`generatePgpKeys`/`uploadPublicPgpKey`), required POM blocks (license/developers/scm), user-token credentials via `ORG_GRADLE_PROJECT_*` env vars, then `./gradlew publishToMavenCentral --no-configuration-cache`.

Rules that bite:
- **Publish everything from one macOS host**: Apple targets need it, and duplicate root-module uploads from two hosts are forbidden.
- No `-SNAPSHOT` versions on Central; use an internal repo for pre-releases.
- One version for all targets, tag ⇔ Gradle `version`; guard the common API with binary-compatibility-validator (klib ABI support included).

## CI topology (GitHub Actions)

| Job | Runner | Why |
|---|---|---|
| `jvmTest` / common tests | ubuntu | Cheapest; covers `commonTest` logic |
| Android assemble/bundle | ubuntu | No macOS needed |
| iOS link + `iosSimulatorArm64Test` + archive | **macos** (~10× cost) | Anything touching Apple targets |

Cache `~/.konan` keyed on the version-catalog hash (rolls over on Kotlin bumps); `gradle/actions/setup-gradle` handles Gradle caching. Fastlane: `gym` + `pilot` with an App Store Connect API key (iOS), `supply` to the Play internal track (Android). Fastlane needs no KMP awareness.

## Test task map

| Task | Runs |
|---|---|
| `./gradlew allTests` | Every target, aggregated HTML report |
| `jvmTest` | Cheapest full run of `commonTest` (if you keep a JVM target) |
| `iosSimulatorArm64Test` | iOS tests on a real simulator, straight from Gradle, with no Xcode invocation |
| `testDebugUnitTest` | Android local unit tests |

`commonTest` uses `kotlin.test` and multiplatform deps only; platform `actual` code gets tests in `androidHostTest`/`iosTest`.

## Common mistakes

- Skipping minified-build QA → release-only crashes from R8 stripping shared code.
- Missing privacy manifest for the shared framework → App Store rejection.
- Publishing Apple targets from Linux, or from two hosts → broken/duplicate publications.
- Running everything on macOS runners → 10× CI bill for no benefit.
- `-SNAPSHOT` to Central Portal → rejected.
