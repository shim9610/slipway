//! Widget-internal logic only (`docs/public/authoring-layout.md`).
//! May mutate this widget's local state and emit typed messages upward.
//! Must not inspect or mutate sibling widget state — cross-widget effects
//! travel as `ShowcaseMessage`s through the reducer in `communication.rs`.
//!
//! Event handling is authored with ONE `event_handling_table!` per widget:
//! the table generates BOTH `SlipwayLogic::handle_event` and
//! `SlipwayEventDispositionPolicy::event_disposition`, so each arm's
//! pattern+guard IS the declared handledness. The framework consults the
//! declaration BEFORE the handler runs (the backend physical path refuses
//! declared-unhandled events without calling the handler), and because
//! both sides expand from the same tokens, the declaration/handler drift
//! diagnostics `event_declaration.handler_ignored_declared_handled` /
//! `event_declaration.handler_handled_declared_unhandled`
//! (docs/public/api/diagnostics.md) are inexpressible in this form —
//! there is no second hand-written predicate to fall out of sync
//! (audit NC-8, ADR-0003).

use slipway::prelude::*;

use crate::communication::ShowcaseMessage;
use crate::ssot::{
    DraftInputWidget, LIST_MAX_SCROLL_ROWS, LIST_ROW_STEP, NESTED_INNER_MAX_SCROLL_ROWS,
    NESTED_INNER_ROW_STEP, NESTED_OUTER_MAX_SCROLL_ROWS, NESTED_OUTER_ROW_STEP,
    NESTED_SELECTABLE_PANEL, NestedFeedWidget, NoteListWidget, OVERLAY_FEED_MAX_SCROLL_ROWS,
    OVERLAY_FEED_ROW_STEP, OverlayWidget, list_offset_y, list_row_at_card_y, nested_inner_offset_y,
    nested_inner_row_at_card_y, nested_outer_offset_y, nested_region_selector,
    nested_visible_field_rect, overlay_clamped_offset, overlay_roam_titlebar_rect,
    overlay_roaming_clamped_offset, overlay_titlebar_rect, point_in_rect,
};

// ---------------------------------------------------------------------------
// Shared transition helpers (pure functions of the SSOT constants)
// ---------------------------------------------------------------------------

/// One wheel notch = one row, in the direction of the content: a negative
/// `delta_y` (wheel down) advances the offset. Clamping to `[0, max]` is
/// what makes at-limit CHAINING work: a region whose handler can no longer
/// move drops out of the wheel candidate pool on the NEXT event because
/// its declared offset stops changing — see
/// `docs/public/api/routing-and-scroll.md` ("Chaining").
pub fn rows_after_wheel(current: i32, delta_y: f32, max_rows: i32) -> i32 {
    let direction = if delta_y < 0.0 {
        1
    } else if delta_y > 0.0 {
        -1
    } else {
        0
    };
    (current + direction).clamp(0, max_rows)
}

/// Inverse of the row-quantized offset functions in `ssot.rs`: a declared
/// offset (arriving as an `InputEvent::Scroll`, e.g. from MCP scroll
/// control) converts back to whole rows with the SAME step constant.
pub fn rows_from_offset(offset_y: f32, row_step: f32, max_rows: i32) -> i32 {
    (offset_y / row_step).round().clamp(0.0, max_rows as f32) as i32
}

/// Applies one text-edit event to the current draft. Only the edit kinds
/// declared by the widget's `SlipwayTextEditCommandPolicy` (view.rs) reach
/// this point on the declared backend path.
pub fn draft_after_edit(current: &str, kind: TextEditKind, text: Option<&str>) -> String {
    match kind {
        TextEditKind::ReplaceSelection | TextEditKind::ReplaceBuffer => text
            .map(str::to_string)
            .unwrap_or_else(|| current.to_string()),
        TextEditKind::InsertText => {
            let mut next = current.to_string();
            if let Some(text) = text {
                next.push_str(text);
            }
            next
        }
        TextEditKind::DeleteBackward => {
            let mut next = current.to_string();
            next.pop();
            next
        }
        TextEditKind::DeleteForward | TextEditKind::MoveCaret | TextEditKind::Unknown => {
            current.to_string()
        }
    }
}

fn change(target: &WidgetId, field: &str, after: impl Into<String>) -> ChangeEvidence {
    ChangeEvidence {
        target: target.clone(),
        slot: Some(WidgetSlotAddress::new(target.clone(), 0)),
        field: field.to_string(),
        before: None,
        after: Some(after.into()),
    }
}

/// Consumed locally with change evidence (debug probes surface it) but no
/// app message — the shape for pure presentation-state transitions.
fn local_change_outcome(
    target: WidgetId,
    field: &str,
    after: impl Into<String>,
) -> EventOutcome<ShowcaseMessage> {
    let mut outcome = EventOutcome::handled();
    outcome.changes.push(change(&target, field, after));
    outcome
}

/// Consumed, emitting one typed message for the app reducer — the ONLY
/// sanctioned way widget logic affects app state or sibling widgets.
fn message_outcome(
    target: WidgetId,
    name: &str,
    message: ShowcaseMessage,
    field: &str,
    after: impl Into<String>,
) -> EventOutcome<ShowcaseMessage> {
    let mut outcome = EventOutcome::message(target.clone(), name, message);
    outcome.changes.push(change(&target, field, after));
    outcome
}

// ---------------------------------------------------------------------------
// Shared routing scaffolding
// ---------------------------------------------------------------------------

/// Target-phase self-route. The declaration-time capability helpers
/// snapshot this route into every hit region, so `route.address` must
/// mirror the event's slot or admission refuses with
/// `view_contract.hit_route_address_mismatch`.
fn target_route(id: WidgetId, event: &InputEvent) -> EventRoutingPolicyDeclaration {
    EventRoutingPolicyDeclaration {
        target: id.clone(),
        event_target: event.target().clone(),
        route: EventRoute {
            route_id: None,
            address: event.target_slot().cloned(),
            path: vec![id],
            phase: EventRoutePhase::Target,
        },
        capture: Vec::new(),
        diagnostics: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Note list: wheel scrolling, row selection, focus
// ---------------------------------------------------------------------------

// PATTERN: sync-by-construction event handling. One table generates the
// handler AND the declared disposition; unmatched events (and events
// targeted elsewhere) are ignored AND declared unhandled — never add a
// `_` catch-all arm (it would declare every event kind handled).
event_handling_table! {
    impl NoteListWidget {
        |widget, external, local| match event {
            InputEvent::Wheel(wheel) => {
                local.scroll_rows =
                    rows_after_wheel(local.scroll_rows, wheel.delta_y, LIST_MAX_SCROLL_ROWS);
                local_change_outcome(widget.id(), "scroll-rows", local.scroll_rows.to_string())
            },
            InputEvent::Scroll(scroll) => {
                local.scroll_rows =
                    rows_from_offset(scroll.offset_y, LIST_ROW_STEP, LIST_MAX_SCROLL_ROWS);
                local_change_outcome(widget.id(), "scroll-rows", local.scroll_rows.to_string())
            },
            // PATTERN: row selection. The pointer position is target-local
            // (the hit region declared PointerEventCoordinateSpace::
            // TargetLocal) and converts back to a row index through the
            // same LIST_* constants that placed the row's paint and hit
            // region — the MUST-agree discipline in ssot.rs. The arm guard
            // is the declared handledness: a press between rows is
            // declared unhandled and never reaches the body.
            InputEvent::Pointer(pointer)
                if pointer.kind == PointerEventKind::Press
                    && list_row_at_card_y(
                        pointer.position.y,
                        list_offset_y(local.scroll_rows),
                    )
                    .is_some() =>
            {
                let offset_y = list_offset_y(local.scroll_rows);
                let Some(row) = list_row_at_card_y(pointer.position.y, offset_y) else {
                    // Unreachable: the arm guard proved Some with the same
                    // inputs; honest refusal instead of a panic.
                    return EventOutcome::ignored();
                };
                message_outcome(
                    widget.id(),
                    "select-note",
                    ShowcaseMessage::SelectNote(row),
                    "selected-note",
                    row.to_string(),
                )
            },
            InputEvent::Pointer(pointer)
                if pointer.kind == PointerEventKind::Release
                    && list_row_at_card_y(
                        pointer.position.y,
                        list_offset_y(local.scroll_rows),
                    )
                    .is_some() =>
            {
                EventOutcome::handled()
            },
            InputEvent::Focus(focus) => {
                local.focused = focus.focused;
                local_change_outcome(widget.id(), "focused", focus.focused.to_string())
            },
        }
    }
}

impl SlipwayEventRoutingPolicy for NoteListWidget {
    fn event_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        event: &InputEvent,
    ) -> EventRoutingPolicyDeclaration {
        target_route(self.id(), event)
    }
}

// ---------------------------------------------------------------------------
// Draft input: text editing through declared text-edit routes
// ---------------------------------------------------------------------------

event_handling_table! {
    impl DraftInputWidget {
        |widget, external, local| match event {
            // PATTERN: text editing. The buffer's source of truth is app
            // state (`ShowcaseState::draft`), so an edit is not applied
            // locally — it becomes a ReplaceDraft message and the reducer
            // writes it. Next frame the buffer policy (view.rs) snapshots
            // the updated draft. Never mutate the buffer directly here.
            InputEvent::TextEdit(edit) => {
                local.edit_count += 1;
                let next = draft_after_edit(&external.draft, edit.kind, edit.text.as_deref());
                message_outcome(
                    widget.id(),
                    "replace-draft",
                    ShowcaseMessage::ReplaceDraft(next.clone()),
                    "draft",
                    next,
                )
            },
            InputEvent::Text(text) => {
                local.edit_count += 1;
                let next = format!("{}{}", external.draft, text.text);
                message_outcome(
                    widget.id(),
                    "replace-draft",
                    ShowcaseMessage::ReplaceDraft(next.clone()),
                    "draft",
                    next,
                )
            },
            InputEvent::Focus(focus) => {
                local.focused = focus.focused;
                local_change_outcome(widget.id(), "focused", focus.focused.to_string())
            },
        }
    }
}

impl SlipwayEventRoutingPolicy for DraftInputWidget {
    fn event_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        event: &InputEvent,
    ) -> EventRoutingPolicyDeclaration {
        target_route(self.id(), event)
    }
}

// ---------------------------------------------------------------------------
// Overlay: titlebar drag + wheel-scrollable feed behind the panel
// ---------------------------------------------------------------------------

event_handling_table! {
    impl OverlayWidget {
        |widget, external, local| match event {
            // PATTERN: drag with PointerCaptureIntent::DuringDrag (the hit
            // region in view.rs declares the capture; the runtime keeps
            // Move/Release routed here even when the pointer leaves the
            // titlebar mid-drag). Failure mode without the capture intent:
            // fast drags escape the region and the overlay "sticks" to the
            // cursor's last in-bounds position.
            //
            // TWO panels, two clamps: the ROAMING titlebar is checked
            // first because its declared z (11) fronts the clamped one
            // (10) — the handler's pick must agree with the declared
            // stacking wherever both titlebars contain the point.
            InputEvent::Pointer(pointer) if pointer.kind == PointerEventKind::Press => {
                let card_width = pointer
                    .target_bounds
                    .map(|bounds| bounds.size.width)
                    .unwrap_or(crate::ssot::CARD_MAX_WIDTH);
                // The roaming clamp reads the PROJECTED window
                // (`external.viewport`) — the same value paint and the
                // declared allowance read, so the handler can never store
                // an offset the next frame's declaration refuses.
                let roam_offset = overlay_roaming_clamped_offset(
                    card_width,
                    external.viewport,
                    local.roam_offset,
                );
                if point_in_rect(pointer.position, overlay_roam_titlebar_rect(roam_offset)) {
                    local.roam_dragging = true;
                    local.roam_anchor = Point {
                        x: pointer.position.x - roam_offset.x,
                        y: pointer.position.y - roam_offset.y,
                    };
                    local_change_outcome(widget.id(), "roam-dragging", "true")
                } else if point_in_rect(pointer.position, overlay_titlebar_rect(local.offset)) {
                    local.dragging = true;
                    local.drag_anchor = Point {
                        x: pointer.position.x - local.offset.x,
                        y: pointer.position.y - local.offset.y,
                    };
                    local_change_outcome(widget.id(), "dragging", "true")
                } else {
                    EventOutcome::handled()
                }
            },
            // The Move guard is the declared disposition: hover moves
            // while nothing drags are declared unhandled and never reach
            // the body.
            InputEvent::Pointer(pointer)
                if pointer.kind == PointerEventKind::Move
                    && (local.dragging || local.roam_dragging) =>
            {
                if !pointer.details.buttons.primary {
                    // A Move without the primary button means the Release
                    // was lost (e.g. outside the window): stop dragging
                    // instead of warping the panel on the next hover.
                    local.dragging = false;
                    local.roam_dragging = false;
                    return local_change_outcome(widget.id(), "dragging", "false");
                }
                let card_width = pointer
                    .target_bounds
                    .map(|bounds| bounds.size.width)
                    .unwrap_or(crate::ssot::CARD_MAX_WIDTH);
                let desired = Point {
                    x: pointer.position.x
                        - if local.roam_dragging {
                            local.roam_anchor.x
                        } else {
                            local.drag_anchor.x
                        },
                    y: pointer.position.y
                        - if local.roam_dragging {
                            local.roam_anchor.y
                        } else {
                            local.drag_anchor.y
                        },
                };
                if local.roam_dragging {
                    // ROAMING clamp: to the declared overflow bounds — the
                    // panel may leave the feed band and the card, roaming
                    // the whole live window (projected viewport).
                    let next =
                        overlay_roaming_clamped_offset(card_width, external.viewport, desired);
                    if next == local.roam_offset {
                        return EventOutcome::handled();
                    }
                    local.roam_offset = next;
                    local_change_outcome(
                        widget.id(),
                        "roam-offset",
                        format!("{:.0},{:.0}", next.x, next.y),
                    )
                } else {
                    // CLAMPED clamp: to the feed band inside layout bounds.
                    let next = overlay_clamped_offset(card_width, desired);
                    if next == local.offset {
                        return EventOutcome::handled();
                    }
                    local.offset = next;
                    local_change_outcome(
                        widget.id(),
                        "offset",
                        format!("{:.0},{:.0}", next.x, next.y),
                    )
                }
            },
            InputEvent::Pointer(pointer)
                if matches!(
                    pointer.kind,
                    PointerEventKind::Release | PointerEventKind::Cancel
                ) =>
            {
                local.dragging = false;
                local.roam_dragging = false;
                local_change_outcome(widget.id(), "dragging", "false")
            },
            // The feed behind the overlay panel. A wheel landing here while
            // the cursor is OVER the panel is the wheel-transparency
            // pattern working: the panel layer is pointer-opaque but
            // wheel-pass-through (view.rs).
            InputEvent::Wheel(wheel) => {
                local.feed_rows =
                    rows_after_wheel(local.feed_rows, wheel.delta_y, OVERLAY_FEED_MAX_SCROLL_ROWS);
                local_change_outcome(widget.id(), "feed-rows", local.feed_rows.to_string())
            },
            InputEvent::Scroll(scroll) => {
                local.feed_rows = rows_from_offset(
                    scroll.offset_y,
                    OVERLAY_FEED_ROW_STEP,
                    OVERLAY_FEED_MAX_SCROLL_ROWS,
                );
                local_change_outcome(widget.id(), "feed-rows", local.feed_rows.to_string())
            },
        }
    }
}

impl SlipwayEventRoutingPolicy for OverlayWidget {
    fn event_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        event: &InputEvent,
    ) -> EventRoutingPolicyDeclaration {
        target_route(self.id(), event)
    }
}

// ---------------------------------------------------------------------------
// Nested feed: region-driven wheel handling (chaining decided by routing)
// + row selection inside the selectable inner panel
// ---------------------------------------------------------------------------

/// Card-local pointer y -> selectable-panel row, at the CURRENT scroll
/// state. Shared by the table arm's GUARD (the declared disposition) and
/// its BODY so the two read the same math; the function is the ssot
/// inverse, fed with the same two offsets the paint and hit regions
/// applied this frame.
fn nested_selectable_row_at(
    local: &crate::view::NestedLocal,
    position: Point,
    card_width: f32,
) -> Option<usize> {
    let outer_offset = nested_outer_offset_y(local.outer_rows);
    let visible = nested_visible_field_rect(NESTED_SELECTABLE_PANEL, outer_offset, card_width)?;
    let inner_offset = nested_inner_offset_y(
        local.inner_rows[NESTED_SELECTABLE_PANEL],
        visible.size.height,
    );
    nested_inner_row_at_card_y(position.y, outer_offset, inner_offset, card_width)
}

event_handling_table! {
    impl NestedFeedWidget {
        |widget, external, local| match event {
            // PATTERN: row selection inside a SCROLLED region. The pointer
            // position is target-local (card-local); the row index comes
            // from the ssot inverse that subtracts BOTH current offsets —
            // the exact rects the per-row hit regions declared this frame.
            // Like the notes list, selection is app state: the click
            // becomes a typed SelectInnerItem message and the reducer is
            // the only writer.
            InputEvent::Pointer(pointer)
                if pointer.kind == PointerEventKind::Press
                    && nested_selectable_row_at(
                        local,
                        pointer.position,
                        pointer
                            .target_bounds
                            .map(|bounds| bounds.size.width)
                            .unwrap_or(crate::ssot::CARD_MAX_WIDTH),
                    )
                    .is_some() =>
            {
                let card_width = pointer
                    .target_bounds
                    .map(|bounds| bounds.size.width)
                    .unwrap_or(crate::ssot::CARD_MAX_WIDTH);
                let Some(row) = nested_selectable_row_at(local, pointer.position, card_width)
                else {
                    // Unreachable: the arm guard proved Some with the same
                    // inputs; honest refusal instead of a panic.
                    return EventOutcome::ignored();
                };
                message_outcome(
                    widget.id(),
                    "select-inner-item",
                    ShowcaseMessage::SelectInnerItem(row),
                    "selected-inner-item",
                    row.to_string(),
                )
            },
            InputEvent::Pointer(pointer)
                if pointer.kind == PointerEventKind::Release
                    && nested_selectable_row_at(
                        local,
                        pointer.position,
                        pointer
                            .target_bounds
                            .map(|bounds| bounds.size.width)
                            .unwrap_or(crate::ssot::CARD_MAX_WIDTH),
                    )
                    .is_some() =>
            {
                EventOutcome::handled()
            },
            // PATTERN: nested wheel. Which region id arrives here is
            // decided ENTIRELY by the declared routing: every region
            // declares the NearestScrollable default (view.rs), so a wheel
            // over an inner panel delivers THAT inner's id while it can
            // consume, and the outer's id once that inner is at its limit
            // (default at-limit chaining — routing-and-scroll.md). The
            // handler stays region-driven and encodes no selection-order
            // assumptions of its own.
            InputEvent::Wheel(wheel) => {
                let selector = wheel
                    .region_id
                    .as_ref()
                    .and_then(nested_region_selector)
                    .unwrap_or(None);
                match selector {
                    Some(index) => {
                        local.inner_rows[index] = rows_after_wheel(
                            local.inner_rows[index],
                            wheel.delta_y,
                            NESTED_INNER_MAX_SCROLL_ROWS,
                        );
                        local_change_outcome(
                            widget.id(),
                            "inner-rows",
                            format!("{:?}", local.inner_rows),
                        )
                    }
                    None => {
                        local.outer_rows = rows_after_wheel(
                            local.outer_rows,
                            wheel.delta_y,
                            NESTED_OUTER_MAX_SCROLL_ROWS,
                        );
                        local_change_outcome(
                            widget.id(),
                            "outer-rows",
                            local.outer_rows.to_string(),
                        )
                    }
                }
            },
            // A Scroll for a region id this widget does not own is
            // declared unhandled by the guard (the disposition) instead of
            // by a body-side early return — same predicate, single source.
            InputEvent::Scroll(scroll)
                if nested_region_selector(&scroll.region_id).is_some() =>
            {
                let Some(selector) = nested_region_selector(&scroll.region_id) else {
                    // Unreachable: the arm guard proved Some.
                    return EventOutcome::ignored();
                };
                match selector {
                    Some(index) => {
                        local.inner_rows[index] = rows_from_offset(
                            scroll.offset_y,
                            NESTED_INNER_ROW_STEP,
                            NESTED_INNER_MAX_SCROLL_ROWS,
                        );
                        local_change_outcome(
                            widget.id(),
                            "inner-rows",
                            format!("{:?}", local.inner_rows),
                        )
                    }
                    None => {
                        local.outer_rows = rows_from_offset(
                            scroll.offset_y,
                            NESTED_OUTER_ROW_STEP,
                            NESTED_OUTER_MAX_SCROLL_ROWS,
                        );
                        local_change_outcome(
                            widget.id(),
                            "outer-rows",
                            local.outer_rows.to_string(),
                        )
                    }
                }
            },
        }
    }
}

impl SlipwayEventRoutingPolicy for NestedFeedWidget {
    fn event_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        event: &InputEvent,
    ) -> EventRoutingPolicyDeclaration {
        target_route(self.id(), event)
    }
}
