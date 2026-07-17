# LLM Contract Checklist

Read this before writing or modifying a Slipway app.

Slipway is an authoring contract system for LLM workers — not a widget
catalog, CSS engine, or automatic Svelte compiler. Write the concrete app
while staying inside the declared contracts.

This file is the CANONICAL statement of the cross-cutting authoring rules
(facade/prelude rule, app shape, declarations, style, backend input proof,
stop-and-report labels and the `GAPS.md` landing zone); other public pages
summarize and defer to it.

## First Rule

Use the public facade and the ordinary authoring surface:

```rust
use slipway::prelude::*;
```

The prelude covers the whole ordinary authoring surface: the widget trio,
every declaration type in "What Must Be Declared" below, the capability
helpers that construct them, the load-bearing policy traits, `TextStyle`,
`reserved_policy_defaults!`, and the pre-flight admission check — enforced
by a doctest on `slipway::prelude` authoring a widget from it alone.

Do not start with `use slipway::*`. The facade root exposes low-level extension
and backend/provider APIs for special cases. If a type is not in the prelude,
pause and classify the need:

- app authoring need: keep using the prelude and declare the missing behavior
  through view/layout/logic traits;
- backend-native wrapper need: read the backend-native wrapper docs and accept
  that the code is backend-specific;
- provider/custom-renderer need: read provider surfaces and expose target-local
  evidence;
- unclear need: report `PUBLIC_DOC_GAP` or `API_GAP` instead of importing
  internals.

## Required App Shape

Split the app into explicit roles — modeled file-for-file by the reference
example `crates/slipway-example-authored` (the designated copy source).
Equivalent names are fine only if the role is obvious.

- `ssot.rs`: source-of-truth data, design tokens, stable ids, source UI data.
- `internal_logic.rs`: widget-local state transitions.
- `communication.rs`: app messages, parent reducers, inter-widget flow.
- `view.rs`: layout, paint, hit/focus/scroll/text declarations.
- `app_runner.rs`: backend feature selection and runtime startup.

This split is not ceremony. It prevents silent drift where a painted element,
state mutation, and event route are hidden inside one large root widget.

## What Must Be Declared

If the UI shows a thing that can be interacted with, declare the matching
contract. Each declaration has a prelude helper — the region structs are
`#[non_exhaustive]` and cannot be built by struct literal:

- pointer behavior needs a `HitRegionDeclaration`
  (`hit_region_from_pointer_capability`);
- focus and text input need a `FocusRegionDeclaration`
  (`focus_region_from_focus_capability`, or
  `text_edit_focus_region_from_capability` with text-edit command
  declarations for text input);
- scrollable content needs a `ScrollRegionDeclaration` derived after layout
  from the final `LayoutOutput` (`scroll_region_from_scrollable_capability`;
  use `_with_order` when regions can overlap — see
  [Routing and scroll](api/routing-and-scroll.md));
- ask the scroll question explicitly: does any content
  exceed its container or the window (a card column taller than the window
  counts)? Then that overflow needs a covering scroll region — the
  page/root pattern is `SlipwayApp::app_scroll_regions` — or an intentional
  clip; admission flags uncovered overflow with the
  `view_contract.content_overflow_without_scroll_region` advisory;
- overlays/popups need explicit `PaintOrderDeclaration`, overflow bounds when
  they can leave a parent box, and matching hit regions;
- repeated children need stable slot identity, not just the same child id.

Painting a shape or text is never enough to make it interactive. Every row
above has a marked `PATTERN:` site in
`crates/slipway-example-authored/src/view.rs`: note-list row hit regions,
plain list focus, the draft-input text edit, list/nested scroll regions
with `_with_order`, the pointer-opaque wheel-transparent overlay layer,
and stable row slot identity.

## Coordinate Rules

Use the coordinate type that matches the owner:

- `TargetLocalRect`: local to the widget that owns the declaration;
- `ParentLocalRect`: placement of a child in the parent;
- provider surface bounds, dirty regions, hit points, and snapshots are
  target-local unless a backend wrapper documents another coordinate space.

Do not manually construct parent-mounted child paths inside a child view. The
app/backend mounting pass owns the final `WidgetSlotAddress`.

## Style Rules

Backend theme/defaults are not Slipway style authority.

- text paint must use `PaintOp::styled_text(...)`;
- text paint must carry an explicit `TextStyle`;
- `TextStyle::plain()` is an explicit baseline for tests, not a hidden default;
- production apps should put reusable design tokens in their own style module;
- override specific style fields near the call site when a state variant needs
  a small change.

If visual parity depends on a font, color, border, selection, or preedit
style, declare it; do not assume iced or egui defaults match the source UI.

## Backend Input Proof

Do not construct or import `BackendInputEvent` in ordinary app authoring.
Visible backend input is backend-owned evidence.

Physical-equivalent success requires all of these to line up:

- the selected backend generated the event;
- dispatch evidence uses source `backend_presented`;
- backend id matches the selected backend;
- command frame and evidence frame have identical `FrameIdentity`;
- selected region, generated event, route, and result identity match the
  current visible `ViewDefinition`;
- the event was handled through declared logic.

`SlipwayRuntime::apply_input_event(...)` is semantic direct control — useful
for debug/tests, never proof that a visible click, wheel, focus, text, or
command works.

Standard command names are not custom demo hooks. If `copy`, `cut`, `paste`,
`select_all`, `undo`, or `redo` appears in a physical/debug trace, verify that
the resulting state change matches the widget's text/edit command contract. Use
a custom command name for probe-only mutations.

## Backend Choice

Backend switching is typed repair, not magic translation.

- backend-neutral app code should not mention iced or egui;
- backend-specific native wrappers may mention that backend, and only it;
- if a backend switch fails to compile because a backend-specific wrapper is
  missing, fix the wrapper or declare unsupported behavior;
- do not hide backend-specific behavior behind neutral-looking app code.

## Provider And Native Wrapper Rule

Provider surfaces and native wrappers are escape hatches, not parity shortcuts.

Use them only when the worker already owns a renderer or backend widget that
must be inserted; the wrapper still exposes enough identity, layout, event,
debug, and unsupported evidence for Slipway to inspect it.

If a provider cannot report target-local bounds, hit/snapshot evidence, or
unsupported diagnostics, do not claim it satisfies visual/debug parity.

## Debug MCP Rule

MCP evidence counts only when it exercises the same visible backend path a
user relies on.

- status/probe evidence can describe current topology and state;
- screenshot/render evidence must use the requested frame/viewport identity;
- physical-control evidence must complete through backend-presented traces;
- semantic direct control must be labeled as semantic/debug, not physical.

If MCP says success but a visible user operation would not work, classify it
as a framework/backend bug or missing physical path; do not paper over it.

## When To Stop And Report

Two checks come before any gap report: an admission refusal is not a doc gap
(every code has a documented trigger and fix in the
[Diagnostics catalog](api/diagnostics.md) — read its row and the message
first), and a missing contract detail is not a doc gap until the retrieval
route is exhausted (rustdoc and the `slipway-core` source, per
[Finding the full contract](api/README.md)).

Stop and report instead of inventing a workaround when:

- public docs do not describe the operation;
- the only path requires importing non-prelude internals;
- a visible interaction would require direct state mutation;
- a backend-specific wrapper is needed but no wrapper contract exists;
- offscreen/canonical output would need to be pasted into a visible backend;
- evidence can only be fabricated by JSON, stale frame reuse, or direct runtime
  mutation.

Use these labels:

- `PUBLIC_DOC_GAP`: public docs do not explain how to do the task.
- `API_GAP`: the needed contract does not exist in the public API.
- `BACKEND_GAP`: the contract exists but the selected backend cannot present
  or prove it.
- `AUTHORING_GAP`: the app did not declare the necessary layout/style/event
  contract.

Reports need a durable landing zone, not chat scroll or stdout. A consuming
agent records every gap in a `GAPS.md` file at its own project root — one
section per gap, carrying:

- the label (`PUBLIC_DOC_GAP` / `API_GAP` / `BACKEND_GAP` /
  `AUTHORING_GAP`);
- what was needed;
- what the docs/API provided instead;
- the workaround taken (or "none — blocked").

Recording a gap does not always mean halting: when a safe workaround exists
— one that stays inside the declared contracts — record the gap in
`GAPS.md` and keep working. Stop only when every available workaround is on
the contract-violating list above (direct state mutation, non-prelude
internals, fabricated evidence, and the rest).

## Final Self-Check

Before reporting success, verify:

- the app compiles through the selected backend feature;
- the UI is assembled from separate authored widgets or explicit containers;
- every visible interactive element has a declared route;
- scroll and overlay geometry are declared and clipped/overflowed intentionally;
- text and visual style do not rely on backend defaults;
- MCP/debug evidence, when requested, uses current frame identity and the
  visible backend path;
- remaining gaps are labeled instead of silently hidden.
