---
name: compose-multiplatform-ui
description: Use when building shared UI with Compose Multiplatform, deciding between a full-Compose iOS shell and a native SwiftUI shell (iOS 26 Liquid Glass), wiring per-platform entry points (MainActivity, MainViewController, ComposeUIViewController configure), using composeResources/Res, Navigation Compose or Navigation 3 or ViewModel in commonMain, embedding SwiftUI/UIKit in Compose (or vice versa), or debugging iOS-specific Compose issues (a startup crash naming CADisableMinimumFrameDurationOnPhone, frame rate caps, laggy or swallowed touches on native views, VoiceOver skipping interop views, viewModel() crashing on iOS).
---

# Compose Multiplatform UI

Shared Compose UI across Android and iOS. Written against **Compose Multiplatform 1.11.x** (Jetpack Compose 1.11.x, Kotlin 2.1+, iOS 14+). Full detail, version tables, and the worked Liquid Glass migration: [reference.md](reference.md).

CMP builds on Jetpack Compose. On Android it resolves to Google's real `androidx` artifacts; other targets use JetBrains ports at `org.jetbrains.androidx.*`. Jetpack knowledge transfers directly, so read this as the list of **iOS-shaped deltas**, not a Compose tutorial.

Build wiring lives next door: the Gradle module and iOS framework block in `kmp-module-setup`, Xcode integration and the Swift-facing API in `kmp-ios-integration`, platform-capability boundaries in `kmp-boundaries`.

## Decide the iOS shell before writing screens

This choice shapes navigation, entry points, and the Swift side at once, and reversing it later is a rewrite rather than a refactor. Two shapes:

- **Full-Compose shell** (default): one `ComposeUIViewController` owns everything, tabs and back stack included. Simplest, most code shared, and what the KMP web wizard generates.
- **Native SwiftUI shell**: SwiftUI owns `TabView` and `NavigationStack`, Compose renders one screen per destination, so you export both a tab-root and a per-route view controller and bridge navigation events in both directions. This is the only way to get **iOS 26 Liquid Glass** chrome, because those effects are drawn by the native containers and Compose has no way to imitate them.

Start full-Compose unless the app has to feel current on iOS 26 or already leans on native navigation affordances. Shipping both behind `if #available(iOS 26.0, *)` is a supported middle path, and it keeps the full-Compose route as the pre-26 fallback.

## Entry points

- **Common**: root `@Composable fun App()` in `commonMain`, wrapped in `MaterialTheme`.
- **Android**: `MainActivity` calls `setContent { App() }`.
- **iOS**: `fun MainViewController(): UIViewController = ComposeUIViewController { App() }` in `iosMain`; Swift hosts it via `UIViewControllerRepresentable`.
- **Required on iOS**: `CADisableMinimumFrameDurationOnPhone = true` in `Info.plist`. Since 1.7.0 this is enforced, so a missing or `false` value **crashes the app at startup**. The silent cap below ProMotion refresh rates is what you get only if the check was opted out via `enforceStrictPlistSanityCheck`.

`ComposeUIViewController(configure = { ... })` is the one lever for iOS-specific behaviour, so reach for it before reaching for a workaround:

| Knob | What it is for |
|---|---|
| `opaque = false` | transparent background over native content, at the cost of extra blending work |
| `onFocusBehavior` | `DoNothing` stops Compose scrolling to a focused input when native chrome owns the layout |
| `enforceStrictPlistSanityCheck` | opt out of the plist crash above; leave it on unless you know why you are not |
| `parallelRendering` | opt-in before 1.11; default-on from 1.11.0, so interop-heavy screens regress here first |

There is no accessibility-sync knob to reach for: since 1.8.0 the iOS accessibility tree is built lazily on first request from the accessibility engine and disposed when interaction stops, so VoiceOver and Voice Control work with no configuration.

## Dependencies

Declare **direct Maven coordinates** in the version catalog. The Gradle plugin's `compose.*` aliases (`compose.ui`, `compose.material3`) are deprecated from 1.10.0-beta01, so `implementation(compose.material3)` is exactly the muscle memory that now writes deprecated code. A CMP BOM is planned but has not shipped, so versions stay pinned by hand.

## Resources

```
src/commonMain/composeResources/
├── drawable/   images, vector XML
├── font/       font files (singular)
├── values/     strings.xml (NOT a strings/ directory)
└── files/      anything else
```

The directory names are exact and there is no `strings/` or `fonts/`: strings live in `values/strings.xml`, and a locale qualifier goes on that directory (`values-es/strings.xml`). A misnamed directory generates nothing and reports nothing, which is why this is worth getting right the first time.

Generated `Res` class: `stringResource(Res.string.app_name)`, `painterResource(Res.drawable.icon)`, `Res.readBytes("files/data.json")` (suspend), `Res.getUri(...)` when a platform API or an external library needs a real path. Density and theme qualifiers work the same way. In a multi-module project `compose.resources { nameOfResClass = "MyRes" }` keeps two generated `Res` classes from colliding, and `customDirectory(sourceSetName, directoryProvider)` maps a generated or downloaded directory into a source set, following the same internal layout.

## Navigation and ViewModel in common code

Two multiplatform navigation libraries, and the shell decision above picks between them:

- **`org.jetbrains.androidx.navigation:navigation-compose`** (2.9.x, Stable everywhere): `rememberNavController()`, `NavHost`, type-safe `@Serializable` routes. Same API as Jetpack. Default choice.
- **Navigation 3** (`NavDisplay` over a back stack you own as a list): launched Alpha in CMP 1.10, with stable 1.1.x artifacts as of 1.11. Reach for it when something outside Compose has to read or drive the back stack, which is what the native SwiftUI shell needs. **ViewModels are not scoped per entry by default** and stay tied to the Activity, so pass `entryDecorators = listOf(rememberSaveableStateHolderNavEntryDecorator(), rememberViewModelStoreNavEntryDecorator())` or state outlives the destination that owned it.

ViewModel (`org.jetbrains.androidx.lifecycle:lifecycle-viewmodel-compose`): **on iOS you cannot call `viewModel()` with no arguments**, because the type reflection it needs does not exist there, so always pass an initializer: `viewModel { OrderViewModel() }`. That form works on every target, so there is no reason to branch by platform. Collect with `collectAsStateWithLifecycle()`, now backed by multiplatform Lifecycle.

## Native UI interop (both directions)

- SwiftUI **in** Compose: pass a `() -> UIViewController` factory (wrapping `UIHostingController`) from Swift into Kotlin, render it with `UIKitViewController(factory = ...)`.
- UIKit views in Compose: `UIKitView(factory = { MKMapView() }, update = { ... })`, where `update` reruns on observed state change; `@ObjCAction` for target-action callbacks.
- Compose in UIKit/SwiftUI: `ComposeUIViewController` is a plain `UIViewController`, so push it, tab it, or wrap it in `UIViewControllerRepresentable`.

`UIKitInteropProperties` is where interop bugs actually get fixed, and its defaults are the surprising part:

- Touches run through a **cooperative 150ms delay** so the parent composable can claim the gesture first. A native view that feels laggy, or a scroll that fights an embedded map, wants `interactionMode = UIKitInteropInteractionMode.NonCooperative` (immediate, the pre-1.7 behaviour). `null` makes the view inert.
- `isNativeAccessibilityEnabled` is **off** by default, so a native view with its own VoiceOver support has to opt in, and VoiceOver skipping an interop view is usually this and not a semantics bug.
- `placedAsOverlay = true` draws the native view above Compose, which a transparent native layer or a shader needs, at the cost of covering composables underneath.

The parameter is `properties =`, and has been since the API shipped in 1.7.0. (`interopProperties =` appears in one JetBrains migration-doc snippet and is a typo there, not an older name.)

## Iteration and verification loop

- **Hot reload is desktop/JVM-only**, bundled and on by default since 1.10. Use the desktop target as the fast sandbox for common UI, then verify Android and iOS. Other targets are still "under exploration", so build the loop around desktop rather than waiting.
- Judge iOS scrolling and animation on **release** builds on a real device; debug Kotlin/Native is much slower and invents problems that never ship.
- `Modifier.testTag` maps to `accessibilityIdentifier`, so XCTest UI automation and `performAccessibilityAudit()` work against Compose screens.

## Common mistakes

- No-arg `viewModel()` on iOS, which fails at runtime; use the initializer lambda.
- Missing `CADisableMinimumFrameDurationOnPhone`, which crashes at startup rather than degrading quietly; reading the crash as a Compose bug is the trap.
- A `strings/` or `fonts/` directory under `composeResources`, which generates nothing silently; the names are `values/` and `font/`.
- Judging performance on debug builds.
- Android-only artifacts (`androidx.compose.*`) declared in `commonMain`; use the `org.jetbrains.*` ports.
- `compose.*` Gradle aliases in new code; write the coordinate instead.
- Navigation 3 without entry decorators, which scopes ViewModels to the Activity instead of the destination.
- Fighting Compose to imitate Liquid Glass; move the chrome to SwiftUI instead.
- Blaming semantics when VoiceOver skips a native interop view; check `isNativeAccessibilityEnabled` first.
- Streaming large files through `Res.readBytes`, which is unsupported; use `Res.getUri()` plus platform APIs.
