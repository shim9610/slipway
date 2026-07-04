# Slipway

Slipway is an experimental Rust UI authoring layer for LLM-assisted desktop
ports.

It is not a browser, CSS engine, automatic converter, or built-in widget
library. A library-using LLM authors the actual UI as explicit Rust contracts:
state, logic, view declarations, local widget state, and app assembly. Slipway
then gives backend adapters enough structure to lift those authored widgets
into desktop backends such as iced and egui, and to expose debug evidence
through MCP.

## What Slipway Is For

Slipway is meant for projects where a web UI, initially Svelte-focused for the
MVP, needs to be re-authored as a desktop UI while keeping the work inspectable:

- widgets remain separate identities instead of one hidden surface;
- external app state and widget-local state are explicit;
- interactions route through declared hit, focus, scroll, text, and command
  contracts;
- backend-visible behavior can be inspected through debug/MCP evidence;
- backend changes should expose missing backend contracts instead of silently
  changing behavior.

## Repository Shape

- `crates/slipway-core` - backend-neutral traits, geometry, declarations, and
  evidence types.
- `crates/slipway-runtime` - runtime assembly, event application, debug bridge
  integration, and app state handoff.
- `crates/slipway-backend-iced` - iced backend adapter and native debug runner.
- `crates/slipway-backend-egui` - egui backend adapter and native debug runner.
- `crates/slipway-debug-*` - first-party debug bridge, MCP transport, and
  request-scoped render evidence support.
- `crates/slipway-example-admission` - small admission/example app.

Evaluation crates are disposable and are not part of the public API.

## Start Here

- Public documentation index: [docs/public/README.md](docs/public/README.md)
- LLM authoring entry point: [docs/public/llm-entry.md](docs/public/llm-entry.md)
- Required file split: [docs/public/authoring-layout.md](docs/public/authoring-layout.md)
- Core API map: [docs/public/api/core.md](docs/public/api/core.md)
- Backend API map: [docs/public/api/backends.md](docs/public/api/backends.md)
- Debug MCP: [docs/public/api/debug-mcp.md](docs/public/api/debug-mcp.md)

Project-maintenance notes are maintained separately from this public manual.
They are not the user manual and should not be treated as public API
documentation.
