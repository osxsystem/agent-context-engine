## 5. CocoaPods Integration

### Environment setup

CocoaPods needs Ruby (1.9+; docs use 3.4.7 via rvm/rbenv). Avoid the Homebrew CocoaPods install (Xcodeproj compatibility issues):

```bash
rbenv install 3.4.7 && rbenv global 3.4.7
sudo gem install -n /usr/local/bin cocoapods
```

### Plugin setup

The CocoaPods Gradle plugin version **must match the Kotlin plugin version** (min Kotlin 1.7.0):

```toml
[plugins]
kotlinCocoapods = { id = "org.jetbrains.kotlin.native.cocoapods", version.ref = "kotlin" }
```

```kotlin
// shared module build.gradle.kts
plugins {
    kotlin("multiplatform")
    kotlin("native.cocoapods")
}

kotlin {
    iosArm64(); iosSimulatorArm64()
    cocoapods {
        // required — feed the generated podspec
        version = "1.0"
        summary = "Shared KMP module"
        homepage = "https://example.com"
        ios.deploymentTarget = "16.0"

        name = "MyCocoaPod"                       // optional; defaults to project name
        framework {
            baseName = "Shared"
            isStatic = false                      // dynamic by default
            transitiveExport = false
        }
        // map custom Xcode configurations to Kotlin build types
        xcodeConfigurationToNativeBuildType["CUSTOM_DEBUG"] = NativeBuildType.DEBUG
        xcodeConfigurationToNativeBuildType["CUSTOM_RELEASE"] = NativeBuildType.RELEASE

        podfile = project.file("../iosApp/Podfile") // enables automatic pod install integration
    }
}
```

The `podspec` Gradle task generates `<name>.podspec` next to the module, embedding a script phase that rebuilds the Kotlin framework during Xcode builds.

### Pod dependencies from Kotlin (`pod()`)

Declared inside `cocoapods {}`; each pod gets a cinterop binding importable as `cocoapods.<PodName>.*`:

```kotlin
cocoapods {
    pod("SDWebImage") { version = "5.20.0" }                         // CDN
    pod("local_dep") { source = path(project.file("../pod_dep")) }   // local path
    pod("SDWebImage") {                                              // git
        source = git("https://github.com/SDWebImage/SDWebImage") { tag = "5.20.0" }
        // also: commit = "...", branch = "..."
    }
    specRepos { url("https://github.com/Kotlin/kotlin-cocoapods-spec.git") } // custom spec repo
    pod("example")

    pod("FirebaseAuth") {
        packageName = "FirebaseAuthWrapper"       // import FirebaseAuthWrapper.Auth
        version = "11.7.0"
        extraOpts += listOf("-compiler-option", "-fmodules")  // needed for @import headers
    }
}
```

Other knobs: `moduleName` for pods whose framework name differs (`pod("SDWebImage/MapKit") { moduleName = "SDWebImageMapKit" }`), `headers = "GNSMessages.h"` for pods without a `.modulemap`, `useInteropBindingFrom()` to share cinterop bindings between dependent pods, `linkOnly` to link without generating bindings. `ios.deploymentTarget` is mandatory. After edits, re-sync Gradle to regenerate bindings.

### Linking the shared module into Xcode via Pods

Podfile in `iosApp/`:

```ruby
target 'iosApp' do
  use_frameworks!            # or use_modular_headers! — one is required
  platform :ios, '16.0'      # deployment target required on every target
  pod 'shared', :path => '../shared'   # local path to the Kotlin module
end
```

Multi-target (e.g. iOS + tvOS): repeat the `pod ... :path =>` line per target and set `tvos.deploymentTarget` etc. in the `cocoapods {}` block.

### Workflow

1. Edit `build.gradle.kts` (pods, framework config).
2. Run `pod install`, or preferably **`./gradlew podInstall`**, which also creates required directories/resources.
3. Open **`.xcworkspace`** (never `.xcodeproj` after pod install).
4. In Xcode Build Settings, **disable "User Script Sandboxing"** for the app target.
5. Build in Xcode. The Kotlin framework rebuilds automatically via the podspec's script phase. With multiple Xcode projects, run `pod install` manually for each.

⚠️ CocoaPods integration is **mutually exclusive with direct integration** (`embedAndSignAppleFrameworkForXcode`); pick one.

### Common errors and fixes

| Symptom | Fix |
|---|---|
| Xcode build can't find `pod` | Set `kotlin.apple.cocoapods.bin=/Users/you/.rbenv/shims/pod` in `local.properties` |
| Module/framework not found | `gem update --system && gem update`; check `use_frameworks!` and deployment targets present |
| Framework name mismatch | `pod("X/Sub") { moduleName = "..." }` |
| Pod has no `.modulemap` | `headers = "SomeHeader.h"` in the `pod()` block |
| Resources missing from app bundle | Use `./gradlew podInstall` instead of raw `pod install` |
| Rsync/sandbox error during build | Disable User Script Sandboxing in Xcode; `./gradlew --stop` |
| Obj-C `@import` headers fail cinterop | `extraOpts += listOf("-compiler-option", "-fmodules")` |

---

