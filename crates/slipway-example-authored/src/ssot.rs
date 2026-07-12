//! SSOT declarations only (`docs/public/authoring-layout.md`).
//! Defines stable identity (widget/app/region ids), the app's semantic
//! source-of-truth state, capability declarations, design tokens, and the
//! shared geometry constants every other module must derive from.
//! No event mutation, sibling communication, backend drawing, or runner
//! code lives here.

use slipway::prelude::*;

use crate::view::{InputLocal, ListLocal, NestedLocal, OverlayLocal};

// ---------------------------------------------------------------------------
// App source-of-truth state
// ---------------------------------------------------------------------------

/// Semantic app state (the checklist's "source-of-truth data"). Widgets
/// receive it read-only as `ExternalState`; the ONLY write path is the app
/// reducer in `communication.rs` consuming typed messages. Widget-local
/// presentation state (scroll offsets, drag anchors, focus flags) is NOT
/// here — it lives in the per-widget local-state structs in `view.rs`.
#[derive(Clone, Debug, PartialEq)]
pub struct ShowcaseState {
    /// Fixed note titles rendered by the list widget.
    pub notes: Vec<String>,
    /// Set by `ShowcaseMessage::SelectNote` (list row click); projected
    /// into the list highlight AND the overlay panel body — the
    /// child A -> reducer -> child B communication shape.
    pub selected_note: Option<usize>,
    /// Set by `ShowcaseMessage::ReplaceDraft` (text-input edits); the text
    /// widget's buffer policy snapshots this field, and the overlay panel
    /// projects it too.
    pub draft: String,
    /// Set by `ShowcaseMessage::SelectInnerItem` (row click INSIDE the
    /// nested Visible inner panel) — the notes-list selection shape
    /// repeated inside a scrolled nested region: click -> typed message ->
    /// reducer -> highlight paint on the next frame.
    pub selected_inner_item: Option<usize>,
    /// PATTERN: platform-truth projection
    /// (`SlipwayApp::project_frame_viewport`). The LIVE window size,
    /// written by the runtime-invoked hook in `communication.rs` — the one
    /// sanctioned external-state writer besides the reducer. Widgets read
    /// it wherever window-derived geometry must agree across paint, hit
    /// declarations, AND event handlers (the roaming overlay's allowance
    /// and drag clamp below): `ViewDefinitionInput.frame.viewport` reaches
    /// only `view_definition`, so deriving window geometry there alone
    /// silently splits the MUST-agree chain. `Size::default()` (0x0) means
    /// "no frame presented yet"; every consumer unions it with authored
    /// constants so the degenerate value stays admissible.
    pub viewport: Size,
}

impl Default for ShowcaseState {
    fn default() -> Self {
        Self {
            notes: (1..=LIST_NOTE_COUNT)
                .map(|index| format!("note {index}"))
                .collect(),
            selected_note: None,
            draft: "hello".to_string(),
            selected_inner_item: None,
            viewport: Size {
                width: 0.0,
                height: 0.0,
            },
        }
    }
}

// ---------------------------------------------------------------------------
// Stable identity: widget ids and presentation-region ids
// ---------------------------------------------------------------------------

pub fn app_id() -> WidgetId {
    WidgetId::from("authored.app")
}

pub fn list_id() -> WidgetId {
    WidgetId::from("authored.list")
}

pub fn input_id() -> WidgetId {
    WidgetId::from("authored.input")
}

pub fn overlay_id() -> WidgetId {
    WidgetId::from("authored.overlay")
}

pub fn nested_id() -> WidgetId {
    WidgetId::from("authored.nested")
}

/// The app-level PAGE scroll region (declared by
/// `ShowcaseApp::app_scroll_regions` in communication.rs). Targets the
/// APP id, so its Wheel/Scroll events route to the app's own
/// `handle_event`, not to any widget.
pub fn page_scroll_region_id() -> PresentationRegionId {
    PresentationRegionId::from("authored.app:page-scroll")
}

pub fn list_scroll_region_id() -> PresentationRegionId {
    PresentationRegionId::from("authored.list:scroll")
}

pub fn list_row_region_id(index: usize) -> PresentationRegionId {
    PresentationRegionId::from(format!("authored.list:row-{index}"))
}

pub fn list_focus_region_id() -> PresentationRegionId {
    PresentationRegionId::from("authored.list:focus")
}

pub fn input_focus_region_id() -> PresentationRegionId {
    PresentationRegionId::from("authored.input:focus")
}

pub fn overlay_feed_region_id() -> PresentationRegionId {
    PresentationRegionId::from("authored.overlay:feed")
}

pub fn overlay_drag_region_id() -> PresentationRegionId {
    PresentationRegionId::from("authored.overlay:drag")
}

pub fn overlay_roam_drag_region_id() -> PresentationRegionId {
    PresentationRegionId::from("authored.overlay:roam-drag")
}

pub fn nested_outer_region_id() -> PresentationRegionId {
    PresentationRegionId::from("authored.nested:outer")
}

pub fn nested_inner_region_id(index: usize) -> PresentationRegionId {
    PresentationRegionId::from(format!("authored.nested:inner-{index}"))
}

/// Per-row hit-region id inside the SELECTABLE inner panel
/// (`NESTED_SELECTABLE_PANEL`). Same id scheme as the list's rows.
pub fn nested_inner_row_region_id(row: usize) -> PresentationRegionId {
    PresentationRegionId::from(format!("authored.nested:panel0-row-{row}"))
}

/// Region-id -> nested region selector: `None` = the outer region,
/// `Some(i)` = inner panel `i`. The wheel/scroll handlers in
/// `internal_logic.rs` are REGION-DRIVEN: which id arrives is decided
/// entirely by the declared routing (`docs/public/api/routing-and-scroll.md`,
/// "Chaining"), so the handler encodes no selection-order assumptions.
pub fn nested_region_selector(region_id: &PresentationRegionId) -> Option<Option<usize>> {
    let text = region_id.as_str();
    if text.ends_with(":outer") {
        return Some(None);
    }
    for index in 0..NESTED_PANEL_COUNT {
        if text.ends_with(&format!(":inner-{index}")) {
            return Some(Some(index));
        }
    }
    None
}

// ---------------------------------------------------------------------------
// The four authored widgets: identity, capabilities, topology
// ---------------------------------------------------------------------------

/// Scrollable, focusable note list (rows are individually clickable).
#[derive(Clone, Debug, PartialEq)]
pub struct NoteListWidget;

/// Single-line text input editing `ShowcaseState::draft`.
#[derive(Clone, Debug, PartialEq)]
pub struct DraftInputWidget;

/// Scrollable feed with a draggable, pointer-opaque, wheel-transparent
/// overlay panel floating above it.
#[derive(Clone, Debug, PartialEq)]
pub struct OverlayWidget;

/// Outer scroll region containing two independently scrollable inner
/// panels (default `NearestScrollable` routing + at-limit chaining).
#[derive(Clone, Debug, PartialEq)]
pub struct NestedFeedWidget;

impl SlipwayWidgetTypes for NoteListWidget {
    type ExternalState = ShowcaseState;
    type LocalState = ListLocal;
    type AppMessage = crate::communication::ShowcaseMessage;
}

impl SlipwayWidgetTypes for DraftInputWidget {
    type ExternalState = ShowcaseState;
    type LocalState = InputLocal;
    type AppMessage = crate::communication::ShowcaseMessage;
}

impl SlipwayWidgetTypes for OverlayWidget {
    type ExternalState = ShowcaseState;
    type LocalState = OverlayLocal;
    type AppMessage = crate::communication::ShowcaseMessage;
}

impl SlipwayWidgetTypes for NestedFeedWidget {
    type ExternalState = ShowcaseState;
    type LocalState = NestedLocal;
    type AppMessage = crate::communication::ShowcaseMessage;
}

// Capability declarations gate admission: every declared capability MUST be
// matched by at least one enabled declaration of the corresponding kind in
// the view, or admission refuses (e.g. `WheelInput` without an enabled
// scroll region refuses with
// `view_contract.scroll_capability_missing_scroll_region`). Catalog of
// every refusal code: docs/public/api/diagnostics.md.

impl SlipwaySsot for NoteListWidget {
    fn id(&self) -> WidgetId {
        list_id()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::PointerInput,
            Capability::WheelInput,
            Capability::FocusInput,
            Capability::ScrollRegionPresentation,
            Capability::Paint,
        ]
    }

    fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
        TopologyNode::leaf(self.id())
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

impl SlipwaySsot for DraftInputWidget {
    fn id(&self) -> WidgetId {
        input_id()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::TextInput,
            Capability::FocusInput,
            Capability::Paint,
        ]
    }

    fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
        TopologyNode::leaf(self.id())
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

impl SlipwaySsot for OverlayWidget {
    fn id(&self) -> WidgetId {
        overlay_id()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::PointerInput,
            Capability::WheelInput,
            Capability::ScrollRegionPresentation,
            Capability::Paint,
        ]
    }

    fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
        TopologyNode::leaf(self.id())
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

impl SlipwaySsot for NestedFeedWidget {
    fn id(&self) -> WidgetId {
        nested_id()
    }

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            // PointerInput backs the per-row hit regions inside the
            // SELECTABLE inner panel (view.rs); a hit region declared
            // without the capability refuses admission.
            Capability::PointerInput,
            Capability::WheelInput,
            Capability::ScrollRegionPresentation,
            Capability::Paint,
        ]
    }

    fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
        TopologyNode::leaf(self.id())
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

// ---------------------------------------------------------------------------
// App card layout constants (consumed by communication.rs layout_plan and
// view.rs per-card layout)
// ---------------------------------------------------------------------------

pub const CARD_MARGIN_X: f32 = 16.0;
pub const CARD_TOP: f32 = 16.0;
pub const CARD_GAP: f32 = 10.0;
pub const CARD_MIN_WIDTH: f32 = 260.0;
/// The FIXED-WIDTH alternative (not the live behavior): clamp the card
/// column to this constant instead of filling the window. The live
/// `layout_plan` derives the width from `LayoutInput.viewport` (fill the
/// window minus `CARD_MARGIN_X` each side — see communication.rs); kept
/// as the teaching contrast and as the width fallback for handlers whose
/// events carry no `target_bounds`.
pub const CARD_MAX_WIDTH: f32 = 560.0;
/// One wheel notch of PAGE scroll, in pixels (the page region is not
/// row-quantized; the app handler clamps the offset to the exact declared
/// travel using the projected `ShowcaseState::viewport`, so no drift or
/// dead-wheel band can accumulate).
pub const PAGE_SCROLL_STEP: f32 = 48.0;

/// Maximum page scroll offset for the CURRENT window: declared content
/// (the full card column, `APP_ROOT_HEIGHT`) minus the live viewport
/// height. MUST agree with the page region declaration in
/// communication.rs — the wheel handler clamps the stored offset with
/// this same function, or the declared offset would refuse admission with
/// `view_contract.scroll_offset_invalid`.
pub fn page_max_scroll_y(viewport: Size) -> f32 {
    (APP_ROOT_HEIGHT - viewport.height).max(0.0)
}
pub const LIST_CARD_HEIGHT: f32 = 120.0;
pub const INPUT_CARD_HEIGHT: f32 = 72.0;
pub const OVERLAY_CARD_HEIGHT: f32 = 160.0;
pub const NESTED_CARD_HEIGHT: f32 = 176.0;

/// Root-local top of the OVERLAY card (list + input cards above it, with
/// the standard top margin and gaps). MUST agree with the stacking order
/// `communication.rs::layout_plan` produces — the roaming allowance below
/// translates the root view area into overlay-card-local space with this.
pub const OVERLAY_CARD_ROOT_TOP: f32 =
    CARD_TOP + LIST_CARD_HEIGHT + CARD_GAP + INPUT_CARD_HEIGHT + CARD_GAP; // 228

/// Height of the four stacked cards' column (top margin + four heights +
/// three gaps + bottom margin). MUST agree with
/// `communication.rs::layout_plan`'s accumulated height. This is also the
/// PAGE scroll region's content height; the live plan declares
/// `max(APP_ROOT_HEIGHT, viewport height)` as the root bounds (the
/// min-height-of-the-window pattern), so the page region is declared —
/// and its Auto indicator appears — exactly when this column exceeds the
/// window.
pub const APP_ROOT_HEIGHT: f32 = CARD_TOP
    + LIST_CARD_HEIGHT
    + CARD_GAP
    + INPUT_CARD_HEIGHT
    + CARD_GAP
    + OVERLAY_CARD_HEIGHT
    + CARD_GAP
    + NESTED_CARD_HEIGHT
    + CARD_TOP; // 590

/// Card height per widget id — the single source `layout_plan`
/// (communication.rs) and each widget's own `layout` (view.rs) both read.
/// MUST agree: a drift between the app-planned placement height and the
/// widget-declared layout height silently clips declarations against the
/// smaller box (admission validates regions against the widget's OWN
/// layout, not against the plan).
pub fn card_height_for(id: &WidgetId) -> f32 {
    if *id == list_id() {
        LIST_CARD_HEIGHT
    } else if *id == input_id() {
        INPUT_CARD_HEIGHT
    } else if *id == overlay_id() {
        OVERLAY_CARD_HEIGHT
    } else {
        NESTED_CARD_HEIGHT
    }
}

// ---------------------------------------------------------------------------
// LIST geometry — the shared-constants discipline
// ---------------------------------------------------------------------------
// The painted row labels (view.rs), the declared per-row hit regions
// (view.rs), the scroll-region geometry (view.rs scroll policy), and the
// pointer/wheel math (internal_logic.rs) MUST all derive from these same
// constants and functions. Admission validates hit bounds against layout,
// but NOTHING can validate them against paint: a drift here silently
// routes clicks to rows the user does not see (the Step-198/199 bug
// family, see `ScrollRegionDeclaration`'s paint-responsibility rustdoc).

pub const LIST_ROWS_TOP: f32 = 28.0;
pub const LIST_ROW_STEP: f32 = 20.0;
pub const LIST_ROW_HEIGHT: f32 = 18.0;
pub const LIST_VISIBLE_HEIGHT: f32 = 80.0; // 4 rows of LIST_ROW_STEP
pub const LIST_NOTE_COUNT: usize = 8;
pub const LIST_CONTENT_HEIGHT: f32 = LIST_NOTE_COUNT as f32 * LIST_ROW_STEP; // 160
pub const LIST_MAX_SCROLL_ROWS: i32 =
    ((LIST_CONTENT_HEIGHT - LIST_VISIBLE_HEIGHT) / LIST_ROW_STEP) as i32; // 4
pub const LIST_INSET_X: f32 = 12.0;

/// Row-quantized scroll offset in content units (non-negative, clamped to
/// `content - viewport` — the `ScrollRegionDeclaration::offset` contract;
/// invalid offsets refuse with `view_contract.scroll_offset_invalid`).
pub fn list_offset_y(scroll_rows: i32) -> f32 {
    (scroll_rows as f32 * LIST_ROW_STEP).clamp(0.0, LIST_CONTENT_HEIGHT - LIST_VISIBLE_HEIGHT)
}

/// Card-local top of row `index` after applying the scroll offset. This is
/// a routed (paint-only) scroll region, so the AUTHOR shifts the painted
/// content by the declared offset — backends never translate authored
/// paint (see `ScrollRegionDeclaration` "PAINT RESPONSIBILITY").
pub fn list_row_top_in_card(index: usize, offset_y: f32) -> f32 {
    LIST_ROWS_TOP + index as f32 * LIST_ROW_STEP - offset_y
}

/// Inverse of `list_row_top_in_card` for pointer selection: card-local y
/// back to the row index, `None` outside the rows band. Same constants as
/// paint and the hit regions — the MUST-agree contract.
pub fn list_row_at_card_y(y: f32, offset_y: f32) -> Option<usize> {
    if !(LIST_ROWS_TOP..LIST_ROWS_TOP + LIST_VISIBLE_HEIGHT).contains(&y) {
        return None;
    }
    let row = ((y - LIST_ROWS_TOP + offset_y) / LIST_ROW_STEP).floor() as i32;
    (0..LIST_NOTE_COUNT as i32)
        .contains(&row)
        .then_some(row as usize)
}

/// The card-local band the list rows are presented through (the scroll
/// region's declared viewport AND the paint clip — same rect by
/// construction).
pub fn list_rows_band(card_width: f32) -> Rect {
    Rect {
        origin: Point {
            x: 0.0,
            y: LIST_ROWS_TOP,
        },
        size: Size {
            width: card_width,
            height: LIST_VISIBLE_HEIGHT,
        },
    }
}

// ---------------------------------------------------------------------------
// INPUT geometry
// ---------------------------------------------------------------------------
// The painted input field and the declared text-edit focus region MUST be
// the same rect (`text_edit_focus_region_from_capability` doc: "bounds is
// the target-local editable area and must match the painted input
// surface").

pub const INPUT_FIELD_TOP: f32 = 32.0;
pub const INPUT_FIELD_HEIGHT: f32 = 28.0;
pub const INPUT_INSET_X: f32 = 12.0;

pub fn input_field_rect(card_width: f32) -> Rect {
    Rect {
        origin: Point {
            x: INPUT_INSET_X,
            y: INPUT_FIELD_TOP,
        },
        size: Size {
            width: (card_width - 2.0 * INPUT_INSET_X).max(1.0),
            height: INPUT_FIELD_HEIGHT,
        },
    }
}

// ---------------------------------------------------------------------------
// OVERLAY geometry
// ---------------------------------------------------------------------------
// The painted feed rows, the declared feed scroll region, the painted
// overlay panel, the declared titlebar drag hit region, and the drag clamp
// in internal_logic.rs MUST all derive from these constants: the drag hit
// region is re-declared each frame at the CURRENT offset, so a drift
// between painted panel and declared titlebar makes the visible titlebar
// un-draggable with no admission diagnostic.

pub const OVERLAY_FEED_TOP: f32 = 28.0;
pub const OVERLAY_FEED_HEIGHT: f32 = 120.0;
pub const OVERLAY_FEED_ROW_STEP: f32 = 20.0;
pub const OVERLAY_FEED_ROW_COUNT: usize = 12;
pub const OVERLAY_FEED_CONTENT_HEIGHT: f32 = OVERLAY_FEED_ROW_COUNT as f32 * OVERLAY_FEED_ROW_STEP; // 240
pub const OVERLAY_FEED_MAX_SCROLL_ROWS: i32 =
    ((OVERLAY_FEED_CONTENT_HEIGHT - OVERLAY_FEED_HEIGHT) / OVERLAY_FEED_ROW_STEP) as i32; // 6
pub const OVERLAY_PANEL_WIDTH: f32 = 220.0;
pub const OVERLAY_PANEL_HEIGHT: f32 = 64.0;
pub const OVERLAY_TITLEBAR_HEIGHT: f32 = 20.0;
pub const OVERLAY_INSET_X: f32 = 12.0;
/// The overlay layer's z-index: any positive z fronts the source-order
/// feed paint; the hit-region order reuses the same value so pointer
/// selection agrees with paint stacking.
pub const OVERLAY_Z_INDEX: i32 = 10;
pub const OVERLAY_LAYER_ORDER: usize = 0;

// The second (ROAMING) overlay panel — the overflow-bounds drag pattern.
// Smaller than the clamped panel, fronts it (higher z) where they overlap,
// and may leave BOTH the feed band and the card's layout bounds — anywhere
// inside the whole root view area — because the widget declares matching
// overflow bounds (view.rs `overlay_overflow_bounds`).
pub const OVERLAY_ROAM_PANEL_WIDTH: f32 = 140.0;
pub const OVERLAY_ROAM_PANEL_HEIGHT: f32 = 44.0;
pub const OVERLAY_ROAM_TITLEBAR_HEIGHT: f32 = 16.0;
/// Fronts the clamped panel (OVERLAY_Z_INDEX = 10) where they overlap;
/// its hit region reuses the same z so selection agrees with stacking.
pub const OVERLAY_ROAM_Z_INDEX: i32 = 11;
pub const OVERLAY_ROAM_LAYER_ORDER: usize = 1;

pub fn overlay_default_offset() -> Point {
    Point { x: 24.0, y: 48.0 }
}

/// Width-independent starting spot (safe for every card width >=
/// CARD_MIN_WIDTH); the view re-clamps the stored offset against the
/// CURRENT card width and projected window each frame anyway, because
/// declarations are per-frame snapshots and the window can resize under a
/// stored offset.
pub fn overlay_roam_default_offset() -> Point {
    Point { x: 96.0, y: 36.0 }
}

pub fn overlay_feed_band(card_width: f32) -> Rect {
    Rect {
        origin: Point {
            x: 0.0,
            y: OVERLAY_FEED_TOP,
        },
        size: Size {
            width: card_width,
            height: OVERLAY_FEED_HEIGHT,
        },
    }
}

pub fn overlay_feed_offset_y(feed_rows: i32) -> f32 {
    (feed_rows as f32 * OVERLAY_FEED_ROW_STEP)
        .clamp(0.0, OVERLAY_FEED_CONTENT_HEIGHT - OVERLAY_FEED_HEIGHT)
}

pub fn overlay_panel_rect(offset: Point) -> Rect {
    Rect {
        origin: offset,
        size: Size {
            width: OVERLAY_PANEL_WIDTH,
            height: OVERLAY_PANEL_HEIGHT,
        },
    }
}

pub fn overlay_titlebar_rect(offset: Point) -> Rect {
    Rect {
        origin: offset,
        size: Size {
            width: OVERLAY_PANEL_WIDTH,
            height: OVERLAY_TITLEBAR_HEIGHT,
        },
    }
}

/// CLAMPED drag pattern (contrast: `overlay_roaming_clamped_offset`).
/// Keeps the whole panel inside the feed band, so the titlebar hit region
/// never leaves the widget's layout bounds
/// (`view_contract.hit_bounds_outside_layout` would refuse admission if a
/// drag could push it out and no overflow bounds were declared). This is
/// the simplest contract-safe shape: no overflow declaration needed —
/// clamp to a band that layout already contains.
pub fn overlay_clamped_offset(card_width: f32, desired: Point) -> Point {
    Point {
        x: desired
            .x
            .clamp(0.0, (card_width - OVERLAY_PANEL_WIDTH).max(0.0)),
        y: desired.y.clamp(
            OVERLAY_FEED_TOP,
            OVERLAY_FEED_TOP + OVERLAY_FEED_HEIGHT - OVERLAY_PANEL_HEIGHT,
        ),
    }
}

/// ROAMING drag pattern (contrast: `overlay_clamped_offset`): declare
/// overflow bounds so hit regions may leave layout bounds — see the
/// admission stress harness (`slipway-example-admission`) for the extreme
/// version (its movable overlay roams ±1600px). The rect every roaming
/// declaration must stay inside; the widget's `PaintOrderDeclaration`
/// declares exactly this rect via `with_overflow_bounds`, which switches
/// admission's containment checks from layout bounds to these bounds
/// (`view_contract.hit_bounds_outside_overflow_bounds` /
/// `view_contract.paint_bounds_outside_overflow_bounds` on violation).
///
/// ARCHITECT Q (2026-07-11, "무제한은 없음?"): why is roaming bounded at
/// all? CONTRACT ANSWER: an overflow allowance must be a FINITE declared
/// rect — admission re-validates every painted/declared rect against THIS
/// rect each frame, so "unlimited" is not a declarable value; the finite
/// declaration IS what makes out-of-layout geometry admissible. The
/// effectively-unlimited authored answer (Step 212): the LIVE WINDOW,
/// unioned with the card column. `viewport` is the projected window size
/// (`ShowcaseState::viewport`, written each frame by
/// `SlipwayApp::project_frame_viewport` — NOT an authored constant), so
/// the panel roams the whole real window at any window size. The window
/// rect and the column share the same root origin, translated into this
/// card's local space by the placement constants `layout_plan` places the
/// card with (card x = CARD_MARGIN_X, card y = OVERLAY_CARD_ROOT_TOP —
/// the MUST-agree pair). The COLUMN half of the union is the
/// reachability floor: every column point stays reachable through the
/// page scroll region even after the window shrinks, and because
/// `overlay_roaming_clamped_offset` keeps the WHOLE panel (titlebar
/// included) inside this same rect, no reachable offset can strand the
/// panel — the keep-titlebar-reachable inset is exactly "the full panel
/// stays inside the allowance".
pub fn overlay_overflow_bounds(card_width: f32, viewport: Size) -> Rect {
    Rect {
        origin: Point {
            x: -CARD_MARGIN_X,
            y: -OVERLAY_CARD_ROOT_TOP,
        },
        size: Size {
            width: (card_width + 2.0 * CARD_MARGIN_X).max(viewport.width),
            height: APP_ROOT_HEIGHT.max(viewport.height),
        },
    }
}

/// Clamp for the roaming panel: to the declared OVERFLOW bounds, not to
/// the feed band — the panel may leave its band and the card and roam over
/// the whole live window (and the whole card column, whichever is
/// larger). MUST agree with `overlay_overflow_bounds`: a clamp looser
/// than the declaration is a per-frame admission refusal at the first
/// out-of-allowance drag, and BOTH must read the same projected
/// `ShowcaseState::viewport` — paint, hit declaration, and the drag
/// handler all reproduce this exact clamp, which is only possible because
/// the viewport is ordinary external state. Clamping the FULL panel rect
/// inside the allowance (not just a corner) is what keeps the titlebar
/// reachable: no reachable offset can strand the panel off-view.
pub fn overlay_roaming_clamped_offset(card_width: f32, viewport: Size, desired: Point) -> Point {
    let allowed = overlay_overflow_bounds(card_width, viewport);
    Point {
        x: desired.x.clamp(
            allowed.origin.x,
            allowed.origin.x + (allowed.size.width - OVERLAY_ROAM_PANEL_WIDTH).max(0.0),
        ),
        y: desired.y.clamp(
            allowed.origin.y,
            allowed.origin.y + (allowed.size.height - OVERLAY_ROAM_PANEL_HEIGHT).max(0.0),
        ),
    }
}

pub fn overlay_roam_panel_rect(offset: Point) -> Rect {
    Rect {
        origin: offset,
        size: Size {
            width: OVERLAY_ROAM_PANEL_WIDTH,
            height: OVERLAY_ROAM_PANEL_HEIGHT,
        },
    }
}

pub fn overlay_roam_titlebar_rect(offset: Point) -> Rect {
    Rect {
        origin: offset,
        size: Size {
            width: OVERLAY_ROAM_PANEL_WIDTH,
            height: OVERLAY_ROAM_TITLEBAR_HEIGHT,
        },
    }
}

// ---------------------------------------------------------------------------
// NESTED geometry
// ---------------------------------------------------------------------------
// The declared inner viewports, the region declarations built from them,
// and the painted panel headers/rows MUST all derive from these constants:
// a pitch or top-offset drift makes the wheel land on a panel the user is
// not pointing at, and admission cannot catch it (it validates geometry
// against layout, not against paint).

pub const NESTED_OUTER_TOP: f32 = 28.0;
pub const NESTED_OUTER_HEIGHT: f32 = 136.0;
pub const NESTED_PANEL_COUNT: usize = 2;
pub const NESTED_PANEL_PITCH: f32 = 76.0;
pub const NESTED_PANEL_HEIGHT: f32 = 68.0;
pub const NESTED_PANEL_HEADER_HEIGHT: f32 = 18.0;
// QUANTIZED-TRAVEL RULE (both nested regions AND the list/overlay feeds):
// declared travel (content - viewport) must be an exact multiple of the
// row step the wheel handler quantizes to. 112 - 42 = 70 = 5 * 14. If the
// declared travel exceeded the quantized maximum (e.g. viewport 50 ->
// travel 62 vs quantized 56), the region would keep winning the wheel
// while the handler can no longer move: a DEAD-WHEEL band with zero
// admission diagnostics, and at-limit chaining never fires.
pub const NESTED_FIELD_HEIGHT: f32 = 42.0;
pub const NESTED_INNER_ROW_STEP: f32 = 14.0;
pub const NESTED_INNER_ROW_COUNT: usize = 8;
pub const NESTED_INNER_CONTENT_HEIGHT: f32 = NESTED_INNER_ROW_COUNT as f32 * NESTED_INNER_ROW_STEP; // 112
pub const NESTED_INNER_MAX_SCROLL_ROWS: i32 = 5; // (112 - 42) / 14
pub const NESTED_OUTER_CONTENT_HEIGHT: f32 = NESTED_PANEL_COUNT as f32 * NESTED_PANEL_PITCH; // 152
pub const NESTED_OUTER_ROW_STEP: f32 = 8.0;
// ANCHORING CAP (admission's Step-199 lesson): the outer's total travel
// (2 * 8 = 16px) must stay strictly under half a panel height (68 / 2 =
// 34px). When the outer consumes — wheel over the outer body, or default
// chaining once the inner under the cursor is at its limit — it displaces
// the panels, and every later wheel resolves the inner under the cursor
// at its DISPLACED position. Travel beyond half a pitch scrolls the
// anchored panel out from under the cursor: dead wheel bands and
// "the wrong panel scrolls" symptoms with zero admission diagnostics.
pub const NESTED_OUTER_MAX_SCROLL_ROWS: i32 = 2; // (152 - 136) / 8
pub const NESTED_INSET_X: f32 = 12.0;
/// The ONE inner panel that also demonstrates row SELECTION (the Visible
/// one). A constant, not a literal: the paint highlight, the per-row hit
/// regions, and the pointer inverse below must all pick the same panel.
pub const NESTED_SELECTABLE_PANEL: usize = 0;
/// Painted/declared height of one selectable inner row (the text op
/// height; rows advance by NESTED_INNER_ROW_STEP).
pub const NESTED_INNER_ROW_HEIGHT: f32 = 12.0;
/// Horizontal inset of the inner rows INSIDE the panel field (the painted
/// row text starts 8px into the field; hit regions must match).
pub const NESTED_INNER_ROW_INSET_X: f32 = 8.0;

pub fn nested_outer_band(card_width: f32) -> Rect {
    Rect {
        origin: Point {
            x: 0.0,
            y: NESTED_OUTER_TOP,
        },
        size: Size {
            width: card_width,
            height: NESTED_OUTER_HEIGHT,
        },
    }
}

pub fn nested_outer_offset_y(outer_rows: i32) -> f32 {
    (outer_rows as f32 * NESTED_OUTER_ROW_STEP)
        .clamp(0.0, NESTED_OUTER_CONTENT_HEIGHT - NESTED_OUTER_HEIGHT)
}

pub fn nested_inner_offset_y(inner_rows: i32, viewport_height: f32) -> f32 {
    (inner_rows as f32 * NESTED_INNER_ROW_STEP).clamp(
        0.0,
        (NESTED_INNER_CONTENT_HEIGHT - viewport_height).max(0.0),
    )
}

/// Card-local top of panel `index` after applying the outer offset.
pub fn nested_panel_top_in_card(index: usize, outer_offset_y: f32) -> f32 {
    NESTED_OUTER_TOP + index as f32 * NESTED_PANEL_PITCH - outer_offset_y
}

/// Full (unclipped) rect of panel `index`'s inner rows field.
pub fn nested_field_rect_in_card(index: usize, outer_offset_y: f32, card_width: f32) -> Rect {
    Rect {
        origin: Point {
            x: NESTED_INSET_X,
            y: nested_panel_top_in_card(index, outer_offset_y) + NESTED_PANEL_HEADER_HEIGHT,
        },
        size: Size {
            width: (card_width - 2.0 * NESTED_INSET_X).max(1.0),
            height: NESTED_FIELD_HEIGHT,
        },
    }
}

/// The VISIBLE part of panel `index`'s field: its rect intersected with
/// the outer band. This is what the inner scroll region declares as its
/// viewport (a region must present through geometry that is actually on
/// screen; the full field may be partially displaced by the outer scroll).
/// `None` = the panel is entirely outside the outer band, so no region is
/// declared this frame.
pub fn nested_visible_field_rect(
    index: usize,
    outer_offset_y: f32,
    card_width: f32,
) -> Option<Rect> {
    intersect_rect(
        nested_field_rect_in_card(index, outer_offset_y, card_width),
        nested_outer_band(card_width),
    )
}

/// Card-local rect of row `row` inside the SELECTABLE inner panel, after
/// applying BOTH scroll offsets (the outer displaces the panel, the inner
/// shifts the rows) — the scrolled-hit-region contract: the painted row,
/// the declared per-row hit region, and the pointer inverse below must all
/// derive from this one function or clicks land on rows the user does not
/// see (admission validates hit bounds against layout, never against
/// paint).
pub fn nested_inner_row_rect_in_card(
    row: usize,
    outer_offset_y: f32,
    inner_offset_y: f32,
    card_width: f32,
) -> Rect {
    let field = nested_field_rect_in_card(NESTED_SELECTABLE_PANEL, outer_offset_y, card_width);
    Rect {
        origin: Point {
            x: field.origin.x + NESTED_INNER_ROW_INSET_X,
            y: field.origin.y + row as f32 * NESTED_INNER_ROW_STEP - inner_offset_y,
        },
        size: Size {
            width: (field.size.width - 2.0 * NESTED_INNER_ROW_INSET_X).max(1.0),
            height: NESTED_INNER_ROW_HEIGHT,
        },
    }
}

/// Pointer inverse for the selectable inner panel: card-local y back to a
/// row index, `None` outside the panel's VISIBLE field (the rows scroll
/// under the field clip, so the inverse must subtract BOTH offsets — the
/// same two the paint and hit regions applied).
pub fn nested_inner_row_at_card_y(
    y: f32,
    outer_offset_y: f32,
    inner_offset_y: f32,
    card_width: f32,
) -> Option<usize> {
    let visible = nested_visible_field_rect(NESTED_SELECTABLE_PANEL, outer_offset_y, card_width)?;
    if !(visible.origin.y..visible.origin.y + visible.size.height).contains(&y) {
        return None;
    }
    let field = nested_field_rect_in_card(NESTED_SELECTABLE_PANEL, outer_offset_y, card_width);
    let row_local = y - field.origin.y + inner_offset_y;
    let row = (row_local / NESTED_INNER_ROW_STEP).floor() as i32;
    if !(0..NESTED_INNER_ROW_COUNT as i32).contains(&row) {
        return None;
    }
    // Only the text band of the row counts (rows advance by the step but
    // paint NESTED_INNER_ROW_HEIGHT tall) — matches the declared hit rect.
    (row_local - row as f32 * NESTED_INNER_ROW_STEP <= NESTED_INNER_ROW_HEIGHT)
        .then_some(row as usize)
}

// ---------------------------------------------------------------------------
// Shared geometry utilities
// ---------------------------------------------------------------------------

pub fn point_in_rect(point: Point, rect: Rect) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height
}

pub fn intersect_rect(left: Rect, right: Rect) -> Option<Rect> {
    let min_x = left.origin.x.max(right.origin.x);
    let min_y = left.origin.y.max(right.origin.y);
    let max_x = (left.origin.x + left.size.width).min(right.origin.x + right.size.width);
    let max_y = (left.origin.y + left.size.height).min(right.origin.y + right.size.height);
    (max_x > min_x && max_y > min_y).then_some(Rect {
        origin: Point { x: min_x, y: min_y },
        size: Size {
            width: max_x - min_x,
            height: max_y - min_y,
        },
    })
}

// ---------------------------------------------------------------------------
// Design tokens (checklist "Style Rules": backend theme defaults are not
// Slipway style authority; every painted text carries an explicit
// TextStyle from this module)
// ---------------------------------------------------------------------------

pub fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color {
        red: f32::from(red) / 255.0,
        green: f32::from(green) / 255.0,
        blue: f32::from(blue) / 255.0,
        alpha: 1.0,
    }
}

pub fn ink() -> Color {
    rgb(15, 23, 42)
}

pub fn muted() -> Color {
    rgb(100, 116, 139)
}

pub fn accent() -> Color {
    rgb(37, 99, 235)
}

pub fn card_background() -> Color {
    rgb(248, 250, 252)
}

pub fn card_border() -> Color {
    rgb(203, 213, 225)
}

pub fn overlay_border() -> Color {
    rgb(124, 58, 237)
}

pub fn overlay_titlebar_background() -> Color {
    rgb(237, 233, 254)
}

pub fn overlay_title_ink() -> Color {
    rgb(76, 29, 149)
}

pub fn header_text() -> TextStyle {
    TextStyle::plain()
}

pub fn row_text() -> TextStyle {
    TextStyle::plain().with_font_size(12.0)
}

/// The text-input token: explicit family and size, never a backend
/// default (`docs/public/api/ime.md` requires the typography policy to
/// convert an author token, not to inherit backend fonts).
pub fn input_text() -> TextStyle {
    TextStyle::plain()
        .with_font_family("system-ui")
        .with_font_size(14.0)
}
