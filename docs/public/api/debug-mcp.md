# Debug MCP

Slipway provides a first-party debug MCP surface outside `slipway-core`.
Core stays protocol-neutral, while runtime/backend crates attach debug support
when configured.

## What MCP Is For

Use MCP to ask a running Slipway app for request-scoped evidence:

- status and admission checks;
- topology, state, layout, view-definition, paint, and diagnostic probes;
- request-scoped render or screenshot evidence;
- pointer, wheel, focus, text, keyboard, command, and resize controls where the
  active backend supports them;
- event traces and dispatch/result identities for physical-equivalent input.

MCP does not own live widget state. It asks the running app/runtime for evidence
at safe points.

## Default Tools

The default debug MCP surface exposes tools with names like:

- `slipway.debug.status`
- `slipway.debug.probe`
- `slipway.debug.render`
- `slipway.debug.screenshot`
- `slipway.debug.control`
- `slipway.debug.physical_control`
- `slipway.debug.resize`

Tool availability may depend on debug configuration and backend support.

## Physical Control Meaning

`slipway.debug.physical_control` is stricter than "mutate app state."

A successful physical-control result should mean:

1. a visible backend declaration was selected;
2. the backend event path accepted the operation;
3. the current view and dispatch evidence passed contract gates;
4. widget logic handled the event as declared;
5. the result was recorded as trace evidence.

If the backend or app cannot prove that path, the MCP response should be an
error or unsupported result, not fake success.

## Frame Identity

Most debug calls bind to a frame:

```text
surface_id
surface_instance_id
revision
frame_index
viewport
```

Use the active frame reported by the running backend. A stale or mismatched
frame can correctly refuse input even when the coordinates look right.

## User-Defined MCP Tools

App/domain-specific MCP tools are extension points. They are separate from the
default debug MCP surface.

Examples:

- app-specific fixture generation;
- custom domain state summaries;
- import/export helpers;
- business commands.

Do not implement default debug operations as app-specific tools when the
standard debug MCP surface already covers them.
