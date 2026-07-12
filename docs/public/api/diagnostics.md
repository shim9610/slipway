# Diagnostics Catalog

Slipway refuses contract violations with structured `Diagnostic` values:
`{ target, severity, code, message }`. Messages embed the offending region
id, the declared rect, the permitted bounds, and the exact fixing API, so
read the message first — this page is the index of every code.

Blocking rule: a diagnostic with severity `error` or `unsupported` is
blocking (`view_definition_has_blocking_contract_diagnostic`). A blocking
admission diagnostic makes the visible backend paint a refusal panel in
place of the widget instead of running it. `warning`/`info` are evidence
only.

## Validate Before Launch

Run the same admission validation the backends run, in a unit test, before
launching a window. Both functions are in `slipway::prelude::*`:

```rust
let diagnostics =
    view_definition_contract_diagnostics_for_capabilities(&view, &widget.capabilities());
assert!(!view_definition_has_blocking_contract_diagnostic(&diagnostics), "{diagnostics:?}");
```

`view_definition_contract_diagnostics(&view)` is the capability-independent
subset. The capability-aware variant additionally checks that every declared
`Capability` has its matching enabled region.

## view_contract: Admission Refusals

Severity is `error` (blocking) for every row except the two marked `warn`.

| Code | Trigger | Fix |
|------|---------|-----|
| `view_contract.paint_order_target_mismatch` | `paint_order.target` differs from the view target | build `PaintOrderDeclaration::source_order(self.id())` for the widget's own id |
| `view_contract.overflow_bounds_missing` | `allow_overflow_paint` without `overflow_bounds` | use `PaintOrderDeclaration::with_overflow_bounds(..)` |
| `view_contract.overflow_bounds_invalid` | overflow bounds non-finite or negative | declare finite, non-negative overflow bounds |
| `view_contract.frame_viewport_invalid` | frame viewport non-finite or negative | pass the backend-provided `FrameIdentity` through unchanged |
| `view_contract.layout_bounds_invalid` | layout bounds non-finite or negative | return finite bounds from `SlipwayView::layout` |
| `view_contract.layout_bounds_not_target_local` | layout bounds origin is not `0,0` | keep layout target-local; parent placement belongs in `ChildPlacement` |
| `view_contract.layout_outside_frame_viewport` (warn) | layout bounds extend outside the frame viewport | size the layout from the `LayoutInput` viewport |
| `view_contract.child_input_viewport_not_target_local` | app layout plan gave a child a non-zero-origin input viewport | place children with `ChildPlacement` bounds; child `LayoutInput` stays origin `0,0` |
| `view_contract.hit_bounds_invalid` | hit bounds non-finite or negative | fix the `bounds` passed to `hit_region_from_pointer_capability` |
| `view_contract.hit_bounds_outside_layout` | enabled hit region leaves layout bounds without overflow paint | keep bounds target-local inside layout, or declare overflow bounds |
| `view_contract.hit_bounds_outside_overflow_bounds` | hit region leaves the declared overflow bounds | grow `overflow_bounds` or shrink the region |
| `view_contract.hit_route_empty` | enabled hit region with an empty route path | let `hit_region_from_pointer_capability` snapshot the route from `SlipwayEventRoutingPolicy` |
| `view_contract.hit_route_target_missing` | route path does not contain the region target | include the widget id in the `EventRoutingPolicyDeclaration` route path |
| `view_contract.hit_route_address_mismatch` | region `address` differs from `route.address` | make the routing policy's `route.address` match the declared region address |
| `view_contract.ambiguous_hit_overlap` | enabled hit regions overlap with identical `HitRegionOrder` | distinct `order` argument of `hit_region_from_pointer_capability`, explicit overlap, or disjoint geometry |
| `view_contract.pointer_capability_missing_hit_region` | `PointerInput`/`HitRegionPresentation` declared, no enabled hit region | declare one via `hit_region_from_pointer_capability` or remove the capability |
| `view_contract.focus_bounds_invalid` | focus bounds non-finite or negative | fix the `bounds` passed to the focus helpers |
| `view_contract.focus_bounds_outside_layout` | enabled focus region leaves layout bounds without overflow paint | keep bounds inside layout, or declare overflow bounds |
| `view_contract.focus_bounds_outside_overflow_bounds` | focus region leaves the declared overflow bounds | grow `overflow_bounds` or shrink the region |
| `view_contract.focus_capability_missing_focus_region` | `FocusInput`/`KeyboardInput`/`FocusRegionPresentation` declared, no enabled focus region | `focus_region_from_focus_capability` (`text_edit_focus_region_from_capability` for text input) |
| `view_contract.text_input_missing_text_edit_focus_region` | `TextInput`/`TextEditRegionPresentation` declared, no enabled text-edit focus region | `text_edit_focus_region_from_capability` or remove `TextInput` |
| `view_contract.text_edit_buffer_target_mismatch` | text buffer target differs from the focus region target | return the region owner's id from `SlipwayTextBufferPolicy` |
| `view_contract.text_edit_selection_target_mismatch` | selection target differs from the focus region target | `SlipwayTextSelectionPolicy` must target the region owner |
| `view_contract.text_edit_composition_target_mismatch` | IME composition target differs from the focus region target | `SlipwayImeCompositionPolicy` must target the region owner |
| `view_contract.text_edit_caret_target_mismatch` | caret geometry target differs from the focus region target | `SlipwayCaretGeometryPolicy` must target the region owner |
| `view_contract.text_edit_visual_style_target_mismatch` | visual style target differs from the focus region target | `SlipwayTextInputVisualStylePolicy` must target the region owner |
| `view_contract.text_edit_typography_target_mismatch` | typography target differs from the focus region target | `SlipwayTextInputTypographyPolicy` must target the region owner |
| `view_contract.text_edit_undo_target_mismatch` | undo/redo target differs from the focus region target | `SlipwayTextUndoRedoPolicy` must target the region owner |
| `view_contract.text_edit_typography_invalid_font_size` | font size non-finite or `<= 0` | return a positive finite size from `SlipwayTextInputTypographyPolicy` |
| `view_contract.text_edit_visual_style_invalid_metric` | border width/radius non-finite or negative | fix the metrics in `SlipwayTextInputVisualStylePolicy` |
| `view_contract.text_edit_selection_out_of_bounds` | selection range exceeds the buffer length | clamp the range in `SlipwayTextSelectionPolicy` |
| `view_contract.text_edit_composition_cursor_out_of_bounds` | composition cursor range exceeds the buffer length | clamp the range in `SlipwayImeCompositionPolicy` |
| `view_contract.text_edit_viewport_range_out_of_bounds` | text viewport visible range exceeds the buffer length | clamp the range in `SlipwayTextFlowPolicy` |
| `view_contract.text_edit_single_line_contains_newline` | `SingleLine` buffer contains `\n`/`\r` | strip newlines, or declare multi-line via `SlipwayTextFlowPolicy` |
| `view_contract.text_edit_missing_insert_command` | editable region without an enabled `InsertText` command | declare it in `SlipwayTextEditCommandPolicy::text_edit_commands` |
| `view_contract.text_edit_missing_delete_command` | editable region without a delete command | declare `DeleteBackward`/`DeleteForward` commands |
| `view_contract.text_edit_missing_replace_buffer_command` | editable region without an enabled `ReplaceBuffer` command | declare it (native backend text widgets replace the buffer) |
| `view_contract.scroll_capability_missing_scroll_region` | `WheelInput`/`ScrollRegionPresentation` declared, no enabled scroll region | declare one via `scroll_region_from_scrollable_capability` or remove the capability |
| `view_contract.scroll_geometry_invalid` | scroll viewport/content bounds non-finite or negative | fix the geometry in `SlipwayScrollBehaviorPolicy` |
| `view_contract.scroll_viewport_outside_layout` | enabled scroll viewport leaves layout bounds without overflow paint | derive the viewport from the final `LayoutOutput` (see [Routing and scroll](routing-and-scroll.md)) |
| `view_contract.scroll_viewport_outside_overflow_bounds` | scroll viewport leaves the declared overflow bounds | grow `overflow_bounds` or shrink the viewport |
| `view_contract.ambiguous_wheel_overlap` | enabled wheel-consuming scroll regions overlap with identical `HitRegionOrder` | `scroll_region_from_scrollable_capability_with_order` (or assign `ScrollRegionDeclaration::order`) |
| `view_contract.scroll_axes_empty` | enabled scroll region with no scroll axis | declare at least one axis in `ScrollAxes` |
| `view_contract.scroll_offset_invalid` | offset non-finite or negative | offsets are `>= 0`; positive y means content scrolled up |
| `view_contract.scroll_offset_on_disabled_x_axis` | non-zero x offset with horizontal disabled | zero the x offset or enable the axis |
| `view_contract.scroll_offset_on_disabled_y_axis` | non-zero y offset with vertical disabled | zero the y offset or enable the axis |
| `view_contract.scroll_content_does_not_cover_viewport` | `content_bounds` does not contain the viewport | content bounds must contain the declared viewport |
| `view_contract.scroll_offset_out_of_range` | offset exceeds `content - viewport` | clamp the offset to the declared travel range |
| `view_contract.paint_bounds_outside_overflow_bounds` | painted op leaves the declared overflow bounds | grow `overflow_bounds` or move the paint |
| `view_contract.paint_bounds_outside_layout` (warn) | painted op leaves layout bounds without overflow allowance | paint inside layout, or declare overflow paint |

The geometry rows share one hint: bounds are target-local (origin `0,0`);
window/parent placement belongs in `ChildPlacement`.

## backend_input: Dispatch-Evidence Refusals

All `error` (blocking for the event: the input is refused before widget
logic runs, and no state mutates) except `dispatch_refused` (`warning`,
retained evidence). These are not authoring APIs — backend adapters attach
the evidence. If one appears during app work, the usual causes are a stale
`FrameIdentity` or declarations that changed between frames.

| Code | Trigger |
|------|---------|
| `backend_input.dispatch_evidence_missing` | backend input carried no declaration dispatch evidence |
| `backend_input.dispatch_evidence_source_mismatch` | evidence source label or backend id differs from the expected backend |
| `backend_input.dispatch_evidence_frame_mismatch` | evidence frame differs from the current view frame |
| `backend_input.dispatch_evidence_unresolved` | evidence resolved no enabled region (or records a refusal reason) |
| `backend_input.dispatch_evidence_kind_mismatch` | evidence kind does not match the event kind |
| `backend_input.dispatch_evidence_event_mismatch` | the event differs from the generated event recorded in evidence |
| `backend_input.dispatch_evidence_region_mismatch` | the selected region does not match the current declarations |
| `backend_input.dispatch_evidence_route_mismatch` | the route differs from the declared event route |
| `backend_input.dispatch_evidence_candidates_mismatch` | the candidate-region set does not match the current declarations |
| `backend_input.dispatch_refused` | a no-consumer refusal was retained in the bounded refusal ring; surfaced by the `diagnostics` probe kind |

## event_declaration: Handler/Declaration Reconciliation

| Code | Severity | Trigger and fix |
|------|----------|-----------------|
| `event_declaration.dispatch_route_mismatch` | error | physical dispatch route differs from the widget's `SlipwayEventRoutingPolicy`; keep the policy deterministic per event |
| `event_declaration.handler_ignored_declared_handled` | warning (semantic path), error (physical path) | the handler returned `ignored()` but `SlipwayEventDispositionPolicy` declared handled; align the two |
| `event_declaration.handler_handled_declared_unhandled` | warning (semantic path), error (physical path) | the handler handled an event the disposition declared unhandled; align the two |

## event_equivalence: MCP vs Backend Input Comparison

Non-blocking comparison evidence between MCP physical-equivalent input and
backend-presented input sharing a frame.

| Code | Severity | Meaning |
|------|----------|---------|
| `event_equivalence.dispatch_identity_mismatch` | warning | same frame, different declared dispatch identity |
| `event_equivalence.result_identity_mismatch` | warning | same dispatch identity, different result identity |
| `event_equivalence.identity_match` | info | dispatch and result identity match |

## Probe, Layout, And Debug-Control Refusals

| Code | Severity | Trigger and fix |
|------|----------|-----------------|
| `probe.frame_mismatch` | warning | a probe requested a frame that is not the live frame; products derive from CURRENT runtime state, not the requested frame |
| `probe-kind-unsupported` | unsupported | the probe kind parses but is never produced; request one of: topology, state, event, paint, render_packet, render_evidence, dispatch_graph, view_definition, diagnostics |
| `resize-unsupported` | debug failure | `slipway.debug.resize` refuses: no backend performs native resizes; the viewport is unchanged |
| `missing-child-layout` | warning | the app layout plan requested a child that is not in the widget list; keep `layout_plan` children matching `SlipwayApp::widgets` |
| `app-font-resolution-refused` | unsupported | the `SlipwayApp::resolve_app_font` default refused honestly: the app declares no loadable font source, so backends fall back to their own fonts; override `resolve_app_font` only to declare a real source ([IME and fonts](ime.md)) |

## Adjacent Families (Not Admission Refusals)

Backends emit repair/evidence diagnostics under backend-prefixed codes
(`iced.visible_scroll.*`, `egui.visible_scroll.*`, paint/provider
`*_unsupported` codes). Those are presentation evidence: treat a scroll
normalization repair as a bug in the widget declaration, per
[Core API](core.md). Debug physical-control failure codes
(`backend-physical-control-*`) are MCP tool failures, not admission.

## Completeness Guarantee

This catalog is drift-tested: `crates/slipway-core/tests/diagnostics_catalog.rs`
extracts every code literal in the families above from the workspace sources
and fails if any code is missing from this file (or listed here but gone
from the code). Adding a diagnostic code requires adding its row.
