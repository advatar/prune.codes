Yes — a Codex plugin makes strong sense. A ChatGPT app may also make sense, but only if you turn Prune into a hosted, user-facing workflow rather than just a local developer tool.

My read: ship Codex first, then ChatGPT Apps SDK second.

Why Codex is the better immediate fit

Prune’s core value proposition is almost exactly aligned with Codex: it turns codebases and docs into “minimal, high-signal context packs,” and its own docs already describe a workflow where Prune runs as an MCP server and agents such as Codex call it for packs.  ￼

Codex already supports MCP servers in the CLI and IDE extension, and OpenAI’s Codex plugin docs say plugins can bundle skills, app integrations, and MCP servers into reusable workflows. That means a Prune Codex plugin does not need to reinvent the product; it can package the thing Prune already is: an MCP-powered context layer for coding agents.  ￼

The plugin would be most useful as a thin but polished install/distribution layer:

Prune Codex Plugin
├─ MCP config: start/connect Prune for the current repo
├─ Skills: “bootstrap repo,” “get focused context,” “refresh packs,” “explain selected files”
├─ Optional hooks: remind Codex to refresh context before large edits
├─ Starter prompts: common workflows for debugging, refactoring, onboarding
└─ AGENTS.md integration: make Prune the default context source

OpenAI’s plugin docs specifically recommend plugins when you want to share workflows across teams, bundle MCP configuration, package hooks, or publish a stable package; they also describe the .codex-plugin/plugin.json manifest and optional .mcp.json, skills, hooks, and assets.  ￼

What I’d publish first

I’d avoid making it feel like “yet another chatbot plugin.” I’d make it feel like “Prune becomes Codex’s repo memory/context engine.”

The first release could include four skills:

Skill	What it does
prune-bootstrap	Initializes Prune for the repo, generates or updates AGENTS.md, and creates initial packs.
prune-context-first	Tells Codex to query Prune before making non-trivial code changes.
prune-explain-context	Explains why certain files were included or excluded from a context pack.
prune-refresh	Rebuilds or updates packs after major edits, dependency changes, or refactors.

The key UX win is not merely “Codex can call Prune.” It is: Codex automatically knows when and how to use Prune.

ChatGPT: yes, but as an App, not a “plugin”

For ChatGPT, I would not brand this as a legacy “ChatGPT plugin.” OpenAI’s current surfaces are ChatGPT Apps / Apps SDK and GPT Actions, while “plugins” in the current OpenAI docs are specifically a Codex distribution concept. GPT Actions are for connecting a Custom GPT to REST APIs, while ChatGPT Apps are MCP-based apps with richer UI and tool-calling behavior.  ￼

A ChatGPT app makes sense if you can give users a non-IDE workflow, for example:

“Analyze my repo and show me the smallest useful context pack for this task.”
“Explain this codebase architecture.”
“Compare two context packs.”
“Show which files Prune thinks are relevant and why.”
“Generate an onboarding pack for a new engineer.”

That product would probably need a hosted MCP server, GitHub/GitLab auth, strong privacy controls, and maybe a UI widget for browsing packs. OpenAI’s app submission docs require a publicly accessible MCP server for app submission, plus metadata, screenshots, tool details, test prompts, and related review materials.  ￼

So the ChatGPT version is a bigger product decision: it turns Prune from a local developer tool into a hosted repo-analysis product. That could be valuable, but it has more privacy, auth, and review overhead.

Distribution strategy I’d use

I’d do this in three stages:

1. Codex local/repo plugin now.
    Publish a small open-source Codex plugin that wraps the existing Prune CLI/MCP workflow. This gets you into the exact place where users feel the pain: Codex working inside a real repo.
2. Workspace/team distribution next.
    Package opinionated team workflows: “Prune before edit,” “Prune before refactor,” “Prune before code review,” and “Prune after architectural change.” OpenAI docs already support sharing plugins within a workspace/org, while broader self-serve public plugin publishing is described as coming soon.  ￼
3. ChatGPT App only once there is a hosted product loop.
    Build the ChatGPT app around repo onboarding, architecture explanation, context-pack inspection, and maybe PM/engineering-lead workflows. OpenAI’s current submission flow says approved apps appear in ChatGPT’s app store and can also result in Codex plugin distribution.  ￼

Biggest risks

The main risk is privacy. Prune’s pitch is especially compelling for private codebases, so a local-first Codex plugin is much easier to trust than a hosted ChatGPT app. I would make “local-only by default” a central part of the Codex plugin story.

The second risk is unclear incremental value. Users need to see that Prune beats “just let Codex read the repo.” I’d publish simple evals:

Task: fix bug / implement feature / refactor component
Compare:
- Codex alone
- Codex + manually selected files
- Codex + Prune context pack
Measure:
- tokens used
- wrong-file edits
- first-pass success
- time to useful patch
- number of clarification turns

My recommendation

Yes: publish a Codex plugin. That is the obvious, high-fit move.

Maybe: publish a ChatGPT App. Do it only if you want Prune to become a hosted repo/context product with a visual workflow and public distribution.

I would not start with a generic ChatGPT chatbot integration. I’d start with a Codex plugin that makes Prune the default context engine for agentic coding, then use the adoption and eval data from that to decide whether the ChatGPT App is worth the extra surface area.
