---
name: kmp-test-seams
description: Use when a Kotlin Multiplatform repo raises the question of where a test belongs or which Gradle task proves it, whether commonTest or androidHostTest/iosTest, seams that live in commonMain over platform services, and choosing jvmTest, testDebugUnitTest or iosSimulatorArm64Test to run a slice.
---

# KMP Test Seams

The platform layer underneath the red → green loop. The loop itself, and what makes a test worth keeping, belong to the `tdd` skill, so run that. This answers the two questions KMP adds to it: **where does the seam go**, and **which Gradle task proves the slice green**.

## Seams and test tasks

In a KMP repo, the seam question has a platform dimension: prefer seams that live in `commonMain` (interfaces over platform services, covered by the `kmp-module-setup` skill), so the red-green loop runs in `commonTest` with `kotlin.test` and multiplatform fakes. Tests land in `commonTest` unless they exercise a platform `actual` (then `androidHostTest` / `iosTest`). Run the loop with the cheapest task that covers the seam, `jvmTest` for pure common logic, or `iosSimulatorArm64Test` / `testDebugUnitTest` before claiming a platform-touching slice green. Task map: the `kmp-release-and-publish` skill.
