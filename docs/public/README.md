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

Install the public facade crate with the backend feature you need. Slipway is
currently distributed from GitHub, not crates.io; do not use
`slipway = { version = "..." }` unless a crates.io release is explicitly
announced.

```powershell
cargo add slipway --git https://github.com/shim9610/slipway.git --tag v0.1.7 --features iced
```

For a human overview:

1. [Project overview](../../README.md)
2. [Quickstart for app authors](quickstart-authoring.md)
3. [Authoring layout](authoring-layout.md)
4. [Core API map](api/core.md)
5. [Backend API map](api/backends.md)
6. [IME and Korean text input](api/ime.md)
7. [Debug MCP](api/debug-mcp.md)
8. [Service MCP](api/service-mcp.md)

For an LLM worker that must author or mirror a UI:

1. [Quickstart for app authors](quickstart-authoring.md)
2. [LLM entry point](llm-entry.md)
3. [LLM contract checklist](llm-contract-checklist.md)
4. [Authoring layout](authoring-layout.md)
5. [Core API map](api/core.md)
6. [Routing and scroll](api/routing-and-scroll.md)
7. [Backend API map](api/backends.md)
8. [IME and Korean text input](api/ime.md)
9. [Web UI mirroring task guide](tasks/mirror-web-ui.md)
10. [Debug MCP](api/debug-mcp.md)
11. [Service MCP](api/service-mcp.md)

Reference pages for specific situations: the
[Diagnostics catalog](api/diagnostics.md) when admission refuses a view, and
[Trait surface](api/trait-surface.md) for load-bearing vs RESERVED trait
status.

## Reference Example

The designated copy source for a new app is the reference example crate
`crates/slipway-example-authored`: facade-only (`use slipway::prelude::*`),
the five-file split, every pattern site marked with a `PATTERN:` comment,
and pre-flight admission asserted in its tests. It runs on both backends
(`--iced` / `--egui`) from the same authored modules.

`crates/slipway-example-admission` is an INTERNAL admission stress fixture
and regression harness, not an authoring template. It intentionally
predates the documented rules (single-file, direct internal-crate imports);
do not copy it and do not read it as authority (LLM-ergonomics audit
2026-07-11, LE-H2/LE-H6).

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
