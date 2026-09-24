# iOS Integration & Swift Interop: Reference

Compiled from official kotlinlang.org docs (2026-08).

---

## 1. Project structure

### Targets, source sets, hierarchy

- **Targets** declare which platforms Kotlin compiles to, inside the `kotlin {}` block: `androidTarget()`, `iosArm64()`, `iosSimulatorArm64()`, `iosX64()`, `jvm()`, etc. Each target determines binary format, available language constructs, and allowed dependencies.
- **Source sets** are collections of sources with their own dependencies and compiler options:
  - `commonMain` / `commonTest` are compiled to every target; they can only use Kotlin stdlib + multiplatform libraries (no `java.io`, no `platform.Foundation`).
  - Per-target sets: `androidMain`, `iosArm64Main`, `iosSimulatorArm64Main`, plus matching `...Test` sets.
  - **Intermediate source sets** share code among a subset of targets: `iosMain` (device + simulator), `appleMain` (all Apple), `nativeMain` (all Kotlin/Native). In `iosMain` you can call Apple APIs like `platform.Foundation.NSUUID` because every target below it is an Apple target.
- **Visibility is one-way:** platform source sets see common code; common never sees platform code. Compiling `iosArm64` merges `commonMain → appleMain → iosMain → iosArm64Main` into one binary.

```
commonMain
    ├── androidMain
    └── appleMain
          └── iosMain
                ├── iosArm64Main
                └── iosSimulatorArm64Main
```

### Default hierarchy template

Since Kotlin 1.9.20 the **default hierarchy template** is applied automatically when you declare targets, so declaring `iosArm64()` + `iosSimulatorArm64()` gives you `iosMain`/`iosTest` (and `appleMain`, `nativeMain`) for free. Only wire `dependsOn` manually for custom groupings:

```kotlin
kotlin {
    applyDefaultHierarchyTemplate() // explicit call only needed after custom dependsOn edits

    androidTarget()
    iosArm64()
    iosSimulatorArm64()

    sourceSets {
        commonMain.dependencies {
            implementation(libs.kotlinx.coroutines.core)
        }
        commonTest.dependencies {
            implementation(kotlin("test"))
        }
        androidMain.dependencies { /* Android-only deps */ }
        iosMain.dependencies { /* iOS-only multiplatform deps, e.g. Ktor Darwin engine */ }
    }
}
```

### Gradle plugins & version catalog conventions

Typical `gradle/libs.versions.toml` for a shared + Compose Multiplatform project:

```toml
[versions]
kotlin = "2.4.10"
agp = "8.10.0"           # use the AGP version compatible with your Kotlin/CMP release
compose-multiplatform = "1.9.x"

[plugins]
kotlinMultiplatform = { id = "org.jetbrains.kotlin.multiplatform", version.ref = "kotlin" }
androidLibrary = { id = "com.android.library", version.ref = "agp" }
composeMultiplatform = { id = "org.jetbrains.compose", version.ref = "compose-multiplatform" }
composeCompiler = { id = "org.jetbrains.kotlin.plugin.compose", version.ref = "kotlin" }
kotlinCocoapods = { id = "org.jetbrains.kotlin.native.cocoapods", version.ref = "kotlin" } # only if using CocoaPods
```

Apply in the shared module's `build.gradle.kts`:

```kotlin
plugins {
    alias(libs.plugins.kotlinMultiplatform)
    alias(libs.plugins.androidLibrary)
    alias(libs.plugins.composeMultiplatform)
    alias(libs.plugins.composeCompiler)
}
```

Conventions: since Kotlin 2.0 the Compose compiler ships with Kotlin itself (`kotlin.plugin.compose`, version pinned to Kotlin). Tests run via `./gradlew iosSimulatorArm64Test`, `./gradlew testDebugUnitTest` (Android), etc. Recommended tooling in 2026: Android Studio or IntelliJ IDEA with the Kotlin Multiplatform IDE plugin (which also drives iOS run configurations), plus the KMP web wizard for project scaffolding.

---

## 2. expect/actual, and when to prefer interfaces + DI

### Mechanism

Declare an `expect` construct (function, property, object, class, enum, annotation, typealias) in `commonMain` with no body; provide a matching `actual` in each platform source set, **same package, same signature**. The compiler verifies every `expect` has an `actual` on every target.

```kotlin
// commonMain
expect fun getPlatformName(): String

// androidMain
actual fun getPlatformName(): String = "Android ${android.os.Build.VERSION.SDK_INT}"

// iosMain
actual fun getPlatformName(): String =
    UIDevice.currentDevice.systemName + " " + UIDevice.currentDevice.systemVersion
```

Actuals can be satisfied by `actual typealias` to an existing platform type (e.g. `actual typealias MyDate = java.time.LocalDate`), actual enums may add extra constants (forcing `else` branches in common `when`), and `@OptionalExpectation` lets an expected annotation be absent on some platforms.

### Status and guidance

- `expect`/`actual` **functions, properties, objects, typealiases: stable.**
- `expect`/`actual` **classes: still Beta.** Using them warns unless you add `freeCompilerArgs.add("-Xexpect-actual-classes")`.
- **Official recommendation: prefer interfaces + factory functions or DI over expect/actual classes.** Reasons:
  - An expect class locks you to exactly one implementation per platform; an interface allows several.
  - Interfaces are trivially fakeable in `commonTest`; actuals are not.
  - Standard language constructs beat compiler magic for readability and tooling.

```kotlin
// Preferred pattern
interface Logger { fun log(message: String) }

// Only the *factory* is expect/actual (a stable function, not a class):
expect fun createLogger(): Logger

// Or skip expect/actual entirely with DI (e.g. Koin):
// androidMain module { single<Logger> { AndroidLogger(get()) } }
// iosMain    module { single<Logger> { IosLogger() } }
```

**Rule of thumb:** expect/actual for small leaf utilities (UUID, time, platform name); interfaces + DI for anything with behavior, state, dependencies, or that tests need to fake.

---

## 3. iOS integration options compared

From the [iOS integration overview](https://kotlinlang.org/docs/multiplatform/multiplatform-ios-integration-overview.html):

| Method | Local/Remote | How | Pick when |
|---|---|---|---|
| **Direct integration** (`embedAndSignAppleFrameworkForXcode`) | Local | Run-script build phase invokes Gradle; Kotlin build becomes part of the Xcode build | Mono-repo, no CocoaPods needed. **Default**, and what the KMP IDE plugin and wizard set up |
| **CocoaPods (local)** | Local | Kotlin CocoaPods Gradle plugin + Podfile | You need CocoaPods *dependencies inside the KMP module*, or the iOS app is already Pod-based |
| **SPM (local package)** | Local | Wrap the framework in a local Swift package | Mono-repo where the iOS team is SwiftPM-first and there are no CocoaPods deps |
| **SPM + XCFramework (remote)** | Remote | Publish an XCFramework as a remote Swift package | Separate repos / shared code distributed as a versioned third-party dependency; SwiftPM-preferring iOS team |
| **CocoaPods + XCFramework (remote)** | Remote | `podPublish*XCFramework` tasks produce XCFramework + podspec | Same as above but the consuming ecosystem is CocoaPods |
| **KMMBridge** (Touchlab, third-party, not in the official overview) | Remote | Gradle tooling that automates building, versioning, and publishing XCFrameworks as SPM/CocoaPods packages (GitHub releases, S3, etc.) | Larger orgs where the iOS team should consume shared code as a normal binary dependency without running Gradle at all |

### Direct integration details (the default choice)

Requirements: `binaries.framework {}` declared on the iOS targets; if migrating from CocoaPods, run `pod deintegrate` and remove the `cocoapods {}` block first.

Xcode setup: add a **Run Script phase**, move it **before Compile Sources**, untick "Based on dependency analysis", and **disable User Script Sandboxing** in Build Settings (restart the Gradle daemon with `./gradlew --stop` if it was built sandboxed):

```bash
if [ "YES" = "$OVERRIDE_KOTLIN_BUILD_IDE_SUPPORTED" ]; then
  echo "Skipping Gradle build task invocation (IDE already built the framework)"
  exit 0
fi
cd "$SRCROOT/.."
./gradlew :shared:embedAndSignAppleFrameworkForXcode
```

The `OVERRIDE_KOTLIN_BUILD_IDE_SUPPORTED=YES` guard prevents double-building when the IDE launches the iOS run configuration. For custom (non Debug/Release) Xcode configurations, add a user-defined `KOTLIN_FRAMEWORK_BUILD_TYPE` setting.

### Decision guidance

- **Team of Android+iOS devs in one repo, Compose Multiplatform UI:** direct integration. Simplest, no dependency-manager overhead, and with CMP the Swift-facing surface is tiny (a `MainViewController()` entry point).
- **iOS app already on CocoaPods, or KMP module needs a Pod:** CocoaPods integration.
- **Independent iOS team that shouldn't touch Gradle:** remote XCFramework via SPM (hand-rolled `multiplatform-spm-export` setup, or KMMBridge to automate publishing).
- CocoaPods is in maintenance mode ecosystem-wide; for new remote setups prefer SPM.

---

## 4. Swift/Kotlin interop

Kotlin exports to iOS through an **Objective-C framework header** (Swift Export, the future ObjC-free path, is Alpha; see §6). Consequences:

### What maps well

| Kotlin | Swift (via ObjC) |
|---|---|
| `class` | `class` |
| `interface` | `protocol` |
| `String`, `List`, `Map` | `String`, `Array`, `Dictionary` (bridged via `NSString`/`NSArray`/`NSDictionary`) |
| `enum class` | a class with static members (NOT a Swift enum) |
| `suspend fun` | `async` function *and* completion-handler variant |
| Top-level functions in `Foo.kt` | static members on `FooKt` |
| `@Throws(...)` functions | Swift `throws` |

### Common pitfalls

- **Sealed classes lose exhaustiveness.** They export as a plain class hierarchy; Swift `switch` needs a `default` case and gives no compile-time completeness check. Fix: SKIE (generates a wrapping Swift enum + `onEnum(of:)`), or Swift Export ≥ 2.4.20 (see §6).
- **Coroutines:** `suspend` maps to completion handlers / basic `async` with **no proper cancellation**, and suspend functions are only callable from the main thread in the default interop. `Flow` exports as an opaque generic object. Fix: **SKIE** (suspend ↔ `async` with cancellation; `Flow` → `AsyncSequence`) or KMP-NativeCoroutines; use exactly one, because they conflict.
- **Default arguments disappear.** ObjC has no default args; Swift callers must pass every parameter. Mitigate with overloads or SKIE (which regenerates default-argument overloads).
- **Generics are crippled:** type parameters surface as nullable unless constrained `<T : Any>`; interfaces lose generics entirely; variance is dropped.
- **Enums aren't Swift enums**: no `switch` exhaustiveness, no `CaseIterable` (SKIE fixes this too).
- **Primitives box:** `Int?` becomes `KotlinInt?`; `List<Int>` becomes `[KotlinInt]`.
- **Collection bridging overhead** on hot paths, so cast to `NSDictionary`/`NSArray` when profiling shows it matters.
- **Exceptions:** un-`@Throws`-annotated Kotlin exceptions **crash** the app when they cross into Swift. Annotate throwing API with `@Throws(Exception::class)`.
- **Subclassing:** only `final`-friendly patterns; overriding ObjC initializers needs `@OverrideInit`; clashing overrides need `@ObjCSignatureOverride`. Inline/value classes don't export properly.

### Naming and surface control

```kotlin
@ObjCName(swiftName = "OrderStore")           // rename for Swift
class OrderStoreImpl { 
    @ObjCName("index") fun indexOf(@ObjCName("of") element: String): Int = TODO()
}

@HiddenFromObjC          // keep internal-ish API out of the framework header
fun kotlinOnlyHelper() {}

@ShouldRefineInSwift     // exported as __-prefixed; write a hand-rolled Swift wrapper
fun rawApi(): Any = TODO()
```

KDoc is exported into the header (Xcode autocomplete shows it); disable with `exportKdoc.set(false)` if needed.

**SKIE** (Touchlab, [skie.touchlab.co](https://skie.touchlab.co/)) is the de-facto standard polish layer: sealed-class enums, real async/await with cancellation, `Flow` → `AsyncSequence`, default arguments, exhaustive enums. Caveats: don't mix its coroutine interop with KMP-NativeCoroutines, and completion-handler call sites stop compiling once SKIE's async interop is on.

**Design advice:** keep the exported surface deliberately small and ObjC-friendly, a thin facade in `commonMain` (view models / repositories returning simple types), with `@HiddenFromObjC` on the rest. With Compose Multiplatform the surface often shrinks to one `fun MainViewController(): UIViewController`.

---

## 5. Recommended Gradle config snippets

### Framework configuration (direct integration, CMP app)

```kotlin
kotlin {
    androidTarget()

    listOf(iosArm64(), iosSimulatorArm64()).forEach { target ->
        target.binaries.framework {
            baseName = "Shared"
            isStatic = true          // see trade-offs below
        }
    }
}
```

### Static vs dynamic

- **Static (`isStatic = true`)**: linked into the app binary. Simpler embedding (no dylib signing/copying), no dynamic-linker startup cost, and the usual choice for Compose Multiplatform templates. App binary is larger.
- **Dynamic (`isStatic = false`, the default of `binaries.framework`)**: required if several app targets/extensions must share one copy at runtime; otherwise adds embedding complexity.
- Recommendation: **static for a single-app setup**; dynamic only with a concrete reason (app extensions, multiple frameworks sharing the runtime).

### Exporting dependencies into the framework

Only `api` dependencies of the source set can be exported; without `export()` their types surface with mangled names or not at all:

```kotlin
kotlin {
    sourceSets.commonMain.dependencies {
        api(project(":core-models"))
        api("io.github.kotlin:example-lib:1.0")
    }
    iosArm64().binaries.framework {
        baseName = "Shared"
        export(project(":core-models"))
        export("io.github.kotlin:example-lib:1.0")
        // transitiveExport = true  // avoid: bloats binary and compile time
    }
}
```

### XCFramework (for remote SPM/CocoaPods distribution)

```kotlin
import org.jetbrains.kotlin.gradle.plugin.mpp.apple.XCFramework

kotlin {
    val xcf = XCFramework()
    listOf(iosArm64(), iosSimulatorArm64()).forEach { target ->
        target.binaries.framework {
            baseName = "Shared"
            xcf.add(this)
        }
    }
}
// Tasks: assembleXCFramework, assembleSharedReleaseXCFramework, assembleSharedDebugXCFramework
// With CocoaPods plugin: podPublishReleaseXCFramework (XCFramework + podspec)
```

### Info.plist metadata

```kotlin
binaries.framework {
    binaryOption("bundleId", "com.example.shared")
    binaryOption("bundleShortVersionString", "1.0")
    binaryOption("bundleVersion", "2")
}
```

---

## 6. Pitfalls & best practices as of 2026

**Ecosystem state:** Kotlin 2.4.10 stable; K2 compiler is long-standard; default hierarchy template means no manual `dependsOn` for standard layouts; Compose compiler is bundled with Kotlin; Compose Multiplatform for iOS has been stable since 1.8.

**Swift Export (the headline change in motion):** the official ObjC-free path ([docs](https://kotlinlang.org/docs/native-swift-export.html)) is **Alpha**. It already handles nullability without boxing, suspend functions, and `Flow` → `AsyncSequence` natively; **Kotlin 2.4.20-Beta (July 2026) added sealed-class → Swift enum mapping (exhaustive `switch`) and cross-language inheritance** (implement a Kotlin interface in Swift and pass it back). Touchlab's June 2026 guidance: teams writing substantial native SwiftUI on shared Kotlin should **stay on SKIE for now**; Compose Multiplatform teams have a tiny interop surface and can adopt Swift Export earlier. Track it, don't bet production on it yet.

**Checklist of recurring traps:**

1. Forgetting `@Throws` on Kotlin API that throws → hard crash in the iOS app instead of a catchable error.
2. Mixing coroutine interop layers (SKIE + KMP-NativeCoroutines) → conflicts; pick one.
3. Using expect/actual classes broadly → Beta warnings and rigid design; prefer interfaces + DI, keep expect/actual for leaf functions.
4. User Script Sandboxing left on in Xcode → direct-integration script silently fails; disable it and `./gradlew --stop`.
5. Exporting `implementation` dependencies → `export()` requires `api`; otherwise consumers see `Kotlinx_coroutines_coreFlow`-style mangled types.
6. `transitiveExport = true` as a shortcut → binary-size and build-time bloat; export explicitly.
7. Unconstrained generics (`<T>`) → everything nullable in Swift; constrain `<T : Any>` where possible.
8. Big Kotlin API surface exported wholesale → slow header generation, ugly Swift; use `@HiddenFromObjC` and a facade.
9. Assuming iOS unit tests run like Android's. Run `./gradlew iosSimulatorArm64Test`; keep logic in `commonTest` against interfaces.
10. Version drift between Kotlin, AGP, and Compose Multiplatform. Pin via the version catalog and consult the official compatibility table when bumping.

---

## Sources

- [Get started with KMP](https://kotlinlang.org/docs/multiplatform/get-started.html) *(fetch returned title only)*
- [iOS integration methods overview](https://kotlinlang.org/docs/multiplatform/multiplatform-ios-integration-overview.html)
- [Discover your project (structure)](https://kotlinlang.org/docs/multiplatform/multiplatform-discover-project.html)
- [Expected and actual declarations](https://kotlinlang.org/docs/multiplatform/multiplatform-expect-actual.html)
- [Share code on platforms](https://kotlinlang.org/docs/multiplatform/multiplatform-share-on-platforms.html)
- [Direct integration](https://kotlinlang.org/docs/multiplatform/multiplatform-direct-integration.html)
- [Build final native binaries](https://kotlinlang.org/docs/multiplatform/multiplatform-build-native-binaries.html)
- [Interoperability with Swift/Objective-C](https://kotlinlang.org/docs/native-objc-interop.html)
- [Swift export (Alpha)](https://kotlinlang.org/docs/native-swift-export.html)
- [Kotlin releases](https://kotlinlang.org/docs/releases.html)
- [SKIE](https://skie.touchlab.co/) · [SKIE features](https://skie.touchlab.co/features/)
- [Touchlab: The Future of KMP's iOS Interop](https://touchlab.co/the-future-of-kmps-ios-interop)
- [Kotlin-Swift interopedia](https://github.com/kotlin-hands-on/kotlin-swift-interopedia)
