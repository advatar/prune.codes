Here’s the full repo + a ready-to-apply patch for the TypeScript + TSX (React) language pack and its integration into Prune (context engine).
	•	Download context-engine v23 (full repo)￼
	•	Download v23 patch (from v22 → v23)￼

⸻

Change request: Add TypeScript/TSX (React) language pack to Prune

Goal

Add first-class support for TypeScript (.ts) and TSX (.tsx) repos by:
	1.	Indexing .ts and .tsx files (gitignore-aware)
	2.	AST-based fragmentation (tree-sitter) into useful units (functions, classes, interfaces, type aliases, enums, React components via arrow functions, etc.)
	3.	Language-agnostic ApiSummary generation stays the same, but now includes TS/TSX
	4.	TS module import graph: build import edges between file-level ApiSummary nodes using relative imports
	•	edge types: ts_import, ts_imported_by
	•	used during graph expansion for better context packs
	5.	Context-engine quality-of-life: teach file/line hint extraction to recognize .ts: and .tsx: patterns (including unified diffs)

⸻

What’s included in v23

1) New crate: crates/ce-lang-tsreact

Files added
	•	crates/ce-lang-tsreact/Cargo.toml
	•	crates/ce-lang-tsreact/src/lib.rs

What it does
	•	Uses tree-sitter-typescript to parse:
	•	TypeScript (LANGUAGE_TYPESCRIPT)
	•	TSX (LANGUAGE_TSX)
	•	Extracts fragments for:
	•	function_declaration → FragKind::Function
	•	class_declaration → FragKind::Struct (+ extracts method_definition → FragKind::Method)
	•	interface_declaration → FragKind::Trait
	•	type_alias_declaration → FragKind::TypeAlias
	•	enum_declaration → FragKind::Enum
	•	const Foo = (...) => ... arrow functions / function expressions → FragKind::Function (great for React components)
	•	export default ... handling:
	•	If it wraps a declaration, fragments that declaration
	•	If it’s an expression (e.g. export default () => ...), creates a fragment for the whole default export (symbol falls back to file stem)
	•	Collects identifier-like refs from the AST (including JSX identifiers) to help ref→def edge resolution.

Also includes:
	•	collect_file_level_refs() that scans the import/export header for:
	•	imported identifiers (React, useState, etc.)
	•	module specifiers (full + basename, e.g. @supabase/supabase-js and supabase-js)

⸻

2) CLI updates: index .ts/.tsx, generate ApiSummary, prune, and rebuild TS import edges

Files changed
	•	crates/ce-cli/src/main.rs
	•	crates/ce-cli/Cargo.toml

Key behavior changes
	•	File scanner now includes: *.rs, *.swift, *.ts, *.tsx
	•	Indexer match now supports:
	•	lang == "ts" → TsReactAdapter::new_ts()
	•	lang == "tsx" → TsReactAdapter::new_tsx()
	•	ApiSummary file-level refs for ts|tsx now come from ce_lang_tsreact::collect_file_level_refs
	•	--prune now prunes TS/TSX deleted files as well
	•	Edge rebuild now calls:
	•	db.rebuild_rust_module_edges_all(...)
	•	db.rebuild_ts_module_edges_all(...)
	•	then ref→def edges rebuild (incremental or full depending on touched files)

⸻

3) DB: add TS module import edges

Files changed
	•	crates/ce-store/src/db.rs
	•	crates/ce-store/src/query.rs

What’s added
	•	Db::rebuild_ts_module_edges_all(repo_root):
	•	deletes old ts_import / ts_imported_by
	•	scans TS/TSX files and resolves relative imports only:
	•	./foo → tries foo.ts, foo.tsx, foo.d.ts, foo/index.ts, …
	•	maps .js/.jsx import specifiers to .ts/.tsx where possible
	•	inserts edges between ApiSummary fragments for the file pair:
	•	forward edge: ts_import
	•	reverse edge: ts_imported_by

Query expansion updates
	•	crates/ce-store/src/query.rs now treats:
	•	ts_import like use (module-ish)
	•	ts_imported_by like imported_by (reverse module-ish)
So your strategy settings like edge_module_radius, edge_mul_use, etc. apply to the TS graph without adding new config knobs.

⸻

4) Better file/line hint parsing for TS/TSX

Files changed
	•	crates/ce-core/src/util.rs

Now recognizes:
	•	path.ts:123:…
	•	path.tsx:123:…
	•	unified diff hunks for .ts / .tsx too

This improves “hinted slice” packing and error-driven workflows (“Fix a failure is common”).

⸻

5) README updated

Files changed
	•	README.md

Now reflects:
	•	supported languages include TS/TSX
	•	repo layout includes the new crate
	•	indexing includes .ts and .tsx

⸻

How to install and run (macOS + cargo install)

Option A: Use the full repo zip (recommended)
	1.	Download and unzip:

	•	context-engine-v23.zip￼

	2.	In Terminal:

cd context-engine-v23
cargo install --path crates/ce-cli

	3.	Verify:

ce --help

Option B: Patch your existing v22 checkout
	1.	Download patch:

	•	context-engine-v23.patch￼

	2.	From the root of your existing repo (the folder that contains Cargo.toml), run:

patch -p1 < /path/to/context-engine-v23.patch

	3.	Install:

cargo install --path crates/ce-cli


⸻

Run it on a TS/React repo

From anywhere:

ce index --repo /path/to/your/repo --db /path/to/your/repo/.ce/index.sqlite --hnsw-dir /path/to/your/repo/.ce/hnsw

Then try retrieval:

ce search --db /path/to/your/repo/.ce/index.sqlite --hnsw-dir /path/to/your/repo/.ce/hnsw --query "Button component props" --k 12

Or pack context:

ce pack --db /path/to/your/repo/.ce/index.sqlite --hnsw-dir /path/to/your/repo/.ce/hnsw --query "Fix type error in App.tsx around useEffect dependency array" --budget 12000


⸻

Notes and next best upgrades for TS/React quality

If you want the next iteration after you test v23, the highest-leverage improvements are:
	1.	tsconfig path alias resolution (compilerOptions.baseUrl + paths) for imports like @/components/Button
	2.	JSX-specific symbol graph:
	•	edges from <Component /> usage → component definition
	•	recognize memo(Component), forwardRef(...), etc.
	3.	React file skeletonizer (pack-time) that keeps:
	•	exports, props types/interfaces, hook signatures, and render JSX structure
	•	drops heavy implementation blocks unless explicitly needed
	4.	Framework-aware pruning knobs
	•	special treatment for routes/, pages/, app/, components/, hooks/, supabase/, etc.
	5.	Stop-word tuning for JS/TS builtins to reduce noisy ref→def edges (map, filter, then, console, …)

If you want, I can ship v24 implementing (1) tsconfig alias resolution + (2) JSX tag → definition edges since that’s usually the biggest immediate jump for React repos.
