# LLM Authoring Entry Point

You are authoring a Slipway app. Do not start by writing one large root widget.
First map the source UI into explicit files and widget identities.

This is a user-authoring document. You should not need to inspect Slipway's
private architecture notes, old evaluation crates, git history, or backend
adapter internals before beginning an app. If the public docs do not explain an
operation, report `PUBLIC_DOC_GAP` with the missing operation.

## Required First Pass

Start with the quickstart:

1. [Quickstart for app authors](quickstart-authoring.md)
2. [Authoring layout](authoring-layout.md)
3. [Core API map](api/core.md)
4. [Backend API map](api/backends.md)
5. [Debug MCP](api/debug-mcp.md)

Depend on the public facade crate, not on individual internal crates. Prefer
`cargo add`:

```powershell
cargo add slipway --git https://github.com/shim9610/slipway.git --features iced
```

Use `features = ["egui"]` for egui, or `features = ["all-backends"]` only when
the task genuinely needs both backend adapters.

If the task is to mirror a web UI, also read:

6. [Web UI mirroring task guide](tasks/mirror-web-ui.md)

If the task uses canvas, plots, media, or an existing renderer, also read:

7. [Provider surfaces](api/provider-surfaces.md)

## Required Output Shape

An authored app should be split into these roles:

```text
ssot.rs
internal_logic.rs
communication.rs
view.rs
app_runner.rs
```

Equivalent file names are acceptable only when the mapping is explicit.

## Mental Model

Slipway code should read like this:

```text
SSOT data
+ widget-local logic
+ app communication and reducers
+ view declarations and local presentation state
+ runner/bootstrap
=> Slipway runtime
=> selected backend adapter
=> debug MCP evidence when enabled
```

## Do Not Do This

- Do not collapse a dashboard into one root widget that paints and routes
  everything by itself.
- Do not use backend default widgets as a visual guarantee.
- Do not paste canonical/offscreen pixels into the visible backend.
- Do not bypass declared hit/focus/scroll/text routes with direct state
  mutation.
- Do not claim MCP physical-control success unless the visible backend path
  accepts the same declared operation.
- Do not reduce a visual parity task into a smaller demo unless the user
  explicitly changes the goal.

## Completion Standard

For UI mirroring or backend acceptance work, complete means:

- the selected backend app runs;
- declared widgets are separately addressable;
- pointer, wheel, text, focus, or command behavior is routed through declared
  contracts where applicable;
- debug MCP can report status/probes and exercise supported control paths;
- resize and scroll behavior are checked against the source UI goal;
- remaining gaps are classified as authoring gaps, backend gaps, or API gaps.
