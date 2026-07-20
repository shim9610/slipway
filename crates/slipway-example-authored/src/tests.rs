//! Behavior tests for the reference example.
//!
//! Three tiers, all headless:
//! 1. PRE-FLIGHT: every widget view and the composed app view admit with
//!    zero blocking contract diagnostics
//!    (`view_definition_contract_diagnostics_for_capabilities`, the
//!    author-side check the quickstart mandates before launching a
//!    window).
//! 2. GEOMETRY INVARIANTS: the shared-constants discipline is pinned —
//!    painted geometry, declared regions, and pointer math must derive
//!    from the same ssot constants (admission cannot validate paint
//!    against declarations, so these tests are the guard).
//! 3. BEHAVIOR: pointer/wheel/text dispatch through the CORE DECLARATION
//!    RESOLVERS (`resolve_declared_*_dispatch_with_evidence`) applied via
//!    the runtime's validated backend-input path — the same gates a
//!    visible backend uses. Test-side `BackendInputEvent` construction is
//!    the sanctioned semantic/debug path (checklist "Backend Input
//!    Proof"); it is NOT proof of visible input, which the live drivers
//!    cover separately.

use slipway::prelude::*;
// Test-only debug/evidence surface (facade root, not prelude): the
// declaration resolvers and dispatch-evidence types are backend/debug
// APIs, deliberately outside ordinary authoring imports.
use slipway::{
    BackendInputEvent, DeclaredEventDispatchKind, DispatchGraphNodeKind, EvidenceSource,
    apply_physical_event_handling_declaration, declared_event_handling,
    declared_focus_text_dispatch_evidence, derive_dispatch_graph_for_composed_view,
    resolve_declared_pointer_dispatch_with_evidence, resolve_declared_wheel_dispatch_with_evidence,
};

use crate::communication::{ShowcaseApp, apply_messages};
use crate::ssot::{
    self, DraftInputWidget, NestedFeedWidget, NoteListWidget, OverlayWidget, ShowcaseState,
};

type AppRuntime = SlipwayRuntime<SlipwayAppWidget<ShowcaseApp>>;

const FRAME_WIDTH: f32 = 600.0;
const FRAME_HEIGHT: f32 = 640.0;

fn app_runtime() -> AppRuntime {
    app_runtime_with_viewport(FRAME_WIDTH, FRAME_HEIGHT)
}

fn app_runtime_with_viewport(width: f32, height: f32) -> AppRuntime {
    let mut runtime = SlipwayRuntime::from_app(ShowcaseApp::new(), ShowcaseState::default());
    // `record_presented_viewport` also runs the platform-truth projection
    // (`SlipwayApp::project_frame_viewport`), so `ShowcaseState::viewport`
    // carries the live window from here on — like a presenting backend.
    runtime.record_presented_viewport(Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: Size { width, height },
    });
    runtime
}

fn composed_view(runtime: &AppRuntime) -> ViewDefinition {
    let frame = runtime.last_frame_identity();
    let viewport = TargetLocalRect::new(frame.viewport);
    let layout_input = LayoutInput {
        viewport,
        content: viewport,
        constraints: LayoutConstraints {
            min: Size {
                width: 0.0,
                height: 0.0,
            },
            max: frame.viewport.size,
        },
    };
    runtime.widget().view_definition(
        runtime.external(),
        runtime.local_state(),
        ViewDefinitionInput::new(frame, layout_input),
    )
}

fn widget_input(width: f32, height: f32) -> LayoutInput {
    let viewport = TargetLocalRect::new(Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: Size { width, height },
    });
    LayoutInput {
        viewport,
        content: viewport,
        constraints: LayoutConstraints {
            min: Size {
                width: 0.0,
                height: 0.0,
            },
            max: Size { width, height },
        },
    }
}

fn widget_view<W>(widget: &W, local: &W::LocalState, height: f32) -> ViewDefinition
where
    W: SlipwayViewDefinition<ExternalState = ShowcaseState> + SlipwaySsot,
{
    widget_view_with_state(widget, &ShowcaseState::default(), local, height)
}

fn widget_view_with_state<W>(
    widget: &W,
    external: &ShowcaseState,
    local: &W::LocalState,
    height: f32,
) -> ViewDefinition
where
    W: SlipwayViewDefinition<ExternalState = ShowcaseState> + SlipwaySsot,
{
    let layout_input = widget_input(560.0, height);
    widget.view_definition(
        external,
        local,
        ViewDefinitionInput::new(
            FrameIdentity {
                surface_id: "test-surface".to_string(),
                surface_instance_id: "test-instance".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: layout_input.viewport.into_rect(),
            },
            layout_input,
        ),
    )
}

fn pointer_input(
    view: &ViewDefinition,
    point: Point,
    kind: PointerEventKind,
    primary_held: bool,
) -> BackendInputEvent {
    let details = PointerDetails {
        buttons: PointerButtons {
            primary: primary_held,
            ..PointerButtons::default()
        },
        ..PointerDetails::default()
    };
    let (dispatch, evidence) = resolve_declared_pointer_dispatch_with_evidence(
        EvidenceSource::backend_presented("iced", "test"),
        view.frame.clone(),
        &view.layout,
        &view.hit_regions,
        point,
        kind,
        Some(PointerButton::Primary),
        details,
        true,
    );
    let dispatch = dispatch.expect("point should resolve to a declared hit region");
    BackendInputEvent::declared(dispatch.input, evidence)
}

fn wheel_input(view: &ViewDefinition, point: Point, delta_y: f32) -> BackendInputEvent {
    let (dispatch, evidence) = resolve_declared_wheel_dispatch_with_evidence(
        EvidenceSource::backend_presented("iced", "test"),
        view.frame.clone(),
        &view.layout,
        &view.scroll_regions,
        point,
        0.0,
        delta_y,
    );
    let dispatch = dispatch.expect("point should resolve to a declared scroll region");
    BackendInputEvent::declared(dispatch.input, evidence)
}

fn text_edit_input(view: &ViewDefinition, text: &str) -> BackendInputEvent {
    let region = view
        .focus_regions
        .iter()
        .find(|region| region.target == ssot::input_id() && region.text_edit.is_some())
        .expect("input widget declares a text-edit focus region");
    let event = InputEvent::TextEdit(TextEditEvent {
        target: ssot::input_id(),
        target_slot: region.address.clone(),
        kind: TextEditKind::ReplaceBuffer,
        text: Some(text.to_string()),
        selection_before: None,
        selection_after: None,
    });
    let evidence = declared_focus_text_dispatch_evidence(
        EvidenceSource::backend_presented("iced", "test"),
        view.frame.clone(),
        &view.focus_regions,
        Some(region),
        DeclaredEventDispatchKind::Text,
        None,
        event.clone(),
    );
    BackendInputEvent::declared(event, evidence)
}

fn apply(runtime: &mut AppRuntime, input: BackendInputEvent) -> bool {
    let mut reducer = apply_messages;
    let report =
        runtime.apply_backend_input_event_for_backend_with_app_reducer(input, "iced", &mut reducer);
    assert!(
        report.diagnostics.is_empty(),
        "backend input apply produced diagnostics: {:?}",
        report.diagnostics
    );
    report.handled
}

// Paint inspection helpers.

fn collect_text_ops(ops: &[PaintOp], into: &mut Vec<(Rect, String)>) {
    for op in ops {
        match op {
            PaintOp::Text {
                bounds, content, ..
            } => into.push((*bounds, content.clone())),
            PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                collect_text_ops(ops, into);
            }
            _ => {}
        }
    }
}

fn group_clip_bounds(ops: &[PaintOp], group_id: &str) -> Option<Rect> {
    for op in ops {
        match op {
            PaintOp::Group { id, clip, ops } => {
                if id.as_deref() == Some(group_id) {
                    return clip.as_ref().map(|clip| clip.bounds);
                }
                if let Some(found) = group_clip_bounds(ops, group_id) {
                    return Some(found);
                }
            }
            PaintOp::Layer { ops, .. } => {
                if let Some(found) = group_clip_bounds(ops, group_id) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn fill_bounds_by_shape_id(ops: &[PaintOp], shape_id: &str) -> Option<Rect> {
    for op in ops {
        match op {
            PaintOp::Fill { shape, .. } if shape.id.as_deref() == Some(shape_id) => {
                return Some(shape.bounds);
            }
            PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                if let Some(found) = fill_bounds_by_shape_id(ops, shape_id) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn overlay_layer_by_id(
    ops: &[PaintOp],
    layer_id: &str,
) -> Option<(PaintInputTransparency, Option<PaintInputTransparency>)> {
    for op in ops {
        match op {
            PaintOp::Layer {
                id,
                input_transparency,
                wheel_transparency,
                ..
            } if id.as_deref() == Some(layer_id) => {
                return Some((*input_transparency, *wheel_transparency));
            }
            PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                if let Some(found) = overlay_layer_by_id(ops, layer_id) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn overlay_layer(
    ops: &[PaintOp],
) -> Option<(PaintInputTransparency, Option<PaintInputTransparency>)> {
    overlay_layer_by_id(ops, "authored.overlay:panel")
}

// ---------------------------------------------------------------------------
// 1. Pre-flight admission
// ---------------------------------------------------------------------------

#[test]
fn every_widget_view_passes_pre_flight_admission() {
    let external = ShowcaseState::default();

    let list = NoteListWidget;
    let list_view = widget_view(&list, &list.initial_local_state(), ssot::LIST_CARD_HEIGHT);
    let diagnostics =
        view_definition_contract_diagnostics_for_capabilities(&list_view, &list.capabilities());
    assert!(
        !view_definition_has_blocking_contract_diagnostic(&diagnostics),
        "list pre-flight: {diagnostics:?}"
    );

    let input = DraftInputWidget;
    let input_view = widget_view(
        &input,
        &input.initial_local_state(),
        ssot::INPUT_CARD_HEIGHT,
    );
    let diagnostics =
        view_definition_contract_diagnostics_for_capabilities(&input_view, &input.capabilities());
    assert!(
        !view_definition_has_blocking_contract_diagnostic(&diagnostics),
        "input pre-flight: {diagnostics:?}"
    );

    let overlay = OverlayWidget;
    let overlay_view = widget_view(
        &overlay,
        &overlay.initial_local_state(),
        ssot::OVERLAY_CARD_HEIGHT,
    );
    let diagnostics = view_definition_contract_diagnostics_for_capabilities(
        &overlay_view,
        &overlay.capabilities(),
    );
    assert!(
        !view_definition_has_blocking_contract_diagnostic(&diagnostics),
        "overlay pre-flight: {diagnostics:?}"
    );

    let nested = NestedFeedWidget;
    // Every reachable scroll position must admit, not just the initial
    // one: declarations are per-frame snapshots.
    for outer_rows in 0..=ssot::NESTED_OUTER_MAX_SCROLL_ROWS {
        for inner in 0..=ssot::NESTED_INNER_MAX_SCROLL_ROWS {
            let local = crate::view::NestedLocal {
                outer_rows,
                inner_rows: [inner, ssot::NESTED_INNER_MAX_SCROLL_ROWS - inner],
            };
            let nested_view = widget_view(&nested, &local, ssot::NESTED_CARD_HEIGHT);
            let diagnostics = view_definition_contract_diagnostics_for_capabilities(
                &nested_view,
                &nested.capabilities(),
            );
            assert!(
                !view_definition_has_blocking_contract_diagnostic(&diagnostics),
                "nested pre-flight (outer {outer_rows}, inner {inner}): {diagnostics:?}"
            );
        }
    }

    let _ = external;
}

#[test]
fn scroll_producers_declare_their_outermost_terminal_owner() {
    let list = widget_view(
        &NoteListWidget,
        &NoteListWidget.initial_local_state(),
        ssot::LIST_CARD_HEIGHT,
    );
    assert_eq!(list.wheel_traversal_boundary.terminal_region_index, Some(0));
    assert_eq!(list.scroll_regions[0].id, ssot::list_scroll_region_id());

    let overlay = widget_view(
        &OverlayWidget,
        &OverlayWidget.initial_local_state(),
        ssot::OVERLAY_CARD_HEIGHT,
    );
    assert_eq!(
        overlay.wheel_traversal_boundary.terminal_region_index,
        Some(0)
    );
    assert_eq!(overlay.scroll_regions[0].id, ssot::overlay_feed_region_id());

    let nested = widget_view(
        &NestedFeedWidget,
        &NestedFeedWidget.initial_local_state(),
        ssot::NESTED_CARD_HEIGHT,
    );
    assert_eq!(
        nested.wheel_traversal_boundary.terminal_region_index,
        Some(0)
    );
    assert_eq!(nested.scroll_regions[0].id, ssot::nested_outer_region_id());
    assert!(
        nested.scroll_regions[1..]
            .iter()
            .all(|region| region.id != ssot::nested_outer_region_id())
    );

    let input = widget_view(
        &DraftInputWidget,
        &DraftInputWidget.initial_local_state(),
        ssot::INPUT_CARD_HEIGHT,
    );
    assert_eq!(input.wheel_traversal_boundary.terminal_region_index, None);
}

#[test]
fn composed_app_view_passes_pre_flight_admission() {
    let runtime = app_runtime();
    let view = composed_view(&runtime);
    let diagnostics = view_definition_contract_diagnostics_for_capabilities(
        &view,
        &runtime.widget().capabilities(),
    );
    assert!(
        !view_definition_has_blocking_contract_diagnostic(&diagnostics),
        "composed pre-flight: {diagnostics:?}"
    );
    // The composed view must keep all four widgets separately addressable.
    for id in [
        ssot::list_id(),
        ssot::input_id(),
        ssot::overlay_id(),
        ssot::nested_id(),
    ] {
        assert!(
            view.hit_regions
                .iter()
                .map(|region| &region.target)
                .chain(view.focus_regions.iter().map(|region| &region.target))
                .chain(view.scroll_regions.iter().map(|region| &region.target))
                .any(|target| *target == id),
            "widget {id:?} declares no region in the composed view"
        );
    }
}

/// Step-212 A: the PAGE region is declared exactly when the card column
/// exceeds the window, stays Auto (so the page scrollbar appears exactly
/// then), and pre-flight stays no-blocking at BOTH window sizes — the
/// shrunken-window case is admissible.
#[test]
fn page_region_appears_exactly_when_the_column_exceeds_the_window() {
    // Tall window: column (614) fits in 640 -> no page region, and the
    // root bounds stretch to the window (the min-height pattern).
    let runtime = app_runtime();
    let view = composed_view(&runtime);
    assert_eq!(view.layout.bounds().as_rect().size.height, FRAME_HEIGHT);
    assert!(
        !view
            .scroll_regions
            .iter()
            .any(|region| region.id == ssot::page_scroll_region_id()),
        "no page region when the column fits the window"
    );

    // Short window: the column overflows -> the page region is declared
    // with the full column as content, Auto indicator, back-most order,
    // and the composed view still admits.
    let runtime = app_runtime_with_viewport(600.0, 400.0);
    let view = composed_view(&runtime);
    let page = view
        .scroll_regions
        .iter()
        .find(|region| region.id == ssot::page_scroll_region_id())
        .expect("column taller than the window declares the page region");
    assert_eq!(page.target, ssot::app_id());
    assert_eq!(page.viewport.into_rect().size.height, 400.0);
    assert_eq!(
        page.content_bounds.into_rect().size.height,
        ssot::APP_ROOT_HEIGHT
    );
    assert_eq!(page.indicator, ScrollIndicatorMode::Auto);
    assert!(
        page.order.z_index < 0,
        "the page region must sit behind every widget region"
    );
    let diagnostics = view_definition_contract_diagnostics_for_capabilities(
        &view,
        &runtime.widget().capabilities(),
    );
    assert!(
        !view_definition_has_blocking_contract_diagnostic(&diagnostics),
        "small-window composed pre-flight: {diagnostics:?}"
    );
}

/// Phase 6 item 2 (NC-13 inducement): at the shrunken window the composed
/// app paints a column taller than the window — the app-level PAGE region
/// is exactly what keeps the `content_overflow_without_scroll_region`
/// advisory silent, and stripping that one region re-draws it. This pins
/// the closed loop the roadmap landed: advisory -> routing-and-scroll.md ->
/// this crate's page-scroll pattern.
#[test]
fn page_region_suppresses_the_content_overflow_advisory() {
    let runtime = app_runtime_with_viewport(600.0, 400.0);
    let view = composed_view(&runtime);
    let diagnostics = view_definition_contract_diagnostics_for_capabilities(
        &view,
        &runtime.widget().capabilities(),
    );
    assert!(
        !diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "view_contract.content_overflow_without_scroll_region"
        }),
        "the page region covers the column overflow: {diagnostics:?}"
    );

    // Counterfactual: strip the page region AND the composed overflow
    // allowance. The allowance must go too because the ROAM overlay child's
    // declared overflow bounds compose into the app view as an allowance
    // containing the whole root layout, and a declared allowance is the
    // Step-210 pattern the advisory deliberately stays silent for — a
    // recorded masking limitation at composed level (Step 217); the
    // consumer-app shape (no overflow declaration) fires directly.
    let mut stripped = view;
    stripped
        .scroll_regions
        .retain(|region| region.id != ssot::page_scroll_region_id());
    stripped.paint_order.allow_overflow_paint = false;
    stripped.paint_order.overflow_bounds = None;
    let diagnostics = view_definition_contract_diagnostics_for_capabilities(
        &stripped,
        &runtime.widget().capabilities(),
    );
    assert!(
        diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "view_contract.content_overflow_without_scroll_region"
        }),
        "without the page region the overflow is uncovered and must draw \
         the advisory: {diagnostics:?}"
    );
}

/// Step-212 B: the card column FILLS the window — declared regions track a
/// changed viewport width (the responsive-width invariant; the fixed-width
/// alternative is the documented clamp in layout_plan).
#[test]
fn responsive_width_tracks_a_changed_viewport_width() {
    for frame_width in [600.0_f32, 900.0] {
        let runtime = app_runtime_with_viewport(frame_width, FRAME_HEIGHT);
        let view = composed_view(&runtime);
        let card_width = frame_width - 2.0 * ssot::CARD_MARGIN_X;
        for placement in view.layout.child_placements() {
            assert_eq!(
                placement.bounds.into_rect().size.width,
                card_width,
                "card {:?} must fill the window minus the margins",
                placement.child
            );
        }
        // The declared regions follow the live width: the list's scroll
        // viewport spans the card, and the input's text-edit field spans
        // the card minus its inset.
        let list = view
            .scroll_regions
            .iter()
            .find(|region| region.id == ssot::list_scroll_region_id())
            .expect("list scroll region");
        assert_eq!(list.viewport.into_rect().size.width, card_width);
        let field = view
            .focus_regions
            .iter()
            .find(|region| region.target == ssot::input_id())
            .expect("input focus region");
        assert_eq!(
            field.bounds.into_rect().size.width,
            card_width - 2.0 * ssot::INPUT_INSET_X
        );
    }
}

/// Step-212 A behavior: a wheel over DEAD SPACE (the margin, outside every
/// card) scrolls the page, and a card region at its limit CHAINS to the
/// page (the outermost containing candidate).
#[test]
fn wheel_at_dead_space_scrolls_the_page_and_card_limits_chain_to_it() {
    let mut runtime = app_runtime_with_viewport(600.0, 400.0);

    // Dead space: inside the window, left of every card (cards start at
    // x = CARD_MARGIN_X).
    let dead_point = Point { x: 8.0, y: 300.0 };
    let view = composed_view(&runtime);
    let input = wheel_input(&view, dead_point, -1.0);
    let InputEvent::Wheel(wheel) = &input.event else {
        panic!("expected wheel input");
    };
    assert_eq!(
        wheel.region_id.as_ref(),
        Some(&ssot::page_scroll_region_id()),
        "dead space must resolve to the page region"
    );
    assert!(apply(&mut runtime, input));
    assert_eq!(
        runtime.local_state().app.page_scroll_y,
        ssot::PAGE_SCROLL_STEP
    );
    // The next frame's declaration carries the new offset.
    let view = composed_view(&runtime);
    let page = view
        .scroll_regions
        .iter()
        .find(|region| region.id == ssot::page_scroll_region_id())
        .expect("page region");
    assert_eq!(page.offset.y, ssot::PAGE_SCROLL_STEP);

    // Reset, then scroll the LIST to its limit; the next wheel over the
    // list chains out to the page region.
    let mut runtime = app_runtime_with_viewport(600.0, 400.0);
    let list_point = Point {
        x: 300.0,
        y: LIST_CARD_TOP + ssot::LIST_ROWS_TOP + 40.0,
    };
    for _ in 0..ssot::LIST_MAX_SCROLL_ROWS {
        let view = composed_view(&runtime);
        let input = wheel_input(&view, list_point, -1.0);
        assert!(apply(&mut runtime, input));
    }
    assert_eq!(
        runtime.local_state().widgets.0.scroll_rows,
        ssot::LIST_MAX_SCROLL_ROWS
    );
    let view = composed_view(&runtime);
    let input = wheel_input(&view, list_point, -1.0);
    let InputEvent::Wheel(wheel) = &input.event else {
        panic!("expected wheel input");
    };
    assert_eq!(
        wheel.region_id.as_ref(),
        Some(&ssot::page_scroll_region_id()),
        "the at-limit list wheel must chain to the page region"
    );
    assert!(apply(&mut runtime, input));
    assert_eq!(
        runtime.local_state().app.page_scroll_y,
        ssot::PAGE_SCROLL_STEP
    );

    // The page offset clamps to the exact declared travel (614 - 400): at
    // the limit the page region drops out of the candidate pool (no
    // consumer is left for a down-wheel over dead space) instead of
    // black-holing the wheel with an unmovable offset.
    let travel = ssot::page_max_scroll_y(Size {
        width: 600.0,
        height: 400.0,
    });
    for _ in 0..16 {
        let view = composed_view(&runtime);
        let (dispatch, _evidence) = resolve_declared_wheel_dispatch_with_evidence(
            EvidenceSource::backend_presented("iced", "test"),
            view.frame.clone(),
            &view.layout,
            &view.scroll_regions,
            dead_point,
            0.0,
            -1.0,
        );
        if dispatch.is_none() {
            break;
        }
        let input = wheel_input(&view, dead_point, -1.0);
        assert!(apply(&mut runtime, input));
    }
    assert_eq!(
        runtime.local_state().app.page_scroll_y,
        travel,
        "page offset must clamp to the exact declared travel"
    );
    let view = composed_view(&runtime);
    let (dispatch, _evidence) = resolve_declared_wheel_dispatch_with_evidence(
        EvidenceSource::backend_presented("iced", "test"),
        view.frame.clone(),
        &view.layout,
        &view.scroll_regions,
        dead_point,
        0.0,
        -1.0,
    );
    assert!(
        dispatch.is_none(),
        "at the page limit the down-wheel over dead space has no consumer"
    );
}

// ---------------------------------------------------------------------------
// 2. Geometry invariants (the shared-constants discipline)
// ---------------------------------------------------------------------------

#[test]
fn list_pointer_math_inverts_row_paint_math() {
    for rows in 0..=ssot::LIST_MAX_SCROLL_ROWS {
        let offset = ssot::list_offset_y(rows);
        for index in 0..ssot::LIST_NOTE_COUNT {
            let top = ssot::list_row_top_in_card(index, offset);
            if top < ssot::LIST_ROWS_TOP
                || top + ssot::LIST_ROW_HEIGHT > ssot::LIST_ROWS_TOP + ssot::LIST_VISIBLE_HEIGHT
            {
                continue;
            }
            let center = top + ssot::LIST_ROW_HEIGHT / 2.0;
            assert_eq!(
                ssot::list_row_at_card_y(center, offset),
                Some(index),
                "row {index} at scroll {rows}: pointer math must invert paint math"
            );
        }
    }
}

#[test]
fn list_hit_regions_match_painted_rows() {
    let list = NoteListWidget;
    let local = crate::view::ListLocal {
        scroll_rows: 2,
        focused: false,
    };
    let view = widget_view(&list, &local, ssot::LIST_CARD_HEIGHT);

    let mut texts = Vec::new();
    collect_text_ops(&view.paint, &mut texts);
    for region in view
        .hit_regions
        .iter()
        .filter(|region| region.id.as_str().contains(":row-"))
    {
        let bounds = region.bounds.into_rect();
        assert!(
            texts
                .iter()
                .any(|(rect, _)| (rect.origin.y - bounds.origin.y).abs() < 0.5
                    && (rect.origin.x - bounds.origin.x).abs() < 0.5),
            "hit region {} has no painted row at the same origin",
            region.id.as_str()
        );
    }
    // Scrolled by 2 rows: rows 2..6 are declared, rows 0/1 and 6/7 are not.
    assert_eq!(
        view.hit_regions
            .iter()
            .filter(|region| region.id.as_str().contains(":row-"))
            .count(),
        4
    );
    // Paint clip equals the declared scroll viewport.
    let clip = group_clip_bounds(&view.paint, "list-rows").expect("list rows are clipped");
    let scroll = view
        .scroll_regions
        .first()
        .expect("list declares a scroll region");
    assert_eq!(clip, scroll.viewport.into_rect());
}

#[test]
fn overlay_drag_region_tracks_painted_titlebar() {
    let overlay = OverlayWidget;
    let local = crate::view::OverlayLocal {
        offset: Point { x: 60.0, y: 60.0 },
        dragging: false,
        drag_anchor: Point { x: 0.0, y: 0.0 },
        roam_offset: ssot::overlay_roam_default_offset(),
        roam_dragging: false,
        roam_anchor: Point { x: 0.0, y: 0.0 },
        feed_rows: 3,
    };
    let view = widget_view(&overlay, &local, ssot::OVERLAY_CARD_HEIGHT);

    let titlebar =
        fill_bounds_by_shape_id(&view.paint, "overlay-titlebar").expect("titlebar painted");
    let drag = view
        .hit_regions
        .iter()
        .find(|region| region.id == ssot::overlay_drag_region_id())
        .expect("drag hit region declared");
    assert_eq!(drag.bounds.into_rect(), titlebar);
    assert_eq!(drag.capture, PointerCaptureIntent::DuringDrag);
    assert_eq!(drag.order.z_index, ssot::OVERLAY_Z_INDEX);

    // The overlay layer is pointer-opaque and wheel-transparent.
    let (pointer, wheel) = overlay_layer(&view.paint).expect("overlay layer painted");
    assert_eq!(pointer, PaintInputTransparency::Opaque);
    assert_eq!(wheel, Some(PaintInputTransparency::PassThrough));

    // The ROAMING panel keeps the same must-agree discipline: painted
    // titlebar == declared drag region, distinct fronting z, and the same
    // pointer-opaque/wheel-transparent channels.
    let roam_titlebar = fill_bounds_by_shape_id(&view.paint, "overlay-roam-titlebar")
        .expect("roaming titlebar painted");
    let roam_drag = view
        .hit_regions
        .iter()
        .find(|region| region.id == ssot::overlay_roam_drag_region_id())
        .expect("roaming drag hit region declared");
    assert_eq!(roam_drag.bounds.into_rect(), roam_titlebar);
    assert_eq!(roam_drag.capture, PointerCaptureIntent::DuringDrag);
    assert_eq!(roam_drag.order.z_index, ssot::OVERLAY_ROAM_Z_INDEX);
    assert_ne!(
        roam_drag.order, drag.order,
        "the two overlay drag regions must carry distinct orders"
    );
    let (roam_pointer, roam_wheel) =
        overlay_layer_by_id(&view.paint, "authored.overlay:roam").expect("roaming layer painted");
    assert_eq!(roam_pointer, PaintInputTransparency::Opaque);
    assert_eq!(roam_wheel, Some(PaintInputTransparency::PassThrough));
}

fn text_op_by_content(ops: &[PaintOp], wanted: &str) -> Option<(Rect, TextStyle)> {
    for op in ops {
        match op {
            PaintOp::Text {
                bounds,
                content,
                style,
                ..
            } if content == wanted => {
                return Some((*bounds, style.clone()));
            }
            PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                if let Some(found) = text_op_by_content(ops, wanted) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

// Declaration-level pin for the declared-text-alignment pattern (NC-14,
// the ScrollIndicatorMode test convention): the roaming titlebar label
// declares the FULL titlebar rect as its bounds and `.centered()` for the
// style — the backends anchor it, no hand-computed insets. Alignment is
// presentation intent (no `view_contract.*` code guards it), so this pin
// is the drift guard.
#[test]
fn overlay_roam_title_declares_centered_alignment() {
    let overlay = OverlayWidget;
    let local = crate::view::OverlayLocal {
        offset: ssot::overlay_default_offset(),
        dragging: false,
        drag_anchor: Point { x: 0.0, y: 0.0 },
        roam_offset: ssot::overlay_roam_default_offset(),
        roam_dragging: false,
        roam_anchor: Point { x: 0.0, y: 0.0 },
        feed_rows: 0,
    };
    let view = widget_view(&overlay, &local, ssot::OVERLAY_CARD_HEIGHT);

    let (bounds, style) = text_op_by_content(&view.paint, "roam me").expect("roam title painted");
    let roam_titlebar = fill_bounds_by_shape_id(&view.paint, "overlay-roam-titlebar")
        .expect("roaming titlebar painted");
    assert_eq!(
        bounds, roam_titlebar,
        "the label must declare the FULL titlebar rect, not an inset guess"
    );
    assert_eq!(style.align_x, TextAlignX::Center);
    assert_eq!(style.align_y, TextAlignY::Center);

    // The clamped panel's title keeps the unspecified default — the
    // historical top-left anchoring (the contrast case).
    let (_, drag_style) = text_op_by_content(&view.paint, "drag me").expect("drag title painted");
    assert_eq!(drag_style.align_x, TextAlignX::Start);
    assert_eq!(drag_style.align_y, TextAlignY::Top);
}

// Declaration-level pin for the per-op wrap opt-out pattern (NC-4, same
// convention as the alignment pin above): the input card's CJK label
// declares a rect NARROWER than its laid-out text and `TextWrap::None`
// for the style — one line, clipped at the rect, on every presenter.
// Wrap is presentation intent (no `view_contract.*` code guards it), so
// this pin is the drift guard.
#[test]
fn input_nowrap_label_declares_single_line_contract() {
    let input = DraftInputWidget;
    let view = widget_view(
        &input,
        &input.initial_local_state(),
        ssot::INPUT_CARD_HEIGHT,
    );

    let (bounds, style) =
        text_op_by_content(&view.paint, ssot::INPUT_NOWRAP_LABEL).expect("no-wrap label painted");
    assert_eq!(style.wrap, TextWrap::None);
    assert_eq!(style, ssot::nowrap_label_text());
    assert_eq!(bounds, ssot::input_nowrap_label_rect(560.0));
    // The default-contrast case: every other input-card text op keeps the
    // unspecified word wrap.
    let (_, header_style) =
        text_op_by_content(&view.paint, "Draft (click, then type)").expect("header painted");
    assert_eq!(header_style.wrap, TextWrap::Word);
}

// The measurement channel end-to-end at the runtime seam (NC-4, slice
// (iii)): `SlipwayRuntime::project_text_metrics` runs the app hook with
// the given provider; a VALID receipt becomes `ShowcaseState::window_badge`
// and the input card sizes the badge rect to the measured label (fill,
// outline, and centered text over the SAME rect — no character-width
// ratios anywhere). Refused receipts land as honest absence: no badge
// ops painted. A fake provider stands in for the backend here; the
// per-backend providers are pinned in their own suites
// (`iced_text_metric_provider_measures_real_layout`,
// `egui_text_metric_provider_measures_real_galley`).
#[test]
fn window_badge_is_sized_by_the_projected_measurement() {
    struct FixtureProvider {
        valid: bool,
    }

    impl SlipwayTextMetricProvider for FixtureProvider {
        fn text_metric_source(&self) -> TextMetricSource {
            TextMetricSource {
                provider_id: "authored-test-provider".to_string(),
                backend_id: Some("authored-test-backend".to_string()),
                api_name: "fixture_measure".to_string(),
                kind: TextMetricSourceKind::OfficialBackendApi,
            }
        }

        fn measure_text(&mut self, request: TextMeasurementRequest) -> TextMeasurementReceipt {
            // The hook must measure with the paint op's own style token.
            assert_eq!(request.style, ssot::badge_text());
            assert_eq!(request.available_bounds, None);
            if self.valid {
                TextMeasurementReceipt::Valid(ValidTextMeasurement {
                    source: self.text_metric_source(),
                    facts: TextMeasurementFacts {
                        measured_size: Size {
                            width: 43.0,
                            height: 15.0,
                        },
                        content_bounds: Rect {
                            origin: Point { x: 0.0, y: 0.0 },
                            size: Size {
                                width: 43.0,
                                height: 15.0,
                            },
                        },
                        baseline: None,
                        line_count: Some(1),
                        caret_bounds: TextCaretGeometry::unavailable(
                            "text flow policy does not claim caret bounds",
                        ),
                    },
                    request,
                })
            } else {
                TextMeasurementReceipt::Unsupported {
                    request,
                    diagnostics: Vec::new(),
                }
            }
        }
    }

    let mut runtime = app_runtime();
    assert_eq!(
        runtime.external().window_badge,
        None,
        "no measurement projected yet -> no badge state"
    );

    runtime.project_text_metrics(&mut FixtureProvider { valid: true });
    let badge = runtime
        .external()
        .window_badge
        .clone()
        .expect("a valid receipt becomes badge state");
    assert_eq!(
        badge.text,
        ssot::window_badge_label(Size {
            width: FRAME_WIDTH,
            height: FRAME_HEIGHT,
        })
    );
    assert_eq!(
        badge.size,
        Size {
            width: 43.0,
            height: 15.0,
        }
    );

    // Paint derives the badge rect from the MEASURED size.
    let input = DraftInputWidget;
    let view = widget_view_with_state(
        &input,
        runtime.external(),
        &input.initial_local_state(),
        ssot::INPUT_CARD_HEIGHT,
    );
    let expected_rect = ssot::input_badge_rect(
        560.0,
        Size {
            width: 43.0,
            height: 15.0,
        },
    );
    assert_eq!(
        expected_rect.size.width,
        43.0_f32.ceil() + 2.0 * ssot::INPUT_BADGE_PAD_X,
        "the badge hugs the measured label plus the declared padding"
    );
    let badge_fill =
        fill_bounds_by_shape_id(&view.paint, "input-badge").expect("badge fill painted");
    assert_eq!(badge_fill, expected_rect);
    let (label_bounds, label_style) =
        text_op_by_content(&view.paint, &badge.text).expect("badge label painted");
    assert_eq!(
        label_bounds, expected_rect,
        "the label declares the FULL badge rect and lets `.centered()` anchor it"
    );
    assert_eq!(label_style, ssot::badge_text());

    // Honest absence: a refused measurement clears the state and the
    // badge ops disappear rather than painting a fabricated size.
    runtime.project_text_metrics(&mut FixtureProvider { valid: false });
    assert_eq!(runtime.external().window_badge, None);
    let view = widget_view_with_state(
        &input,
        runtime.external(),
        &input.initial_local_state(),
        ssot::INPUT_CARD_HEIGHT,
    );
    assert!(fill_bounds_by_shape_id(&view.paint, "input-badge").is_none());
}

// The two drag clamps, contrasted (the copy-source decision surface):
// clamped keeps the panel inside the feed band; roaming may leave the band
// AND the card, but never the declared overflow allowance.
#[test]
fn clamped_offset_stays_in_band_while_roaming_offset_leaves_it() {
    let width = 560.0;
    let viewport = Size {
        width: FRAME_WIDTH,
        height: FRAME_HEIGHT,
    };
    let far = Point {
        x: -500.0,
        y: -500.0,
    };
    let clamped = ssot::overlay_clamped_offset(width, far);
    assert_eq!(
        clamped.y,
        ssot::OVERLAY_FEED_TOP,
        "clamped stops at the band top"
    );
    assert_eq!(clamped.x, 0.0);

    let roamed = ssot::overlay_roaming_clamped_offset(width, viewport, far);
    assert_eq!(
        roamed,
        Point {
            x: -ssot::CARD_MARGIN_X,
            y: -ssot::OVERLAY_CARD_ROOT_TOP,
        },
        "roaming stops at the overflow allowance edge — the window's top-left corner, outside the band and the card"
    );
    // Every roaming-reachable panel rect stays inside the declared
    // overflow bounds (the clamp/declaration must-agree contract).
    let allowance = ssot::overlay_overflow_bounds(width, viewport);
    for desired in [
        far,
        Point { x: 900.0, y: 900.0 },
        Point { x: 96.0, y: 36.0 },
    ] {
        let offset = ssot::overlay_roaming_clamped_offset(width, viewport, desired);
        let panel = ssot::overlay_roam_panel_rect(offset);
        assert!(
            ssot::intersect_rect(panel, allowance)
                .is_some_and(|intersection| intersection == panel),
            "panel {panel:?} must stay inside allowance {allowance:?}"
        );
    }
}

// Step-212 roaming allowance: the declared overflow bounds ARE the LIVE
// window (unioned with the card column), translated into the overlay
// card's local space — so the panel roams the whole real window at any
// window size, and the clamp (which keeps the FULL panel inside the same
// rect) can never strand the titlebar off-view. MUST-agree pins: the
// allowance == the declared overflow bounds at TWO window sizes, the
// window size flows through the PROJECTED `ShowcaseState::viewport`, and
// every card placement stays roamable.
#[test]
fn roaming_clamp_equals_declared_allowance_at_two_window_sizes() {
    for (frame_width, frame_height) in [(FRAME_WIDTH, FRAME_HEIGHT), (900.0, 900.0)] {
        let mut runtime = SlipwayRuntime::from_app(ShowcaseApp::new(), ShowcaseState::default());
        // The projection hook mirrors the recorded viewport into external
        // state — the value the allowance, the drag clamp, and the paint
        // all read.
        runtime.record_presented_viewport(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: frame_width,
                height: frame_height,
            },
        });
        assert_eq!(
            runtime.external().viewport,
            Size {
                width: frame_width,
                height: frame_height,
            },
            "record_presented_viewport must project the live window into ShowcaseState"
        );
        let view = composed_view(&runtime);
        let root_bounds = view.layout.bounds().into_rect();

        // The allowance, mapped from overlay-card-local space to root
        // space, covers the whole window (and the whole column when the
        // window is shorter).
        let card_width = root_bounds.size.width - 2.0 * ssot::CARD_MARGIN_X;
        let viewport = runtime.external().viewport;
        let allowance = ssot::overlay_overflow_bounds(card_width, viewport);
        let allowance_in_root = Rect {
            origin: Point {
                x: allowance.origin.x + ssot::CARD_MARGIN_X,
                y: allowance.origin.y + ssot::OVERLAY_CARD_ROOT_TOP,
            },
            size: allowance.size,
        };
        assert_eq!(
            allowance_in_root.size.width, frame_width,
            "the roaming allowance must span the whole window width"
        );
        assert_eq!(
            allowance_in_root.size.height,
            frame_height.max(ssot::APP_ROOT_HEIGHT),
            "the roaming allowance must span the whole window height (column floor)"
        );

        // MUST-agree: the overlay view DECLARES exactly this allowance as
        // its overflow bounds (clamp == declaration).
        let overlay = OverlayWidget;
        let external = ShowcaseState {
            viewport,
            ..ShowcaseState::default()
        };
        let overlay_local = crate::view::OverlayLocal {
            offset: ssot::overlay_default_offset(),
            dragging: false,
            drag_anchor: Point { x: 0.0, y: 0.0 },
            roam_offset: ssot::overlay_roam_default_offset(),
            roam_dragging: false,
            roam_anchor: Point { x: 0.0, y: 0.0 },
            feed_rows: 0,
        };
        let overlay_view = overlay.view_definition(
            &external,
            &overlay_local,
            ViewDefinitionInput::new(
                runtime.last_frame_identity(),
                widget_input(card_width, ssot::OVERLAY_CARD_HEIGHT),
            ),
        );
        assert_eq!(
            overlay_view
                .paint_order
                .overflow_bounds
                .map(|bounds| bounds.into_rect()),
            Some(allowance),
            "declared overflow bounds must equal the roaming clamp's allowance"
        );

        // Every card placement is inside the allowance: the panel may
        // roam over each of them.
        for placement in view.layout.child_placements() {
            let bounds = placement.bounds.into_rect();
            assert!(
                ssot::intersect_rect(bounds, allowance_in_root)
                    .is_some_and(|intersection| intersection == bounds),
                "card {:?} must lie inside the roaming allowance",
                placement.child
            );
        }

        // Titlebar reachability: at every clamp extreme the FULL panel
        // (and so its titlebar) stays inside the allowance.
        for desired in [
            Point {
                x: -9000.0,
                y: -9000.0,
            },
            Point {
                x: 9000.0,
                y: 9000.0,
            },
            Point {
                x: -9000.0,
                y: 9000.0,
            },
            Point {
                x: 9000.0,
                y: -9000.0,
            },
        ] {
            let offset = ssot::overlay_roaming_clamped_offset(card_width, viewport, desired);
            let panel = ssot::overlay_roam_panel_rect(offset);
            assert!(
                ssot::intersect_rect(panel, allowance)
                    .is_some_and(|intersection| intersection == panel),
                "clamped panel {panel:?} must stay fully inside the allowance {allowance:?}"
            );
        }
    }
}

// Step-210 declared indicator states: one inner Visible, one inner Hidden,
// outer and list stay Auto. Pins the DECLARATIONS the backends honor
// (per-backend honor is pinned by backend-crate tests; reverting either
// backend's Hidden honor also fails those).
#[test]
fn nested_inner_indicator_modes_are_declared() {
    let runtime = app_runtime();
    let view = composed_view(&runtime);
    let indicator_for = |id: &PresentationRegionId| {
        view.scroll_regions
            .iter()
            .find(|region| region.id == *id)
            .unwrap_or_else(|| panic!("region {id:?} declared"))
            .indicator
    };
    assert_eq!(
        indicator_for(&ssot::nested_inner_region_id(0)),
        ScrollIndicatorMode::Visible
    );
    assert_eq!(
        indicator_for(&ssot::nested_inner_region_id(1)),
        ScrollIndicatorMode::Hidden
    );
    assert_eq!(
        indicator_for(&ssot::nested_outer_region_id()),
        ScrollIndicatorMode::Auto
    );
    assert_eq!(
        indicator_for(&ssot::list_scroll_region_id()),
        ScrollIndicatorMode::Auto
    );
}

// The overflow declaration is LOAD-BEARING: with the roaming panel dragged
// past the card's layout bounds, pre-flight admission is clean — and the
// revert simulation (same view, overflow declaration dropped) refuses with
// the exact code the pattern comment names.
#[test]
fn roaming_overlay_outside_layout_admits_only_with_the_overflow_declaration() {
    let overlay = OverlayWidget;
    let mut local = overlay.initial_local_state();
    // Fully out of the band and past the card's top-left layout corner
    // (negative origin), inside the declared allowance — the root view's
    // top-left corner, i.e. over the list card's area.
    local.roam_offset = Point {
        x: -ssot::CARD_MARGIN_X,
        y: -ssot::OVERLAY_CARD_ROOT_TOP,
    };
    let view = widget_view(&overlay, &local, ssot::OVERLAY_CARD_HEIGHT);
    let diagnostics =
        view_definition_contract_diagnostics_for_capabilities(&view, &overlay.capabilities());
    assert!(
        !view_definition_has_blocking_contract_diagnostic(&diagnostics),
        "roaming outside layout must admit under the overflow declaration: {diagnostics:?}"
    );

    // REVERT-AND-FAIL: drop the overflow declaration and the same frame
    // refuses.
    let mut reverted = view;
    reverted.paint_order = PaintOrderDeclaration::source_order(ssot::overlay_id());
    let diagnostics =
        view_definition_contract_diagnostics_for_capabilities(&reverted, &overlay.capabilities());
    assert!(
        diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "view_contract.hit_bounds_outside_layout"),
        "without overflow bounds the out-of-layout titlebar must refuse: {diagnostics:?}"
    );
}

#[test]
fn nested_regions_honor_the_anchoring_cap_and_match_paint_clips() {
    // Outer travel = content - viewport, quantized to whole rows, and
    // strictly under half a panel pitch (the anchoring cap in ssot.rs).
    let travel = ssot::NESTED_OUTER_CONTENT_HEIGHT - ssot::NESTED_OUTER_HEIGHT;
    assert_eq!(
        travel,
        ssot::NESTED_OUTER_MAX_SCROLL_ROWS as f32 * ssot::NESTED_OUTER_ROW_STEP
    );
    assert!(
        travel < ssot::NESTED_PANEL_HEIGHT / 2.0,
        "outer travel {travel} must stay under half a panel height"
    );

    let nested = NestedFeedWidget;
    for outer_rows in 0..=ssot::NESTED_OUTER_MAX_SCROLL_ROWS {
        let local = crate::view::NestedLocal {
            outer_rows,
            inner_rows: [1, 2],
        };
        let view = widget_view(&nested, &local, ssot::NESTED_CARD_HEIGHT);
        // Declared inner viewports equal the painted inner clips.
        for panel in 0..ssot::NESTED_PANEL_COUNT {
            let region = view
                .scroll_regions
                .iter()
                .find(|region| region.id == ssot::nested_inner_region_id(panel));
            let clip = group_clip_bounds(&view.paint, &format!("nested-inner-{panel}"));
            match (region, clip) {
                (Some(region), Some(clip)) => {
                    assert_eq!(
                        region.viewport.into_rect(),
                        clip,
                        "panel {panel} at outer {outer_rows}: declared viewport != paint clip"
                    );
                }
                (None, None) => {}
                (region, clip) => panic!(
                    "panel {panel} at outer {outer_rows}: region {:?} vs clip {:?} — \
                     a region without matching clipped paint (or vice versa)",
                    region.map(|region| region.id.as_str().to_string()),
                    clip
                ),
            }
        }
        // Overlapping wheel-consuming regions must carry distinct orders
        // (the ambiguous_wheel_overlap defense).
        for (left_index, left) in view.scroll_regions.iter().enumerate() {
            for right in view.scroll_regions.iter().skip(left_index + 1) {
                if ssot::intersect_rect(left.viewport.into_rect(), right.viewport.into_rect())
                    .is_some()
                {
                    assert_ne!(
                        left.order,
                        right.order,
                        "overlapping regions {} and {} share an order",
                        left.id.as_str(),
                        right.id.as_str()
                    );
                }
            }
        }
    }
}

// Fix-2 geometry invariants: the selectable inner panel's pointer inverse
// and per-row hit regions track BOTH scroll offsets (the scrolled-hit-
// region discipline).

#[test]
fn nested_row_pointer_math_inverts_row_paint_math_under_both_offsets() {
    let width = 560.0;
    for outer_rows in 0..=ssot::NESTED_OUTER_MAX_SCROLL_ROWS {
        let outer_offset = ssot::nested_outer_offset_y(outer_rows);
        let Some(visible) =
            ssot::nested_visible_field_rect(ssot::NESTED_SELECTABLE_PANEL, outer_offset, width)
        else {
            continue;
        };
        for inner_rows in 0..=ssot::NESTED_INNER_MAX_SCROLL_ROWS {
            let inner_offset = ssot::nested_inner_offset_y(inner_rows, visible.size.height);
            for row in 0..ssot::NESTED_INNER_ROW_COUNT {
                let rect =
                    ssot::nested_inner_row_rect_in_card(row, outer_offset, inner_offset, width);
                if rect.origin.y < visible.origin.y
                    || rect.origin.y + rect.size.height > visible.origin.y + visible.size.height
                {
                    continue;
                }
                let center = rect.origin.y + rect.size.height / 2.0;
                assert_eq!(
                    ssot::nested_inner_row_at_card_y(center, outer_offset, inner_offset, width),
                    Some(row),
                    "row {row} at outer {outer_rows}/inner {inner_rows}: pointer math must invert paint math"
                );
            }
        }
    }
}

#[test]
fn nested_row_hit_regions_track_both_scroll_offsets_and_match_painted_rows() {
    let nested = NestedFeedWidget;
    for (outer_rows, inner) in [
        (0, 0),
        (0, 3),
        (1, 2),
        (2, ssot::NESTED_INNER_MAX_SCROLL_ROWS),
    ] {
        let local = crate::view::NestedLocal {
            outer_rows,
            inner_rows: [inner, 0],
        };
        let view = widget_view(&nested, &local, ssot::NESTED_CARD_HEIGHT);
        let mut texts = Vec::new();
        collect_text_ops(&view.paint, &mut texts);
        let row_regions: Vec<_> = view
            .hit_regions
            .iter()
            .filter(|region| region.id.as_str().contains(":panel0-row-"))
            .collect();
        assert!(
            !row_regions.is_empty(),
            "the selectable panel declares row hit regions at outer {outer_rows}/inner {inner}"
        );
        let visible = ssot::nested_visible_field_rect(
            ssot::NESTED_SELECTABLE_PANEL,
            ssot::nested_outer_offset_y(outer_rows),
            560.0,
        )
        .expect("selectable panel visible in every reachable outer offset");
        for region in &row_regions {
            let bounds = region.bounds.into_rect();
            // Every declared row rect has a painted row text at the same
            // origin (hit == paint), and lies inside the visible field
            // (rows scrolled out of the clip are not declared).
            assert!(
                texts
                    .iter()
                    .any(|(rect, content)| content.starts_with("p1 item ")
                        && (rect.origin.y - bounds.origin.y).abs() < 0.5
                        && (rect.origin.x - bounds.origin.x).abs() < 0.5),
                "hit region {} has no painted row at the same origin",
                region.id.as_str()
            );
            assert!(
                bounds.origin.y >= visible.origin.y - 0.5
                    && bounds.origin.y + bounds.size.height
                        <= visible.origin.y + visible.size.height + 0.5,
                "hit region {} must stay inside the visible field",
                region.id.as_str()
            );
        }
        // No hit regions for the Hidden (non-selectable) panel.
        assert!(
            view.hit_regions
                .iter()
                .all(|region| !region.id.as_str().contains("panel1")),
            "only the selectable panel declares row hit regions"
        );
    }
}

// ---------------------------------------------------------------------------
// 3. Behavior through the core declaration resolvers
// ---------------------------------------------------------------------------

// Root-local card origins for the 600x640 test frame (cards stack in
// ssot::card_height_for order with CARD_TOP/CARD_GAP spacing).
const LIST_CARD_TOP: f32 = ssot::CARD_TOP;
const OVERLAY_CARD_TOP: f32 = LIST_CARD_TOP
    + ssot::LIST_CARD_HEIGHT
    + ssot::CARD_GAP
    + ssot::INPUT_CARD_HEIGHT
    + ssot::CARD_GAP;
const NESTED_CARD_TOP: f32 = OVERLAY_CARD_TOP + ssot::OVERLAY_CARD_HEIGHT + ssot::CARD_GAP;

#[test]
fn list_row_click_selects_note_through_the_reducer() {
    let mut runtime = app_runtime();
    let view = composed_view(&runtime);
    // Row 2 center in root coordinates (card at x=CARD_MARGIN_X).
    let point = Point {
        x: ssot::CARD_MARGIN_X + ssot::LIST_INSET_X + 10.0,
        y: LIST_CARD_TOP + ssot::list_row_top_in_card(2, 0.0) + ssot::LIST_ROW_HEIGHT / 2.0,
    };
    let input = pointer_input(&view, point, PointerEventKind::Press, true);
    assert!(apply(&mut runtime, input));
    assert_eq!(
        runtime.external().selected_note,
        Some(2),
        "row click must reach the reducer as SelectNote(2)"
    );
}

// Fix-2 behavior: a click on a row inside the SCROLLED inner panel reaches
// the reducer as SelectInnerItem and the next frame paints the highlight —
// including after the inner panel has been scrolled (the offset-mapped
// click path).
#[test]
fn nested_inner_row_click_selects_item_through_the_reducer() {
    let mut runtime = app_runtime();

    // First scroll the selectable inner panel by two rows, so the click
    // must be offset-mapped to land on the right item.
    let wheel_point = Point {
        x: 300.0,
        y: NESTED_CARD_TOP + ssot::NESTED_OUTER_TOP + ssot::NESTED_PANEL_HEADER_HEIGHT + 20.0,
    };
    for _ in 0..2 {
        let view = composed_view(&runtime);
        let input = wheel_input(&view, wheel_point, -1.0);
        assert!(apply(&mut runtime, input));
    }
    assert_eq!(runtime.local_state().widgets.3.inner_rows[0], 2);

    // Click what is VISUALLY the second visible row: with the panel
    // scrolled by 2, that is item index 3.
    let outer_offset = ssot::nested_outer_offset_y(0);
    let inner_offset = ssot::nested_inner_offset_y(2, ssot::NESTED_FIELD_HEIGHT);
    let row_rect = ssot::nested_inner_row_rect_in_card(3, outer_offset, inner_offset, 560.0);
    let point = Point {
        x: ssot::CARD_MARGIN_X + row_rect.origin.x + 10.0,
        y: NESTED_CARD_TOP + row_rect.origin.y + row_rect.size.height / 2.0,
    };
    let view = composed_view(&runtime);
    let input = pointer_input(&view, point, PointerEventKind::Press, true);
    assert!(apply(&mut runtime, input));
    assert_eq!(
        runtime.external().selected_inner_item,
        Some(3),
        "the scrolled row click must reach the reducer as SelectInnerItem(3)"
    );

    // Next frame the nested widget paints the selected-row highlight from
    // the REDUCED state, at the same rect the hit region declared.
    let nested = NestedFeedWidget;
    let nested_view = widget_view_with_state(
        &nested,
        runtime.external(),
        &runtime.local_state().widgets.3,
        ssot::NESTED_CARD_HEIGHT,
    );
    let highlight = fill_bounds_by_shape_id(&nested_view.paint, "nested-row-selected")
        .expect("selected-row highlight painted");
    assert_eq!(highlight, row_rect);
}

#[test]
fn list_wheel_scrolls_and_updates_the_declared_offset() {
    let mut runtime = app_runtime();
    let view = composed_view(&runtime);
    let point = Point {
        x: 300.0,
        y: LIST_CARD_TOP + ssot::LIST_ROWS_TOP + 40.0,
    };
    let input = wheel_input(&view, point, -1.0);
    assert!(apply(&mut runtime, input));
    assert_eq!(runtime.local_state().widgets.0.scroll_rows, 1);

    // The next frame's declaration carries the new offset.
    let view = composed_view(&runtime);
    let scroll = view
        .scroll_regions
        .iter()
        .find(|region| region.id == ssot::list_scroll_region_id())
        .expect("list scroll region");
    assert_eq!(scroll.offset.y, ssot::list_offset_y(1));
}

#[test]
fn nested_wheel_scrolls_inner_then_chains_to_outer_at_limit() {
    let mut runtime = app_runtime();
    // A point over panel 0's inner field (root coordinates).
    let point = Point {
        x: 300.0,
        y: NESTED_CARD_TOP + ssot::NESTED_OUTER_TOP + ssot::NESTED_PANEL_HEADER_HEIGHT + 20.0,
    };

    // Wheel down to the inner's limit: each notch scrolls PANEL 0 only.
    for step in 1..=ssot::NESTED_INNER_MAX_SCROLL_ROWS {
        let view = composed_view(&runtime);
        let input = wheel_input(&view, point, -1.0);
        let InputEvent::Wheel(wheel) = &input.event else {
            panic!("expected wheel input");
        };
        assert_eq!(
            wheel.region_id.as_ref(),
            Some(&ssot::nested_inner_region_id(0)),
            "step {step}: the pointed-at inner region wins under NearestScrollable"
        );
        assert!(apply(&mut runtime, input));
        assert_eq!(runtime.local_state().widgets.3.inner_rows[0], step);
        assert_eq!(runtime.local_state().widgets.3.outer_rows, 0);
    }

    // At the inner's limit the next wheel CHAINS to the outer region.
    let view = composed_view(&runtime);
    let input = wheel_input(&view, point, -1.0);
    let InputEvent::Wheel(wheel) = &input.event else {
        panic!("expected wheel input");
    };
    assert_eq!(
        wheel.region_id.as_ref(),
        Some(&ssot::nested_outer_region_id()),
        "at-limit wheel must chain inner -> outer"
    );
    assert!(apply(&mut runtime, input));
    assert_eq!(runtime.local_state().widgets.3.outer_rows, 1);

    // An up-wheel is reclaimed by the inner as soon as it has travel
    // again (hand-off is not sticky).
    let view = composed_view(&runtime);
    let input = wheel_input(&view, point, 1.0);
    let InputEvent::Wheel(wheel) = &input.event else {
        panic!("expected wheel input");
    };
    assert_eq!(
        wheel.region_id.as_ref(),
        Some(&ssot::nested_inner_region_id(0))
    );
    assert!(apply(&mut runtime, input));
    assert_eq!(
        runtime.local_state().widgets.3.inner_rows[0],
        ssot::NESTED_INNER_MAX_SCROLL_ROWS - 1
    );
}

#[test]
fn overlay_titlebar_drag_moves_the_panel() {
    let mut runtime = app_runtime();
    let start = ssot::overlay_default_offset();
    // Press in the titlebar (root coordinates), then move while holding
    // the primary button, then release.
    let press_point = Point {
        x: ssot::CARD_MARGIN_X + start.x + 110.0,
        y: OVERLAY_CARD_TOP + start.y + 10.0,
    };
    let view = composed_view(&runtime);
    let input = pointer_input(&view, press_point, PointerEventKind::Press, true);
    assert!(apply(&mut runtime, input));
    assert!(runtime.local_state().widgets.2.dragging);

    // Small move that stays inside the (still current) titlebar region.
    let move_point = Point {
        x: press_point.x + 20.0,
        y: press_point.y + 4.0,
    };
    let view = composed_view(&runtime);
    let input = pointer_input(&view, move_point, PointerEventKind::Move, true);
    assert!(apply(&mut runtime, input));
    let moved = runtime.local_state().widgets.2.offset;
    assert_eq!(
        (moved.x - start.x, moved.y - start.y),
        (20.0, 4.0),
        "panel must follow the pointer by the drag delta"
    );

    let view = composed_view(&runtime);
    let input = pointer_input(&view, move_point, PointerEventKind::Release, false);
    assert!(apply(&mut runtime, input));
    assert!(!runtime.local_state().widgets.2.dragging);
    // Next frame the drag region sits at the NEW titlebar position.
    let view = composed_view(&runtime);
    let drag = view
        .hit_regions
        .iter()
        .find(|region| region.id == ssot::overlay_drag_region_id())
        .expect("drag hit region");
    assert_eq!(drag.bounds.into_rect().origin.x, moved.x);
}

#[test]
fn wheel_over_the_overlay_panel_scrolls_the_feed_behind_it() {
    let mut runtime = app_runtime();
    let view = composed_view(&runtime);

    // Occlusion evidence: the dispatch graph materializes the overlay
    // layer as an occluder that blocks the pointer but NOT the wheel.
    // (The COMPOSED variant is required: child paint lives in mounted
    // child paint units, not in the root view's own `paint`.)
    let graph = derive_dispatch_graph_for_composed_view(
        runtime.widget(),
        runtime.external(),
        runtime.local_state(),
        &view,
    );
    let occluders: Vec<_> = graph
        .nodes
        .iter()
        .filter(|node| {
            node.kind == DispatchGraphNodeKind::Occlusion
                && node.order.z_index == ssot::OVERLAY_Z_INDEX
        })
        .collect();
    assert!(!occluders.is_empty(), "overlay layer must occlude");
    for occluder in &occluders {
        assert_eq!(occluder.blocks_pointer, Some(true));
        assert_eq!(
            occluder.blocks_wheel,
            Some(false),
            "the overlay must be wheel-transparent"
        );
    }

    // Wheel over the panel BODY (inside the overlay, inside the feed
    // band): the feed scroll region consumes.
    let offset = ssot::overlay_default_offset();
    let point = Point {
        x: ssot::CARD_MARGIN_X + offset.x + 110.0,
        y: OVERLAY_CARD_TOP + offset.y + ssot::OVERLAY_TITLEBAR_HEIGHT + 20.0,
    };
    let input = wheel_input(&view, point, -1.0);
    let InputEvent::Wheel(wheel) = &input.event else {
        panic!("expected wheel input");
    };
    assert_eq!(
        wheel.region_id.as_ref(),
        Some(&ssot::overlay_feed_region_id())
    );
    assert!(apply(&mut runtime, input));
    assert_eq!(runtime.local_state().widgets.2.feed_rows, 1);
}

// The roaming drag end to end: press the roam titlebar, walk the panel up
// out of the feed band past the card's layout top, and verify the composed
// view (with the region now outside the overlay card) still admits — the
// child overflow allowance composes into the app root's paint order.
#[test]
fn roaming_overlay_drag_leaves_the_band_and_stays_admissible() {
    let mut runtime = app_runtime();
    let start = ssot::overlay_roam_default_offset();
    let press_point = Point {
        x: ssot::CARD_MARGIN_X + start.x + 70.0,
        y: OVERLAY_CARD_TOP + start.y + 8.0,
    };
    let view = composed_view(&runtime);
    let input = pointer_input(&view, press_point, PointerEventKind::Press, true);
    assert!(apply(&mut runtime, input));
    assert!(runtime.local_state().widgets.2.roam_dragging);
    assert!(!runtime.local_state().widgets.2.dragging);

    // Walk upward in steps that stay inside the CURRENT (re-declared)
    // titlebar each frame (the press sat 8px below the titlebar top, so
    // steps must stay under 8px), until the clamp stops at the allowance
    // edge.
    let mut point = press_point;
    for _ in 0..10 {
        point = Point {
            x: point.x,
            y: point.y - 6.0,
        };
        let view = composed_view(&runtime);
        let input = pointer_input(&view, point, PointerEventKind::Move, true);
        assert!(apply(&mut runtime, input));
    }
    let roam_offset = runtime.local_state().widgets.2.roam_offset;
    assert!(
        roam_offset.y < 0.0,
        "the roaming panel must leave the band (top 28) and the card (top 0); got y={}",
        roam_offset.y
    );
    assert!(
        roam_offset.y >= -ssot::OVERLAY_CARD_ROOT_TOP,
        "the roaming panel must stay inside the declared overflow allowance; got y={}",
        roam_offset.y
    );

    let view = composed_view(&runtime);
    let input = pointer_input(&view, point, PointerEventKind::Release, false);
    assert!(apply(&mut runtime, input));
    assert!(!runtime.local_state().widgets.2.roam_dragging);

    // Composed pre-flight with the region OUTSIDE the overlay card: clean,
    // because the child's overflow allowance composes into the root view.
    let view = composed_view(&runtime);
    let diagnostics = view_definition_contract_diagnostics_for_capabilities(
        &view,
        &runtime.widget().capabilities(),
    );
    assert!(
        !view_definition_has_blocking_contract_diagnostic(&diagnostics),
        "composed pre-flight with the roaming panel out of the card: {diagnostics:?}"
    );
    // The clamped panel did not move.
    assert_eq!(
        runtime.local_state().widgets.2.offset,
        ssot::overlay_default_offset()
    );
}

// The roaming panel is wheel-transparent like the clamped one: wheeling
// over its body scrolls the feed behind it.
#[test]
fn wheel_over_the_roaming_panel_scrolls_the_feed_behind_it() {
    let mut runtime = app_runtime();
    let view = composed_view(&runtime);
    let start = ssot::overlay_roam_default_offset();
    let point = Point {
        x: ssot::CARD_MARGIN_X + start.x + 70.0,
        y: OVERLAY_CARD_TOP + start.y + ssot::OVERLAY_ROAM_TITLEBAR_HEIGHT + 12.0,
    };
    let input = wheel_input(&view, point, -1.0);
    let InputEvent::Wheel(wheel) = &input.event else {
        panic!("expected wheel input");
    };
    assert_eq!(
        wheel.region_id.as_ref(),
        Some(&ssot::overlay_feed_region_id())
    );
    assert!(apply(&mut runtime, input));
    assert_eq!(runtime.local_state().widgets.2.feed_rows, 1);
}

#[test]
fn text_edit_replaces_the_draft_and_projects_into_the_overlay() {
    let mut runtime = app_runtime();
    let view = composed_view(&runtime);
    let input = text_edit_input(&view, "hello world");
    assert!(apply(&mut runtime, input));
    assert_eq!(runtime.external().draft, "hello world");

    // Inter-widget proof: the OVERLAY widget paints the draft it never
    // touched — the projection travelled input -> reducer -> overlay
    // (paint from the overlay's OWN view; the composed root's `paint`
    // carries only the app background, children are mounted paint units).
    let overlay = OverlayWidget;
    let overlay_view = widget_view_with_state(
        &overlay,
        runtime.external(),
        &runtime.local_state().widgets.2,
        ssot::OVERLAY_CARD_HEIGHT,
    );
    let mut texts = Vec::new();
    collect_text_ops(&overlay_view.paint, &mut texts);
    assert!(
        texts
            .iter()
            .any(|(_, content)| content.contains("draft: hello world")),
        "overlay must project the reduced draft"
    );
}

// ---------------------------------------------------------------------------
// Sync-by-construction event handling (audit NC-8, ADR-0003)
// ---------------------------------------------------------------------------

/// The `event_handling_table!` law: for ANY event, the declared
/// disposition and the actual handler outcome agree — the same
/// pattern+guard tokens decide both — so the physical-path reconciliation
/// (`event_declaration.handler_*`, the NC-8 live Error pair) cannot fire.
/// Returns the agreed handledness so callers can also pin expectations.
fn declaration_and_handler_agree<W>(
    widget: &W,
    external: &W::ExternalState,
    make_local: impl Fn() -> W::LocalState,
    event: InputEvent,
    label: &str,
) -> bool
where
    W: SlipwayLogic + SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy,
{
    let declaration = declared_event_handling(widget, external, &make_local(), &event);
    let declared = declaration.disposition.final_disposition.handled;
    let mut local = make_local();
    let raw = widget.handle_event(external, &mut local, event.clone());
    assert_eq!(
        raw.handled, declared,
        "{label}: handler and declared disposition must agree for {event:?}"
    );
    if declared {
        let outcome = apply_physical_event_handling_declaration(declaration, raw);
        assert!(
            !outcome
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.starts_with("event_declaration.handler_")),
            "{label}: reconciliation mismatch must be impossible: {:?}",
            outcome.diagnostics
        );
    }
    declared
}

#[test]
fn event_tables_declare_exactly_what_the_handlers_do() {
    let external = ShowcaseState::default();

    // Note list: geometry-free cases pin expected handledness; the press
    // scan proves agreement at EVERY probed y — on-row and between-row
    // alike — without duplicating the row math in the test.
    let list = NoteListWidget;
    let list_local = || list.initial_local_state();
    let wheel = |target: WidgetId, delta_y: f32| {
        InputEvent::Wheel(WheelEvent {
            target,
            target_slot: None,
            region_id: None,
            delta_x: 0.0,
            delta_y,
        })
    };
    let press_at = |target: WidgetId, y: f32, primary: bool| {
        InputEvent::Pointer(PointerEvent {
            target,
            target_slot: None,
            position: Point { x: 24.0, y },
            target_bounds: None,
            kind: PointerEventKind::Press,
            button: Some(PointerButton::Primary),
            details: PointerDetails {
                buttons: PointerButtons {
                    primary,
                    ..PointerButtons::default()
                },
                ..PointerDetails::default()
            },
        })
    };
    assert!(declaration_and_handler_agree(
        &list,
        &external,
        list_local,
        wheel(ssot::list_id(), -1.0),
        "list wheel"
    ));
    assert!(!declaration_and_handler_agree(
        &list,
        &external,
        list_local,
        wheel(ssot::input_id(), -1.0),
        "list wrong-target wheel"
    ));
    assert!(declaration_and_handler_agree(
        &list,
        &external,
        list_local,
        InputEvent::Focus(FocusEvent {
            target: ssot::list_id(),
            target_slot: None,
            focused: true,
        }),
        "list focus"
    ));
    let mut on_row = 0;
    let mut off_row = 0;
    for step in 0..80 {
        let y = step as f32 * 10.0;
        if declaration_and_handler_agree(
            &list,
            &external,
            list_local,
            press_at(ssot::list_id(), y, true),
            "list press scan",
        ) {
            on_row += 1;
        } else {
            off_row += 1;
        }
    }
    assert!(on_row > 0, "the scan must cross at least one declared row");
    assert!(off_row > 0, "the scan must cross between-row dead space");

    // Draft input: consumes text-edit/text/focus, nothing else.
    let input = DraftInputWidget;
    let input_local = || input.initial_local_state();
    assert!(declaration_and_handler_agree(
        &input,
        &external,
        input_local,
        InputEvent::TextEdit(TextEditEvent {
            target: ssot::input_id(),
            target_slot: None,
            kind: TextEditKind::InsertText,
            text: Some("a".to_string()),
            selection_before: None,
            selection_after: None,
        }),
        "draft text edit"
    ));
    assert!(declaration_and_handler_agree(
        &input,
        &external,
        input_local,
        InputEvent::Text(TextInputEvent {
            target: ssot::input_id(),
            target_slot: None,
            text: "b".to_string(),
        }),
        "draft text"
    ));
    assert!(!declaration_and_handler_agree(
        &input,
        &external,
        input_local,
        wheel(ssot::input_id(), -1.0),
        "draft wheel is not consumed"
    ));

    // Overlay: the Move guard is the state-dependent disposition the NC-8
    // drift class got wrong — agreement holds in BOTH drag states.
    let overlay = OverlayWidget;
    let overlay_idle = || overlay.initial_local_state();
    let overlay_dragging = || {
        let mut local = overlay.initial_local_state();
        local.dragging = true;
        local
    };
    let move_at = |y: f32, primary: bool| {
        InputEvent::Pointer(PointerEvent {
            target: ssot::overlay_id(),
            target_slot: None,
            position: Point { x: 30.0, y },
            target_bounds: None,
            kind: PointerEventKind::Move,
            button: None,
            details: PointerDetails {
                buttons: PointerButtons {
                    primary,
                    ..PointerButtons::default()
                },
                ..PointerDetails::default()
            },
        })
    };
    assert!(declaration_and_handler_agree(
        &overlay,
        &external,
        overlay_idle,
        press_at(ssot::overlay_id(), 30.0, true),
        "overlay press"
    ));
    assert!(!declaration_and_handler_agree(
        &overlay,
        &external,
        overlay_idle,
        move_at(40.0, false),
        "overlay hover move while idle"
    ));
    assert!(declaration_and_handler_agree(
        &overlay,
        &external,
        overlay_dragging,
        move_at(40.0, true),
        "overlay drag move"
    ));
    assert!(declaration_and_handler_agree(
        &overlay,
        &external,
        overlay_idle,
        wheel(ssot::overlay_id(), -1.0),
        "overlay feed wheel"
    ));

    // Nested feed: region-driven scroll — a foreign region id is declared
    // unhandled by the arm guard, not by a body-side early return.
    let nested = NestedFeedWidget;
    let nested_local = || nested.initial_local_state();
    let scroll_for = |region_id: PresentationRegionId| {
        InputEvent::Scroll(ScrollEvent {
            target: ssot::nested_id(),
            target_slot: None,
            region_id,
            offset_x: 0.0,
            offset_y: 24.0,
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            }),
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 400.0,
                },
            }),
        })
    };
    assert!(declaration_and_handler_agree(
        &nested,
        &external,
        nested_local,
        wheel(ssot::nested_id(), -1.0),
        "nested wheel"
    ));
    assert!(declaration_and_handler_agree(
        &nested,
        &external,
        nested_local,
        scroll_for(ssot::nested_outer_region_id()),
        "nested owned-region scroll"
    ));
    assert!(!declaration_and_handler_agree(
        &nested,
        &external,
        nested_local,
        scroll_for(PresentationRegionId::from("someone-elses-region")),
        "nested foreign-region scroll"
    ));
    let mut nested_on_row = 0;
    let mut nested_off_row = 0;
    for step in 0..80 {
        let y = step as f32 * 10.0;
        if declaration_and_handler_agree(
            &nested,
            &external,
            nested_local,
            press_at(ssot::nested_id(), y, true),
            "nested press scan",
        ) {
            nested_on_row += 1;
        } else {
            nested_off_row += 1;
        }
    }
    assert!(
        nested_on_row > 0,
        "the scan must cross the selectable panel's rows"
    );
    assert!(
        nested_off_row > 0,
        "the scan must cross non-selectable space"
    );
}
