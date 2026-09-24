---
name: kmp-module-setup
description: Use when creating, auditing, or upgrading a Kotlin Multiplatform shared module, declaring targets/source sets, wiring the version catalog (Kotlin/AGP/Compose Multiplatform), configuring the iOS framework block, or deciding between expect/actual and interfaces + DI for platform-specific code.
---

# KMP Module Setup

Scaffold or audit a shared Kotlin Multiplatform module: targets, source-set hierarchy, version catalog, framework config, and platform-abstraction choices. Full details with worked config in [reference.md](reference.md).

## Source-set hierarchy

Declaring `androidTarget()`, `iosArm64()`, `iosSimulatorArm64()` gives you the default hierarchy free (Kotlin ≥1.9.20), with no manual `dependsOn`:

```
commonMain ─┬─ androidMain
            └─ appleMain ── iosMain ─┬─ iosArm64Main
                                     └─ iosSimulatorArm64Main
```

Visibility is one-way: platform code sees common; common never sees platform. `commonMain` takes only multiplatform deps; `iosMain` may use `platform.*` Apple APIs.

## Version catalog (the compatibility contract)

Pin Kotlin, AGP, and Compose Multiplatform together in `gradle/libs.versions.toml` and bump them together against the official compatibility table, because version drift between the three is the top setup failure.

```toml
[versions]
kotlin = "2.4.10"                 # check current stable
agp = "8.10.0"
compose-multiplatform = "1.11.1"  # maps to a specific Jetpack Compose release

[plugins]
kotlinMultiplatform = { id = "org.jetbrains.kotlin.multiplatform", version.ref = "kotlin" }
androidLibrary = { id = "com.android.library", version.ref = "agp" }
composeMultiplatform = { id = "org.jetbrains.compose", version.ref = "compose-multiplatform" }
composeCompiler = { id = "org.jetbrains.kotlin.plugin.compose", version.ref = "kotlin" }  # ships with Kotlin since 2.0
```

## Framework block (iOS)

```kotlin
listOf(iosArm64(), iosSimulatorArm64()).forEach { target ->
    target.binaries.framework {
        baseName = "Shared"
        isStatic = true   // default choice for a single app; dynamic only for app extensions / shared runtime
    }
}
```

Exporting types from other modules requires `api(...)` + `export(...)`; `implementation` deps surface as mangled names (`Kotlinx_coroutines_coreFlow`). Avoid `transitiveExport = true` (binary bloat).

## expect/actual vs interface + DI

| Situation | Use |
|---|---|
| Leaf utility (UUID, clock, platform name) | `expect fun` / `actual fun` |
| Anything with behavior, state, deps, or that tests fake | interface in common + platform impls via DI (Koin etc.), or `expect fun createX(): X` factory |
| expect/actual **classes** | Avoid: still Beta, warns without `-Xexpect-actual-classes`; official docs recommend interfaces |

## Verification

- `./gradlew :shared:compileKotlinIosSimulatorArm64 :shared:compileDebugKotlinAndroid`: both targets compile.
- `./gradlew allTests` or `iosSimulatorArm64Test` + `testDebugUnitTest`, since common tests run on every target.

## Common mistakes

- Manual `dependsOn` wiring for standard layouts, which the default hierarchy template already does.
- `implementation` + `export()` → build error or mangled Swift types; must be `api`.
- Unpinned versions ("latest everything") → Kotlin/AGP/CMP incompatibility; always go through the catalog.
- Writing `expect class` for services → rigid, untestable; prefer interfaces.
- Java APIs (`java.io`, `java.time`) in `commonMain` → only compiles for Android; use kotlinx libraries.
