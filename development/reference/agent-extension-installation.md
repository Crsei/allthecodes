# Codex / Claude Code / OpenClaw / Hermes Agent Extension Installation

Last checked: 2026-05-08.

This note compares how Codex, Claude Code, OpenClaw, and Hermes Agent install
skills, MCP servers, and plugins. It is a reference snapshot, not an
implementation contract for cc-rust.

## Summary

| Product | Skills | MCP servers | Plugins |
| --- | --- | --- | --- |
| Codex | Put skill directories under repo/user/admin skill locations such as `.agents/skills`, `$HOME/.agents/skills`, or `/etc/codex/skills`; curated skills can be installed with `$skill-installer`. | Use `codex mcp add <name> -- <stdio-command>` or configure `[mcp_servers.<name>]` in `config.toml`. | Install through Codex App Plugins or the CLI plugin browser; plugins can package skills, apps, and MCP servers. |
| Claude Code | Put skills under `~/.claude/skills/<name>/SKILL.md` or project `.claude/skills/<name>/SKILL.md`; plugins may also contribute skills. | Use `claude mcp add`, with `--transport http` or `--transport stdio`, and optional `--scope local/project/user`. | Use `/plugin` or `/plugin install <name>@<marketplace>`; the official marketplace is `claude-plugins-official`. |
| OpenClaw | Install from ClawHub with `openclaw skills install <slug>`, or place skills in workspace/user skill directories. | Use `openclaw mcp set/list/show/unset` for outbound MCP server config; `openclaw mcp serve` exposes OpenClaw itself as an MCP server. | Use `openclaw plugins install <pkg>`, `clawhub:<pkg>`, a local directory, or an archive. OpenClaw can also read Codex/Claude/Cursor bundles. |
| Hermes Agent | Use `hermes skills install <hub-id-or-url>`, or place skills under `~/.hermes/skills/<name>/SKILL.md`. | Configure `mcp_servers` in `~/.hermes/config.yaml`; reload with `/reload-mcp`. | Put plugins under `~/.hermes/plugins/<plugin>/plugin.yaml`, or install from Git with `hermes plugins install user/repo`. |

## Codex

### Skills

Codex skills are directories containing `SKILL.md`, optionally with
`scripts/`, `references/`, or `assets/`.

Common locations:

```text
.agents/skills/
$HOME/.agents/skills/
/etc/codex/skills/
```

Curated skills can be installed with the skill installer command:

```text
$skill-installer linear
```

For distribution to other users, Codex documentation recommends packaging
skills inside a plugin.

Source: [Codex Agent Skills](https://developers.openai.com/codex/skills)

### MCP servers

Codex supports stdio and streamable HTTP MCP servers.

CLI example:

```bash
codex mcp add context7 -- npx -y @upstash/context7-mcp
```

Equivalent `config.toml` shape:

```toml
[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
```

Source: [Codex MCP](https://developers.openai.com/codex/mcp)

### Plugins

Codex plugins are installable bundles that can include skills, apps, and MCP
servers. They can be installed from the Codex App Plugins page or the CLI plugin
browser.

Source: [Codex Plugins](https://developers.openai.com/codex/plugins)

## Claude Code

### Skills

Claude Code skills are directories containing a `SKILL.md` file.

Common locations:

```text
~/.claude/skills/<skill-name>/SKILL.md
.claude/skills/<skill-name>/SKILL.md
```

The project-local path is useful when a skill should travel with a repository.
The user path is useful for personal reusable workflows.

Source: [Claude Code Skills](https://code.claude.com/docs/en/skills)

### MCP servers

HTTP transport example:

```bash
claude mcp add --transport http notion https://mcp.notion.com/mcp
```

Stdio transport example:

```bash
claude mcp add --transport stdio airtable -- npx -y airtable-mcp-server
```

Claude Code supports MCP scopes such as local, project, and user:

```bash
claude mcp add --scope project --transport stdio my-server -- npx -y my-mcp-server
```

Use `/mcp` inside Claude Code to inspect server status.

Source: [Claude Code MCP](https://code.claude.com/docs/en/mcp)

### Plugins

Install through the plugin UI or slash command:

```text
/plugin
/plugin install github@claude-plugins-official
```

Claude Code supports official, GitHub, local path, and remote marketplace JSON
sources. After installing or changing plugins, `/reload-plugins` can refresh the
runtime without restarting.

Source: [Claude Code Plugins](https://code.claude.com/docs/en/discover-plugins)

## OpenClaw

### Skills

OpenClaw uses AgentSkills-style skill directories containing `SKILL.md`.

Install from ClawHub:

```bash
openclaw skills install <skill-slug>
openclaw skills update --all
```

Skills can also be placed in local skill directories such as:

```text
<workspace>/skills/
~/.openclaw/skills/
~/.agents/skills/
```

Source: [OpenClaw Skills](https://docs.openclaw.ai/tools/skills)

### MCP servers

OpenClaw has two MCP surfaces:

- Outbound registry: OpenClaw connects to configured MCP servers.
- Server mode: other MCP clients connect to OpenClaw.

Outbound examples:

```bash
openclaw mcp set context7 '{"command":"uvx","args":["context7-mcp"]}'
openclaw mcp set docs '{"url":"https://mcp.example.com"}'
```

Server mode:

```bash
openclaw mcp serve
```

Source: [OpenClaw MCP](https://docs.openclaw.ai/cli/mcp)

### Plugins

Install examples:

```bash
openclaw plugins install @openclaw/voice-call
openclaw plugins install ./my-plugin
openclaw plugins install clawhub:<pkg>
```

After changing plugin installations, restart the gateway when needed:

```bash
openclaw gateway restart
```

OpenClaw can also recognize bundle formats from Codex, Claude, and Cursor and
map their skills, MCP servers, LSP servers, and related extension metadata.

Sources:

- [OpenClaw Plugins](https://docs.openclaw.ai/tools/plugin)
- [OpenClaw Plugin Bundles](https://docs.openclaw.ai/plugins/bundles)

## Hermes Agent

### Skills

Install from the Hermes skill hub:

```bash
hermes skills install official/research/arxiv
```

Install from a URL:

```bash
hermes skills install https://example.com/SKILL.md
```

Local skills live under:

```text
~/.hermes/skills/<skill-name>/SKILL.md
```

Installed skills become slash commands inside Hermes.

Source: [Hermes Working with Skills](https://hermes-agent.nousresearch.com/docs/guides/work-with-skills)

### MCP servers

Hermes MCP servers are configured in `~/.hermes/config.yaml`:

```yaml
mcp_servers:
  filesystem:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/home/user/projects"]
  remote_api:
    url: "https://mcp.example.com/mcp"
    headers:
      Authorization: "Bearer ***"
```

Reload MCP configuration inside Hermes:

```text
/reload-mcp
```

If MCP support is missing from an editable/local install, install the MCP extra:

```bash
uv pip install -e ".[mcp]"
```

Source: [Hermes MCP](https://hermes-agent.nousresearch.com/docs/user-guide/features/mcp)

### Plugins

Local plugin layout:

```text
~/.hermes/plugins/<plugin-name>/plugin.yaml
```

Install from Git:

```bash
hermes plugins install user/repo --enable
hermes plugins enable my-plugin
```

Hermes discovers plugins, but they generally need to be enabled explicitly in
configuration or with the CLI.

Source: [Hermes Plugins](https://hermes-agent.nousresearch.com/docs/user-guide/features/plugins)
