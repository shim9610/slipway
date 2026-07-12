# Trait Surface: Load-Bearing vs RESERVED

Slipway declares more contract surface than it currently consumes. Every
policy trait is either LOAD-BEARING (a runtime, backend, or helper path
consults it today) or RESERVED (declared ahead of consumption; no path
consults it yet). This page is the public index of that status.

The one rule that matters while authoring: **real logic written in a RESERVED
trait method is a total silent no-op.** Nothing calls it, nothing warns.
Implement RESERVED bounds with the documented empty defaults instead:

```rust
reserved_policy_defaults!(MyWidget);
```

The macro (exported by `slipway::prelude::*`) implements all 23 RESERVED
traits for a widget type that already implements `SlipwayWidgetTypes` and
`SlipwaySsot`. In the source, every RESERVED trait carries a
`RESERVED contract surface` doc marker — `grep "RESERVED"` over
`crates/slipway-core/src/lib.rs` enumerates the reserved surface (see
[Finding the full contract](README.md)).

## Always Load-Bearing: The Mandatory Surface

| Trait | Consumed by |
|-------|-------------|
| `SlipwayWidgetTypes` | associated types for every other trait |
| `SlipwaySsot` | identity, topology, capabilities, admission |
| `SlipwayLogic` | event handling on every dispatch |
| `SlipwayView` | layout, paint, state observation every frame |
| `SlipwayEventRoutingPolicy` | live dispatch + route snapshot in `hit_region_from_pointer_capability` |
| `SlipwayEventDispositionPolicy` | live dispatch (handled/propagate reconciliation) |
| `SlipwayScrollBehaviorPolicy` | `scroll_region_from_scrollable_capability[_with_order]` |
| `SlipwayWheelRoutingPolicy` | same helpers; declaration-time routing snapshot |
| nine text policies + three measurement policies | `text_edit_focus_region_from_capability`, measurement/layout paths |
| `SlipwayFontResolutionPolicy` | egui text-edit font installation + the egui root gate; `SlipwayAppWidget` delegates to `SlipwayApp::resolve_app_font` (default: honest refusal) |

`SlipwayWidget` is shorthand for the trio `SlipwaySsot + SlipwayLogic +
SlipwayView`.

## RESERVED Traits (covered by reserved_policy_defaults!)

`SlipwayContainerLayoutPolicy`, `SlipwayChildConstraintPolicy`,
`SlipwayLayoutInvalidationPolicy`, `SlipwayLayoutEvidencePolicy`,
`SlipwayViewportObservationPolicy`, `SlipwayVirtualCollectionPolicy`,
`SlipwayHitTesting`, `SlipwayViewportContracts`, `SlipwayOverlayContracts`,
`SlipwaySemantics`, `SlipwayFocusTraversal` (exception below),
`SlipwayDebugEventTracePolicy`, `SlipwayPointerCapturePolicy`,
`SlipwayCommandContracts`, `SlipwayCommandInvocationPolicy`,
`SlipwayCommandStatusPolicy`, `SlipwayShortcutRoutingPolicy`,
`SlipwayUndoRedoPolicy`, `SlipwayTimeSourcePolicy`,
`SlipwayRandomSourcePolicy`, `SlipwayExternalDataSnapshotPolicy`,
`SlipwayAnimationTimelinePolicy`, `SlipwayRenderSurfaces`.

**Exception — `SlipwayFocusTraversal::focus_member`:** the plain-focus
helper `focus_region_from_focus_capability` snapshots `focus_member` at
declaration time (it becomes the region's traversal member; `None` declares
no explicit tab order). The macro's `None` default remains valid; implement
the trait by hand only when plain focus regions need an explicit tab order.
`next_focus`/`previous_focus` stay RESERVED.

Also RESERVED at the value level: `WheelRouting::Custom` (routes exactly
like `NearestScrollable` today) and
`ScrollBehaviorPolicyDeclaration.extent` (declared but not consumed by any
workspace path).

## Capability Bundle Triage

The eight capability bundles group these traits. A missing bound fails at
the bundle with a compile error that carries this same triage. Implement the
LOAD-BEARING column by hand; one `reserved_policy_defaults!` call covers the
RESERVED column.

| Bundle | LOAD-BEARING bounds | RESERVED bounds |
|--------|---------------------|-----------------|
| `SlipwayPointerRegionCapability` | `SlipwayWidget`, `SlipwayEventRoutingPolicy`, `SlipwayEventDispositionPolicy` | none |
| `SlipwayScrollableContainerCapability` | trio, `SlipwayScrollBehaviorPolicy`, `SlipwayWheelRoutingPolicy`, routing + disposition | `SlipwayContainerLayoutPolicy`, `SlipwayChildConstraintPolicy`, `SlipwayLayoutInvalidationPolicy`, `SlipwayLayoutEvidencePolicy`, `SlipwayViewportObservationPolicy`, `SlipwayVirtualCollectionPolicy`, `SlipwayHitTesting`, `SlipwaySemantics` |
| `SlipwayTextInputCapability` | trio, the nine text policies, the three measurement policies, routing + disposition | `SlipwayFocusTraversal` (except `focus_member`), `SlipwaySemantics`, `SlipwayDebugEventTracePolicy` |
| `SlipwayPopupCapability` | trio, routing + disposition | `SlipwayOverlayContracts`, `SlipwayFocusTraversal` (except `focus_member`), `SlipwaySemantics`, `SlipwayHitTesting`, `SlipwayPointerCapturePolicy`, `SlipwayCommandContracts` |
| `SlipwayCommandSurfaceCapability` | trio, `SlipwayEventRoutingPolicy` | `SlipwayCommandContracts`, `SlipwayCommandInvocationPolicy`, `SlipwayCommandStatusPolicy`, `SlipwayShortcutRoutingPolicy`, `SlipwayUndoRedoPolicy` |
| `SlipwayDeterministicSourceCapability` | trio | `SlipwayTimeSourcePolicy`, `SlipwayRandomSourcePolicy`, `SlipwayExternalDataSnapshotPolicy`, `SlipwayAnimationTimelinePolicy` |
| `SlipwayProviderSurfaceCapability` | the four provider enumerations, `SlipwayProviderHitTestPolicy`, `SlipwayProviderSnapshotPolicy` | `SlipwayRenderSurfaces` |
| `SlipwayBackendAdmissionCapability` | probe, unsupported-evidence, and parity traits (backend adapters, not app authoring) | none |

No bundle bound may be removed by an author: the bundles are contract
surface declared ahead of full consumption. When a RESERVED trait gains a
consumer, its status changes here, in the code markers, and in the bundle
error notes together.
