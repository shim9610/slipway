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
2. [LLM contract checklist](llm-contract-checklist.md)
3. [Authoring layout](authoring-layout.md)
4. [Core API map](api/core.md)
5. [Routing and scroll](api/routing-and-scroll.md)
6. [Backend API map](api/backends.md)
7. [IME and Korean text input](api/ime.md)
8. [Debug MCP](api/debug-mcp.md)

When admission refuses a view or a diagnostic code needs reading, use the
[Diagnostics catalog](api/diagnostics.md). When a trait bound looks unused,
check [Trait surface](api/trait-surface.md) before writing logic in it.

Depend on the public facade crate, not on individual internal crates. Prefer
`cargo add`:

```powershell
cargo add slipway --git https://github.com/shim9610/slipway.git --features iced
```

Use `features = ["egui"]` for egui, or `features = ["all-backends"]` only when
the task genuinely needs both backend adapters.

For ordinary app authoring, import `slipway::prelude::*`. Do not use
`use slipway::*` as a shortcut: the facade root also exposes backend-extension
and provider-wrapper APIs that are not part of the normal authoring surface.

If the task is to mirror a web UI, also read:

9. [Web UI mirroring task guide](tasks/mirror-web-ui.md)

If the task uses canvas, plots, media, or an existing renderer, also read:

10. [Provider surfaces](api/provider-surfaces.md)

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

The designated COPY TARGET for this shape is the reference example crate
`crates/slipway-example-authored`: facade-only, exactly these five
modules, every pattern site marked with a `PATTERN:` comment naming the
rule it models, pre-flight admission asserted in its tests. Start new
apps from it. (`crates/slipway-example-admission` is an internal
admission stress fixture, not a template — do not copy it.)

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

## Contract Failure Policy

When code fails to compile because a trait, style, event route, backend wrapper,
or provider contract is missing, treat that as useful guidance. Fix the
authored contract or report `API_GAP`; do not route around it with direct state
mutation, backend defaults, stale evidence, or internal imports.

When the public pages end and a contract detail is still missing, the
sanctioned retrieval route is rustdoc and the `slipway-core` source itself
(grep the symbol, read the range) — see
[Finding the full contract](api/README.md). If that route does not answer it
either, report `PUBLIC_DOC_GAP` with the missing public operation. Private
project notes and old evaluation crates stay out of bounds; public docs are
the authority for user-side app authoring.

## Where Gap Reports Land

Gap reports go in a file, not into chat scroll: record every
`PUBLIC_DOC_GAP` / `API_GAP` / `BACKEND_GAP` (and `AUTHORING_GAP`) in a
`GAPS.md` at your own project root — one section per gap: the label, what
was needed, what the docs/API provided instead, and the workaround taken
(or "none — blocked").

A recorded gap is not always a stop: when a safe workaround exists — one
that stays inside the declared contracts — record the gap in `GAPS.md` and
keep working. Stop only when every available workaround would violate a
contract. The labels, the stop conditions, and the canonical statement of
this convention live in the
[LLM contract checklist](llm-contract-checklist.md).

## Do Not Do This

- Do not collapse a dashboard into one root widget that paints and routes
  everything by itself.
- Do not stop and inspect private architecture just because the app has more
  than eight widgets. Tuple child lists are supported up to 16 children in the
  public facade; for larger or dynamic child sets, write an explicit
  container/collection widget with declared child, hit, scroll, and paint
  contracts.
- Do not use backend default widgets as a visual guarantee.
- Do not paste canonical/offscreen pixels into the visible backend.
- Do not bypass declared hit/focus/scroll/text routes with direct state
  mutation.
- Do not import or construct `BackendInputEvent` in ordinary app authoring.
  Backend adapters or explicit backend-native wrappers may use it, but authored
  widgets should declare regions and logic instead.
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
- scroll regions are derived after layout from the final `LayoutOutput`, not
  from a wider incoming `LayoutInput`;
- nested scroll paint is clipped to the declared inner viewport, and wheel
  routing evidence identifies the region that consumed the event (see
  [Routing and scroll](api/routing-and-scroll.md));
- overlay/popup z-order is explicitly declared and testable through hit regions
  and paint order, not inferred from incidental draw order;
- debug MCP can report status/probes and exercise supported control paths;
- resize and scroll behavior are checked against the source UI goal;
- remaining gaps are classified as authoring gaps, backend gaps, or API gaps.
