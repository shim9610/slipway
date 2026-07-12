# Core API Map

This page names the main backend-neutral concepts in `slipway-core`.

It is a map, not a complete Rustdoc replacement. Use it to decide which API
area to inspect next.

User apps should normally import these through the public facade crate:

```rust
use slipway::prelude::*;
```

Do not use `use slipway::*` as the ordinary authoring surface. The facade root
also re-exports low-level extension APIs for backend authors and provider
wrappers. If app code needs a type that is not available from the prelude,
confirm that the task is actually backend-extension work before importing it.
If that decision is unclear, read the
[LLM contract checklist](../llm-contract-checklist.md) (the canonical
statement of this rule) and report `PUBLIC_DOC_GAP` or `API_GAP` rather
than importing internals.

## State And Identity

Important concepts:

- `WidgetId` - stable identity for a widget or app.
- `WidgetSlotAddress` - concrete slot for a child instance inside an app or
  container.
- `SlipwayWidgetTypes` - associated types for external state, local state, and
  app messages.
- `SlipwaySsot` - stable identity, topology, capabilities, unsupported
  diagnostics, and child traversal.

Authoring rule: a repeated widget needs repeated slot identity. Do not rely on
only the child id when multiple instances can exist.

## Logic

Important concepts:

- `SlipwayLogic` - handles an `InputEvent` and returns an `EventOutcome`.
- `EventOutcome` - handled/propagate flags, emitted messages, state changes,
  observations, probes, and diagnostics.
- `EmittedMessage` - typed message emitted upward.
- `ChangeEvidence` - structured evidence for a semantic change.

Authoring rule: widget logic mutates only its own local state and emits typed
messages. App-level state changes happen in the parent/app reducer.

## View And Layout

Important concepts:

- `SlipwayView` - local-state initialization, layout, paint, and observation.
- `SlipwayViewDefinition` - full declaration bundle for a frame.
- `ViewDefinition` - layout, paint, hit/focus/scroll declarations, semantics,
  order, and probes.
- `LayoutInput` and `LayoutOutput` - layout request and result.
- `TargetLocalRect` - geometry local to the widget that owns the declaration.
- `ParentLocalRect` - child placement geometry in the parent coordinate space.
- `FrameIdentity` - surface id, instance id, revision, frame index, and
  viewport.

Authoring rule: child view/layout inputs are target-local. Parent placement is
represented only by `ParentLocalRect`.

Scroll declarations are created after layout: use
`scroll_region_from_scrollable_capability` (or its `_with_order` variant when
regions can overlap) with the final `LayoutOutput`, not the incoming
`LayoutInput`, so the viewport derives from the presented bounds. Routing,
ordering, and overlays: [Routing and scroll](routing-and-scroll.md).

## Interaction Declarations

Important concepts:

- `HitRegionDeclaration` - pointer region.
- `FocusRegionDeclaration` - focus or text-edit region.
- `ScrollRegionDeclaration` - scrollable region.
- `TextEditCommandDeclaration` - text edit command support.
- `PaintOrderDeclaration` - stable order for layers, overlays, and source
  traversal.
- `SlipwayEventRoutingPolicy` - declares how an event routes through the widget
  tree.
- `SlipwayEventDispositionPolicy` - declares whether the event is handled,
  consumed, or propagated. The disposition is consulted BEFORE the handler
  runs, and it must state exactly what `handle_event` will do.
- `event_handling_table!` - generates `SlipwayLogic::handle_event` AND
  `SlipwayEventDispositionPolicy::event_disposition` from one match table.

Authoring rule: write the event handler and its disposition from ONE
`event_handling_table!` — each arm's pattern+guard is the declared
handledness, so the declaration cannot drift from the handler (the
`event_declaration.handler_*` diagnostics in
[Diagnostics](diagnostics.md) become inexpressible). Hand-write the two
impls only for capture/bubble-phase declarations or custom propagation,
building the evidence with `target_event_disposition`; the reference
example's `internal_logic.rs` models the table form.

Authoring rule: a visual region that should react must have the matching
declaration. Painting something clickable is not enough.

LLM rule: if a click, wheel, focus, text input, or command works only because
you directly changed state, the app has not satisfied the interaction contract.

Address rule: child widgets may describe their own local slot identity, but the
app/backend mounting pass owns the final parent-mounted `WidgetSlotAddress`.
Do not manually construct a parent path inside a child view.

## App Composition

Important concepts:

- `SlipwayApp` - combines N authored widgets into one app.
- `SlipwayAppWidget<A>` - adapts a `SlipwayApp` into a runtime widget.
- `ChildLayoutSeed` and `ChildLayoutPlan` - app/container child layout
  planning.
- `ChildPlacement` - final child placement and slot identity.

Root rule: a bare `SlipwayAppWidget::new(app)` is the supported root on
both backends. Core and the backend adapters provide every policy impl the
root gates require (font resolution delegates to
`SlipwayApp::resolve_app_font`, default honest refusal), so no delegation
wrapper around the adapter is ever needed — the reference example's
`app_runner.rs` runs the bare root on both backends, and the wrapper idiom
in the internal admission fixture (`AdmissionRuntimeAppWidget`) is
historical, not a requirement.

Composition caution: nesting a `SlipwayAppWidget` as a CHILD of another
app compiles and admits, but live press dispatch to the inner app's
widgets is a known open defect on both backends (2026-07-13). Keep one
`SlipwayAppWidget` at the root and compose plain widgets or explicit
container widgets under it.

Authoring rule: an app with N widgets should expose N authored child widgets.
Do not fake child widgets by painting all children inside one root view.

LLM rule: when the source UI has meaningful components, preserve those
boundaries as widgets or explicit container children. A single root surface is
only acceptable for a genuinely single-widget UI.

Tuple child lists are a fixed-arity convenience for authored apps and
containers; the public facade supports them up to 16 children across core,
iced, and egui. They are not the only composition pattern: for larger or
dynamic sets, use a dedicated container or collection widget that declares
its own child/region contract instead of one painted surface.

Layering rule: overlays and popups need explicit `PaintOrderDeclaration`,
declared overflow bounds, and matching hit regions. The backend preserves the
declared order. It does not guess z-order from paint order after the fact.

Scroll clipping rule: a scrollable region owns a viewport and content bounds.
If a widget declares nested scroll regions, each inner scroll viewport should
also produce paint clipped to that viewport. Backend renderers must honor the
clip; authors must expose the viewport/content relationship instead of relying
on invisible overflow.

Visible backends may defensively crop or disable invalid scroll geometry to keep
the window from presenting impossible rectangles. That repair is diagnostic
evidence, not authoring authority. Treat a scroll-normalization diagnostic as a
bug in the widget declaration unless the task explicitly accepts degraded
scroll behavior. Refusal codes are cataloged in
[Diagnostics](diagnostics.md).

## Backend Input Evidence

Important concepts:

- `BackendInputEvent` - backend-presented input plus optional dispatch
  evidence.
- `DeclaredEventDispatchEvidence` - proof of selected declaration, frame,
  route, coordinate space, and generated event.
- `DeclaredEventDispatchIdentity` - comparable identity that ignores provenance
  but preserves operation meaning.
- `EventResultIdentity` - comparable semantic result shape.

Authoring rule: physical-equivalent input is not just an event. It needs
dispatch evidence and a handled result trace.

`BackendInputEvent::direct(...)` is not a visible backend physical-input API.
Runtime and backend adapters must refuse it before authored handlers run. It is
only useful as a negative test or for purely semantic/debug paths that do not
claim backend-presented physical equivalence.

`BackendInputEvent` is intentionally not part of `slipway::prelude::*`.
Ordinary app authoring should not import or construct it. Backend adapters and
explicit backend-extension code may use it, but visible backend ingress must use
declared dispatch evidence.

There is no implicit `InputEvent -> BackendInputEvent` conversion. If a backend
is presenting physical input, it must attach declared dispatch evidence. If a
tool only needs semantic state mutation, use semantic/debug control APIs and do
not report the result as backend-presented physical equivalence.

Declared backend input is also checked against the current visible
`ViewDefinition`. The evidence source, backend id, frame identity, selected
region, candidate regions, generated event, and event route must match the
current declarations. Forged, stale, wrong-backend, or unresolved evidence is a
contract error and must not mutate widget state.

For backend-presented physical proof, the `FrameIdentity` in the MCP/debug
command and the `FrameIdentity` in backend dispatch evidence must be identical.
Sharing only viewport or bounds is not enough, because stale or different
backend frames can otherwise fabricate parity.

`SlipwayRuntime::apply_input_event(...)` is a semantic direct event path. It is
not backend-presented physical evidence and should not be used to prove that a
visible backend click, wheel, focus, or text operation works.

## Text And Paint Style

Important concepts:

- `TextStyle` - explicit font family, size, weight, style, decoration,
  baseline, alignment (`align_x`/`align_y`), and wrap mode (`wrap`) within
  the text op's bounds.
- `TextStyle::plain()` - an explicit Slipway baseline style for tests or simple
  examples.
- `TextStyle::centered()` / `.with_align_x(TextAlignX::...)` /
  `.with_align_y(TextAlignY::...)` - declared text alignment: both visible
  backends anchor the laid-out text within `PaintOp::Text.bounds`.
  Unspecified = `Start`/`Top`, the historical top-left anchoring. Declare the
  full control rect and align inside it; do not hand-center labels with
  estimated character widths.
- `TextStyle::no_wrap()` / `.with_wrap(TextWrap::...)` - declared per-op
  wrap mode. Unspecified = `TextWrap::Word`, the historical word wrap at
  the rect width; `TextWrap::None` keeps the text on one line, clipped at
  the rect. When geometry must depend on real text size, measure through
  `SlipwayApp::project_text_metrics` — never estimate character widths.
  Per-backend honoring, order of operations, and the measurement pattern:
  [Backend API Map](backends.md), "Text Wrap and Alignment".
- `PaintOp::styled_text(...)` - the only text paint constructor.

Authoring rule: text paint must carry an explicit `TextStyle`. Slipway does not
read backend theme defaults as style authority, and `TextStyle` intentionally
does not implement `Default`.

Production apps should normally put reusable design tokens in their own style
module and pass those tokens into view code. Override only the fields that need
to differ for a specific widget state.
