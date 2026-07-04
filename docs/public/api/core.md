# Core API Map

This page names the main backend-neutral concepts in `slipway-core`.

It is a map, not a complete Rustdoc replacement. Use it to decide which API
area to inspect next.

User apps should normally import these through the public facade crate:

```rust
use slipway::prelude::*;
```

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
  consumed, or propagated.

Authoring rule: a visual region that should react must have the matching
declaration. Painting something clickable is not enough.

## App Composition

Important concepts:

- `SlipwayApp` - combines N authored widgets into one app.
- `SlipwayAppWidget<A>` - adapts a `SlipwayApp` into a runtime widget.
- `ChildLayoutSeed` and `ChildLayoutPlan` - app/container child layout
  planning.
- `ChildPlacement` - final child placement and slot identity.

Authoring rule: an app with N widgets should expose N authored child widgets.
Do not fake child widgets by painting all children inside one root view.

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
