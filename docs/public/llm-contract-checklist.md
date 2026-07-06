# LLM Contract Checklist

Read this before writing or modifying a Slipway app.

Slipway is an authoring contract system for LLM workers. It is not a widget
catalog, not a CSS engine, and not an automatic Svelte compiler. Your job is to
write the concrete app while staying inside the declared contracts.

## First Rule

Use the public facade and the ordinary authoring surface:

```rust
use slipway::prelude::*;
```

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

Split the app into explicit roles. Equivalent names are fine only if the role
is obvious.

- `ssot.rs`: source-of-truth data, design tokens, stable ids, source UI data.
- `internal_logic.rs`: widget-local state transitions.
- `communication.rs`: app messages, parent reducers, inter-widget flow.
- `view.rs`: layout, paint, hit/focus/scroll/text declarations.
- `app_runner.rs`: backend feature selection and runtime startup.

This split is not ceremony. It prevents silent drift where a painted element,
state mutation, and event route are hidden inside one large root widget.

## What Must Be Declared

If the UI shows a thing that can be interacted with, declare the matching
contract:

- pointer behavior needs a `HitRegionDeclaration`;
- focus and text input need a `FocusRegionDeclaration` and text-edit command
  declarations;
- scrollable content needs a `ScrollRegionDeclaration` derived after layout
  from the final `LayoutOutput`;
- overlays/popups need explicit `PaintOrderDeclaration`, overflow bounds when
  they can leave a parent box, and matching hit regions;
- repeated children need stable slot identity, not just the same child id.

Painting a shape or text is never enough to make it interactive.

## Coordinate Rules

Use the coordinate type that matches the owner:

- `TargetLocalRect`: local to the widget that owns the declaration;
- `ParentLocalRect`: placement of a child in the parent;
- provider surface bounds, dirty regions, hit points, and snapshots are
  target-local unless a backend-specific wrapper explicitly documents another
  coordinate space.

Do not manually construct parent-mounted child paths inside a child view. The
app/backend mounting pass owns the final `WidgetSlotAddress`.

## Style Rules

Backend theme/defaults are not Slipway style authority.

- text paint must use `PaintOp::styled_text(...)`;
- text paint must carry an explicit `TextStyle`;
- `TextStyle::plain()` is an explicit baseline for simple examples/tests, not
  a hidden default;
- production apps should put reusable design tokens in their own style module;
- override specific style fields near the call site when a state variant needs
  a small change.

If visual parity depends on a font, color, border, selection color, or preedit
style, declare it. Do not assume iced or egui defaults match the source UI.

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

`SlipwayRuntime::apply_input_event(...)` is semantic direct control. It can be
useful for debug or tests, but it is not proof that a visible click, wheel,
focus, text, or command works.

## Backend Choice

Backend switching is typed repair, not magic translation.

- backend-neutral app code should not mention iced or egui;
- backend-specific native wrappers may mention that backend, and only that
  backend;
- if switching from iced to egui fails to compile because a backend-specific
  wrapper is missing, fix the wrapper or declare unsupported behavior;
- do not hide backend-specific behavior behind neutral-looking app code.

## Provider And Native Wrapper Rule

Provider surfaces and native wrappers are escape hatches, not parity shortcuts.

Use them only when the worker already owns a renderer or backend widget that
must be inserted. The wrapper still has to expose enough identity, layout,
event, debug, and unsupported evidence for Slipway to inspect it.

If a provider cannot report target-local bounds, hit evidence, snapshot
evidence, or unsupported diagnostics, do not claim it satisfies visual/debug
parity.

## Debug MCP Rule

MCP evidence is useful only when it exercises the same visible backend path a
user would rely on.

- status/probe evidence can describe current topology and state;
- screenshot/render evidence must use the requested frame/viewport identity;
- physical-control evidence must complete through backend-presented traces;
- semantic direct control must be labeled as semantic/debug, not physical.

If MCP says success but a visible user operation would not work, classify that
as a framework/backend bug or missing physical path. Do not paper over it in
app code.

## When To Stop And Report

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
