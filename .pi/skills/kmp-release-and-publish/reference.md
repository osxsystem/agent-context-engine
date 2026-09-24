# KMP Release & Publish: Reference

Compiled from official kotlinlang.org docs (2026-08).

---

## 1. Publishing the Android app from a KMP project

**The official position is simple: it's a normal Android app.** The shared module produces an ordinary Android library (AAR), and the `androidApp` module is a standard Android Gradle module. Release builds, signing, and Play Store upload follow the [standard Android publishing docs](https://developer.android.com/studio/publish) with no KMP-specific steps.

Practical checklist:

```kotlin
// androidApp/build.gradle.kts
android {
    signingConfigs {
        create("release") {
            storeFile = file(System.getenv("KEYSTORE_PATH") ?: "release.keystore")
            storePassword = System.getenv("KEYSTORE_PASSWORD")
            keyAlias = System.getenv("KEY_ALIAS")
            keyPassword = System.getenv("KEY_PASSWORD")
        }
    }
    buildTypes {
        release {
            isMinifyEnabled = true
            isShrinkResources = true
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfig = signingConfigs.getByName("release")
        }
    }
}
```

- Build the Play Store artifact with `./gradlew :androidApp:bundleRelease` (AAB is mandatory for new Play Store apps; `assembleRelease` gives an APK for sideload/QA).
- Play Store specifics (tracks, listings, review) are unchanged by KMP or Compose Multiplatform, and Google can't tell the difference from a Jetpack Compose app.

### ProGuard / R8 with Compose Multiplatform

R8 runs only on the Android target (it operates on class files; KMP has no ProGuard equivalent for iOS, since Kotlin/Native release binaries get their own dead-code elimination instead). Points that bite KMP teams specifically:

- **Shared-module code is shrunk too.** Anything in `commonMain`/`androidMain` reached via reflection, kotlinx.serialization, Ktor, Room, Koin etc. needs keep rules just like in a single-platform app. R8 failures typically only surface in release builds, so always QA a minified build, and read R8's `missing_rules.txt` when it fails.
- **Shipping consumer rules from a KMP library module** changed with the new [`com.android.kotlin.multiplatform.library` plugin](https://developer.android.com/kotlin/multiplatform/plugin): the old `consumerProguardFiles` in `android.defaultConfig` (published by default with `com.android.library`) is replaced by an explicit opt-in:

```kotlin
kotlin {
    androidLibrary {
        optimization {
            consumerKeepRules.publish = true
            consumerKeepRules.files.add(project.file("proguard-rules.pro"))
        }
    }
}
```

- Add rules incrementally, dependency by dependency; most crashes-in-release-only stem from one library's reflection use ([community writeup](https://medium.com/@ali.cse233/part-1-navigating-the-maze-of-obfuscation-in-kotlin-compose-multiplatform-projects-android-side-2a34bacf9c68), [Kotlin Slack thread](https://slack-chats.kotlinlang.org/t/29832644/does-anyone-know-how-to-declare-a-kmp-library-module-proguar)).

---

## 2. Publishing the iOS app

The iOS app is a normal Xcode project; publishing follows the standard [Apple submission flow](https://developer.apple.com/ios/submit/) (archive in Xcode → Organizer → distribute to App Store Connect). The KMP-specific parts:

**Embedded shared framework.** The shared Kotlin module compiles to a native `.framework` that must be linked and embedded in the app. Two supported integration routes:

1. **Kotlin CocoaPods Gradle plugin**: exposes the shared module as a pod dependency.
2. **Manual/direct integration**: an Xcode build phase runs `./gradlew :shared:embedAndSignAppleFrameworkForXcode`, which builds the framework for the active architecture/configuration and lets Xcode sign it along with the app. This is what the default KMP project wizard generates. Signing and provisioning are then entirely standard: the embedded framework is signed with the app's certificate during archiving, with no separate provisioning for the framework.

**Archiving.** Xcode's Release configuration triggers the `linkReleaseFramework*` Gradle tasks (optimized Kotlin/Native binary, noticeably slower to build than debug). Archive from `iosApp/iosApp.xcworkspace` (or `.xcodeproj`), not from Gradle.

**App config without Xcode.** The KMP template routes bundle ID and app name through `iosApp/Configuration/Config.xcconfig` (`BUNDLE_ID`, `APP_NAME`); everything else (icons, capabilities, entitlements) is edited in Xcode.

**Symbols and crash reporting.** Release iOS frameworks built from Kotlin include `.dSYM` files by default, so crash reports symbolicate down to Kotlin source lines. Upload dSYMs (including the shared framework's) to your crash reporter / App Store Connect as usual. See [Kotlin/Native debugging](https://kotlinlang.org/docs/native-debugging.html#debug-ios-applications).

**Bitcode:** not a consideration anymore, since Apple deprecated and removed bitcode (Xcode 14+); Kotlin/Native no longer embeds it. Nothing to configure.

**Privacy manifests (real rejection risk).** Since Apple's Spring 2024 policy, missing/incomplete privacy manifests can get the app rejected, and KMP apps need special attention because Kotlin/Native and common libraries touch "required-reason" APIs. Follow [Privacy manifest for iOS apps](https://kotlinlang.org/docs/apple-privacy-manifest.html): typically you add a `PrivacyInfo.xcprivacy` to the app covering the shared framework's API usage.

**TestFlight automation:** the docs point at a [TeamCity pipeline guide](https://kotlinlang.org/docs/multiplatform/ios-ci-cd-teamcity.html); GitHub Actions + Fastlane equivalents are covered in §4.

---

## 3. Publishing KMP libraries to Maven

### Publication anatomy (`maven-publish`)

Applying `maven-publish` to a KMP module makes the Kotlin plugin auto-create one publication **per target buildable on the current host**, plus a root publication:

```kotlin
plugins {
    id("maven-publish")
}
group = "com.example"
version = "1.0"

publishing {
    repositories {
        maven { /* url, credentials */ }
    }
}
```

For `group = "test"`, project `lib`, targets `jvm()` + `iosArm64()` you get:

- Target publications: `test:lib-jvm:1.0`, `test:lib-iosarm64:1.0`, which contain the platform artifact; non-JVM targets ship the **klib** format (Kotlin's serialized IR + metadata, the portable library format for Native/JS/Wasm and common metadata).
- Root publication `test:lib:1.0` (`kotlinMultiplatform`): the entry point consumers depend on. It carries Gradle Module Metadata referencing every target publication (variant-aware resolution picks the right one), plus an empty-ish JAR without classifier so Maven Central's "must have a JAR" rule is satisfied.

Useful tasks: `publishAllPublicationsTo<RepoName>Repository`, `publishToMavenLocal`, or per-target `publishJvmPublicationTo<RepoName>`.

Sources jars are published by default; disable globally or per target with `withSourcesJar(publish = false)` (on `kotlin {}` or on an individual target).

Android target inside a library published this way (new AGP-KMP plugin):

```kotlin
kotlin {
    androidLibrary {
        namespace = "org.example.library"
        compileSdk = libs.versions.android.compileSdk.get().toInt()
        minSdk = libs.versions.android.minSdk.get().toInt()
    }
}
```

### Host requirements

- Any host can *produce* klibs for Apple targets, **but macOS is required** when the library has cinterop dependencies, uses CocoaPods integration, or needs to build/test final Apple binaries. In practice: **publish everything from a single macOS machine/runner.**
- Single-host publishing also avoids duplicate publications (e.g., the `kotlinMultiplatform` root module uploaded from two hosts), which **Maven Central explicitly forbids**.

### Maven Central via the Central Portal (the 2026 route)

Legacy OSSRH is gone; accounts go through [central.sonatype.com](https://central.sonatype.com). The official tutorial standardizes on the **vanniktech `com.vanniktech.maven.publish` plugin**, which wraps `maven-publish`, signing, and the Central Portal upload API:

1. **Namespace verification**: claim `io.github.<username>` (verify by creating a repo named after the verification key) or a reverse-DNS domain (verify via TXT record) at [Maven Central Namespaces](https://central.sonatype.com/publishing/namespaces).
2. **GPG key pair**: either `gpg --full-generate-key` or the Kotlin Gradle plugin's built-in: `./gradlew -Psigning.password=... generatePgpKeys --name "Name <email>"`. Upload the public key (`gpg --keyserver keyserver.ubuntu.com --send-keys <ID>` or `./gradlew uploadPublicPgpKey`), export the private key with `gpg --armor --export-secret-keys <ID> > key.gpg`.
3. **Plugin config**:

```kotlin
plugins {
    id("com.vanniktech.maven.publish") version "0.36.0"
}

mavenPublishing {
    publishToMavenCentral()
    signAllPublications()
    coordinates(group.toString(), "fibonacci", version.toString())
    pom {
        name = "Fibonacci library"
        description = "A mathematics calculation library."
        inceptionYear = "2024"
        url = "https://github.com/kotlin-hands-on/fibonacci/"
        licenses { license { name = "The Apache License, Version 2.0"; url = "https://www.apache.org/licenses/LICENSE-2.0.txt" } }
        developers { developer { id = "kotlin-hands-on"; name = "..." } }
        scm { url = "..."; connection = "scm:git:git://..."; developerConnection = "scm:git:ssh://..." }
    }
}
```

   License, developers, and scm blocks are **required** by [Central's POM rules](https://central.sonatype.org/publish/requirements/). Versions cannot end in `-SNAPSHOT`.
4. **Pre-flight checks**: `./gradlew checkSigningConfiguration` (public key reachable on keyservers) and `./gradlew checkPomFileForMavenPublication`.
5. **Credentials**: generate a [user token](https://central.sonatype.com/usertoken) (not your portal password) and feed everything via `ORG_GRADLE_PROJECT_*` env vars: `mavenCentralUsername`, `mavenCentralPassword`, `signingInMemoryKeyId` (last 8 chars of key ID), `signingInMemoryKeyPassword`, `signingInMemoryKey` (armored private key contents).
6. **Publish**: `./gradlew publishToMavenCentral --no-configuration-cache` (the plugin doesn't support configuration cache), then manually press **Publish** on the [Deployments dashboard](https://central.sonatype.com/publishing/deployments) once validated, or use `publishAndReleaseToMavenCentral` for full automation. Availability takes ~15-30 min (search indexing longer).

Official publish workflow from the tutorial (release-triggered, macOS runner because of the Apple targets):

```yaml
# .github/workflows/publish.yml
name: Publish
on:
  release:
    types: [released, prereleased]
jobs:
  publish:
    name: Release build and publish
    runs-on: macOS-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-java@v4
        with: { distribution: 'zulu', java-version: 21 }
      - name: Publish to MavenCentral
        run: ./gradlew publishToMavenCentral --no-configuration-cache
        env:
          ORG_GRADLE_PROJECT_mavenCentralUsername: ${{ secrets.MAVEN_CENTRAL_USERNAME }}
          ORG_GRADLE_PROJECT_mavenCentralPassword: ${{ secrets.MAVEN_CENTRAL_PASSWORD }}
          ORG_GRADLE_PROJECT_signingInMemoryKeyId: ${{ secrets.SIGNING_KEY_ID }}
          ORG_GRADLE_PROJECT_signingInMemoryKeyPassword: ${{ secrets.SIGNING_PASSWORD }}
          ORG_GRADLE_PROJECT_signingInMemoryKey: ${{ secrets.GPG_KEY_CONTENTS }}
```

Reference implementation: [Kotlin/multiplatform-library-template](https://github.com/Kotlin/multiplatform-library-template/). Published KMP libraries get discoverable on [klibs.io](https://klibs.io/).

---

## 4. CI/CD patterns

### Job topology (official recommendation)

The [official GitHub Actions guide](https://kotlinlang.org/docs/multiplatform/github-actions-for-kmp.html) recommends a **composite action** for shared JDK/Gradle setup plus three jobs: cheap Ubuntu runners for everything except the iOS build (macOS runners cost ~10× per minute):

```yaml
# .github/actions/gradle-setup/action.yml
name: gradle-setup
runs:
  using: "composite"
  steps:
    - uses: actions/setup-java@v4
      with: { java-version: "17", distribution: "temurin" }
    - uses: gradle/actions/setup-gradle@v5.0.0   # handles Gradle caching automatically
```

```yaml
# .github/workflows/build.yml
env:
  GRADLE_OPTS: "-Dorg.gradle.jvmargs=-Xmx4096M -Dorg.gradle.daemon=false -Dorg.gradle.parallel=true -Dorg.gradle.caching=true"
jobs:
  test:                    # shared logic tests on the JVM — cheapest runner
    runs-on: ubuntu-latest
    steps: [checkout, gradle-setup, run: ./gradlew jvmTest, upload test reports]
  build-android:
    runs-on: ubuntu-latest
    needs: test
    steps: [checkout, gradle-setup, run: ./gradlew :mobile:assembleDebug, upload APK]
  build-ios:
    runs-on: macos-latest  # xcodebuild requires macOS
    needs: test
    steps:
      - uses: actions/checkout@v4
      - uses: ./.github/actions/gradle-setup
      - run: |
          xcodebuild build -project iosApp/iosApp.xcodeproj \
            -scheme iosApp -configuration Debug -sdk iphonesimulator \
            -derivedDataPath ./build
```

Cost-control patterns from the community ([KMP Bits](https://www.kmpbits.com/posts/drafts/kmp-github-actions), [Marco Gomiero's series](https://www.marcogomiero.com/posts/2024/kmp-ci-ios/)):

- Run `commonTest` on the JVM (`jvmTest`) on Ubuntu; reserve macOS for framework linking (`linkDebugFrameworkIosSimulatorArm64`, which is fast) and optionally `linkReleaseFrameworkIosSimulatorArm64` to catch release-only linker/DCE issues.
- Run `iosSimulatorArm64Test` on macOS when you have iOS-specific `actual` code worth testing on the simulator (see §5).

### Caching

- **Gradle**: `gradle/actions/setup-gradle` handles it; add `gradle-home-cache-cleanup: true` to keep the cache lean.
- **Konan** (`~/.konan`, the Kotlin/Native prebuilt compiler + platform dependencies, hundreds of MB, re-downloaded otherwise on every macOS job):

```yaml
- uses: actions/cache@v4
  with:
    path: ~/.konan
    key: ${{ runner.os }}-konan-${{ hashFiles('**/libs.versions.toml') }}
```

Keying on the version catalog hash makes the cache roll over automatically on Kotlin upgrades.

### Fastlane, TestFlight, Play internal track

The official docs don't cover Fastlane (they show raw `xcodebuild`, plus a TeamCity tutorial); the de-facto community pattern ([Gomiero](https://www.marcogomiero.com/posts/2024/kmp-ci-ios/), [Android variant](https://www.marcogomiero.com/posts/2024/kmp-ci-android/)) is:

- **iOS → TestFlight**: Fastlane `match` (or manual keychain import of cert + provisioning profile in the workflow) → `gym`/`build_app` to archive, because the Xcode build phase transparently runs the Gradle framework build, so Fastlane needs no KMP awareness → `pilot`/`upload_to_testflight` with an **App Store Connect API key** (no 2FA headaches). Runs on `macos-latest`.
- **Android → Play internal track**: `./gradlew :androidApp:bundleRelease` with the keystore decoded from a base64 secret → Fastlane `supply` (`upload_to_play_store(track: "internal")`) with a Play service-account JSON, or the `r0adkll/upload-google-play` action. Runs on Ubuntu.
- Keep secrets in GitHub Actions secrets: base64 keystore, keystore passwords, ASC API key, match repo token, Play service-account JSON.

The one genuinely KMP-specific CI fact: **anything touching Apple targets (framework link, iOS tests, library publishing with Apple targets) must run on macOS; everything else should not.**

---

## 5. Testing across platforms

From the [testing docs](https://kotlinlang.org/docs/multiplatform/multiplatform-run-tests.html):

**Common tests** live in `commonTest`, use the `kotlin.test` library (platform-agnostic `@Test`, `assertEquals`, `assertContains`, …), and run on **every** target with that target's native runner: JUnit on JVM/Android host tests, the Kotlin/Native test runner on iOS:

```kotlin
// shared/build.gradle.kts
sourceSets {
    commonTest.dependencies {
        implementation(libs.kotlin.test)
    }
}
```

```kotlin
// shared/src/commonTest/kotlin/GrepTest.kt
class GrepTest {
    @Test
    fun shouldFindMatches() {
        val results = mutableListOf<String>()
        grep(sampleData, "[a-z]+") { results.add(it) }
        assertEquals(2, results.size)
    }
}
```

**Platform tests** exercise `actual` implementations: `androidHostTest` (new AGP-KMP layout; local JVM unit tests, not instrumented) and `iosTest`/`iosSimulatorArm64Test` source sets, still written with `kotlin.test` but free to use platform APIs (`Platform.osFamily`, `System.getProperty`, …). Instrumented Android tests come via `withDeviceTestBuilder` in the `androidLibrary {}` block.

**Gradle tasks:**

- `./gradlew allTests`: every target's tests, with an aggregated HTML report at `build/reports/tests/allTests/index.html` (Android unit-test reports are generated separately and not merged in).
- `./gradlew iosSimulatorArm64Test`: **runs iOS tests on an actual iOS Simulator directly from Gradle**; Gradle boots the default simulator itself, no Xcode invocation needed (macOS + Xcode installed required). The simulator device can be overridden per-test-run in the target's testRuns configuration if you need a specific device/OS.
- `./gradlew testDebugUnitTest` / `testReleaseUnitTest`: Android local unit tests.
- `./gradlew jvmTest`: if you keep a JVM target, the cheapest way to run all common tests in CI.

Best practices from the docs: only multiplatform libraries in `commonTest`; don't touch the `Asserter` type directly; run the suite on every framework/target you ship, since runtime behavior (and physics like scrolling/inertia in UI tests) differs per platform.

---

## 6. Versioning strategy for shared libraries

(The official docs prescribe mechanics more than strategy; this synthesizes the docs' constraints with established ecosystem practice.)

- **One version for all targets.** The `kotlinMultiplatform` root module and every target publication share `group:version` and are published atomically from one host. Never version `-jvm`/`-iosarm64` artifacts independently, because consumers resolve through the root module and Gradle metadata; skew breaks resolution.
- **SemVer, driven by the common API.** The shared library's public API surface in `commonMain` is the contract both apps consume. Breaking `expect`/public API → major; additive → minor. Enforce mechanically with JetBrains' **binary-compatibility-validator** (now with klib ABI support for non-JVM targets) so accidental breaks fail CI rather than surprise the iOS build.
- **Mind klib/ABI coupling to the Kotlin version.** klibs are more tightly coupled to compiler versions than JARs; document the Kotlin version each release is built with, and bump at least a minor when upgrading Kotlin. (Kotlin 2.x forward compatibility has improved this, but consumers on older Kotlin can still fail to read newer klibs.)
- **No `-SNAPSHOT` to Maven Central** (Portal rejects it). For internal pre-releases use an internal repo (GitHub Packages, Artifactory) or `-alpha.N`/`-rc.N` semver pre-release tags; `mavenLocal` for day-to-day iteration.
- **Apps consuming the shared code:** if the shared module lives in the same repo (typical for a KMP app), version it with the app via git; library versioning only becomes real when teams split repos or ship the shared SDK to third parties. In that split-repo world, the iOS side consumes either the Maven klib (via KMP tooling) or a distributed XCFramework, and an XCFramework release should carry the same version number as the Maven release, published from the same tag, ideally via SPM (`Package.swift` pinned per release).
- Match the release tag to the Gradle `version` (the official workflow triggers publishing off GitHub Releases, so tag `1.2.0` ⇔ `version = "1.2.0"` is the natural invariant to enforce in CI).

---

### Source list

- https://kotlinlang.org/docs/multiplatform/multiplatform-publish-apps.html
- https://kotlinlang.org/docs/multiplatform/multiplatform-publish-libraries-to-maven.html
- https://kotlinlang.org/docs/multiplatform/multiplatform-publish-lib-setup.html
- https://kotlinlang.org/docs/multiplatform/multiplatform-run-tests.html
- https://kotlinlang.org/docs/multiplatform/github-actions-for-kmp.html
- https://kotlinlang.org/docs/apple-privacy-manifest.html
- https://developer.android.com/kotlin/multiplatform/plugin
- https://www.marcogomiero.com/posts/2024/kmp-ci-ios/ and /2024/kmp-ci-android/
- https://www.kmpbits.com/posts/drafts/kmp-github-actions
- https://github.com/Kotlin/multiplatform-library-template/
