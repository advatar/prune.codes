# Install the Prune Agent Skill

Prune ships one portable `prune-context` package in
`integrations/skills/prune-context/SKILL.md`. The YAML frontmatter and progressive
disclosure workflow follow the [Agent Skills format](https://agentskills.io/specification).

From the repository you want Prune to index, inspect and apply the installation:

```sh
ce integrate --agent codex --repo . --dry-run
ce integrate --agent codex --repo .
```

The Codex installer creates `.agents/skills/prune-context/SKILL.md` and creates
`AGENTS.md` only if absent. Configure the Prune MCP server separately, or explicitly
opt in to updating the current user's Codex config:

```sh
ce integrate --agent codex --repo . --write-global
```

This appends a Prune entry to `~/.codex/config.toml` only when none exists, keeping
existing settings and comments. It quotes the absolute repository path as TOML.
An existing Prune entry is preserved, even if it targets another repository; review
that entry before relying on it. Inline TOML layouts that cannot accept a new
subtable are rejected with a manual-merge instruction.

## Client paths

| Client | Skill location | Instructions | MCP configuration |
| --- | --- | --- | --- |
| Codex | `.agents/skills/prune-context/SKILL.md` | `AGENTS.md` | Optional `~/.codex/config.toml` |
| Claude Code | `.claude/skills/prune-context/SKILL.md` | `CLAUDE.md` | `.mcp.json` |
| OpenCode | `.claude/skills/prune-context/SKILL.md` | `AGENTS.md`, `CLAUDE.md` | `opencode.json` |

Use `ce integrate --agent claude --repo .` or `ce integrate --agent opencode --repo .` for the
other clients. `ce integrate --agent all --repo .` installs the same skill content for
all three clients. Client discovery depends on the installed client version;
verify that the selected host lists the skill and the Prune MCP tools.

## Preservation and validation

- Existing instruction files and customized skill files are left unchanged.
  Compare them with the bundled templates and merge updates manually.
- Existing MCP servers and Prune-specific settings are preserved. JSON files
  are parsed and merged; they may be reformatted when a new entry is added.
- Invalid JSON/TOML, wrong configuration types, and symlinked destinations are
  rejected during planning, before installation writes begin. Each file is
  written to a same-directory temporary file and installed after the complete
  write succeeds. Existing permission bits are retained and read-only configs
  are refused. This is not a multi-file transaction; later filesystem errors
  can still leave an incomplete installation that requires a retry.
- `--dry-run` validates and reports planned writes without creating directories
  or changing files. Repeated installation preserves identical file contents.

Build or refresh the local index when authorized:

```sh
ce index --repo . --db .ce/index.sqlite --hnsw-dir .ce/hnsw
ce doctor --repo . --agent codex
```

`doctor` checks the expected files, including the Codex skill; it does not prove
that a client loaded the skill, that the index is current, or that MCP is reachable.

The local server exposes `context.pack`, `context.search`, `fragment.get`,
`strategy.list`, and `strategy.get`. Hosted freshness/sync tools must be discovered
from that deployment; do not assume they are available locally. The skill adds no
permissions, tool grants, or execution authority.

## Installer regression checks

From the embedded engine directory:

```sh
cargo test --locked -p ce-cli --lib --test integrations
```

These tests check fresh installation, preservation of existing content and MCP
servers, malformed-config rejection, dry runs, repeated installation, symlink
refusal, and TOML path escaping. They use temporary directories and do not require
an installed agent, a model, or a live MCP service.
