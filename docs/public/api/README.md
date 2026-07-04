# Public API Guide

The API guide is split by responsibility so an LLM worker can read only the
part needed for the current task.

Most users depend on the facade crate:

```powershell
cargo add slipway --git https://github.com/shim9610/slipway.git --features iced
```

Backend choice is the feature split. Do not add separate `slipway-core`,
`slipway-runtime`, or backend crate dependencies unless you are deliberately
working on the Slipway crates themselves.

## Files

- [Core API](core.md) - backend-neutral identity, state, logic, view,
  geometry, declarations, and event evidence.
- [Backend API](backends.md) - iced/egui adapter gates, backend-specific
  wrappers, visible backend rules, and backend switching expectations.
- [Debug MCP](debug-mcp.md) - request-scoped debug tools, physical-control
  meaning, and frame identity.
- [Provider surfaces](provider-surfaces.md) - canvas, plot, media, GPU, and
  already-owned renderer insertion.

## How To Choose

- Writing normal widgets or app state: read [Core API](core.md).
- Running on iced or egui: read [Backend API](backends.md).
- Testing with debug/control/screenshot/probe evidence: read
  [Debug MCP](debug-mcp.md).
- Inserting an existing chart, canvas, or GPU renderer: read
  [Provider surfaces](provider-surfaces.md).

If a task touches more than one area, read the files in that order.
