//! View and widget-local presentation state only
//! (`docs/public/authoring-layout.md`). Returns layout, paint, and the
//! hit/focus/scroll/text declarations; owns the per-widget local-state
//! structs (scroll rows, drag anchors, focus flags — presentation state,
//! never app truth). Must not implement app reducers or sibling
//! communication.
//!
//! Every declaration here is built by its prelude capability helper — the
//! region structs are `#[non_exhaustive]` and cannot be built by struct
//! literal (`docs/public/llm-contract-checklist.md`, "What Must Be
//! Declared").

use slipway::prelude::*;

use crate::ssot::{
    self, DraftInputWidget, INPUT_CARD_HEIGHT, LIST_CARD_HEIGHT, LIST_INSET_X, LIST_NOTE_COUNT,
    LIST_ROW_HEIGHT, NESTED_CARD_HEIGHT, NESTED_INNER_CONTENT_HEIGHT, NESTED_INNER_MAX_SCROLL_ROWS,
    NESTED_INNER_ROW_COUNT, NESTED_INNER_ROW_STEP, NESTED_INSET_X, NESTED_OUTER_CONTENT_HEIGHT,
    NESTED_OUTER_TOP, NESTED_PANEL_COUNT, NESTED_PANEL_HEADER_HEIGHT, NESTED_PANEL_HEIGHT,
    NESTED_SELECTABLE_PANEL, NestedFeedWidget, NoteListWidget, OVERLAY_CARD_HEIGHT,
    OVERLAY_FEED_CONTENT_HEIGHT, OVERLAY_FEED_ROW_COUNT, OVERLAY_FEED_ROW_STEP, OVERLAY_FEED_TOP,
    OVERLAY_LAYER_ORDER, OVERLAY_ROAM_LAYER_ORDER, OVERLAY_ROAM_TITLEBAR_HEIGHT,
    OVERLAY_ROAM_Z_INDEX, OVERLAY_TITLEBAR_HEIGHT, OVERLAY_Z_INDEX, OverlayWidget, accent,
    card_background, card_border, header_text, ink, input_field_rect, input_focus_region_id,
    input_text, list_focus_region_id, list_offset_y, list_row_region_id, list_row_top_in_card,
    list_rows_band, list_scroll_region_id, muted, nested_field_rect_in_card, nested_inner_offset_y,
    nested_inner_region_id, nested_inner_row_rect_in_card, nested_inner_row_region_id,
    nested_outer_band, nested_outer_offset_y, nested_outer_region_id, nested_panel_top_in_card,
    nested_visible_field_rect, overlay_border, overlay_drag_region_id, overlay_feed_band,
    overlay_feed_offset_y, overlay_feed_region_id, overlay_overflow_bounds, overlay_panel_rect,
    overlay_roam_drag_region_id, overlay_roam_panel_rect, overlay_roam_titlebar_rect,
    overlay_roaming_clamped_offset, overlay_title_ink, overlay_titlebar_background,
    overlay_titlebar_rect, rgb, row_text,
};

// ---------------------------------------------------------------------------
// Widget-local presentation state
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ListLocal {
    /// Row-quantized scroll position (0..=LIST_MAX_SCROLL_ROWS).
    pub scroll_rows: i32,
    pub focused: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputLocal {
    pub focused: bool,
    /// Local edit counter — presentation-side evidence only; the draft
    /// text itself is app state (`ShowcaseState::draft`).
    pub edit_count: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayLocal {
    /// Card-local origin of the CLAMPED floating panel (stays inside the
    /// feed band; `ssot::overlay_clamped_offset`).
    pub offset: Point,
    pub dragging: bool,
    /// Pointer-to-panel-origin delta captured at drag start.
    pub drag_anchor: Point,
    /// Card-local origin of the ROAMING panel (may leave the band and the
    /// card, inside the declared overflow bounds;
    /// `ssot::overlay_roaming_clamped_offset`).
    pub roam_offset: Point,
    pub roam_dragging: bool,
    pub roam_anchor: Point,
    /// Row-quantized scroll position of the feed BEHIND the panels.
    pub feed_rows: i32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct NestedLocal {
    pub outer_rows: i32,
    pub inner_rows: [i32; NESTED_PANEL_COUNT],
}

// ---------------------------------------------------------------------------
// Small paint helpers (explicit style always; checklist "Style Rules")
// ---------------------------------------------------------------------------

fn shape(id: &str, bounds: Rect, kind: ShapeKind) -> ShapeDeclaration {
    ShapeDeclaration {
        id: Some(id.to_string()),
        kind,
        bounds,
        path: None,
        clip: None,
    }
}

fn fill(id: &str, bounds: Rect, color: Color) -> PaintOp {
    PaintOp::Fill {
        shape: shape(id, bounds, ShapeKind::RoundedRectangle),
        color,
    }
}

fn stroke(id: &str, bounds: Rect, color: Color, width: f32) -> PaintOp {
    PaintOp::Stroke {
        shape: shape(id, bounds, ShapeKind::RoundedRectangle),
        color,
        width,
    }
}

/// Clips `ops` to `clip_bounds`. Scrolled content MUST be painted inside a
/// clip matching the declared scroll viewport: admission validates
/// geometry only, so unclipped overdraw past the declared window is a
/// silent visual bug (llm-entry.md completion standard requires nested
/// scroll paint clipped to the declared inner viewport).
fn clipped(id: &str, clip_bounds: Rect, ops: Vec<PaintOp>) -> PaintOp {
    PaintOp::Group {
        id: Some(id.to_string()),
        clip: Some(ClipDeclaration {
            id: Some(format!("{id}:clip")),
            bounds: clip_bounds,
            path: None,
        }),
        ops,
    }
}

fn card_chrome(id_prefix: &str, bounds: Rect) -> Vec<PaintOp> {
    vec![
        fill(&format!("{id_prefix}-panel"), bounds, card_background()),
        stroke(&format!("{id_prefix}-outline"), bounds, card_border(), 1.0),
    ]
}

fn header_op(bounds: Rect, content: String) -> PaintOp {
    PaintOp::styled_text(
        Rect {
            origin: Point { x: 12.0, y: 6.0 },
            size: Size {
                width: (bounds.size.width - 24.0).max(1.0),
                height: 16.0,
            },
        },
        content,
        ink(),
        header_text(),
    )
}

fn card_layout(
    input: &LayoutInput,
    height: f32,
    output: slipway::LayoutOutputBuilder,
) -> LayoutOutput {
    output.finish(TargetLocalRect::new(Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: Size {
            width: input.viewport.size.width.max(1.0),
            height,
        },
    }))
}

fn self_slot(id: WidgetId) -> Option<WidgetSlotAddress> {
    // Leaf widgets declare a self-rooted slot; the app/backend MOUNTING
    // pass owns the final composed WidgetSlotAddress (checklist
    // "Coordinate Rules") and rewrites it when the child view is mounted.
    Some(WidgetSlotAddress::new(id, 0))
}

// ---------------------------------------------------------------------------
// Note list widget
// ---------------------------------------------------------------------------

impl SlipwayView for NoteListWidget {
    fn initial_local_state(&self) -> Self::LocalState {
        ListLocal::default()
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: slipway::LayoutOutputBuilder,
    ) -> LayoutOutput {
        card_layout(&input, LIST_CARD_HEIGHT, output)
    }

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        let bounds = layout.bounds().into_rect();
        let width = bounds.size.width;
        let offset_y = list_offset_y(local.scroll_rows);
        let selected = external.selected_note;

        let mut ops = card_chrome("list", bounds);
        ops.push(header_op(
            bounds,
            format!(
                "Notes — selected: {}",
                selected.map_or("none".to_string(), |index| format!("note {}", index + 1)),
            ),
        ));

        // PATTERN: routed (paint-only) scroll region — the AUTHOR paints
        // the visible window with the offset already applied; backends
        // never translate authored paint by the declared offset
        // (`ScrollRegionDeclaration` "PAINT RESPONSIBILITY"). Row y comes
        // from the same `list_row_top_in_card` the hit regions use.
        let mut rows = Vec::new();
        for (index, note) in external.notes.iter().enumerate().take(LIST_NOTE_COUNT) {
            let top = list_row_top_in_card(index, offset_y);
            let row_bounds = Rect {
                origin: Point {
                    x: LIST_INSET_X,
                    y: top,
                },
                size: Size {
                    width: (width - 2.0 * LIST_INSET_X).max(1.0),
                    height: LIST_ROW_HEIGHT,
                },
            };
            if selected == Some(index) {
                rows.push(fill("list-row-selected", row_bounds, rgb(219, 234, 254)));
            }
            rows.push(PaintOp::styled_text(
                row_bounds,
                note.clone(),
                if selected == Some(index) {
                    accent()
                } else {
                    ink()
                },
                row_text(),
            ));
        }
        ops.push(clipped("list-rows", list_rows_band(width), rows));

        if local.focused {
            // Plain-focus visual state: the ring derives from the same
            // band rect the focus region declares below.
            ops.push(stroke(
                "list-focus-ring",
                list_rows_band(width),
                accent(),
                1.5,
            ));
        }
        ops
    }

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        observations(
            self.id(),
            format!("selected-note: {:?}", external.selected_note),
            format!("{local:?}"),
        )
    }
}

impl SlipwayViewDefinition for NoteListWidget {
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let (frame, layout) = slipway::layout_view_definition(self, external, local, input);
        let width = layout.bounds().as_rect().size.width;
        let offset_y = list_offset_y(local.scroll_rows);
        let band = list_rows_band(width);
        let address = self_slot(self.id());

        // PATTERN: per-row hit regions
        // (docs/public/llm-contract-checklist.md "What Must Be Declared").
        // WHEN: each painted row must be individually clickable. Painting
        // a row is NEVER enough to make it interactive. Bounds derive from
        // the same LIST_* constants as the paint above; only rows fully
        // inside the band are declared (a region outside layout bounds
        // refuses with `view_contract.hit_bounds_outside_layout`; a region
        // over unpainted area routes clicks the user cannot see).
        let mut hit_regions = Vec::new();
        for index in 0..LIST_NOTE_COUNT {
            let top = list_row_top_in_card(index, offset_y);
            if top < band.origin.y - 0.5
                || top + LIST_ROW_HEIGHT > band.origin.y + band.size.height + 0.5
            {
                continue;
            }
            hit_regions.push(hit_region_from_pointer_capability(
                self,
                external,
                local,
                list_row_region_id(index),
                address.clone(),
                TargetLocalRect::new(Rect {
                    origin: Point {
                        x: LIST_INSET_X,
                        y: top,
                    },
                    size: Size {
                        width: (width - 2.0 * LIST_INSET_X).max(1.0),
                        height: LIST_ROW_HEIGHT,
                    },
                }),
                PointerEventCoordinateSpace::TargetLocal,
                // Rows never overlap each other, but distinct orders are
                // still declared: overlapping same-order regions refuse
                // with `view_contract.ambiguous_hit_overlap`.
                HitRegionOrder {
                    z_index: 0,
                    paint_order: index,
                    traversal_order: index,
                },
                Some(list_row_region_id(index).as_str().to_string()),
                CursorCapability::Pointer,
                true,
                PointerCaptureIntent::OnPress,
            ));
        }

        // PATTERN: plain (non-text) focus region via
        // `focus_region_from_focus_capability` — the constructor for
        // focusable widgets WITHOUT text editing (text inputs use the
        // text-edit helper instead; see DraftInputWidget). Declaring
        // Capability::FocusInput with no enabled focus region refuses with
        // `view_contract.focus_capability_missing_focus_region`.
        let focus_regions = vec![focus_region_from_focus_capability(
            self,
            external,
            local,
            list_focus_region_id(),
            address.clone(),
            TargetLocalRect::new(band),
            true,
        )];

        // PATTERN: scroll region via the PLAIN helper
        // (docs/public/api/routing-and-scroll.md). WHEN plain vs
        // `_with_order`: this widget declares exactly ONE scroll region
        // and nothing can overlap it, so the 0/0/0 default order is safe.
        // Any widget with several regions, or whose region sits under an
        // overlay's hit region, must use the `_with_order` variant (see
        // OverlayWidget and NestedFeedWidget below) or admission refuses
        // overlaps with `view_contract.ambiguous_wheel_overlap`.
        let mut scroll_regions = Vec::new();
        let terminal_region_index = scroll_regions.len();
        scroll_regions.push(scroll_region_from_scrollable_capability(
            self,
            external,
            local,
            &layout,
            Some(list_scroll_region_id()),
            address,
            true,
        ));

        let paint = self.paint(external, local, &layout);
        assemble_view(
            self.id(),
            frame,
            layout,
            paint,
            hit_regions,
            focus_regions,
            scroll_regions,
            Some(terminal_region_index),
        )
    }
}

/// PATTERN: scroll geometry authored in `SlipwayScrollBehaviorPolicy`
/// (docs/public/api/routing-and-scroll.md "Declare A Scroll Region After
/// Layout"). The helper copies these fields into the declaration, so the
/// geometry contract lives HERE: content must contain the viewport,
/// travel = content - viewport, offset non-negative and clamped
/// (`view_contract.scroll_geometry_invalid` /
/// `view_contract.scroll_offset_invalid` otherwise).
impl SlipwayScrollBehaviorPolicy for NoteListWidget {
    fn scroll_behavior_policy(
        &self,
        _external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ScrollBehaviorPolicyDeclaration {
        let band = list_rows_band(input.viewport.size.width.max(1.0));
        let content = Rect {
            origin: band.origin,
            size: Size {
                width: band.size.width,
                height: ssot::LIST_CONTENT_HEIGHT.max(band.size.height),
            },
        };
        ScrollBehaviorPolicyDeclaration {
            target: self.id(),
            region_id: Some(list_scroll_region_id()),
            address: None,
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            // `extent` is RESERVED (unconsumed); keep it consistent with
            // content_bounds.size (docs/public/api/trait-surface.md).
            extent: content.size,
            viewport: TargetLocalRect::new(band),
            content_bounds: TargetLocalRect::new(content),
            offset: Point {
                x: 0.0,
                y: list_offset_y(local.scroll_rows),
            },
            consumption: ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayWheelRoutingPolicy for NoteListWidget {
    fn wheel_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _region: &PresentationRegionId,
    ) -> WheelRoutingPolicyDeclaration {
        // NearestScrollable default; the full routing breadcrumb (why the
        // default is usually right, and how to author a non-default mode)
        // is on NestedFeedWidget's impl below.
        WheelRoutingPolicyDeclaration {
            target: self.id(),
            routing: WheelRouting::NearestScrollable,
            modifiers: None,
            diagnostics: Vec::new(),
        }
    }
}

// PATTERN: reserved_policy_defaults! — one macro call implements all 23
// RESERVED policy traits with the documented empty defaults. NEVER write
// real logic in a RESERVED trait: nothing consults it, nothing warns
// (docs/public/api/trait-surface.md). One call per widget type.
reserved_policy_defaults!(NoteListWidget);

// ---------------------------------------------------------------------------
// Draft input widget
// ---------------------------------------------------------------------------

impl SlipwayView for DraftInputWidget {
    fn initial_local_state(&self) -> Self::LocalState {
        InputLocal::default()
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: slipway::LayoutOutputBuilder,
    ) -> LayoutOutput {
        card_layout(&input, INPUT_CARD_HEIGHT, output)
    }

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        let bounds = layout.bounds().into_rect();
        let field = input_field_rect(bounds.size.width);
        let mut ops = card_chrome("input", bounds);
        ops.push(header_op(bounds, "Draft (click, then type)".to_string()));
        ops.push(fill("input-field", field, rgb(255, 255, 255)));
        // Focused state = one border-color variant, styled near the call
        // site — local state picks variants, it does not own new tokens
        // (docs/public/api/ime.md).
        ops.push(stroke(
            "input-field-border",
            field,
            if local.focused {
                accent()
            } else {
                card_border()
            },
            if local.focused { 1.5 } else { 1.0 },
        ));
        // Do NOT paint the buffer value here: the backend mounts a native
        // text control for the declared text-edit region and OWNS the
        // value/caret/selection presentation (its font comes from the
        // typography policy below). Painting the value again double-draws
        // it under the native control.

        // PATTERN: measurement-sized element (docs/public/api/backends.md
        // "Text Wrap and Alignment"). The badge rect derives from the
        // MEASURED label size the `project_text_metrics` hook wrote
        // (communication.rs) — with the SAME `badge_text()` style this op
        // declares — so the badge hugs its label at every window size
        // with NO character-width guessing (the NC-4/NC-14 anti-pattern).
        // `None` = no valid measurement yet: the badge is honestly absent
        // rather than painted at a fabricated size.
        if let Some(badge) = &external.window_badge {
            let badge_rect = ssot::input_badge_rect(bounds.size.width, badge.size);
            ops.push(fill("input-badge", badge_rect, rgb(224, 231, 255)));
            ops.push(stroke("input-badge-outline", badge_rect, accent(), 1.0));
            ops.push(PaintOp::styled_text(
                badge_rect,
                badge.text.clone(),
                overlay_title_ink(),
                ssot::badge_text(),
            ));
        }

        // PATTERN: per-op wrap opt-out (docs/public/api/backends.md "Text
        // Wrap and Alignment"). The CJK label's rect is deliberately
        // narrower than the laid-out text; `.no_wrap()` (TextWrap::None,
        // via ssot::nowrap_label_text) keeps it on ONE line, clipped at
        // the rect edge. Unset wrap = word wrap at the rect width — the
        // NC-4 shape that force-wrapped the consumer's CJK headers.
        ops.push(PaintOp::styled_text(
            ssot::input_nowrap_label_rect(bounds.size.width),
            ssot::INPUT_NOWRAP_LABEL.to_string(),
            muted(),
            ssot::nowrap_label_text(),
        ));
        ops
    }

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        observations(self.id(), external.draft.clone(), format!("{local:?}"))
    }
}

impl SlipwayViewDefinition for DraftInputWidget {
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let layout_input = input.layout_input.clone();
        let (frame, layout) = slipway::layout_view_definition(self, external, local, input);
        let field = input_field_rect(layout.bounds().as_rect().size.width);
        let address = self_slot(self.id());

        // PATTERN: text-input region via
        // `text_edit_focus_region_from_capability` — the ONLY constructor
        // that satisfies Capability::TextInput (a plain focus region
        // refuses with
        // `view_contract.text_input_missing_text_edit_focus_region`). It
        // snapshots all nine text policies below at declaration time. The
        // bounds MUST match the painted field (same `input_field_rect`).
        // `member` reuses the widget's own focus_member (the
        // reserved-defaults `None` = no explicit tab order); `measurement`
        // stays `None` — never fabricate text metrics
        // (docs/public/api/ime.md).
        let focus_regions = vec![text_edit_focus_region_from_capability(
            self,
            external,
            local,
            input_focus_region_id(),
            address,
            TargetLocalRect::new(field),
            SlipwayFocusTraversal::focus_member(self, external, local),
            true,
            &layout_input,
            None,
        )];

        let paint = self.paint(external, local, &layout);
        assemble_view(
            self.id(),
            frame,
            layout,
            paint,
            Vec::new(),
            focus_regions,
            Vec::new(),
            None,
        )
    }
}

// The nine text policies (docs/public/api/ime.md): all snapshotted by the
// text-edit helper above. LOAD-BEARING — see api/trait-surface.md.

impl SlipwayTextBufferPolicy for DraftInputWidget {
    fn text_buffer(
        &self,
        external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> TextBufferSnapshot {
        // The buffer IS the app state — the reducer is the only writer.
        TextBufferSnapshot {
            target: self.id(),
            text: external.draft.clone(),
            revision: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayTextSelectionPolicy for DraftInputWidget {
    fn text_selection(
        &self,
        external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> TextSelectionPolicyDeclaration {
        // Caret pinned to the end of the buffer; a real editor would keep
        // the caret in local state and declare it here.
        let caret = external.draft.chars().count();
        TextSelectionPolicyDeclaration {
            target: self.id(),
            selection: None,
            carets: CaretSet {
                carets: vec![caret],
                primary: Some(caret),
            },
            editable: true,
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayImeCompositionPolicy for DraftInputWidget {
    fn ime_composition(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> ImeCompositionPolicyDeclaration {
        // Composition (preedit) is backend/OS-owned; the app declares the
        // inactive baseline (docs/public/api/ime.md).
        ImeCompositionPolicyDeclaration {
            target: self.id(),
            active: false,
            preedit_text: None,
            cursor_range: None,
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayCaretGeometryPolicy for DraftInputWidget {
    fn caret_geometry(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _measurement: Option<&TextMeasurementEvidence>,
    ) -> CaretGeometryEvidence {
        // No measured caret rectangles are claimed: heuristic pixel math
        // is INVALID measurement evidence (api-authoring-model). Empty
        // evidence lets the backend own caret presentation.
        CaretGeometryEvidence {
            target: self.id(),
            caret_bounds: Vec::new(),
            selection_bounds: Vec::new(),
            measurement_request_ids: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayTextEditCommandPolicy for DraftInputWidget {
    fn text_edit_commands(
        &self,
        external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> Vec<TextEditCommandDeclaration> {
        // Only declared commands reach handle_event on the declared
        // backend path; `enabled` gates them per state.
        vec![
            TextEditCommandDeclaration {
                command_id: "insert-text".to_string(),
                kind: TextEditKind::InsertText,
                enabled: true,
            },
            TextEditCommandDeclaration {
                command_id: "delete-backward".to_string(),
                kind: TextEditKind::DeleteBackward,
                enabled: !external.draft.is_empty(),
            },
            TextEditCommandDeclaration {
                command_id: "replace-buffer".to_string(),
                kind: TextEditKind::ReplaceBuffer,
                enabled: true,
            },
        ]
    }
}

impl SlipwayTextInputVisualStylePolicy for DraftInputWidget {
    fn text_input_visual_style(
        &self,
        _external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextInputVisualStyleDeclaration {
        // Explicit colors from the ssot tokens — backend theme defaults
        // are not style authority (checklist "Style Rules").
        TextInputVisualStyleDeclaration::explicit(
            self.id(),
            ink(),              // value
            muted(),            // placeholder
            ink(),              // preedit
            rgb(191, 219, 254), // selection
            rgb(255, 255, 255), // background
            if local.focused {
                accent()
            } else {
                card_border()
            }, // border
            1.0,
            4.0,
            ink(), // icon/caret
        )
    }
}

impl SlipwayTextInputTypographyPolicy for DraftInputWidget {
    fn text_input_typography(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> TextInputTypographyDeclaration {
        TextInputTypographyDeclaration::explicit(self.id(), input_text())
    }
}

impl SlipwayTextUndoRedoPolicy for DraftInputWidget {
    fn text_undo_redo(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> TextUndoRedoEvidence {
        // Honest evidence: this example keeps no undo stack.
        TextUndoRedoEvidence {
            target: self.id(),
            can_undo: false,
            can_redo: false,
            undo_depth: Some(0),
            redo_depth: Some(0),
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayTextFlowPolicy for DraftInputWidget {
    fn text_flow_policy(
        &self,
        external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: &LayoutInput,
    ) -> TextFlowPolicy {
        // SingleLine: a buffer containing a newline would refuse with
        // `view_contract.text_edit_single_line_contains_newline`.
        TextFlowPolicy {
            target: self.id(),
            line_mode: TextLineMode::SingleLine,
            wrap: TextWrapMode::NoWrap,
            line_clamp: Some(1),
            allow_ellipsis: true,
            baseline: None,
            caret_bounds: Vec::new(),
            viewport: Some(TextViewport {
                scroll_x: 0.0,
                scroll_y: 0.0,
                visible_range: Some(TextSelectionRange {
                    anchor: 0,
                    focus: external.draft.chars().count(),
                }),
            }),
        }
    }
}

// The three measurement policies: this example does not require text
// metrics, so the policy declares `required: false` and returns empty
// receipts — an authored widget must never invent measurements.

impl SlipwayTextMeasurementPolicy for DraftInputWidget {
    fn text_measurement_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: &LayoutInput,
    ) -> TextMeasurementPolicyDeclaration {
        TextMeasurementPolicyDeclaration {
            target: self.id(),
            required: false,
            purposes: Vec::new(),
            requests: Vec::new(),
            cache_policies: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn text_measurement_evidence<P>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
        _provider: &mut P,
    ) -> TextMeasurementEvidence
    where
        P: SlipwayTextMetricProvider,
    {
        TextMeasurementEvidence {
            target: self.id(),
            policy: self.text_measurement_policy(external, local, input),
            receipts: Vec::new(),
            cache: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayTextMeasurementCachePolicy for DraftInputWidget {
    fn text_measurement_cache_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: &LayoutInput,
    ) -> Vec<TextMeasurementCachePolicyDeclaration> {
        Vec::new()
    }
}

impl SlipwayCachedTextMeasurementPolicy for DraftInputWidget {
    fn cached_text_measurement_evidence<P, C>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
        provider: &mut P,
        _cache: &mut C,
    ) -> TextMeasurementEvidence
    where
        P: SlipwayTextMetricProvider,
        C: SlipwayTextMeasurementCache,
    {
        SlipwayTextMeasurementPolicy::text_measurement_evidence(
            self, external, local, input, provider,
        )
    }
}

reserved_policy_defaults!(DraftInputWidget);

// ---------------------------------------------------------------------------
// Overlay widget
// ---------------------------------------------------------------------------

impl SlipwayView for OverlayWidget {
    fn initial_local_state(&self) -> Self::LocalState {
        OverlayLocal {
            offset: ssot::overlay_default_offset(),
            dragging: false,
            drag_anchor: Point { x: 0.0, y: 0.0 },
            roam_offset: ssot::overlay_roam_default_offset(),
            roam_dragging: false,
            roam_anchor: Point { x: 0.0, y: 0.0 },
            feed_rows: 0,
        }
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: slipway::LayoutOutputBuilder,
    ) -> LayoutOutput {
        card_layout(&input, OVERLAY_CARD_HEIGHT, output)
    }

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        let bounds = layout.bounds().into_rect();
        let width = bounds.size.width;
        let band = overlay_feed_band(width);
        let offset_y = overlay_feed_offset_y(local.feed_rows);

        let mut ops = card_chrome("overlay", bounds);
        ops.push(header_op(
            bounds,
            "Feed + overlays (purple clamps, amber roams; wheel anywhere)".to_string(),
        ));

        // The feed behind the panels: same shifted-paint discipline as the
        // list, same constants as the scroll declaration below. Rows
        // outside the band are CULLED, not just clipped: this view
        // declares overflow bounds (for the roaming panel below), which
        // hardens admission's paint containment from a warning into
        // `view_contract.paint_bounds_outside_overflow_bounds` — and the
        // check measures each op's UNCLIPPED bounds, so off-band rows must
        // not be painted at all.
        let mut feed = Vec::new();
        for index in 0..OVERLAY_FEED_ROW_COUNT {
            let row = Rect {
                origin: Point {
                    x: ssot::OVERLAY_INSET_X,
                    y: OVERLAY_FEED_TOP + index as f32 * OVERLAY_FEED_ROW_STEP - offset_y,
                },
                size: Size {
                    width: (width - 2.0 * ssot::OVERLAY_INSET_X).max(1.0),
                    height: 18.0,
                },
            };
            if ssot::intersect_rect(row, band).is_none() {
                continue;
            }
            feed.push(PaintOp::styled_text(
                row,
                format!("feed item {}", index + 1),
                if index % 2 == 0 { ink() } else { muted() },
                row_text(),
            ));
        }
        ops.push(clipped("overlay-feed", band, feed));

        // PATTERN: pointer-opaque, wheel-transparent overlay
        // (docs/public/api/routing-and-scroll.md "Wheel-Transparent
        // Overlays"). `keyed_layer` layers default to pointer-OPAQUE
        // (clicks under the panel are occluded); the explicit
        // `with_wheel_transparency(PassThrough)` opens ONLY the wheel
        // channel, so wheeling over the panel scrolls the feed behind it.
        // Omitting the call would make the panel a wheel black hole — the
        // wheel channel follows pointer opacity by default.
        let panel = overlay_panel_rect(local.offset);
        let titlebar = overlay_titlebar_rect(local.offset);
        let panel_ops = vec![
            fill("overlay-panel-bg", panel, rgb(255, 255, 255)),
            stroke("overlay-panel-outline", panel, overlay_border(), 1.5),
            fill("overlay-titlebar", titlebar, overlay_titlebar_background()),
            PaintOp::styled_text(
                Rect {
                    origin: Point {
                        x: titlebar.origin.x + 8.0,
                        y: titlebar.origin.y + 4.0,
                    },
                    size: Size {
                        width: titlebar.size.width - 16.0,
                        height: 14.0,
                    },
                },
                if local.dragging {
                    "dragging…"
                } else {
                    "drag me"
                }
                .to_string(),
                overlay_title_ink(),
                row_text(),
            ),
            // Projection of app state written by the OTHER widgets: list
            // clicks and input edits arrive here through the reducer only
            // (communication.rs) — never by reading sibling local state.
            PaintOp::styled_text(
                Rect {
                    origin: Point {
                        x: panel.origin.x + 8.0,
                        y: panel.origin.y + OVERLAY_TITLEBAR_HEIGHT + 6.0,
                    },
                    size: Size {
                        width: panel.size.width - 16.0,
                        height: 32.0,
                    },
                },
                format!(
                    "selected: {} | draft: {}",
                    external
                        .selected_note
                        .map_or("none".to_string(), |index| format!("note {}", index + 1)),
                    external.draft,
                ),
                ink(),
                row_text(),
            ),
        ];
        ops.push(
            PaintOp::keyed_layer(
                PaintLayerKey::ordered(OVERLAY_Z_INDEX, OVERLAY_LAYER_ORDER),
                panel_ops,
            )
            .with_layer_id("authored.overlay:panel")
            // Clip the layer to the panel rect. Besides being the honest
            // geometry, the clip is LOAD-BEARING for iced presentation:
            // clipped groups (the feed above) composite as their own
            // backend sub-layers, and an UNclipped explicit layer's fills
            // draw into the parent layer underneath them — the feed text
            // would bleed through the panel. Clipping the layer gives it
            // its own composited sub-layer above the feed's.
            .with_layer_clip(ClipDeclaration {
                id: Some("authored.overlay:panel:clip".to_string()),
                bounds: panel,
                path: None,
            })
            .with_wheel_transparency(PaintInputTransparency::PassThrough),
        );

        // PATTERN: the SECOND overlay panel — the ROAMING drag pattern.
        // Contrast with the clamped panel above: the clamped one is the
        // simplest contract-safe shape (its drag clamp keeps it inside the
        // feed band, which layout already contains); this one declares
        // overflow bounds (see view_definition below) so its paint AND its
        // titlebar hit region may leave the band and even the card's
        // layout bounds — see the admission stress harness for the extreme
        // version. The offset is re-clamped against the CURRENT card width
        // and the PROJECTED live window (`external.viewport` — the same
        // value view_definition and the drag handler read, so paint, hit
        // region, and clamp cannot diverge) with the same
        // `overlay_roaming_clamped_offset` the drag handler uses, so every
        // reachable frame stays inside the declared allowance. Distinct z
        // (OVERLAY_ROAM_Z_INDEX = 11) fronts the clamped panel where they
        // overlap; the layer stays pointer-opaque + wheel-transparent like
        // the clamped one.
        let roam_offset =
            overlay_roaming_clamped_offset(width, external.viewport, local.roam_offset);
        let roam_panel = overlay_roam_panel_rect(roam_offset);
        let roam_titlebar = overlay_roam_titlebar_rect(roam_offset);
        let roam_ops = vec![
            fill("overlay-roam-bg", roam_panel, rgb(255, 251, 235)),
            stroke("overlay-roam-outline", roam_panel, rgb(217, 119, 6), 1.5),
            fill("overlay-roam-titlebar", roam_titlebar, rgb(254, 243, 199)),
            // PATTERN: declared text alignment (docs/public/api/backends.md
            // "Text Wrap and Alignment"). Declare the FULL control rect
            // (the whole titlebar) as the text op's bounds and let
            // `.centered()` anchor the label inside it on both backends —
            // do not shrink the rect with hand-computed insets from
            // estimated glyph widths (the NC-14 anti-pattern: the guess
            // drifts off-center wherever it is wrong). Unspecified
            // alignment = the historical top-left anchoring (see the
            // clamped panel's title above).
            PaintOp::styled_text(
                roam_titlebar,
                if local.roam_dragging {
                    "roaming…"
                } else {
                    "roam me"
                }
                .to_string(),
                rgb(146, 64, 14),
                row_text().centered(),
            ),
            PaintOp::styled_text(
                Rect {
                    origin: Point {
                        x: roam_panel.origin.x + 8.0,
                        y: roam_panel.origin.y + OVERLAY_ROAM_TITLEBAR_HEIGHT + 4.0,
                    },
                    size: Size {
                        width: roam_panel.size.width - 16.0,
                        height: 20.0,
                    },
                },
                "leaves the band".to_string(),
                muted(),
                row_text(),
            ),
        ];
        ops.push(
            PaintOp::keyed_layer(
                PaintLayerKey::ordered(OVERLAY_ROAM_Z_INDEX, OVERLAY_ROAM_LAYER_ORDER),
                roam_ops,
            )
            .with_layer_id("authored.overlay:roam")
            // Same load-bearing clip rationale as the clamped panel's
            // layer: an unclipped explicit layer's fills draw into the
            // parent iced layer underneath.
            .with_layer_clip(ClipDeclaration {
                id: Some("authored.overlay:roam:clip".to_string()),
                bounds: roam_panel,
                path: None,
            })
            .with_wheel_transparency(PaintInputTransparency::PassThrough),
        );
        ops
    }

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        observations(
            self.id(),
            format!(
                "draft: {} selected: {:?}",
                external.draft, external.selected_note
            ),
            format!("{local:?}"),
        )
    }
}

impl SlipwayViewDefinition for OverlayWidget {
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let (frame, layout) = slipway::layout_view_definition(self, external, local, input);
        let address = self_slot(self.id());

        let width = layout.bounds().as_rect().size.width;

        // PATTERN: drag surface with PointerCaptureIntent::DuringDrag.
        // The hit region is re-declared each frame at the CURRENT panel
        // offset (declarations are per-frame snapshots, not persistent
        // objects). The capture intent keeps Move/Release routed here
        // while the button is held, even outside the titlebar.
        //
        // TWO drag patterns, side by side, pick one when copying:
        //  * CLAMPED (this region): simplest contract-safe — the drag
        //    clamp keeps the titlebar inside layout bounds, so no overflow
        //    declaration is needed.
        //  * ROAMING (second region below): declare overflow bounds so hit
        //    regions may leave layout bounds — see admission for the
        //    stress version.
        let mut hit_regions = vec![hit_region_from_pointer_capability(
            self,
            external,
            local,
            overlay_drag_region_id(),
            address.clone(),
            TargetLocalRect::new(overlay_titlebar_rect(local.offset)),
            PointerEventCoordinateSpace::TargetLocal,
            // Same z as the panel's paint layer: pointer selection must
            // agree with the visible stacking.
            HitRegionOrder {
                z_index: OVERLAY_Z_INDEX,
                paint_order: 1,
                traversal_order: 1,
            },
            Some(overlay_drag_region_id().as_str().to_string()),
            if local.dragging {
                CursorCapability::Grabbing
            } else {
                CursorCapability::Grab
            },
            true,
            PointerCaptureIntent::DuringDrag,
        )];

        // The ROAMING panel's titlebar. Its bounds may leave the card's
        // layout bounds; that is admissible ONLY because this view
        // declares matching overflow bounds below (otherwise:
        // `view_contract.hit_bounds_outside_layout`). Distinct z/order —
        // the two overlay titlebars CAN overlap, and overlapping enabled
        // hit regions with identical orders refuse admission with
        // `view_contract.ambiguous_hit_overlap`.
        let roam_offset =
            overlay_roaming_clamped_offset(width, external.viewport, local.roam_offset);
        hit_regions.push(hit_region_from_pointer_capability(
            self,
            external,
            local,
            overlay_roam_drag_region_id(),
            address.clone(),
            TargetLocalRect::new(overlay_roam_titlebar_rect(roam_offset)),
            PointerEventCoordinateSpace::TargetLocal,
            HitRegionOrder {
                z_index: OVERLAY_ROAM_Z_INDEX,
                paint_order: 2,
                traversal_order: 2,
            },
            Some(overlay_roam_drag_region_id().as_str().to_string()),
            if local.roam_dragging {
                CursorCapability::Grabbing
            } else {
                CursorCapability::Grab
            },
            true,
            PointerCaptureIntent::DuringDrag,
        ));

        // `_with_order` even for a single scroll region: this region sits
        // UNDER the overlay's hit region, the documented rule-of-thumb
        // trigger for explicit ordering
        // (docs/public/api/routing-and-scroll.md "Nesting Order").
        let mut scroll_regions = Vec::new();
        let terminal_region_index = scroll_regions.len();
        scroll_regions.push(scroll_region_from_scrollable_capability_with_order(
            self,
            external,
            local,
            &layout,
            Some(overlay_feed_region_id()),
            address,
            true,
            HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 1,
            },
        ));

        let paint = self.paint(external, local, &layout);
        let mut view = assemble_view(
            self.id(),
            frame,
            layout,
            paint,
            hit_regions,
            Vec::new(),
            scroll_regions,
            Some(terminal_region_index),
        );
        // PATTERN: the overflow-bounds declaration that makes the roaming
        // panel legal (docs/public/api/routing-and-scroll.md "Overlay
        // Drag Patterns"). `with_overflow_bounds` switches admission's
        // containment checks from layout bounds to THIS rect for paint,
        // hit, focus, and scroll declarations — everything painted or
        // declared by this widget must stay inside it
        // (`view_contract.*_outside_overflow_bounds` otherwise). The rect
        // comes from the same `overlay_overflow_bounds` the roaming drag
        // clamp uses — card width plus the PROJECTED live window
        // (`external.viewport`, Step 212): declaration and clamp MUST
        // agree or the first out-of-allowance drag refuses admission.
        view.paint_order =
            view.paint_order
                .with_overflow_bounds(TargetLocalRect::new(overlay_overflow_bounds(
                    width,
                    external.viewport,
                )));
        view
    }
}

impl SlipwayScrollBehaviorPolicy for OverlayWidget {
    fn scroll_behavior_policy(
        &self,
        _external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ScrollBehaviorPolicyDeclaration {
        let band = overlay_feed_band(input.viewport.size.width.max(1.0));
        let content = Rect {
            origin: band.origin,
            size: Size {
                width: band.size.width,
                height: OVERLAY_FEED_CONTENT_HEIGHT.max(band.size.height),
            },
        };
        ScrollBehaviorPolicyDeclaration {
            target: self.id(),
            region_id: Some(overlay_feed_region_id()),
            address: None,
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            extent: content.size,
            viewport: TargetLocalRect::new(band),
            content_bounds: TargetLocalRect::new(content),
            offset: Point {
                x: 0.0,
                y: overlay_feed_offset_y(local.feed_rows),
            },
            consumption: ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayWheelRoutingPolicy for OverlayWidget {
    fn wheel_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _region: &PresentationRegionId,
    ) -> WheelRoutingPolicyDeclaration {
        // NearestScrollable default; breadcrumb on NestedFeedWidget.
        WheelRoutingPolicyDeclaration {
            target: self.id(),
            routing: WheelRouting::NearestScrollable,
            modifiers: None,
            diagnostics: Vec::new(),
        }
    }
}

reserved_policy_defaults!(OverlayWidget);

// ---------------------------------------------------------------------------
// Nested feed widget
// ---------------------------------------------------------------------------

impl SlipwayView for NestedFeedWidget {
    fn initial_local_state(&self) -> Self::LocalState {
        NestedLocal::default()
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: slipway::LayoutOutputBuilder,
    ) -> LayoutOutput {
        card_layout(&input, NESTED_CARD_HEIGHT, output)
    }

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        let bounds = layout.bounds().into_rect();
        let width = bounds.size.width;
        let band = nested_outer_band(width);
        let outer_offset = nested_outer_offset_y(local.outer_rows);

        let mut ops = card_chrome("nested", bounds);
        ops.push(header_op(
            bounds,
            format!(
                "Nested scroll — outer {} inner {:?} — pick: {}",
                local.outer_rows,
                local.inner_rows,
                external
                    .selected_inner_item
                    .map_or("none".to_string(), |row| format!("p1 item {}", row + 1)),
            ),
        ));

        // Outer content: panels shifted by the outer offset, clipped to
        // the outer band; inner rows shifted by their own offsets, clipped
        // to each panel's VISIBLE field. All geometry derives from the
        // NESTED_* constants — the same functions the scroll declarations
        // below use (the MUST-agree contract in ssot.rs).
        let mut outer_ops = Vec::new();
        for panel in 0..NESTED_PANEL_COUNT {
            let panel_top = nested_panel_top_in_card(panel, outer_offset);
            let panel_rect = Rect {
                origin: Point {
                    x: NESTED_INSET_X,
                    y: panel_top,
                },
                size: Size {
                    width: (width - 2.0 * NESTED_INSET_X).max(1.0),
                    height: NESTED_PANEL_HEIGHT,
                },
            };
            outer_ops.push(fill("nested-panel-bg", panel_rect, rgb(255, 255, 255)));
            outer_ops.push(stroke(
                "nested-panel-outline",
                panel_rect,
                card_border(),
                1.0,
            ));
            outer_ops.push(PaintOp::styled_text(
                Rect {
                    origin: Point {
                        x: panel_rect.origin.x + 8.0,
                        y: panel_top + 2.0,
                    },
                    size: Size {
                        width: panel_rect.size.width - 16.0,
                        height: NESTED_PANEL_HEADER_HEIGHT - 4.0,
                    },
                },
                format!(
                    "panel {} — rows {}/{}",
                    panel + 1,
                    local.inner_rows[panel],
                    NESTED_INNER_MAX_SCROLL_ROWS
                ),
                ink(),
                row_text(),
            ));

            let field_full = nested_field_rect_in_card(panel, outer_offset, width);
            if let Some(visible) = nested_visible_field_rect(panel, outer_offset, width) {
                let inner_offset =
                    nested_inner_offset_y(local.inner_rows[panel], visible.size.height);
                let mut inner_rows = Vec::new();
                for row in 0..NESTED_INNER_ROW_COUNT {
                    // PATTERN: row selection inside a SCROLLED region — the
                    // selectable panel's rows derive from the ONE shared
                    // rect function (`nested_inner_row_rect_in_card`, both
                    // offsets applied) that the hit regions and the pointer
                    // inverse also use; the highlight is the same
                    // fill-behind-text shape as the notes list, driven by
                    // REDUCED app state (`selected_inner_item`), never by
                    // local pointer bookkeeping.
                    let selectable = panel == NESTED_SELECTABLE_PANEL;
                    let row_rect = if selectable {
                        nested_inner_row_rect_in_card(row, outer_offset, inner_offset, width)
                    } else {
                        Rect {
                            origin: Point {
                                x: field_full.origin.x + 8.0,
                                y: field_full.origin.y + row as f32 * NESTED_INNER_ROW_STEP
                                    - inner_offset,
                            },
                            size: Size {
                                width: field_full.size.width - 16.0,
                                height: 12.0,
                            },
                        }
                    };
                    let selected = selectable && external.selected_inner_item == Some(row);
                    if selected {
                        inner_rows.push(fill("nested-row-selected", row_rect, rgb(219, 234, 254)));
                    }
                    inner_rows.push(PaintOp::styled_text(
                        row_rect,
                        format!("p{} item {}", panel + 1, row + 1),
                        if selected {
                            accent()
                        } else if row % 2 == 0 {
                            ink()
                        } else {
                            muted()
                        },
                        row_text(),
                    ));
                }
                outer_ops.push(clipped(
                    &format!("nested-inner-{panel}"),
                    visible,
                    inner_rows,
                ));
            }
        }
        // Tail marker at the outer content's end — proves outer travel.
        outer_ops.push(PaintOp::styled_text(
            Rect {
                origin: Point {
                    x: NESTED_INSET_X,
                    y: NESTED_OUTER_TOP + NESTED_OUTER_CONTENT_HEIGHT - 14.0 - outer_offset,
                },
                size: Size {
                    width: (width - 2.0 * NESTED_INSET_X).max(1.0),
                    height: 12.0,
                },
            },
            "— outer end —".to_string(),
            muted(),
            row_text(),
        ));
        ops.push(clipped("nested-outer", band, outer_ops));
        ops
    }

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        observations(
            self.id(),
            format!("selected-inner-item: {:?}", external.selected_inner_item),
            format!("{local:?}"),
        )
    }
}

impl SlipwayViewDefinition for NestedFeedWidget {
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let (frame, layout) = slipway::layout_view_definition(self, external, local, input);
        let width = layout.bounds().as_rect().size.width;
        let outer_offset = nested_outer_offset_y(local.outer_rows);
        let address = self_slot(self.id());

        // PATTERN: nested scroll regions with `_with_order`
        // (docs/public/api/routing-and-scroll.md "Nesting Order"). The
        // outer region and the inner panels OVERLAP, so every region gets
        // a distinct HitRegionOrder — inners in front (z 1) so the
        // pointed-at inner wins under the default NearestScrollable
        // routing; identical orders on overlapping wheel-consuming regions
        // refuse admission with `view_contract.ambiguous_wheel_overlap`.
        let mut scroll_regions = Vec::new();

        // The outer region: geometry comes straight from the scroll
        // policy below (single-declaration surface, no patch needed).
        let terminal_region_index = scroll_regions.len();
        scroll_regions.push(scroll_region_from_scrollable_capability_with_order(
            self,
            external,
            local,
            &layout,
            Some(nested_outer_region_id()),
            address.clone(),
            true,
            HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
        ));

        // Helper-then-patch idiom, and WHY the per-region override is
        // legitimate: `SlipwayScrollBehaviorPolicy` is a
        // single-declaration surface (one geometry per widget), but this
        // widget declares THREE overlapping regions with distinct
        // viewports and offsets. The `_with_order` helper still does the
        // contract wiring worth keeping — region-id resolution, the
        // per-region wheel-routing snapshot, the consumption policy — and
        // the geometry fields are then overridden per region BEFORE the
        // declaration is pushed, so admission validates exactly what is
        // declared. Do not copy this shape for a single-region widget:
        // there the policy itself should return the real geometry (see
        // `SlipwayScrollBehaviorPolicy`'s rustdoc).
        for panel in 0..NESTED_PANEL_COUNT {
            let Some(visible) = nested_visible_field_rect(panel, outer_offset, width) else {
                // Panel scrolled fully out of the outer band: declaring a
                // region with an off-screen viewport would refuse with
                // `view_contract.scroll_viewport_outside_layout`.
                continue;
            };
            let mut declaration = scroll_region_from_scrollable_capability_with_order(
                self,
                external,
                local,
                &layout,
                Some(nested_inner_region_id(panel)),
                address.clone(),
                true,
                HitRegionOrder {
                    z_index: 1,
                    paint_order: panel + 1,
                    traversal_order: panel + 1,
                },
            );
            declaration.viewport = TargetLocalRect::new(visible);
            declaration.content_bounds = TargetLocalRect::new(Rect {
                origin: visible.origin,
                size: Size {
                    width: visible.size.width,
                    height: NESTED_INNER_CONTENT_HEIGHT.max(visible.size.height),
                },
            });
            declaration.offset = Point {
                x: 0.0,
                y: nested_inner_offset_y(local.inner_rows[panel], visible.size.height),
            };
            // PATTERN: declared scroll indicators — the
            // `ScrollRegionDeclaration::indicator` field, set with
            // `with_scroll_indicator` (docs/public/api/routing-and-scroll.md
            // "Scroll Indicators"). BOTH explicit states, pick one when
            // copying:
            //  * panel 0 -> Visible: renders the track/thumb on both
            //    backends whenever content overflows the viewport;
            //  * panel 1 -> Hidden: never renders an indicator (visual
            //    control only — wheel routing is unchanged).
            // The OUTER region above and the list keep the unspecified
            // default (Auto = backend-automatic): explicit when declared,
            // automatic when unspecified.
            declaration = declaration.with_scroll_indicator(if panel == 0 {
                ScrollIndicatorMode::Visible
            } else {
                ScrollIndicatorMode::Hidden
            });
            scroll_regions.push(declaration);
        }

        // PATTERN: per-row hit regions inside a SCROLLED region (the
        // scrolled-hit-region discipline; contrast the list's static band).
        // The rows live under TWO offsets — the outer scroll displaces the
        // panel, the inner scroll shifts the rows — so the hit rects are
        // re-declared each frame from the SAME
        // `nested_inner_row_rect_in_card` the paint uses, and only rows
        // fully inside the panel's VISIBLE field are declared (a region
        // over clipped-away rows would route clicks the user cannot see;
        // one outside layout would refuse with
        // `view_contract.hit_bounds_outside_layout`).
        let mut hit_regions = Vec::new();
        if let Some(visible) =
            nested_visible_field_rect(NESTED_SELECTABLE_PANEL, outer_offset, width)
        {
            let inner_offset = nested_inner_offset_y(
                local.inner_rows[NESTED_SELECTABLE_PANEL],
                visible.size.height,
            );
            for row in 0..NESTED_INNER_ROW_COUNT {
                let row_rect =
                    nested_inner_row_rect_in_card(row, outer_offset, inner_offset, width);
                if row_rect.origin.y < visible.origin.y - 0.5
                    || row_rect.origin.y + row_rect.size.height
                        > visible.origin.y + visible.size.height + 0.5
                {
                    continue;
                }
                hit_regions.push(hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    nested_inner_row_region_id(row),
                    address.clone(),
                    TargetLocalRect::new(row_rect),
                    PointerEventCoordinateSpace::TargetLocal,
                    // Distinct orders per row (ambiguous_hit_overlap
                    // defense). The z MUST front the inner scroll region's
                    // z (1, see the `_with_order` declarations above): a
                    // backend may route the pointer by the COMBINED region
                    // order (egui does), and a clickable row declared UNDER
                    // its own scroll region's z is unreachable there — the
                    // press resolves to the scroll region and no click
                    // arrives (Step 211 live finding; same rule as the
                    // overlay titlebar fronting the feed region). z 2 stays
                    // behind the overlay panels (z 10/11) that may roam
                    // over this card.
                    HitRegionOrder {
                        z_index: 2,
                        paint_order: row,
                        traversal_order: row,
                    },
                    Some(nested_inner_row_region_id(row).as_str().to_string()),
                    CursorCapability::Pointer,
                    true,
                    PointerCaptureIntent::OnPress,
                ));
            }
        }

        let paint = self.paint(external, local, &layout);
        assemble_view(
            self.id(),
            frame,
            layout,
            paint,
            hit_regions,
            Vec::new(),
            scroll_regions,
            Some(terminal_region_index),
        )
    }
}

impl SlipwayScrollBehaviorPolicy for NestedFeedWidget {
    fn scroll_behavior_policy(
        &self,
        _external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ScrollBehaviorPolicyDeclaration {
        // Returns the OUTER region's geometry; the inner declarations
        // override theirs per region (see view_definition above).
        let band = nested_outer_band(input.viewport.size.width.max(1.0));
        let content = Rect {
            origin: band.origin,
            size: Size {
                width: band.size.width,
                height: NESTED_OUTER_CONTENT_HEIGHT.max(band.size.height),
            },
        };
        ScrollBehaviorPolicyDeclaration {
            target: self.id(),
            region_id: Some(nested_outer_region_id()),
            address: None,
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            extent: content.size,
            viewport: TargetLocalRect::new(band),
            content_bounds: TargetLocalRect::new(content),
            offset: Point {
                x: 0.0,
                y: nested_outer_offset_y(local.outer_rows),
            },
            consumption: ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayWheelRoutingPolicy for NestedFeedWidget {
    fn wheel_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        region: &PresentationRegionId,
    ) -> WheelRoutingPolicyDeclaration {
        // ROUTING BREADCRUMB (docs/public/api/routing-and-scroll.md
        // "Wheel Routing Modes"). This policy is consulted once per region
        // at declaration time with the region's resolved id, so ONE impl
        // can author a different mode per region:
        //
        //     routing: if region.as_str().ends_with(":outer") {
        //         WheelRouting::SelfFirst
        //     } else {
        //         WheelRouting::NearestScrollable
        //     },
        //
        // This example deliberately authors NOTHING non-default. The
        // NearestScrollable default is usually right: the region the user
        // points at scrolls first, chains to the outer at its limit, and
        // is reclaimed on the way back. An authored SelfFirst outer was
        // live-reverted (commit 902f99eae) because wheeling over an inner
        // panel moved the outer — users read that as broken. Per-event
        // dynamic routing is unsupported by construction: the returned
        // mode is frozen into the declaration for the frame, so
        // delta-dependent logic here can never take effect.
        let _ = region;
        WheelRoutingPolicyDeclaration {
            target: self.id(),
            routing: WheelRouting::NearestScrollable,
            modifiers: None,
            diagnostics: Vec::new(),
        }
    }
}

reserved_policy_defaults!(NestedFeedWidget);

// ---------------------------------------------------------------------------
// Shared view assembly
// ---------------------------------------------------------------------------

fn observations(id: WidgetId, external: String, local: String) -> Vec<StateObservation> {
    let slot = Some(WidgetSlotAddress::new(id.clone(), 0));
    vec![
        StateObservation {
            target: id.clone(),
            slot: slot.clone(),
            name: "external".to_string(),
            value: external,
        },
        StateObservation {
            target: id,
            slot,
            name: "local".to_string(),
            value: local,
        },
    ]
}

/// One assembly point so every widget's ViewDefinition carries the same
/// shape: source-order paint (the overlay fronts itself via its keyed
/// layer, not via paint-order tricks), no semantic/probe extras.
fn assemble_view(
    target: WidgetId,
    frame: FrameIdentity,
    layout: LayoutOutput,
    paint: Vec<PaintOp>,
    hit_regions: Vec<HitRegionDeclaration>,
    focus_regions: Vec<FocusRegionDeclaration>,
    scroll_regions: Vec<ScrollRegionDeclaration>,
    terminal_region_index: Option<usize>,
) -> ViewDefinition {
    let diagnostics = layout.diagnostics().to_vec();
    let mut view = ViewDefinition {
        target: target.clone(),
        frame,
        layout,
        paint,
        paint_order: PaintOrderDeclaration::source_order(target),
        hit_regions,
        focus_regions,
        scroll_regions,
        wheel_traversal_boundary: Default::default(),
        semantic_slots: Vec::new(),
        probe_metadata: Vec::new(),
        diagnostics,
    };
    view.wheel_traversal_boundary.terminal_region_index = terminal_region_index;
    view
}
