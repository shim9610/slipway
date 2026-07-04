# Slipway Public Documentation

This folder is the public documentation set for users and user-side LLM
workers.

It is separate from project-maintenance notes. Public users should start here
and should not need internal project records to author an app.

## Reading Paths

Start with [Quickstart for app authors](quickstart-authoring.md). If that page
and the linked public pages do not explain the next authoring step, report a
public documentation gap instead of investigating internal architecture or old
evaluation history.

Install the public facade crate with the backend feature you need:

```powershell
cargo add slipway --git https://github.com/shim9610/slipway.git --features iced
```

For a human overview:

1. [Project overview](../../README.md)
2. [Quickstart for app authors](quickstart-authoring.md)
3. [Authoring layout](authoring-layout.md)
4. [Core API map](api/core.md)
5. [Backend API map](api/backends.md)
6. [Debug MCP](api/debug-mcp.md)

For an LLM worker that must author or mirror a UI:

1. [Quickstart for app authors](quickstart-authoring.md)
2. [LLM entry point](llm-entry.md)
3. [Authoring layout](authoring-layout.md)
4. [Core API map](api/core.md)
5. [Backend API map](api/backends.md)
6. [Web UI mirroring task guide](tasks/mirror-web-ui.md)
7. [Debug MCP](api/debug-mcp.md)

For custom rendering or already-owned renderer integration:

1. [Provider surfaces](api/provider-surfaces.md)
2. [Backend API map](api/backends.md)
3. [Debug MCP](api/debug-mcp.md)

## Public Non-Goals

Slipway does not provide:

- automatic Svelte-to-Rust conversion;
- full CSS compatibility;
- built-in button, dropdown, table, chart, or dashboard widgets;
- visible rendering by pasting an offscreen raster into a backend window;
- a cross-backend promise for backend-native wrapper code.

The user-side LLM still implements the concrete UI. Slipway provides the
contracts, backend lifting path, and debug evidence surface.
