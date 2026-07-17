# Wheel Routing And Scroll Regions

The authoring contract for scrollable regions, overlapping-region order,
wheel routing, scroll indicators, overlay drag patterns, and wheel
transparency. All items on this page import from `slipway::prelude::*`.

## Declare A Scroll Region After Layout

Scroll regions are built by the capability helpers, never by struct literal
(`ScrollRegionDeclaration` is `#[non_exhaustive]`):

```rust
// Both return a ScrollRegionDeclaration.
scroll_region_from_scrollable_capability(
    &widget, &external, &local, &layout_output, region_id, address, enabled)
scroll_region_from_scrollable_capability_with_order(
    &widget, &external, &local, &layout_output, region_id, address, enabled,
    order: HitRegionOrder)
```

Both require `SlipwayScrollableContainerCapability` (implement
`SlipwayScrollBehaviorPolicy` and `SlipwayWheelRoutingPolicy` by hand; cover
the RESERVED bounds with `reserved_policy_defaults!`, see
[Trait surface](trait-surface.md)). Pass the final `LayoutOutput`, never the
incoming `LayoutInput`. Geometry contract at admission (see
[Diagnostics](diagnostics.md)): `content_bounds` must contain the viewport;
travel requires content larger than viewport; `offset` is non-negative and
clamped to `content - viewport` per enabled axis; enabled regions declare at
least one axis. The wheel is consumed only when the declared
`ScrollConsumptionPolicy.wheel` is true.

Clamp the source state before you declare the region. The visible backends may
defensively crop invalid scroll geometry so a live window can show a refusal
panel instead of impossible rectangles, but that is not an authoring API. A
state value below zero, above `content - viewport`, or derived from stale
pre-layout size is a widget/app declaration bug; fix the reducer or projection
that produced the offset.

If admission sent you here with the
`view_contract.content_overflow_without_scroll_region` advisory, your view
paints content beyond its layout/frame viewport that no enabled scroll
region's `content_bounds` covers: declare one of the regions above sized to
the full content (for a whole page taller than the window, the app-level
page region under "Chaining" below), or clip the overflow intentionally
(group/layer clip, or `PaintOrderDeclaration::with_overflow_bounds` for
deliberate overflow paint).

## Nesting Order: HitRegionOrder And The Equal-Order Refusal

`HitRegionOrder { z_index, paint_order, traversal_order }` is the one total
order every declared-region selector uses to pick a front-most region:
`z_index` first, then `paint_order`, then `traversal_order`
(`compare_hit_region_order`). Equal orders are NOT front of each other.

The plain helper defaults the order to `{0, 0, 0}`. Enabled wheel-consuming
scroll regions that can overlap with identical ordering refuse admission
with `view_contract.ambiguous_wheel_overlap` (naming both region ids and the
shared order); overlapping hit regions with identical ordering refuse with
`view_contract.ambiguous_hit_overlap`. The fix is the one the refusal names:
distinct orders via `scroll_region_from_scrollable_capability_with_order`
(or the `order` argument of `hit_region_from_pointer_capability`). Rule of
thumb: any widget declaring more than one scroll region — or whose scroll
region can sit under an overlay's hit region — uses `_with_order` instead of
the 0/0/0 default.

Paint-overlap allowance is NOT hit-ambiguity acceptance: setting
`PaintOrderDeclaration.allow_overlap` (the overlay/popup paint intent) does
not silence `ambiguous_hit_overlap` — an overlay whose hit region ties with
the regions it covers still refuses admission, because equal orders make
pointer selection in the shared area arbitrary. The only escape hatch is the
explicit `PaintOrderDeclaration.allow_ambiguous_hits` flag; prefer a
distinct order on the overlay's hit region instead (a `z_index` above
everything it covers, mirroring its paint layer). The composed app view
re-checks the flattened regions of ALL children, so a full-viewport modal
child whose hit region ties with sibling regions is refused at app
admission even though each child alone validates cleanly.

## Wheel Routing Modes

`ScrollRegionDeclaration.wheel_routing` carries a `WheelRouting` mode:

- `NearestScrollable` (default, recommended): the front-most scroll region
  under the cursor that can consume the delta wins — the region the user
  points at scrolls first. Re-confirmed as the house default by a live-UX
  decision (a `SelfFirst` outer read as broken over inner panels).
- `SelfFirst`: if this region is under the cursor and can consume the delta,
  it wins outright — even over a fronter region that would win by order.
  Displaces inner regions; author it only when the outer surface must own the
  wheel.
- `ParentFirst`: this region defers to the front-most eligible ancestor
  (a candidate whose viewport strictly contains this region's viewport). With
  no eligible ancestor it consumes normally.
- `Custom`: RESERVED. It currently routes exactly like `NearestScrollable`;
  do not author it expecting distinct semantics.

Selection precedence (total and deterministic): (1) only enabled regions
containing the point that can consume the delta are candidates — an at-limit
region drops out, so no declared preference black-holes the wheel; (2) any
`SelfFirst` candidate wins; (3) otherwise the front-most candidate by
`HitRegionOrder`, with `ParentFirst` deference applied; (4) all-default
selection is exactly the front-most containing consumable candidate.

## Authoring A Non-Default Mode (Declaration-Time Only)

The mode comes from the widget's `SlipwayWheelRoutingPolicy`:

```rust
impl SlipwayWheelRoutingPolicy for MyWidget {
    fn wheel_routing_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        region: &PresentationRegionId,
    ) -> WheelRoutingPolicyDeclaration {
        WheelRoutingPolicyDeclaration {
            target: self.id(),
            // Per-region authoring: match on `region`.
            routing: if region.as_str().ends_with(":outer") {
                WheelRouting::SelfFirst
            } else {
                WheelRouting::NearestScrollable
            },
            modifiers: None,
            diagnostics: Vec::new(),
        }
    }
}
```

The policy is consulted once per region at declaration time — it receives
only the resolved `PresentationRegionId`, and the returned mode is frozen
into the declaration for the frame. Per-event dynamic wheel routing is
unsupported by construction: delta- or event-dependent logic here can
never take effect.

## Declared Scroll Indicators

`ScrollRegionDeclaration.indicator` carries a `ScrollIndicatorMode`, set
with `.with_scroll_indicator(mode)` on a helper-built declaration —
explicit when declared, automatic when unspecified:

- `Auto` (every helper's default): backend-automatic, byte-identical to the
  pre-control behavior — both backends draw an indicator for an enabled,
  vertically overflowing region they present themselves; iced leaves
  native-backed regions (real child placements) to the native scrollbar.
- `Hidden`: never draws an indicator; on an iced native-backed region the
  native rail is suppressed too (zero-width). Visual control only — wheel
  routing and scrolling are unchanged.
- `Visible`: draws whenever geometrically sensible (vertical axis, content
  taller than viewport); on an iced native-backed region the native
  scrollbar IS the indicator (nothing is double-drawn).

Every PRESENT indicator is interactive by default — no extra declaration:
dragging the thumb scrolls the region (the backend captures the drag
internally and synthesizes declared Scroll events, exactly like a native
scrollbar sync; nothing is injected into your hit regions), and a track
click JUMPS the thumb to the clicked position (and keeps dragging from
there). The modes above control PRESENCE only. An authored opaque layer
painted over an indicator owns its pixels — presses there route to the
layer, not the thumb.

Worked example: `crates/slipway-example-authored/src/view.rs` (one nested
inner `Visible`, one `Hidden`, outer and list keep `Auto`).

## Chaining: Inner To Outer To Page

When the pointed-at region cannot consume the delta direction (at its scroll
limit), it drops out of the candidate pool and the next front-most
containing consumable region wins, per event, in both directions: a
down-wheel over a nested inner scrolls the inner, hands off to the outer at
the inner's limit, then to the page (root) region at the outer's; an
up-wheel is reclaimed by the innermost region as soon as it has travel again
— hand-off is not sticky.

The page (root) region is an app-level declaration:
`SlipwayApp::app_scroll_regions` appends regions to the composed view —
declare one covering the frame viewport with the full column as content,
gated on the column actually exceeding the window, at a negative `z_index`
so every widget region fronts it. Chaining to it is then automatic (it is
the outermost containing candidate), and wheel over dead space scrolls the
page.

Worked examples: `crates/slipway-example-authored/src/view.rs`
(`nested_scroll_regions` — outer plus two inners, `_with_order` distinct
orders, at-limit chaining, and the anchoring cap) and
`crates/slipway-example-authored/src/communication.rs`
(`app_scroll_regions` — the page region and its app-level handler).

## Wheel-Transparent Overlays (Pointer-Opaque, Wheel-Pass-Through)

`PaintOp::Layer` carries two independent occlusion channels:
`input_transparency` (pointer) and `wheel_transparency` (wheel);
`wheel_transparency: None` is automatic (the wheel follows the pointer
channel), setting it declares the wheel channel independently:

```rust
// Pointer-opaque body; wheeling over it scrolls the page behind it.
PaintOp::keyed_layer(PaintLayerKey::ordered(z_index, order), ops)
    .with_layer_id("overlay-body")
    .with_wheel_transparency(PaintInputTransparency::PassThrough)
```

`keyed_layer` layers default to pointer-opaque. The overlay still needs its
own `HitRegionDeclaration`s (with a front `HitRegionOrder`) for its
controls; wheel transparency only opens the wheel channel to the scroll
owner behind the layer. The reverse also works:
`with_wheel_transparency(PaintInputTransparency::Opaque)` blocks the wheel
on a pointer-pass-through layer.

Your own opaque layer never blocks your own hit regions declared at the
layer's `z_index`: a hit region rides the layer it accompanies (declare the
region's `z_index` equal to the layer's z, as in a full-viewport modal with
a `z_index: 100` layer and a `z_index: 100` hit region). Within one
widget's same-z stack, only an AUTHORED within-z order
(`PaintLayerKey::ordered(z, order)`) fronts a lower declared `paint_order`
— overlapping card stacks keep absorbing presses for the cards beneath
them. A press over an opaque layer that reaches NO hit region is consumed,
and the backend records refusal evidence naming the blocking layer
(inspect it via the `diagnostics` probe kind or the physical-control
reply's `post_hoc_diagnosis` attachment) — blocked presses never vanish
silently.

Worked example: the movable overlays in the same `view.rs` (pointer-opaque,
wheel-transparent `keyed_layer`s, drag capture, and the layer clip that
iced presentation needs).

## Overlay Drag Patterns: Clamped Or Roaming

Two contract-safe ways to keep a dragged overlay admissible; pick one:

- **Clamped** (simplest): clamp the drag offset so panel paint and titlebar
  hit region stay inside layout bounds — no extra declaration (exceeding
  layout refuses with `view_contract.hit_bounds_outside_layout`).
- **Roaming**: declare `view.paint_order.with_overflow_bounds(rect)` so hit
  regions and paint may leave layout bounds. Admission then checks
  EVERYTHING this view paints or declares against that rect
  (`view_contract.*_outside_overflow_bounds`), so the drag clamp and the
  declared rect must share one source; a child's allowance composes into
  the app root's view automatically.

For a WINDOW-TRUE allowance (the panel roams the whole live window at any
window size), the window size must be readable by paint, the hit
declaration, AND the drag handler — `ViewDefinitionInput.frame.viewport`
reaches only `view_definition`. Override
`SlipwayApp::project_frame_viewport` (the runtime-invoked platform-truth
hook, the one sanctioned external-state writer besides the reducer) to
mirror the live viewport into external state, and derive both the
allowance and the clamp from that field.

Worked example: the same `view.rs` (`OverlayWidget` — the purple panel
clamps to its band, the amber panel roams the whole live window via the
projected `ShowcaseState::viewport`; the admission stress harness is the
extreme version).

## Inspecting Live Routing: The dispatch_graph Probe

The derived dispatch graph is the declared-route oracle for one presented
frame; request it through the debug MCP (see [Debug MCP](debug-mcp.md)):

```json
{"jsonrpc":"2.0","id":"graph-1","method":"tools/call","params":{"name":"slipway.debug.probe","arguments":{"frame":"current","kinds":["dispatch_graph"]}}}
```

Nodes are hit/focus/scroll/occlusion regions; edges carry a channel and a
meaning: `HitOrder` (who wins an overlap), `Occlusion` (who blocks a
channel), `Chaining` (where the wheel goes at-limit), `Capture`, and
`FocusRoute`. A non-default `WheelRouting` shows up as flipped or deferred
wheel `HitOrder`/`Chaining` edges; a wheel-transparent overlay has pointer
`Occlusion` edges but no wheel ones. For routing bugs, dump this graph first
and compare dispatch evidence against its edges before hypothesizing.
