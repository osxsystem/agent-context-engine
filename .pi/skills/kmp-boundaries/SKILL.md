---
name: kmp-boundaries
description: Use when common code needs to reach a platform API and you are picking the boundary shape, whether a common interface with per-platform bindings, expect/actual, or separate platform implementations. Covers capability granularity, keeping actuals thin, the Activity-owned platform-UI binding, declaring a custom intermediate source set to share code across a target subset (such as Android + Desktop JVM), and the AGP-9 constraints that shape what can live in shared code.
---

# Kotlin Multiplatform Boundary Design

Core rules for any KMP boundary:

- **Keep `commonMain` semantic**: describe *what* the product needs, not Android/iOS mechanics: `currentRegion()`, never `currentRegionFromAndroidLocale(context)`.
- **Split by capability**: `Clipboard`, `ShareSheet`, `Haptics`, `Biometrics` as separate interfaces, not one `Platform` god object.
- **Keep actuals thin**: they translate, they don't decide; a business `if`/`when` inside an actual belongs in common, tested with a fake.
- **Prefer a common `interface` + per-platform binding over `expect class`.** JetBrains' own guidance: using expect/actual classes "for simple cases where interfaces would suffice is not recommended. Interfaces offer greater flexibility, allowing for multiple implementations per platform and easier substitution in tests." Reach for an interface whenever you need fakes / DI / lifecycle / runtime selection.
- **`expect`/`actual` *functions and properties* are still the standard way to reach a platform API.** The rule above is about *classes*, not the mechanism as a whole. A one-off `expect fun currentTimeSeconds(): Long` needs no interface.
- **`expect`/`actual` classes are Beta.** They compile with a warning unless you opt in with `freeCompilerArgs.add("-Xexpect-actual-classes")`, and JetBrains warn they "may require future migration steps." One more reason an interface is the cheaper default.
- **Introduce an intermediate source set** only when two or more targets genuinely share an implementation, either one `applyDefaultHierarchyTemplate()` already creates (`iosMain`), or a custom one you declare yourself (below).

Three boundaries get the most detail below: the **Activity-owned** platform-UI boundary, the **custom intermediate source set**, and the **AGP-9 KMP-library** constraints.

**Related:** [`kmp-ktor`](../kmp-ktor/SKILL.md) (network boundary), [`compose-multiplatform-ui`](../compose-multiplatform-ui/SKILL.md) (Compose-MP mechanics and SwiftUI/UIKit interop). For the iOS↔Swift bridge, meaning `@Throws`, sealed-class exhaustiveness, SKIE, and the rest of the Swift-facing API review, see the "Swift-facing API review checklist" in [`kmp-ios-integration`](../kmp-ios-integration/SKILL.md) when authoring the iOS-side implementation.

## Platform-UI bindings are Activity-owned, not Context-owned

The single most common Android boundary mistake: passing `applicationContext` / `LocalContext.current` into a binding that actually needs an `Activity`, then papering over the lifecycle gap with `Intent.FLAG_ACTIVITY_NEW_TASK`. That flag is a smell, because it hides that this is a foreground-UI operation. Hold an `Activity` instead.

```kotlin
// commonMain — semantic interface; DOCUMENT what `suspend` means
interface ShareSheet {
    /** Launches the system share sheet. Returns when the sheet is PRESENTED — not when the user
     *  completes or cancels. (Otherwise callers write incorrect retry/confirmation logic.) */
    suspend fun shareText(text: String)
}

// androidMain — thin: build the intent and launch it. Activity-owned.
class AndroidShareSheet(private val activity: Activity) : ShareSheet {
    override suspend fun shareText(text: String) {
        activity.startActivity(Intent.createChooser(
            Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, text), null,
        ))
    }
}
```

You don't app-wide-inject an `Activity` (it's framework-created and lifecycle-bound), so construct the binding in an **activity scope** in the Android app module (Hilt `@InstallIn(ActivityComponent::class)`, where `Activity` is a default binding; Koin `scoped`). `commonMain` only ever sees the interface; the `Activity` never leaves the app module. If a longer-lived (app-scoped) object needs it, hold it behind a lifecycle-aware provider (set in `onResume`, cleared in `onPause`) so a destroyed Activity can't leak.

## Custom intermediate source sets: sharing across a target subset

Sometimes the boundary isn't "common vs platform" but "this *group* of targets vs the rest". The usual trigger is a JVM-only library, whether Jackson, OkHttp or anything JVM-bound, that Android and Desktop can both use and iOS cannot. Putting it in `commonMain` breaks the iOS build; duplicating it across `androidMain` and `jvmMain` duplicates the wiring.

`applyDefaultHierarchyTemplate()` will **not** solve this for you. Its built-in groupings cover native families (`iosMain` and friends); it deliberately does not create a shared Android+JVM source set. For any combination the template doesn't cover, JetBrains document declaring one by hand; their term is a **custom source set** under manual configuration, and their own worked example (`jvmAndMacos`) is this same shape with different targets.

```kotlin
kotlin {
    androidTarget()
    jvm()
    iosArm64()
    iosSimulatorArm64()

    applyDefaultHierarchyTemplate() // still worth calling — it creates iosMain etc. for the targets above

    sourceSets {
        val jvmAndroid by creating {
            dependsOn(commonMain.get())
            dependencies {
                api(libs.jackson.module.kotlin)   // JVM-only: fine on Android + Desktop, absent on iOS
            }
        }

        jvmMain.get().dependsOn(jvmAndroid)
        androidMain.get().dependsOn(jvmAndroid)
    }
}
```

**The intermediate set is not a platform.** It is a shared layer that platform source sets opt into via `dependsOn`. Declaring it buys you one place for the dependency and one place for the code that uses it.

**On declaration order:** there is no Gradle or KMP rule that a custom source set must be declared before `androidMain`/`jvmMain`. The only constraint is ordinary Kotlin scoping: a `val` must exist before you reference it. If you see advice framing this as a build-system requirement, it's conflating the two.

**When *not* to reach for one:** pure Kotlin belongs in `commonMain`; genuinely platform-specific APIs belong in the platform source set. An intermediate set earns its place only when two or more targets share a real implementation.

## Team heuristics: what we abstract, and what we don't

⚠️ **This section is our own convention, not JetBrains guidance.** The rules above are sourced from the official docs; the table below is accumulated team judgment. Treat it as a starting position to argue with, not an authority, since JetBrains publish no domain-category guidance of this kind either way.

| Category | Position | Why |
|---|---|---|
| Crypto, core protocol / domain logic | **Share** | Needed on every target; platform security APIs differ, so the seam is worth it |
| I/O, logging, serialization | **Usually share** | Commonly reused, and credible platform implementations exist |
| Business logic, state holders / ViewModels | **Usually share** | State and transitions are platform-agnostic; `StateFlow`/`SharedFlow` cross the boundary cleanly |
| Complex UI components | **Rarely share** | Heavy platform dependencies make the abstraction leak |
| Navigation, permissions, platform UX | **Don't share** | The paradigms differ enough that any shared API becomes a lowest common denominator |

Two failure modes this is meant to head off, in both directions:

- **Premature abstraction**: building `expect`/`actual` before a second target actually needs it, which fixes the boundary in the wrong place. Wait for the second caller.
- **Under-sharing**: duplicating domain logic across `androidMain` and `jvmMain`, so every bug is fixed twice and every test written twice. That's what `commonMain` (or an intermediate set) is for.

## AGP-9 KMP-library constraints (structural, since they shape what can live in shared code)

AGP 9 replaces `com.android.library` with **`com.android.kotlin.multiplatform.library`** for the Android side of a KMP module, and rejects `com.android.application` + `kotlin.multiplatform` outright. The new plugin enforces a single-variant architecture:

- **`BuildConfig` is unavailable**: compile-time constants come from [BuildKonfig](https://github.com/yshrsmz/BuildKonfig) or an injected `AppConfiguration` interface. Don't design `commonMain` APIs that assume `BuildConfig.X` exists.
- **No build variants**: variant-specific deps/resources/signing live in the app module; a debug/release decision surfaces as a runtime config value injected into common code, not a build-variant split inside the KMP module.
- **No NDK / JNI**: extract native (C/C++) into a separate `com.android.library` module, wrapped behind a common interface the KMP module consumes.
- **Compose-MP resources need explicit enable**: add `androidResources { enable = true }` inside `kotlin { android { … } }`, or `Res.string.*` / `Res.drawable.*` crash at runtime on Android (the build still succeeds, which is easy to miss).
- **Consumer ProGuard rules need migration**: `consumerProguardFiles("rules.pro")` from the old `android {}` block is silently dropped; use `consumerProguardFiles.add(file("rules.pro"))` in the new DSL.
- **The KMP module can't also be `com.android.application`**: the Android entry point (`MainActivity`, Application class, launcher manifest, `applicationId` / `targetSdk` / `versionCode` / `versionName`) moves to a separate `androidApp` module that depends on the shared library. `MainActivity`, app-level Hilt setup, and nav-host wiring all move out of the shared `androidMain`.
- **kapt is incompatible** with AGP 9's built-in Kotlin, so migrate annotation processors to KSP (2.3.1+), or fall back to `com.android.legacy-kapt` for processors with no KSP equivalent.

| Concern | Pre-AGP-9 (monolithic) | AGP 9 KMP library |
|---|---|---|
| `MainActivity`, Application class, launcher manifest | `androidMain` of shared module | Separate `androidApp` module |
| `applicationId`, `versionCode`, `targetSdk` | Shared module's `android {}` | `androidApp` only |
| Compile-time constants (env, flags) | `BuildConfig` field | `BuildKonfig` in common, or runtime DI |
| NDK / JNI native code | `androidMain` (any module) | Separate `com.android.library`, behind a common interface |

For migrating an existing project, see JetBrains' [`kotlin-tooling-agp9-migration`](https://github.com/Kotlin/kotlin-agent-skills/tree/main/skills/kotlin-tooling-agp9-migration) skill for the full mechanics.