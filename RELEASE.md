Absolutely. Here’s a **complete launch-ready change request** for:

1) a **macOS Prune desktop app** that can **install**, **start/stop**, show **help**, and handle **analytics**, and  
2) a **DMG distribution** that installs the app (and ensures the Prune binaries/services get installed in a stable place).

I’m assuming your target is: **Lovable (cloud) ↔ MCP connector ↔ your local Prune via tunnel**, and **GitHub default branch sync** (Lovable pushes after edits, Prune picks them up).

---

# Epic: Prune for macOS + DMG Distribution (Launch v1)

## Goals

### G1 — One-click local operation
A user should be able to:
- download a DMG
- drag **Prune.app** to Applications
- open it
- click **Install**
- click **Start**
…and be ready to connect Lovable.

### G2 — Reliability
- Services stay running across restarts (optional for v1, recommended)
- Start/stop is deterministic
- Sync responds to GitHub pushes quickly (webhook + explicit sync)

### G3 — Trust & supportability
- Built-in help + diagnostics bundle
- Clear status UI (running/stopped, last sync SHA, webhook health)
- Basic analytics (opt-in) to detect breakage and improve onboarding

---

# Deliverable 1: macOS App “Prune”

## 1.1 App personality: Menu bar control center (recommended)
A full window app is fine, but for “start/stop + status” this works best as a **menu bar app** with a **Settings** window.

### Menu bar drop-down (must-have)
- **Status indicator**
  - “Stopped”
  - “Running”
  - “Indexing…”
  - “Syncing…”
  - “Tunnel offline” / “Webhook failing”
- Buttons:
  - **Start**
  - **Stop**
  - **Open Dashboard** (settings/status window)
  - **View Logs**
  - **Help**
  - **Quit**

### Settings / Dashboard window (must-have)
Tabs:
1) **Setup**
2) **Services**
3) **Integrations**
4) **Help**
5) **Privacy & Analytics**

---

## 1.2 Install flow (what “Install” actually does)

“Install” should create a **stable runtime environment** for Prune that does *not* break if the user moves the `.app`.

### Install responsibilities
When user clicks **Install**:
1) Create directories (in user space)
   - `~/Library/Application Support/Prune/`
   - `~/Library/Application Support/Prune/bin/`
   - `~/Library/Application Support/Prune/mirrors/ORG__REPO/`
   - `~/Library/Application Support/Prune/mirrors/ORG__REPO/.ce/` (db + hnsw)
   - `~/Library/Logs/Prune/`

2) **Copy/Extract service binaries** from the app bundle into:
   - `…/Application Support/Prune/bin/`
   - Example binaries:
     - `prune-mcp`
     - `prune-sync`
     - `prune-cli` (optional wrapper)
   - (These are the “binaries to be installed” — stable path, stable permissions.)

3) Create a configuration file:
   - `~/Library/Application Support/Prune/config.json`
   - includes repo full name, default branch, paths, ports, tunnel settings
   - secrets are **not** stored here (see Keychain below)

4) Store secrets in macOS Keychain (must-have)
   - GitHub token (fine-grained PAT recommended)
   - GitHub webhook secret
   - optional: MCP bearer token for Lovable connector auth

5) Optional but recommended: install background jobs (LaunchAgents)
   - Adds per-user autostart for `prune-mcp` and `prune-sync`
   - This avoids “app must be open forever” issues

---

## 1.3 Start / Stop behavior

### Services to manage
You need 3 runtime pieces:
1) **MCP server** (Lovable calls it)
2) **Sync service** (GitHub webhook listener + git fetch/reset + incremental indexing)
3) **Tunnel** (exposes webhook endpoint + MCP endpoint publicly)

**Key requirement:** Lovable is cloud-based. Your MCP server must be reachable from the internet. Lovable’s docs explicitly describe adding an MCP server via a **Server URL** Lovable can reach.  [oai_citation:0‡Lovable Documentation](https://docs.lovable.dev/integrations/mcp-servers?utm_source=chatgpt.com)

### Recommended “single public base URL”
For simplicity: one public URL from tunnel that serves both:
- `https://<public>/mcp` (Lovable connector)
- `https://<public>/github/webhook` (GitHub webhook)

This reduces setup mistakes.

### Start button (must do, in order)
When user clicks **Start**:
1) Start tunnel (or verify running)
2) Start `prune-sync` (webhook endpoint live)
3) Start `prune-mcp` (MCP endpoint live)
4) Health-check:
   - webhook endpoint responds OK
   - mcp responds OK
5) Show “Copy MCP Server URL” + “Copy webhook URL”

### Stop button
- stop MCP first
- stop sync
- stop tunnel last

### Background jobs
Use per-user LaunchAgents so start/stop is reliable and works without admin privileges. (You’ll use `launchctl bootstrap/bootout/kickstart` style commands; `kickstart -k` restarts if already running.  [oai_citation:1‡ss64.com](https://ss64.com/mac/launchctl.html?utm_source=chatgpt.com))

---

## 1.4 Integrations tab (Lovable + GitHub)

### Lovable integration UX (must-have)
Lovable setup steps must be shown in the app because it reduces friction:

**Display:**
- “MCP Server URL” (copy button)
- “Authentication method” (none or bearer token)
- “Test connection” button (runs an MCP self-check)

**Instructions (in-app):**
Lovable’s documentation describes adding personal connectors:
- Settings → Connectors → Personal connectors → New MCP server
- Provide Server name + Server URL + authentication  [oai_citation:2‡Lovable Documentation](https://docs.lovable.dev/integrations/mcp-servers?utm_source=chatgpt.com)

### GitHub integration UX (must-have)
Lovable’s GitHub integration syncs both ways and notes changes in GitHub sync back on the default branch (`main`).  [oai_citation:3‡Lovable Documentation](https://docs.lovable.dev/integrations/github?utm_source=chatgpt.com)  
Your Prune sync assumes default branch only (as you requested).

**App should support:**
- input repo: `ORG/REPO`
- test “can fetch repo”
- create webhook button (recommended)
  - uses GitHub API, not `gh`, so the app is self-contained

**Token requirements (fine-grained PAT)**
GitHub’s REST docs for “Create a repository webhook” state fine-grained tokens need **Webhooks repository permissions (write)**.  [oai_citation:4‡GitHub Docs](https://docs.github.com/rest/webhooks/repos?utm_source=chatgpt.com)  
(You can also support GitHub App installation tokens later.)

---

## 1.5 Help (must-have for launch)

Help tab should include:

### Quickstart (step-by-step)
1) Install Prune.app
2) Click Install
3) Start
4) Copy MCP Server URL
5) In Lovable: add Personal Connector (MCP server) with that URL  [oai_citation:5‡Lovable Documentation](https://docs.lovable.dev/integrations/mcp-servers?utm_source=chatgpt.com)
6) Connect Lovable project to GitHub (default branch sync)  [oai_citation:6‡Lovable Documentation](https://docs.lovable.dev/integrations/github?utm_source=chatgpt.com)
7) Confirm: “last indexed SHA” updates after Lovable edits

### Troubleshooting playbook
- “MCP not reachable from Lovable”
- “Webhook deliveries failing”
- “Index not updating”
- “Tunnel expired”

### Diagnostics bundle
One button: “Export diagnostics”
- config (redacted)
- service logs
- last 50 webhook events (metadata only)
- last indexing stats
- versions + OS info

---

## 1.6 Analytics (launch-safe, privacy-first)

### Requirements
- **Opt-in** on first run (default off is safest)
- never collect code, prompts, files, or repository contents
- only collect operational events + performance counters

### Suggested event schema (examples)
- `app_install_completed`
- `app_start_clicked`, `app_stop_clicked`
- `tunnel_started`, `tunnel_failed`
- `github_webhook_received`
- `sync_completed` (duration_ms, changed_files_count)
- `index_completed` (duration_ms, files_indexed)
- `mcp_request` (method name only, duration_ms, success/fail)
- `error` (error_code, component)

### UX
Privacy & Analytics tab:
- toggle: “Share anonymous usage analytics”
- link: privacy policy

---

# Deliverable 2: DMG Packaging (with “binaries installed”)

## 2.1 DMG contents (recommended)
DMG should contain:
- `Prune.app`
- `Applications` shortcut (standard drag-to-install UX)
- optional: “Uninstall Prune” doc/link

The “binaries to be installed” requirement is satisfied by:
- shipping binaries inside the app bundle
- **install step extracts** them into `~/Library/Application Support/Prune/bin/` (stable)

This avoids needing admin privileges and keeps the DMG clean.

---

## 2.2 Signing & notarization (required for real users)

If distributing outside the Mac App Store, you’ll want the standard “Gatekeeper-friendly” workflow:
- Sign with **Developer ID**
- Notarize with Apple notary service (use `notarytool`, not `altool`)
- Staple the ticket so it works offline

Apple’s notarization docs reference `notarytool` and `stapler`.  [oai_citation:7‡Apple Developer](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution?utm_source=chatgpt.com)  
Also, `altool` is deprecated for notarization; `notarytool` is the recommended path.  [oai_citation:8‡GitHub](https://github.com/orgs/tauri-apps/discussions/8630?utm_source=chatgpt.com)

### Suggested release pipeline (high-level)
1) Build app bundle
2) Sign app bundle (hardened runtime)
3) Create DMG
4) Sign DMG
5) Submit DMG to notarization
6) Staple ticket to DMG
7) Validate

*(Exact command details belong in a `RELEASING.md`, but this is the flow.)*

---

# Deliverable 3: App-assisted Lovable + “ensure_fresh/sync” policy

You want Lovable to always do:
- before pack → `repo.ensure_fresh`
- after push → `repo.sync(expected_sha)`

**The macOS app should help enforce this** by providing:
- a “Lovable Instructions” snippet the user can copy/paste into their Lovable project instructions
- a test button that simulates the two calls

This matters because Lovable docs describe how to add MCP servers and that the agent uses them as context providers.  [oai_citation:9‡Lovable Documentation](https://docs.lovable.dev/integrations/introduction?utm_source=chatgpt.com)

---

# Acceptance Criteria

## AC1 Install
- Fresh machine → user drags app → opens → clicks Install
- App creates directories + installs binaries into Application Support
- App stores secrets in Keychain

## AC2 Start/Stop
- Start makes:
  - tunnel reachable
  - webhook endpoint reachable
  - MCP endpoint reachable
- Stop reliably shuts everything down
- Services do not duplicate (no multiple orphan processes)

## AC3 GitHub sync
- When Lovable pushes to default branch (GitHub integration), Prune picks it up
- “Last indexed SHA” updates accordingly
- Webhook deliveries show success (when enabled)

Lovable’s GitHub sync behavior (default branch sync) is documented.  [oai_citation:10‡Lovable Documentation](https://docs.lovable.dev/integrations/github?utm_source=chatgpt.com)

## AC4 Lovable connector works
- User can add Prune as an MCP server in Lovable using Server URL.  [oai_citation:11‡Lovable Documentation](https://docs.lovable.dev/integrations/mcp-servers?utm_source=chatgpt.com)
- Lovable can call:
  - `repo.ensure_fresh`
  - `context.pack`
  - `repo.sync(expected_sha=…)`

## AC5 Analytics
- Off by default
- Opt-in toggle works
- No code content collected

---

# Implementation Breakdown: Sub-issues checklist

### [ ] macOS app: UI + local state
- [ ] Menu bar UI with status + start/stop
- [ ] Settings window with Setup/Services/Integrations/Help/Privacy
- [ ] Diagnostics export

### [ ] Installer logic (in-app)
- [ ] Extract/copy binaries to Application Support
- [ ] Keychain storage for secrets
- [ ] Config file creation + migrations

### [ ] Service manager
- [ ] Start/stop tunnel
- [ ] Start/stop prune-sync
- [ ] Start/stop prune-mcp
- [ ] Health checks + log viewer

### [ ] GitHub webhook management
- [ ] Create/list/delete webhook via GitHub REST API
- [ ] Validate webhook secret and deliveries
- [ ] Default branch filtering

### [ ] Notarized DMG release pipeline
- [ ] Code signing
- [ ] Notarization with notarytool + stapling
- [ ] DMG creation and validation

---

## Clickable references
```text
Lovable MCP servers (Personal connectors): https://docs.lovable.dev/integrations/mcp-servers
Lovable GitHub sync (default branch):      https://docs.lovable.dev/integrations/github
Lovable integrations intro:                https://docs.lovable.dev/integrations/introduction

GitHub webhooks (create + secrets):        https://docs.github.com/en/webhooks/using-webhooks/creating-webhooks
GitHub REST webhooks endpoints:            https://docs.github.com/rest/webhooks/repos
Fine-grained token perms for webhooks:     https://docs.github.com/rest/webhooks/repos (see “Create a repository webhook” permissions)

Apple notarization overview:               https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution
Apple notarization workflow notes:         https://developer.apple.com/documentation/security/customizing-the-notarization-workflow
altool deprecated (context):               https://github.com/orgs/tauri-apps/discussions/8630
launchctl kickstart reference (man-like):  https://ss64.com/mac/launchctl.html
bootstrap/bootout equivalence discussion:  https://discussions.apple.com/thread/7396950
```

---

**two concrete docs** has been dropped into this repo:

1) `docs/PRUNE-MACOS-APP.md` (product + UX + service lifecycle)  
2) `docs/RELEASING-MACOS-DMG.md` (step-by-step signing/notarization validation checklist)

…and a “Launch v1” milestone checklist that maps exactly to GitHub issues.
