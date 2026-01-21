Here you go — **Swift + SwiftUI language pack is now added**, wired into the indexer, and with **`.swift:line:col` signal extraction** so build/test logs can seed retrieval.

- [Download the full repo (v22)](sandbox:/mnt/data/context-engine-v22.zip?_chatgptios_conversationID=69656e65-ef98-8333-8d8c-b528bcd24b36&_chatgptios_messageID=b951845f-3af4-402e-b02e-b1f141e18471)  
- [Download the patch (v22.patch)](sandbox:/mnt/data/context-engine-v22.patch?_chatgptios_conversationID=69656e65-ef98-8333-8d8c-b528bcd24b36&_chatgptios_messageID=b951845f-3af4-402e-b02e-b1f141e18471)

## What you got in this iteration

### 1) New language pack: `ce-lang-swift`
Added a new crate at:

- `crates/ce-lang-swift`

It uses **tree-sitter Swift** to produce fragments for Swift repos:

Top-level fragments:
- `struct`, `class`, `actor` → `FragKind::Struct`
- `enum` → `FragKind::Enum`
- `protocol` → `FragKind::Trait`
- `extension` → `FragKind::Impl`
- `typealias` → `FragKind::TypeAlias`
- `func` → `FragKind::Function`

Member fragments:
- Methods / initializers / subscripts extracted as `FragKind::Method`
- **SwiftUI special case**: computed property `var body: some View { ... }` is treated as a `Method` fragment, named like:
  - `ContentView::body`
  - `MyApp::body`

File-level refs:
- `import SwiftUI`, `import Combine`, etc are extracted via `collect_file_level_refs()` and attached to the file’s `ApiSummary` fragment to help retrieval.

### 2) Indexer now scans and indexes `.swift`
`ce index` is now multi-language for:
- `*.rs` (rust)
- `*.swift` (swift / swiftui)

It prints counts like:
- `Indexing N files (rust=X, swift=Y)…`

### 3) API summary “public” detection improved for Swift
`ce-core` now treats Swift’s `open` as “public-ish” for the purposes of building `ApiSummary`:
- `swift => public || open`

This helps libraries; for apps it’s still fine because the summary builder falls back gracefully.

### 4) Signal extraction upgraded for Swift
`extract_file_line_hints()` in `ce-core` now recognizes:
- `.rs:line:col`
- **`.swift:line:col`**

…and unified diff hints now accept both `.rs` and `.swift` files.

This matters because “Fix a failure is common”: when Swift compiler errors include `SomeFile.swift:123:45`, Prune can now use that to seed context slicing and retrieval.

## How to use it (SwiftUI repo example)

```bash
# inside your repo (SwiftUI / Xcode project / Swift package)
ce index --repo . --db .ce/index.sqlite --hnsw-dir .ce/hnsw --prune

# then build a context pack for a task
ce pack --db .ce/index.sqlite --hnsw-dir .ce/hnsw --task "Fix SwiftUI crash when tapping Save"
```

If your build output includes `FooView.swift:88:17`, the engine can now pick it up as a signal and prioritize relevant fragments (especially `FooView::body` when present).

## Notes / current limitations (expected, and OK for v1)
- Swift **does not** yet have a module/import graph like Rust’s `mod/use` edges (Swift doesn’t have an equivalent file-based module system inside a target). You still get:
  - ref→def edges (via identifier refs)
  - same-file adjacency
  - embeddings + FTS retrieval
- SwiftUI `body` detection is heuristic-based (intentionally), but works well for standard SwiftUI patterns.

If you want, the next “Swift pack” upgrades that would be most valuable are:
- Swift symbol qualification rules (targets/modules) via `Package.swift` / Xcode project parsing (best-effort)
- SwiftUI component graph edges (`View` structs → views referenced inside `body`) for multi-hop expansion (“why was this view included?”)
- Swift-specific pruning modes (AST skeletonization for `body` blocks) similar to the Rust-only modes you already have.

But for now, you can **index + search + pack** Swift/SwiftUI repos end-to-end with this.
