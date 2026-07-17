# Debug MCP

Slipway provides a first-party debug MCP surface outside `slipway-core`.
Core stays protocol-neutral, while runtime/backend crates attach debug support
when configured.

## What MCP Is For

Use MCP to ask a running Slipway app for request-scoped evidence:

- status and admission checks (refusal codes are cataloged in
  [Diagnostics](diagnostics.md));
- topology, state, layout, view-definition, paint, and diagnostic probes
  (the dispatch-graph probe is described in
  [Routing and scroll](routing-and-scroll.md));
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

## Finding The MCP Endpoint

The standard debug backend path starts a loopback MCP endpoint and includes the
address in the running window title:

```text
Slipway Backend Iced - Iced MCP: 127.0.0.1:52883
```

Connect to that TCP address and send one JSON-RPC request per line. The MCP
surface uses `tools/call`:

```json
{"jsonrpc":"2.0","id":"status-1","method":"tools/call","params":{"name":"slipway.debug.status","arguments":{"frame":"current"}}}
```

Use `tools/list` to discover the currently admitted tool list:

```json
{"jsonrpc":"2.0","id":"tools-1","method":"tools/list","params":{}}
```

Screenshot/render requests should use the current visible frame unless the
task intentionally compares a saved frame identity:

```json
{"jsonrpc":"2.0","id":"shot-1","method":"tools/call","params":{"name":"slipway.debug.screenshot","arguments":{"frame":"current"}}}
```

Pointer physical control example:

```json
{"jsonrpc":"2.0","id":"click-1","method":"tools/call","params":{"name":"slipway.debug.physical_control","arguments":{"frame":"current","operation":{"type":"pointer","phase":"press","position":{"x":120.0,"y":80.0},"button":"primary","device":"mouse"}}}}
```

Resize example:

```json
{"jsonrpc":"2.0","id":"resize-1","method":"tools/call","params":{"name":"slipway.debug.resize","arguments":{"frame":"current"}}}
```

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

LLM workers must not turn semantic direct control into a physical-control
claim. A semantic state mutation can be useful debugging evidence, but it does
not prove that the visible iced/egui path accepts the same user operation.

## Command Events And Standard Shortcuts

Keyboard shortcuts may arrive as a raw `keyboard` operation, as native text
editing, or as a `command` operation such as `copy`, `cut`, `paste`,
`select_all`, `undo`, or `redo`, depending on the backend and focused region.
MCP reports the command event, dispatch evidence, emitted messages, and result
identity; it does not currently enforce a global semantic rule that, for
example, `copy` must leave author state unchanged.

Authoring rule: standard command behavior belongs in the widget's declared text
or command policy and in tests. If a command mutates state, make that mutation
intentional and visible in the result evidence. Do not use a standard command
name for a custom probe/demo mutation; use a custom command name instead.

Current diagnostic limit: MCP can show that a standard command produced a state
change, but it does not yet classify that as a framework-level policy warning.
Treat unexpected state changes after `copy`/`select_all` as an authoring bug
unless the app has an explicit command contract saying otherwise.

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

For physical-control proof, the command frame and backend-presented dispatch
evidence frame must match as a full `FrameIdentity`. A matching viewport alone
is not enough.

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
