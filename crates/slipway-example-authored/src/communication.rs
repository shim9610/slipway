//! Inter-widget/app communication only (`docs/public/authoring-layout.md`).
//! Owns the typed app messages, the app reducer, the `SlipwayApp`
//! composition (including the app/container layout plan that places the
//! four widget cards and the app-level PAGE scroll region), and NOTHING
//! widget-internal.
//!
//! The one sanctioned inter-widget flow
//! (`docs/agents/authoring-file-boundaries.md`):
//!
//! ```text
//! Child A -> typed message -> parent reducer -> projected input -> Child B
//! ```
//!
//! Concretely here: the list emits `SelectNote`, the input emits
//! `ReplaceDraft`; `apply_messages` writes `ShowcaseState`; the overlay
//! panel (a DIFFERENT widget) paints `selected_note` and `draft` from that
//! state on the next frame. No widget ever reads or writes a sibling's
//! local state.

use slipway::prelude::*;

use crate::ssot::{
    CARD_GAP, CARD_MARGIN_X, CARD_MIN_WIDTH, CARD_TOP, DraftInputWidget, LIST_NOTE_COUNT,
    MeasuredLabel, NestedFeedWidget, NoteListWidget, OverlayWidget, PAGE_SCROLL_STEP,
    ShowcaseState, app_id, badge_text, card_height_for, page_max_scroll_y, page_scroll_region_id,
    rgb, window_badge_label,
};

/// Typed child-output -> app-message surface. Every variant names one
/// semantic app-state change; widgets never encode "please poke widget X".
#[derive(Clone, Debug, PartialEq)]
pub enum ShowcaseMessage {
    /// Emitted by the list on a row click.
    SelectNote(usize),
    /// Emitted by the text input on every committed edit.
    ReplaceDraft(String),
    /// Emitted by the nested feed on a row click inside its SELECTABLE
    /// inner panel (the notes-list selection shape inside a scrolled
    /// nested region).
    SelectInnerItem(usize),
}

/// The app reducer (`apply_messages` in the quickstart). The backend
/// runner calls it with every message batch the widgets emitted for a
/// frame; it is the ONLY writer of `ShowcaseState` — except the
/// runtime-invoked `project_frame_viewport` hook below, the sanctioned
/// platform-truth channel for the live window size.
pub fn apply_messages(state: &mut ShowcaseState, messages: Vec<ShowcaseMessage>) {
    for message in messages {
        match message {
            ShowcaseMessage::SelectNote(index) => {
                state.selected_note = Some(index.min(LIST_NOTE_COUNT.saturating_sub(1)));
            }
            ShowcaseMessage::ReplaceDraft(draft) => state.draft = draft,
            ShowcaseMessage::SelectInnerItem(row) => {
                state.selected_inner_item =
                    Some(row.min(crate::ssot::NESTED_INNER_ROW_COUNT.saturating_sub(1)));
            }
        }
    }
}

/// App-level presentation state: the PAGE scroll offset. This is the
/// app's own local state (`SlipwayApp::LocalState`), NOT `ShowcaseState`:
/// a scroll position is presentation state, so it lives next to the
/// handler that moves it — the same discipline as the widgets' per-widget
/// local structs in view.rs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ShowcaseAppLocal {
    /// Page scroll offset in pixels, non-negative, clamped to the exact
    /// declared travel (`ssot::page_max_scroll_y`) — the
    /// `ScrollRegionDeclaration::offset` contract.
    pub page_scroll_y: f32,
}

/// The app composite (`docs/public/api/core.md` "App Composition"): N
/// authored widgets stay N addressable children through a widget tuple.
/// Do not fake children by painting them inside one root widget — the
/// runtime addresses each child's local state through its own slot.
#[derive(Clone, Debug, PartialEq)]
pub struct ShowcaseApp {
    widgets: (
        NoteListWidget,
        DraftInputWidget,
        OverlayWidget,
        NestedFeedWidget,
    ),
}

impl ShowcaseApp {
    pub fn new() -> Self {
        Self {
            widgets: (
                NoteListWidget,
                DraftInputWidget,
                OverlayWidget,
                NestedFeedWidget,
            ),
        }
    }
}

impl SlipwayApp for ShowcaseApp {
    type ExternalState = ShowcaseState;
    type LocalState = ShowcaseAppLocal;
    type AppMessage = ShowcaseMessage;
    type Widgets = (
        NoteListWidget,
        DraftInputWidget,
        OverlayWidget,
        NestedFeedWidget,
    );

    fn id(&self) -> WidgetId {
        app_id()
    }

    fn widgets(&self) -> &Self::Widgets {
        &self.widgets
    }

    fn initial_local_state(&self) -> Self::LocalState {
        ShowcaseAppLocal::default()
    }

    /// PATTERN: platform-truth projection
    /// (`SlipwayApp::project_frame_viewport`). The runtime calls this
    /// whenever the presenting backend records the live frame viewport;
    /// mirroring it into `ShowcaseState::viewport` makes the window size
    /// ordinary external state, so the roaming overlay's allowance, its
    /// drag clamp, AND its paint all read the SAME value (the MUST-agree
    /// chain — `ViewDefinitionInput.frame.viewport` reaches only
    /// `view_definition`, never `paint` or `handle_event`).
    fn project_frame_viewport(&self, external: &mut Self::ExternalState, viewport: Rect) {
        external.viewport = viewport.size;
    }

    /// PATTERN: measurement-truth projection
    /// (`SlipwayApp::project_text_metrics`,
    /// docs/public/api/backends.md "Text Wrap and Alignment"). The
    /// runtime calls this with the presenting backend's REAL text-metric
    /// provider on the same cadence as the viewport projection above.
    /// The window badge label (the live window size — content that
    /// CHANGES with every resize, so no fixed guess could stay correct)
    /// is measured with the SAME `badge_text()` style its paint op
    /// declares; the valid receipt's measured size becomes ordinary
    /// external state, and the input card sizes the badge rect to it —
    /// the replacement for the NC-4/NC-14 hand-computed character-width
    /// ratios. Refused/invalid receipts land as honest absence: no badge,
    /// never a fabricated size.
    fn project_text_metrics(
        &self,
        external: &mut Self::ExternalState,
        metrics: &mut dyn SlipwayTextMetricProvider,
    ) {
        if external.viewport.width <= 0.0 || external.viewport.height <= 0.0 {
            // No frame presented yet: nothing truthful to measure.
            external.window_badge = None;
            return;
        }
        let text = window_badge_label(external.viewport);
        let receipt = metrics.measure_text(TextMeasurementRequest {
            target: app_id(),
            request_id: "authored.app:window-badge".to_string(),
            content: text.clone(),
            style: badge_text(),
            // Intrinsic size: the badge hugs the label, so no wrap width.
            available_bounds: None,
            flow: None,
            purposes: vec![TextMeasurementPurpose::IntrinsicSize],
        });
        external.window_badge = match receipt {
            TextMeasurementReceipt::Valid(valid) => Some(MeasuredLabel {
                text,
                size: valid.facts.measured_size,
            }),
            TextMeasurementReceipt::Invalid { .. } | TextMeasurementReceipt::Unsupported { .. } => {
                None
            }
        };
    }

    /// PATTERN: app-level page scroll — the handler half. The PAGE region
    /// below targets the app id, so its Wheel events (declared dispatch,
    /// including at-limit CHAINING from the card regions) and Scroll
    /// events (a backend's native scrollbar/rail sync) arrive HERE, not at
    /// any widget. Both arms clamp to the exact declared travel with the
    /// projected viewport, so the stored offset can never drift past what
    /// the declaration admits (`view_contract.scroll_offset_invalid`).
    fn handle_event(
        &self,
        external: &Self::ExternalState,
        local: &mut Self::LocalState,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        match event {
            InputEvent::Wheel(wheel) if wheel.region_id == Some(page_scroll_region_id()) => {
                let direction = if wheel.delta_y < 0.0 {
                    PAGE_SCROLL_STEP
                } else if wheel.delta_y > 0.0 {
                    -PAGE_SCROLL_STEP
                } else {
                    0.0
                };
                local.page_scroll_y = (local.page_scroll_y + direction)
                    .clamp(0.0, page_max_scroll_y(external.viewport));
                page_scroll_outcome(self.id(), local.page_scroll_y)
            }
            InputEvent::Scroll(scroll) if scroll.region_id == page_scroll_region_id() => {
                // A Scroll event carries its own declared geometry, so the
                // clamp uses the event's viewport/content — exact even if
                // the projection has not run yet this frame.
                let travel =
                    (scroll.content_bounds.size.height - scroll.viewport.size.height).max(0.0);
                local.page_scroll_y = scroll.offset_y.clamp(0.0, travel);
                page_scroll_outcome(self.id(), local.page_scroll_y)
            }
            _ => EventOutcome::ignored(),
        }
    }

    /// PATTERN: app/container layout planning. The plan stacks the four
    /// cards vertically; each child receives a TARGET-LOCAL layout input
    /// (origin 0,0 — anything else refuses admission with
    /// `view_contract.child_input_viewport_not_target_local`) plus its
    /// parent-local placement. Card heights come from
    /// `ssot::card_height_for`, the same source each widget's own
    /// `layout` reads — the plan and the widget must agree on the box.
    ///
    /// RESPONSIVE WIDTH (the live behavior): the card column fills the
    /// window — width derives from `LayoutInput.viewport` minus
    /// `CARD_MARGIN_X` each side, floored at `CARD_MIN_WIDTH`. The
    /// fixed-width alternative is one clamp away:
    /// `width.min(CARD_MAX_WIDTH + 2.0 * CARD_MARGIN_X)` (clamp to a
    /// constant; the pre-Step-212 shape). Every dependent geometry —
    /// hit/paint/pointer/overflow — already takes the card width as a
    /// parameter, so both choices keep the MUST-agree chains intact.
    fn layout_plan(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        children: Vec<ChildLayoutSeed>,
    ) -> AppLayoutPlan {
        let width = input
            .viewport
            .size
            .width
            .max(CARD_MIN_WIDTH + 2.0 * CARD_MARGIN_X);
        let card_width = width - 2.0 * CARD_MARGIN_X;
        let mut y = CARD_TOP;
        let mut plans = Vec::new();

        for seed in children {
            let height = card_height_for(&seed.child);
            let bounds = Rect {
                origin: Point {
                    x: CARD_MARGIN_X,
                    y,
                },
                size: Size {
                    width: card_width,
                    height,
                },
            };
            plans.push(ChildLayoutPlan::placed_for_seed(
                seed,
                LayoutInput {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: bounds.size,
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: bounds.size,
                    },
                },
                ParentLocalRect::new(bounds),
            ));
            y += height + CARD_GAP;
        }

        // Root bounds: the column, but never shorter than the window (the
        // min-height-of-the-window pattern) — the app background always
        // covers the window, and the PAGE region below overflows exactly
        // when the column is taller than the window. The accumulated
        // column height MUST equal `ssot::APP_ROOT_HEIGHT` (the page
        // content height and the roaming allowance both derive from it).
        let column_height = y - CARD_GAP + CARD_TOP;
        AppLayoutPlan {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width,
                    height: column_height.max(input.viewport.size.height),
                },
            }),
            children: plans,
            diagnostics: Vec::new(),
        }
    }

    /// PATTERN: app-level PAGE scroll region — the declaration half
    /// (`SlipwayApp::app_scroll_regions`; the admission stress harness
    /// proves the same root-scroll shape through a runtime wrapper).
    /// This region is the fix the admission advisory
    /// `view_contract.content_overflow_without_scroll_region` induces
    /// (NC-13): painted content taller than the window with no covering
    /// enabled scroll region draws that warning, naming
    /// `docs/public/api/routing-and-scroll.md` — which points back here.
    /// Declared ONLY when the card column exceeds the live window:
    /// viewport = the frame viewport, content = the full column, offset
    /// from the app-local state the handler above writes. Indicator mode
    /// stays the unspecified Auto, so the page scrollbar appears exactly
    /// when the content overflows the window (the shrunken-window case).
    /// Wheel over any dead space scrolls the page; card regions CHAIN to
    /// it at their limits automatically — once declared, this region is
    /// the outermost containing candidate
    /// (`docs/public/api/routing-and-scroll.md`, "Chaining").
    fn app_scroll_regions(
        &self,
        _external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &ViewDefinitionInput,
        layout: &LayoutOutput,
    ) -> Vec<ScrollRegionDeclaration> {
        let viewport = input.frame.viewport;
        let content = layout.bounds.into_rect();
        if content.size.height <= viewport.size.height + 0.5 {
            // No overflow -> no region (declaring zero travel refuses with
            // `view_contract.scroll_geometry_invalid`), and no indicator.
            return Vec::new();
        }
        vec![ScrollRegionDeclaration::explicit(
            page_scroll_region_id(),
            self.id(),
            // The app's own root slot: Wheel/Scroll route to handle_event
            // above, and backends may present the region natively around
            // the mounted cards (iced mounts a native scrollable whose
            // rail IS the Auto indicator; egui shifts the mounted children
            // by the offset and draws the declared indicator).
            Some(WidgetSlotAddress::new(self.id(), 0)),
            TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: viewport.size,
            }),
            TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: content.size.width.max(viewport.size.width),
                    height: content.size.height.max(viewport.size.height),
                },
            }),
            Point {
                x: 0.0,
                // Same clamp as the handler: declaration and handler MUST
                // agree or the first at-limit frame refuses admission.
                y: local
                    .page_scroll_y
                    .clamp(0.0, (content.size.height - viewport.size.height).max(0.0)),
            },
            ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            // The house default (Step 200 revert): the pointed-at region
            // wins first; the page only consumes what chains out.
            WheelRouting::NearestScrollable,
            // BACK-most order: every widget region (z >= 0) fronts the
            // page, so the page wins only dead space and at-limit
            // hand-offs. Distinct from every other declared order (the
            // ambiguous_wheel_overlap defense).
            HitRegionOrder {
                z_index: -1,
                paint_order: 0,
                traversal_order: 0,
            },
            ScrollConsumptionPolicy::exclusive_wheel(),
            true,
        )]
    }

    fn paint(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        // App-level paint is background only; every interactive element is
        // painted (and declared) by the widget that owns it.
        vec![PaintOp::Fill {
            shape: ShapeDeclaration {
                id: Some("authored-app-background".to_string()),
                kind: ShapeKind::Rectangle,
                bounds: layout.bounds.into_rect(),
                path: None,
                clip: None,
            },
            color: rgb(241, 245, 249),
        }]
    }
}

/// Handled-with-evidence outcome for the page scroll (the same shape as
/// the widgets' `local_change_outcome` in internal_logic.rs: consumed
/// locally, change evidence for debug probes, no app message).
fn page_scroll_outcome(target: WidgetId, offset_y: f32) -> EventOutcome<ShowcaseMessage> {
    let mut outcome = EventOutcome::handled();
    outcome.changes.push(ChangeEvidence {
        target: target.clone(),
        slot: Some(WidgetSlotAddress::new(target, 0)),
        field: "page-scroll-y".to_string(),
        before: None,
        after: Some(format!("{offset_y:.1}")),
    });
    outcome
}
