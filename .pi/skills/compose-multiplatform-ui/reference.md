# Compose Multiplatform: Reference

Compiled from official kotlinlang.org and JetBrains multiplatform-dev docs (2026-08), against Compose Multiplatform 1.11.x.

1. [Creating a Compose Multiplatform App](#1-creating-a-compose-multiplatform-app): project creation, module layout, per-platform entry points, running
2. [Compose Multiplatform vs Jetpack Compose](#2-compose-multiplatform-vs-jetpack-compose): relationship, differences, version table, breaking changes
3. [Resources, Navigation, ViewModel in Common Code](#3-resources-navigation-viewmodel-in-common-code): `Res`, navigation-compose, Navigation 3, ViewModel gotchas
4. [Interop with Native iOS UI](#4-interop-with-native-ios-ui): both directions, `ComposeUIViewController(configure)`, `UIKitInteropProperties`, text input
5. [The iOS Shell: Full-Compose vs Native SwiftUI](#5-the-ios-shell-full-compose-vs-native-swiftui-liquid-glass): the Liquid Glass decision and the worked migration
6. [Build and Xcode Integration](#6-build-and-xcode-integration): pointer to the sibling skills that own it
7. [Hot Reload, Previews, and iOS-Specific Concerns](#7-hot-reload-previews-and-ios-specific-concerns): the iteration loop, performance, accessibility

---

## 1. Creating a Compose Multiplatform App

### Project creation

Two official routes:

- **Kotlin Multiplatform IDE plugin** in IntelliJ IDEA / Android Studio: *File | New | Project → Kotlin Multiplatform*, pick targets (Android, iOS, Desktop, Web) and enable the **Share UI** option for iOS/web.
- **KMP web wizard** at [kmp.jetbrains.com](https://kmp.jetbrains.com/) generates a downloadable project with the same layout.

### Module layout

The generated project (full-target template) contains:

| Module | Role |
|---|---|
| `shared` (or `composeApp`) | Kotlin Multiplatform module: shared logic + shared Compose UI (Gradle) |
| `androidApp` | Android application wrapper |
| `iosApp` | Xcode project producing the iOS app |
| `desktopApp` | JVM desktop app |
| `webApp` | Kotlin/JS and Kotlin/Wasm apps |

Inside the shared module, source sets split by platform: `commonMain` (shared Kotlin + Compose), `androidMain`, `iosMain` (Kotlin/Native), `jvmMain` (desktop), `jsMain`, `wasmJsMain`.

### Entry points per platform

- **Common:** a root `@Composable fun App()` in `commonMain` (e.g. `shared/src/commonMain/kotlin/App.kt`) wrapping content in `MaterialTheme`.
- **Android:** a `MainActivity` in `androidMain`/`androidApp` calling `setContent { App() }`.
- **iOS:** a function in `iosMain` returning a `UIViewController`:

```kotlin
fun MainViewController(): UIViewController = ComposeUIViewController { App() }
```

The Swift side (`iosApp`) hosts this view controller, via SwiftUI's `UIViewControllerRepresentable` or directly in UIKit (see §4).

- **Desktop:** a `main()` using `application { Window { App() } }` in `jvmMain`.
- **Web:** `ComposeViewport(document.body) { App() }`.

Template `App.kt` (abbreviated, from the docs):

```kotlin
@Composable
@Preview
fun App() {
    MaterialTheme {
        var showContent by remember { mutableStateOf(false) }
        Column(
            modifier = Modifier
                .background(MaterialTheme.colorScheme.primaryContainer)
                .safeContentPadding()
                .fillMaxSize(),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Button(onClick = { showContent = !showContent }) { Text("Click me!") }
            AnimatedVisibility(showContent) {
                val greeting = remember { Greeting().greet() }
                Image(painterResource(Res.drawable.compose_multiplatform), null)
                Text("Compose: $greeting")
            }
        }
    }
}
```

### Running

- **Android:** run the `androidApp` configuration against an emulator/device.
- **iOS:** run the `iosApp` configuration against a simulator; for physical devices set the Team ID in Xcode and enable developer mode on the phone.
- **Desktop:** `desktopApp [hot] 🔥` configuration ships with Compose Hot Reload by default (CMP 1.10.0+).
- **Web:** `webApp[js]` / `webApp[wasmJs]`, serves at `http://localhost:8080/`. For cross-browser compatibility builds: `./gradlew composeCompatibilityBrowserDistribution`.

---

## 2. Compose Multiplatform vs Jetpack Compose

**Relationship:** Compose Multiplatform (JetBrains) builds directly on Jetpack Compose (Google), sharing the same compiler, runtime, and core APIs (`@Composable`, `remember`, modifiers, layout, animation, Material/Material3). Jetpack Compose knowledge transfers directly.

**Key mechanism:** on the Android target, CMP resolves to **Google's official Jetpack artifacts** (`androidx.compose.material3:material3`); on other targets it uses JetBrains' ports (`org.jetbrains.compose.material3:material3`). Gradle Module Metadata handles the swap automatically, so Android builds are literally running Jetpack Compose.

**Differences:**
- Platforms: CMP = Android + iOS + desktop + web; Jetpack Compose = Android only.
- Not available in CMP: `androidx.compose.runtime.rxjava2/rxjava3`, Android-only components, and platform-specific APIs (desktop window handling, iOS UIKit interop exist only per-platform).
- Jetpack-only ecosystem pieces (e.g. Maps Compose) need Android-specific handling; CMP libraries are consumable from plain Android Jetpack apps (backward compatible).
- Jetpack multiplatform ports available from JetBrains: Lifecycle, ViewModel, Navigation Compose (`org.jetbrains.androidx.*` coordinates).

**Versioning (as of 2026):**
- **Current stable Compose Multiplatform: 1.11.1**, mapping to **Jetpack Compose 1.11.2**.
- CMP releases ship separately from Kotlin/Jetpack Compose, typically **1-3 months after** the corresponding Jetpack Compose release.
- Kotlin: the compatibility page states minimum 2.1.0, but the 1.10.0 release notes state Kotlin **2.2 is required for native and web targets**. For an iOS-first project treat 2.2+ as the floor and do not rely on the 2.1.0 figure. K2 compiler mandatory since CMP 1.8.0.
- Recent mapping table (CMP → Jetpack Compose): 1.11.1 → 1.11.2; 1.10.3 → 1.10.5; 1.9.3 → 1.9.4; 1.8.2 → 1.8.2; 1.7.3 → 1.7.6.
- Supported platforms at 1.11.1: Android 5.0+ (API 21), **iOS 14+**, macOS 13 arm64+, Windows 10+, Ubuntu 20.04+, WasmGC browsers. 64-bit only.

### Breaking changes worth knowing

| Since | Change | What to do |
|---|---|---|
| 1.10.0-beta01 | Gradle-plugin dependency aliases (`compose.ui`, `compose.material3`, …) **deprecated** | Declare direct Maven coordinates in the version catalog. A CMP BOM is planned but not shipped, so pin versions by hand. |
| 1.11.0 | **Concurrent rendering on by default** (was the opt-in `parallelRendering`, introduced 1.8.0; `useSeparateRenderThreadWhenPossible` is changelog prose, never a real property name) | Rendering moves to a dedicated thread. Interop-heavy screens are where regressions show first; the 1.8-era guidance was that parallel rendering helps most *without* UIKit interop. |
| 1.10.0 | `WindowInsetsRulers` plus a unified inset implementation across platforms | Positioning against status bar / nav bar / keyboard is now consistent between `WindowInsets` and `WindowInsetsRulers`. |
| 1.10.0 | Compose Hot Reload bundled and enabled by default for desktop targets | Drop the manual `org.jetbrains.compose.hot-reload` plugin unless pinning an older version. |
| 1.7.0 | iOS interop touch handling became **cooperative** (150ms delay) | See §4 for `UIKitInteropProperties.interactionMode`. |

---

## 3. Resources, Navigation, ViewModel in Common Code

### Resources (`compose.resources` / `Res` class)

Library: `org.jetbrains.compose.components:components-resources` (included automatically when using the Compose Gradle plugin; explicit dependency needed for library modules).

Directory layout under each source set:

```
src/commonMain/composeResources/
├── drawable/   # images, vector XML
├── values/     # strings.xml (per-locale via qualifiers, e.g. values-es/)
├── font/       # font files (singular)
└── files/      # raw files
```

A `Res` class is **generated at build time**. Usage:

```kotlin
Text(stringResource(Res.string.app_name))
Image(painterResource(Res.drawable.icon), contentDescription = null)
Text("Hello", fontFamily = FontFamily(Font(Res.font.custom_font)))
val bytes: ByteArray = Res.readBytes("files/data.json")   // suspend
val uri = Res.getUri("files/video.mp4")                    // for platform APIs / external libs
```

Notes:
- Qualifiers supported for language, screen density, theme.
- **Multi-module resources** work with Kotlin 2.0+ and Gradle 7.6+, so resources can live in any module/source set.
- Web resource loading is async; most other reads are synchronous on the caller thread. Large-file streaming isn't supported, so use `Res.getUri()` and hand it to system APIs.
- On iOS, resources land in `UNLOCALIZED_RESOURCES_FOLDER_PATH`, never in `.lproj` directories, so `CFBundleLocalizations` does not affect them. Locale selection is Compose Resources' own runtime filtering.

Gradle knobs in the `compose.resources {}` block:

```kotlin
compose.resources {
    // avoid two generated `Res` classes colliding in a multi-module project
    nameOfResClass = "MyRes"

    // map an extra directory into a source set
    customDirectory(
        sourceSetName = "jvmMain",
        directoryProvider = provider { layout.projectDirectory.dir("desktopResources") }
    )

    // or one a Gradle task populates (generated or downloaded files)
    customDirectory(
        sourceSetName = "iosMain",
        directoryProvider = tasks.register<DownloadRemoteFiles>("downloadedRemoteFiles")
            .map { it.outputDir.get() }
    )
}
```

`Res.getUri` is what hands a resource to something that needs a real path, a `WebView` being the common case:

```kotlin
val uri = Res.getUri("files/webview/index.html")
AndroidView(factory = { WebView(it) }, update = { it.loadUrl(uri) })
```

### Navigation

Multiplatform Navigation is **Stable** across Android, iOS, desktop, and web.

```kotlin
commonMain.dependencies {
    implementation("org.jetbrains.androidx.navigation:navigation-compose:2.9.2")
}
```

Same API as Jetpack: `rememberNavController()`, `NavHost`, type-safe `@Serializable` routes:

```kotlin
@Serializable data object StartScreen
@Serializable data class Patient(val name: String, val age: Long)

@Composable
fun App() {
    val navController = rememberNavController()
    NavHost(navController, startDestination = StartScreen) {
        composable<StartScreen> { /* ... */ }
        composable<Patient> { backStackEntry -> /* ... */ }
    }
}
```

- Each back-stack entry is a `LifecycleOwner` (RESUMED when settled, STARTED while transitioning).
- Web: `navController.bindToBrowserNavigation()` syncs routes to the URL fragment and browser back/forward; `@SerialName("start")` gives readable URLs.
- Deep links have their own doc page (`compose-navigation-deep-links.html`); type-safe routes need `org.jetbrains.kotlinx:kotlinx-serialization-json` and the serialization plugin.
- Reference samples: JetBrains `nav_cupcake` example and the KotlinConf app.

### Navigation 3

Supported in Compose Multiplatform **1.10 and later**, covering the UI libraries, ViewModel integration, and Material 3 adaptive layouts. It launched as Alpha on non-Android targets in 1.10; as of 1.11 the artifacts are stable 1.1.x and the docs carry no alpha label. Navigation 3 treats destinations as items in a list you own, which is what makes it the right fit when something outside Compose has to read or drive the back stack, the native SwiftUI shell in §5 being the motivating case.

```kotlin
NavDisplay(
   entryDecorators = listOf(
       rememberSaveableStateHolderNavEntryDecorator(),  // saves Compose state per entry
       rememberViewModelStoreNavEntryDecorator()        // scopes ViewModel per entry
   ),
   backStack = backStack,
   entryProvider = entryProvider { }
)
```

⚠️ Without `entryDecorators`, **ViewModels are not scoped to navigation entries**: they stay tied to the Activity and survive navigation, so state leaks into the next visit to a destination. Nothing warns about it. In a multiplatform project the ViewModel decorator comes from `org.jetbrains.androidx.lifecycle:lifecycle-viewmodel-navigation3` (the JetBrains port), and the saveable one from `androidx.navigation3:navigation3-runtime`, which Google publishes multiplatform; there is no `org.jetbrains` runtime artifact, since JetBrains ships only `navigation3-ui`.

### ViewModel / Lifecycle in common code

```toml
[versions]
androidx-viewmodel = "2.10.0"
[libraries]
androidx-lifecycle-viewmodel-compose = { module = "org.jetbrains.androidx.lifecycle:lifecycle-viewmodel-compose", version.ref = "androidx-viewmodel" }
```

```kotlin
@Composable
fun CupcakeApp(viewModel: OrderViewModel = viewModel { OrderViewModel() }) {
    val uiState by viewModel.uiState.collectAsStateWithLifecycle()
}
```

Gotchas:
- On non-JVM platforms (iOS in particular) you **cannot call `viewModel()` with no arguments**, because type reflection is unavailable, so always pass an initializer lambda or explicit factory. The lambda form works on every target, so there is no reason to branch by platform.
- Prefer `collectAsStateWithLifecycle()` (from `org.jetbrains.androidx.lifecycle:lifecycle-runtime-compose`) over `collectAsState()`: multiplatform Lifecycle now backs it on every target, so collection stops when the host is not started.
- Desktop needs `kotlinx-coroutines-swing` so `Dispatchers.Main.immediate` works for ViewModel coroutines.
- The canonical shape is a `ViewModel` in `commonMain` exposing `StateFlow<UiState>`, consumed identically by every platform UI. How much sits below it is a separate call: fully shared ViewModels, or shared repositories only with per-platform ViewModels.

---

## 4. Interop with Native iOS UI

Bidirectional, both with SwiftUI and UIKit.

### Compose inside SwiftUI

```kotlin
fun MainViewController(): UIViewController = ComposeUIViewController {
    Box(Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        Text("This is Compose code", fontSize = 20.sp)
    }
}
```

```swift
struct ComposeViewController: UIViewControllerRepresentable {
    func makeUIViewController(context: Context) -> UIViewController {
        Main_iosKt.MainViewController()
    }
    func updateUIViewController(_ uiViewController: UIViewController, context: Context) {}
}
```

**Required:** add `CADisableMinimumFrameDurationOnPhone = true` to `Info.plist`. Since **1.7.0** its presence is strictly enforced: the app **crashes at startup** when the key is missing or `false`. Opting out via `enforceStrictPlistSanityCheck = false` turns the crash back into the old silent symptom, a frame rate capped below ProMotion, so the crash is the friendlier of the two.

### `ComposeUIViewController(configure = { ... })`

Almost every iOS-specific behaviour is a property on this block rather than something to work around in composables:

| Property | Effect |
|---|---|
| `opaque = false` | transparent view background, so native content shows through. Costs extra blending per frame. |
| `enforceStrictPlistSanityCheck` | `false` opts out of the startup crash on a missing `CADisableMinimumFrameDurationOnPhone`, trading it for a silent frame-rate cap |
| `onFocusBehavior` | `OnFocusBehavior.DoNothing` stops Compose scrolling content when a text field gains focus, which is what you want when native chrome owns the layout |
| `parallelRendering` | render on a separate thread. Opt-in before 1.11, default-on from 1.11.0. |

```kotlin
fun MainViewController(): UIViewController = ComposeUIViewController(
    configure = { onFocusBehavior = OnFocusBehavior.DoNothing },
) { App() }
```

### SwiftUI inside Compose

Pass a `UIViewController` factory from Swift (wrapping SwiftUI in `UIHostingController`) into a Kotlin entry point that uses `UIKitViewController`:

```kotlin
@OptIn(ExperimentalForeignApi::class)
fun ComposeEntryPointWithUIViewController(
    createUIViewController: () -> UIViewController
): UIViewController = ComposeUIViewController {
    Column(Modifier.fillMaxSize()) {
        Text("SwiftUI inside Compose")
        UIKitViewController(factory = createUIViewController, modifier = Modifier.size(300.dp))
    }
}
```

```swift
Main_iosKt.ComposeEntryPointWithUIViewController(createUIViewController: {
    UIHostingController(rootView: VStack { Text("SwiftUI in CMP") })
})
```

### UIKit both directions

- Compose in UIKit: the `ComposeUIViewController` is just a `UIViewController`, so drop it into a `UINavigationController`/`UITabBarController`.
- UIKit in Compose: `UIKitView(factory = { MKMapView() }, modifier = ..., update = { ... })`. `factory` creates the view, `update` runs when observed Compose state changes; `@ObjCAction` is needed for Objective-C target-action callbacks. Docs show worked examples for maps, `UITextField` two-way binding, `AVCaptureSession` camera, and `WKWebView`.

Two-way `UITextField` binding, as the canonical shape for state that lives on both sides:

```kotlin
@OptIn(ExperimentalForeignApi::class)
@Composable
fun UseUITextField(modifier: Modifier = Modifier) {
    var message by remember { mutableStateOf("Hello, World!") }
    UIKitView(
        factory = {
            val textField = object : UITextField(CGRectMake(0.0, 0.0, 0.0, 0.0)) {
                @ObjCAction fun editingChanged() { message = text ?: "" }
            }
            textField.addTarget(
                target = textField,
                action = NSSelectorFromString(textField::editingChanged.name),
                forControlEvents = UIControlEventEditingChanged
            )
            textField
        },
        modifier = modifier.fillMaxWidth().height(30.dp),
        update = { it.text = message },   // Compose state -> UIKit
    )
}
```

A native view hosting a `CALayer` (camera preview, video) needs `layoutSubviews()` overridden to resize both `layer` and the sublayer, wrapped in a `CATransaction` with `kCATransactionDisableActions` so the resize does not animate.

### `UIKitInteropProperties`: where interop bugs get fixed

Passed as `properties = UIKitInteropProperties(...)`, which is the parameter name since the API shipped in 1.7.0. (`interopProperties =` shows up in one JetBrains migration-doc snippet; it is a typo there, not an older name, so do not write it.) The defaults are the surprising part:

| Property | Default | Why it bites |
|---|---|---|
| `interactionMode` | `Cooperative` (tunable delay, **150ms**) | Since 1.7.0, Compose holds a touch briefly so the parent composable can claim the gesture first. Symptom: the native view feels laggy, or a scroll fights an embedded map. `UIKitInteropInteractionMode.NonCooperative` restores immediate pass-through (pre-1.7 behaviour, experimental); `null` makes the view inert. |
| `isNativeAccessibilityEnabled` | **off** | Compose does not taint the merged subtree with native accessibility resolution. Symptom: VoiceOver skips a native view that has perfectly good accessibility support of its own. Opt in only for views that need it, since it is not free. |
| `placedAsOverlay` | `false` | `true` draws the native view above the Compose UI, which is what a transparent native layer or a shader effect needs, at the cost of covering composables underneath. |

```kotlin
@OptIn(ExperimentalComposeUiApi::class)   // the UIKitInteropProperties constructor is experimental
@Composable
fun NativeMap(modifier: Modifier = Modifier) {
    UIKitViewController(
        modifier = modifier,
        update = {},
        factory = { factory.createNativeMap() },
        properties = UIKitInteropProperties(placedAsOverlay = true),
    )
}
```

### iOS text input

`PlatformImeOptions` reaches the native UIKit text input traits from common code:

```kotlin
BasicTextField(
    value = "", onValueChange = {},
    keyboardOptions = KeyboardOptions(
        platformImeOptions = PlatformImeOptions { keyboardType(UIKeyboardTypeEmailAddress) }
    )
)
```

Compose adopts the Compose window-insets model and imitates it on iOS for safe areas, so the software keyboard sits slightly differently than on Android. Check on both platforms that it does not cover anything important.

---

## 5. The iOS Shell: Full-Compose vs Native SwiftUI (Liquid Glass)

**The decision.** In the standard setup a single `ComposeUIViewController` owns the whole UI, navigation and tabs included. **Liquid Glass** (the iOS 26 translucency/fluidity design system) is rendered by the system through native containers, `TabView` and `NavigationStack`, so there is no Compose API and no shader that reproduces it. Getting it means inverting ownership: SwiftUI owns the navigation chrome, Compose renders individual screens.

Requires Xcode 26+ and the iOS 26 SDK. No Liquid-Glass-specific code is needed once the native containers are in place: the system applies the effects. The pattern scales to any number of tabs.

```text
Before                          After
ContentView                     ContentView
  └── ComposeView                 └── TabView  (Liquid Glass, iOS 26)
      (all navigation)                  ├── Tab: Schedule
                                        │     └── NavigationStack
                                        │           ├── NativeNavComposeView  ← tab root
                                        │           └── DetailComposeView     ← one per destination
                                        └── Tab: Info
                                              └── NavigationStack ...
```

**Shared Kotlin side:**

1. Add title metadata to routes, so SwiftUI can label the native bar.
2. Export two entry points: a tab-root controller and a per-route `ScreenViewController(route, ...)`, each taking Swift callbacks (`onNavigate`, `onGoBack`, `onSet`, `onActivate`).
3. Intercept navigation inside Compose and forward it out, then drop it from Compose's own stack so the two do not both push:

```kotlin
if (onNavigate != null) {
    LaunchedEffect(navState) {
        snapshotFlow { navState.currentBackstack.toList() }.collect { backstack ->
            val detailRoutes = backstack.drop(1)
            if (detailRoutes.isNotEmpty()) {
                detailRoutes.forEach { onNavigate(it) }
                navState.currentBackstack.removeRange(1, navState.currentBackstack.size)
            }
        }
    }
}
if (onActivate != null) {   // user switched tabs from inside Compose
    LaunchedEffect(navState) {
        snapshotFlow { navState.topLevelRoute }.collect { it?.let(onActivate) }
    }
}
```

4. Hide Compose's own navigation UI when SwiftUI owns it, via a composition local:

```kotlin
val LocalUseNativeNavigation = staticCompositionLocalOf { false }
```

**Swift side:** a `TabView` of `NavigationStack`s, plus two `UIViewControllerRepresentable` bridges, one for tab roots and one for detail screens, each wiring the Kotlin callbacks into a coordinator that owns `push` / `pop` / `popToRoot` / `activateTab`.

**Keep the fallback.** The full-Compose route stays as the pre-26 path, and the two coexist behind an availability check:

```swift
struct ContentView: View {
    var body: some View {
        if #available(iOS 26.0, *) { NativeNavContentView() }
        else { ComposeView(topLevelRoute: ScheduleScreen()).ignoresSafeArea(.all) }
    }
}
```

```kotlin
// Pre-iOS 26 fallback: full Compose navigation, no native callbacks
fun MainViewController(topLevelRoute: TopLevelRoute): UIViewController = ComposeUIViewController(
    configure = { onFocusBehavior = OnFocusBehavior.DoNothing },
) { App(appGraph, topLevelRoute) }
```

**Cost.** A navigation bridge in both directions, two exported entry points instead of one, and route metadata that only the native shell reads. Worth it when the app has to feel current on iOS 26; not worth it for an app whose navigation is a single stack.

---

## 6. Build and Xcode Integration

Not this skill's territory. Framework wiring, CocoaPods vs direct integration vs SPM vs KMMBridge,
`embedAndSignAppleFrameworkForXcode`, User Script Sandboxing, and the Kotlin-to-Swift API surface all
live in **`kmp-ios-integration`** (`interop-reference.md`, `cocoapods-reference.md`). Gradle targets,
source sets, and the version catalog live in **`kmp-module-setup`**.

Two facts worth carrying here, because they change what Compose code can assume:

- Compose resources land in `UNLOCALIZED_RESOURCES_FOLDER_PATH` on iOS, never in `.lproj`
  directories, so `CFBundleLocalizations` has no effect on them. Locale selection is done entirely by
  Compose Resources' own runtime filtering.
- Resources go missing from the app bundle when a Pod-based project runs raw `pod install`; use
  `./gradlew podInstall`, which also creates the directories the resource sync expects.

---

## 7. Hot Reload, Previews, and iOS-Specific Concerns

### Compose Hot Reload

- **JVM/desktop only.** Other targets are "under exploration", so build the loop around desktop rather than waiting for iOS. Official guidance: use the desktop target as your fast-iteration sandbox for common UI, then verify on Android/iOS.
- Bundled and on by default for desktop targets since **CMP 1.10.0**; earlier versions apply plugin `org.jetbrains.compose.hot-reload` manually.
- Requires Kotlin 2.1.20+, JetBrains Runtime (Java 21), IntelliJ 2025.2.2+ / Android Studio Otter+.
- Workflow: run "desktopApp with Compose Hot Reload", save a file, changes apply to the running app.
- `@Preview` on common composables (as in the template's `App()`) renders in the IDE; hot reload is a separate, running-app mechanism.

### iOS performance

- Compose renders via its own canvas (Skia/Skiko) inside `ComposeUIViewController`. `CADisableMinimumFrameDurationOnPhone` must be `true` in `Info.plist` or the app crashes at startup (§4); the sub-ProMotion frame cap is the pre-1.7.0 symptom, and now only appears if `enforceStrictPlistSanityCheck` was turned off.
- Debug Kotlin/Native builds are markedly slower than release, so judge scrolling/animation performance on **release** builds on real devices.

### iOS accessibility

- Compose semantics map automatically to iOS Accessibility (VoiceOver, screen readers); `Modifier.testTag` maps to `accessibilityIdentifier`, enabling **XCTest** UI automation and `performAccessibilityAudit()`.
- Native views embedded via `UIKitView`/`UIKitViewController` are the exception: `isNativeAccessibilityEnabled` is off by default (§4), so VoiceOver skipping an interop view is a properties problem, not a semantics problem.
- **There is nothing to configure.** Since **1.8.0** the tree is built lazily on the first request from the iOS accessibility engine and disposed when interaction stops, which is fully compatible with VoiceOver and Voice Control. The old `AccessibilitySyncOptions` knob (`Never` / `WhenRequiredByAccessibilityServices` / `Always`, plus a debug logger) was **removed in 1.8.0**, so any snippet still passing it does not compile. Some doc pages still describe it.
- Material3 has no built-in high-contrast scheme: detect `UIAccessibilityDarkerSystemColorsEnabled` and supply custom palettes (WCAG 4.5:1 standard text, 7:1 for stricter compliance).
- AssistiveTouch and Full Keyboard Access work with Compose content.

---

