Yes — **use both**:

- **`AGENTS.md`** = the “policy” (what kinds of tests to write, where they live, what’s required for merge).
- **Skills** = the “procedure” (a repeatable, deterministic workflow Codex can invoke every time it touches Swift). Codex can explicitly invoke a skill when you include `$skill-name` in the prompt, and skills can live in your repo under `.codex/skills/…`.  [oai_citation:0‡OpenAI Developers](https://developers.openai.com/codex/skills/)

Below is a **drop-in Swift 6 testing kit** you can copy into your repo.

---

## 1) Put Swift 6 test policy into `AGENTS.md`

Codex reads `AGENTS.md` *before doing any work*, and you can layer overrides per directory (root → subfolder overrides).  [oai_citation:1‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)

### `AGENTS.md` (add this section)
```md
## Swift 6 testing policy

### Framework choice (Swift Testing vs XCTest)
- If the repo already uses Swift Testing (`import Testing`, `@Test`, `#expect`), use Swift Testing for NEW unit/integration tests.
- Keep using XCTest for:
  - UI automation tests (XCUITest / XCUIApplication)
  - performance tests (XCTMetric)
- Swift Testing and XCTest may coexist in the same target; migrate incrementally.
- Never mix assertion APIs inside a single test (don’t call XCTest asserts from Swift Testing tests, and don’t call `#expect/#require` from XCTest tests).

### What to add on every change
- Bug fix: add a regression test that fails before the fix and passes after.
- New behavior: add unit tests for success + failure + boundary cases.
- If the change crosses module boundaries or does I/O, add/extend an integration test.

### Determinism and parallel safety
- Tests must be deterministic (no real network, no time-based flakiness).
- Avoid shared mutable state. Prefer value-semantics suites (Swift Testing `struct` suites) and fresh SUT per test.

### Where tests live
- SwiftPM packages:
  - Unit tests: `Tests/<Module>Tests/`
  - Integration tests: `Tests/<Module>IntegrationTests/`
- Xcode projects:
  - Unit tests target: `<AppOrFramework>Tests`
  - Integration tests target: `<AppOrFramework>IntegrationTests` (or a clearly labeled suite)

### How to run tests (must be kept up-to-date)
- Canonical verifier: `./scripts/codex/verify.sh`
- SwiftPM: `swift test`
- Xcode: `./scripts/codex/xcode_test.sh` (configured by `.github/codex/swift_xcode.env`)
```

Why these rules:
- Apple’s Swift Testing guidance explicitly says **Swift Testing + XCTest can co-exist**, `swift test` runs both, and you should keep XCTest for UI automation and performance testing; also avoid mixing assertion APIs across frameworks.  [oai_citation:2‡Apple Developer](https://developer.apple.com/videos/play/wwdc2024/10179/)  
- Swift Testing encourages `struct` suites to reduce accidental shared state.  [oai_citation:3‡Apple Developer](https://developer.apple.com/videos/play/wwdc2024/10179/)  

---

## 2) Add a Swift-specific override near your Swift code

If your repo is multi-language, create an override near the Swift subtree (example: `ios/AGENTS.override.md`). Codex loads these later in the chain and they override root guidance.  [oai_citation:4‡OpenAI Developers](https://developers.openai.com/codex/guides/agents-md/)

### `ios/AGENTS.override.md` (example)
```md
## iOS / Xcode specifics

### Required CI command
- Run: ./scripts/codex/xcode_test.sh

### Xcode testing configuration
- Workspace: MyApp.xcworkspace
- Scheme: MyApp
- Destination: platform=iOS Simulator,name=iPhone 15,OS=latest

### Integration tests
- Network-related integration tests must use a local stub server or deterministic fake.
- UI tests remain in XCTest UI test targets only.
```

---

## 3) Create a skill that *always* writes Swift tests the way you want

Skills are a great fit because they turn “how we write tests here” into an executable ritual that every Codex instance can reuse. Skills live in your repo at `.codex/skills/<skill>/SKILL.md`.  [oai_citation:5‡OpenAI Developers](https://developers.openai.com/codex/skills/)

### A) `.codex/skills/swift-tests/SKILL.md`
```md
---
name: swift-tests
description: Add/extend Swift 6 unit + integration tests (Swift Testing or XCTest) and run the correct test command.
metadata:
  short-description: Write Swift tests + verify
---

When invoked, follow this checklist:

1) Detect framework preference:
   - If the codebase already contains Swift Testing tests (look for `import Testing`, `@Test`, or `#expect`), use Swift Testing for NEW unit/integration tests.
   - Otherwise use XCTest.
   - For UI automation and performance tests, ALWAYS use XCTest.
   - Never mix assertion APIs inside a single test.

2) Detect build system:
   - If `Package.swift` exists: treat as SwiftPM and use `swift test`.
   - Else if an `.xcworkspace` or `.xcodeproj` exists: treat as Xcode and use `./scripts/codex/xcode_test.sh`.

3) Unit test requirements for every Swift change:
   - For bug fixes, add a regression test that fails pre-fix.
   - Cover: success path, error path, boundary conditions.
   - Prefer deterministic tests: no real network, no real clocks.
   - Prefer parallel-safe tests:
     - Swift Testing: use `struct` suites and avoid shared mutable state.
     - XCTest: fresh SUT per test; isolate filesystem/temp dirs.

4) Integration test requirements:
   - If the change touches I/O boundaries (HTTP, persistence, file system, process), add or update integration tests.
   - Put integration tests in a dedicated folder/target named `*IntegrationTests`.
   - Keep them deterministic (use stub servers / fakes, not the public internet).

5) Verification:
   - Always run `./scripts/codex/verify.sh` if present.
   - Otherwise run the Swift-specific command and report it in the PR summary.
```

This skill is intentionally “policy-heavy” so multiple Codex instances converge on the same behavior.

### B) Optional: add templates (Codex can copy/paste instead of inventing style)

#### `.codex/skills/swift-tests/assets/SwiftTestingSuite.template.swift`
```swift
import Testing
@testable import __MODULE__

@Suite struct __NAME__ {
  @Test func __testName__() async throws {
    // Arrange
    // Act
    // Assert
    #expect(true)
  }
}
```

Swift Testing test functions can be async/throws, and Swift Testing uses `@Test` plus `#expect/#require`.  [oai_citation:6‡Apple Developer](https://developer.apple.com/videos/play/wwdc2024/10179/)

#### `.codex/skills/swift-tests/assets/XCTestCase.template.swift`
```swift
import XCTest
@testable import __MODULE__

final class __NAME__: XCTestCase {
  func test___testName__() async throws {
    // Arrange
    // Act
    // Assert
    XCTAssertTrue(true)
  }
}
```

---

## 4) Add verifier scripts Codex (and CI) can always run

### A) `scripts/codex/verify.sh` (top-level canonical entrypoint)
```bash
#!/usr/bin/env bash
set -euo pipefail

# Swift (SwiftPM or Xcode)
if [[ -f "Package.swift" ]] || compgen -G "*.xcworkspace" > /dev/null || compgen -G "*.xcodeproj" > /dev/null; then
  ./scripts/codex/verify_swift.sh
  exit 0
fi

echo "No verifier configured for this repo. Extend scripts/codex/verify.sh."
exit 1
```

### B) `scripts/codex/verify_swift.sh`
```bash
#!/usr/bin/env bash
set -euo pipefail

echo "Swift toolchain:"
swift --version || true

# SwiftPM
if [[ -f "Package.swift" ]]; then
  echo "Detected SwiftPM. Running: swift test"
  swift test
  exit 0
fi

# Xcode
if compgen -G "*.xcworkspace" > /dev/null || compgen -G "*.xcodeproj" > /dev/null; then
  echo "Detected Xcode project. Running xcode tests via xcode_test.sh"
  ./scripts/codex/xcode_test.sh
  exit 0
fi

echo "No Swift project detected."
exit 1
```

### C) `scripts/codex/xcode_test.sh` (Xcode runner; reads env config)
```bash
#!/usr/bin/env bash
set -euo pipefail

CONFIG=".github/codex/swift_xcode.env"
if [[ -f "$CONFIG" ]]; then
  set -a
  source "$CONFIG"
  set +a
fi

: "${XCODE_SCHEME:?Set XCODE_SCHEME in .github/codex/swift_xcode.env}"
DESTINATION="${XCODE_DESTINATION:-platform=iOS Simulator,name=iPhone 15}"

COMMON_ARGS=(
  -scheme "$XCODE_SCHEME"
  -destination "$DESTINATION"
  -derivedDataPath ".derivedData"
)

if [[ -n "${XCODE_WORKSPACE:-}" ]]; then
  xcodebuild -workspace "$XCODE_WORKSPACE" "${COMMON_ARGS[@]}" test
elif [[ -n "${XCODE_PROJECT:-}" ]]; then
  xcodebuild -project "$XCODE_PROJECT" "${COMMON_ARGS[@]}" test
else
  echo "Set XCODE_WORKSPACE or XCODE_PROJECT in .github/codex/swift_xcode.env"
  exit 1
fi
```

### D) `.github/codex/swift_xcode.env` (you edit these once)
```bash
# Pick ONE:
XCODE_WORKSPACE=MyApp.xcworkspace
# XCODE_PROJECT=MyApp.xcodeproj

XCODE_SCHEME=MyApp
XCODE_DESTINATION=platform=iOS Simulator,name=iPhone 15,OS=latest
```

---

## 5) Force your Codex “implementer” prompt to always use the Swift test skill

If you’re using the Codex GitHub Action / CI prompts from earlier, edit your implementer prompt to explicitly invoke the skill.

### Example: add to `.github/codex/prompts/issue_to_pr.md`
```md
If you modify Swift code, you MUST invoke the $swift-tests skill and satisfy its checklist.
```

Codex supports explicit skill invocation by mentioning `$skill-name` in the prompt.  [oai_citation:7‡OpenAI Developers](https://developers.openai.com/codex/skills/)

---

## 6) CI reality check for Swift 6

- **SwiftPM** tests (`swift test`) can run on GitHub-hosted Ubuntu and macOS runners (GitHub documents both).  [oai_citation:8‡GitHub Docs](https://docs.github.com/actions/guides/building-and-testing-swift)  
- **Xcode / `xcodebuild test`** requires **macOS runners** (it will fail on Linux).  [oai_citation:9‡GitHub](https://github.com/marketplace/actions/xcodebuild-action?utm_source=chatgpt.com)  

So if your “fully automatic Codex pipeline” runs verification, make sure the jobs that execute `xcodebuild` are `runs-on: macos-latest`.

---

## If you want, I can tailor this to your exact Swift layout without back-and-forth

If you already know your:
- workspace/project name
- scheme(s)
- whether you’re using SwiftPM, Xcode, or both
- XCTest vs Swift Testing preference for new tests

…you can just paste those values, and you can copy/paste the scripts as-is with correct defaults.
