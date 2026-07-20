//! # INTERNAL admission stress fixture — NOT the authoring template
//!
//! Do NOT copy this crate to author a Slipway app. The designated copy
//! source is `crates/slipway-example-authored` (facade-only,
//! `use slipway::prelude::*`, five-file split). This crate violates the
//! documented authoring rules on purpose-preserving legacy grounds — one
//! 7,000+-line `main.rs`, direct `slipway_core`/`slipway_runtime`/backend
//! crate imports — and was formally demoted from example duty by the
//! LLM-ergonomics audit (2026-07-11, findings LE-H2/LE-H6) and roadmap
//! Phase 4.
//!
//! ## What this crate actually is (why it stays)
//!
//! * an admission/contract STRESS HARNESS: nine widget kinds exercising
//!   action, segment, text-edit, toggle, slider, list-scroll, movable
//!   overlay, overlay-stack z-order, and triple-nested-scroll admission;
//! * a load-bearing REGRESSION FIXTURE: its 45 tests pin declaration and
//!   dispatch behavior that step-packet procedures reference;
//! * the debug-MCP LIVE-VERIFICATION target: drivers and step packets
//!   launch `slipway-example-admission --iced|--egui` by name.
//!
//! Do not rename, move, or restructure it; keep tests and launch behavior
//! stable. New authoring patterns belong in `slipway-example-authored`.
//!
//! ## Named-item section index
//!
//! * entry/launch: `main`, `run_egui`, `run_iced`, `print_probe`
//! * app composition: `AdmissionApp`, `admission_widget_tuple`,
//!   `AdmissionRuntimeAppWidget` (pre-Step-209 root-wrapper idiom),
//!   `root_scroll_region_for_admission_app`
//! * app state/messages: `AdmissionState`, `AdmissionMessage`,
//!   `apply_messages`, `apply_text_edit_to_draft`
//! * shared geometry constants: `LIST_ROW_*`, `CARD_*`, `NESTED_*`,
//!   `ADMISSION_OVERLAY_*` (MUST agree with paint/hit/pointer math)
//! * widget enum + locals: `AdmissionWidget`, `AdmissionLocal`
//! * declarations: `AdmissionViewDeclarations`, the policy `impl` blocks
//!   on `AdmissionWidget`, `unresolved_example_font_evidence`
//! * outcome/style helpers: `message_outcome`, `local_change_outcome`,
//!   `admission_text_input_style_token`, `admission_cjk_font_source`
//! * regression tests: `mod tests` (45 tests, includes the pre-flight
//!   admission suite)
//!
//! Authoring docs (read these, not this file):
//! `docs/public/llm-entry.md`, `docs/public/quickstart-authoring.md`,
//! `docs/public/llm-contract-checklist.md`,
//! `docs/public/authoring-layout.md`.

use slipway_backend_egui::{
    SlipwayEguiAuthoredChildren, SlipwayEguiWidgetListVisitor,
    run_slipway_egui_runtime_app_with_default_bridge,
};
use slipway_backend_iced::{
    IcedChildTraversalOrder, SlipwayIcedAuthoredChildren, SlipwayIcedWidgetListVisitor,
    run_slipway_iced_runtime_app_with_config,
};
use slipway_core::{
    AppLayoutPlan, BoxSpacing, Capability, CaretGeometryEvidence, CaretSet, ChangeEvidence,
    ChildLayoutPlan, ChildLayoutSeed, Color, CursorCapability, Diagnostic, EdgeInsets,
    EmittedMessage, EventOutcome, EventRoute, EventRoutePhase, EvidenceSource,
    FocusRegionDeclaration, FocusTraversalMember, FontResolutionEvidence, FontResolutionRequest,
    HitRegionDeclaration, HitRegionOrder, ImeCompositionPolicyDeclaration, InputEvent, LayoutInput,
    LayoutOutput, PaintInputTransparency, PaintLayerKey, PaintOp, PaintOrderDeclaration,
    PathCommand, PathDeclaration, Point, PointerCaptureIntent, PresentationRegionId,
    ProbeCollector, ProbeMetadataDeclaration, ProbeProduct, Rect, ResourceInstallationEvidence,
    ResourceInstallationStatus, ResourceRefusalEvidence, ResourceSourceDeclaration,
    ResourceSourceKind, ScrollRegionDeclaration, SemanticNode, SemanticSlotDeclaration,
    ShapeDeclaration, ShapeKind, Size, SlipwayApp, SlipwayAppWidget, SlipwayFontResolutionPolicy,
    SlipwayLogic, SlipwaySsot, SlipwayView, SlipwayViewDefinition, SlipwayWidgetListVisitor,
    SlipwayWidgetTypes, SourceValidityEvidence, SourceValidityKind, StateObservation,
    TargetLocalRect, TextAlignX, TextAlignY, TextBufferSnapshot, TextEditCommandDeclaration,
    TextEditKind, TextInputTypographyDeclaration, TextLineMode, TextSelectionPolicyDeclaration,
    TextSelectionRange, TextStyle, TextViewport, TopologyNode, ViewDefinition, ViewDefinitionInput,
    WheelRouting, WidgetId, WidgetSlotAddress,
};
use slipway_runtime::{SlipwayRuntime, SlipwayRuntimeConfig};

fn admission_demo_spacing(widget_id: &str) -> BoxSpacing {
    match widget_id {
        "admission.action" => BoxSpacing::new(
            EdgeInsets::trbl(8.0, 24.0, 12.0, 4.0),
            EdgeInsets::trbl(6.0, 28.0, 18.0, 10.0),
        ),
        "admission.segment" => BoxSpacing::new(
            EdgeInsets::trbl(3.0, 7.0, 15.0, 21.0),
            EdgeInsets::trbl(5.0, 13.0, 23.0, 31.0),
        ),
        "admission.text" => BoxSpacing::new(
            EdgeInsets::trbl(4.0, 20.0, 12.0, 6.0),
            EdgeInsets::trbl(9.0, 17.0, 25.0, 11.0),
        ),
        _ => BoxSpacing::ZERO,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("--probe") => {
            print_probe();
            Ok(())
        }
        Some("--iced") => run_iced(),
        Some("--egui") | None => run_egui(),
        Some(other) => {
            eprintln!("unknown argument: {other}");
            eprintln!("usage: slipway-example-admission [--egui|--probe|--iced]");
            Ok(())
        }
    }
}

fn run_egui() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = SlipwayRuntime::new(
        AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        }),
        AdmissionState::default(),
    );
    run_slipway_egui_runtime_app_with_default_bridge(
        "Slipway admission authored app",
        runtime,
        apply_messages,
    )?;
    Ok(())
}

fn run_iced() -> Result<(), Box<dyn std::error::Error>> {
    let config = SlipwayRuntimeConfig::admitted_debug().with_platform_ime_always_allowed();
    run_slipway_iced_runtime_app_with_config(
        AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        }),
        AdmissionState::default(),
        apply_messages,
        config,
    )?;
    Ok(())
}

fn print_probe() {
    let external = AdmissionState::default();
    let widgets = admission_widgets();
    let mut collector = ProbeCollector::new();

    for widget in &widgets {
        let mut local = widget.initial_local_state();
        let topology = widget.topology(&external);
        collector.push(ProbeProduct::Topology(slipway_core::TopologyProbe {
            traversal: topology.traverse_depth_first(),
            root: topology,
        }));
        collector.push(ProbeProduct::State(slipway_core::StateProbe {
            target: widget.id(),
            observations: widget.observe_state(&external, &local),
        }));
        let outcome = widget.handle_event(
            &external,
            &mut local,
            InputEvent::Command(slipway_core::CommandEvent {
                target: widget.id(),
                target_slot: None,
                command: "probe".to_string(),
                payload_ref: None,
                source: None,
            }),
        );
        collector.extend(outcome.probes);
    }

    for product in collector.take() {
        println!("{product:?}");
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AdmissionApp {
    widgets: (
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
    ),
}

fn admission_widget_tuple() -> (
    AdmissionWidget,
    AdmissionWidget,
    AdmissionWidget,
    AdmissionWidget,
    AdmissionWidget,
    AdmissionWidget,
    AdmissionWidget,
    AdmissionWidget,
    AdmissionWidget,
) {
    (
        AdmissionWidget::Action(ActionWidget),
        AdmissionWidget::Segment(SegmentWidget),
        AdmissionWidget::Text(TextWidget),
        AdmissionWidget::Toggle(ToggleWidget),
        AdmissionWidget::Slider(SliderWidget),
        AdmissionWidget::List(ListWidget),
        AdmissionWidget::Overlay(OverlayWidget),
        AdmissionWidget::OverlayStack(OverlayStackWidget),
        AdmissionWidget::NestedScroll(NestedScrollWidget),
    )
}

impl SlipwayApp for AdmissionApp {
    type ExternalState = AdmissionState;
    type LocalState = AdmissionAppLocal;
    type AppMessage = AdmissionMessage;
    type Widgets = (
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
        AdmissionWidget,
    );

    fn id(&self) -> WidgetId {
        WidgetId::from("admission.app")
    }

    fn widgets(&self) -> &Self::Widgets {
        &self.widgets
    }

    fn initial_local_state(&self) -> Self::LocalState {
        AdmissionAppLocal::default()
    }

    fn handle_event(
        &self,
        _external: &Self::ExternalState,
        local: &mut Self::LocalState,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        match event {
            InputEvent::Scroll(scroll)
                if scroll.region_id == AdmissionAppLocal::root_scroll_region_id() =>
            {
                local.root_scroll_y = scroll.offset_y.max(0.0);
                local_change_outcome(
                    self.id(),
                    "root-scroll",
                    "root-scroll-y",
                    format!("{:.1}", local.root_scroll_y),
                )
            }
            InputEvent::Wheel(wheel)
                if wheel.region_id == Some(AdmissionAppLocal::root_scroll_region_id()) =>
            {
                let direction = if wheel.delta_y < 0.0 {
                    48.0
                } else if wheel.delta_y > 0.0 {
                    -48.0
                } else {
                    0.0
                };
                local.root_scroll_y = (local.root_scroll_y + direction).max(0.0);
                local_change_outcome(
                    self.id(),
                    "root-wheel",
                    "root-scroll-y",
                    format!("{:.1}", local.root_scroll_y),
                )
            }
            _ => EventOutcome::ignored(),
        }
    }

    fn layout_plan(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        children: Vec<ChildLayoutSeed>,
    ) -> AppLayoutPlan {
        let width = input.viewport.size.width.clamp(260.0, 640.0);
        let mut y = 16.0;
        let mut plans = Vec::new();

        for seed in children {
            let height = match seed.child.as_str() {
                "admission.list" => LIST_CARD_HEIGHT,
                "admission.overlay" => 228.0,
                "admission.overlay-stack" => 276.0,
                "admission.nested-scroll" => 292.0,
                _ => 76.0,
            };
            let bounds = Rect {
                origin: Point { x: 16.0, y },
                size: Size {
                    width: (width - 32.0).max(1.0),
                    height,
                },
            };
            let spacing = admission_demo_spacing(seed.child.as_str());
            plans.push(ChildLayoutPlan::explicit_border(
                seed,
                slipway_core::ContentLocalRect::new(bounds),
                spacing,
            ));
            y += height + 12.0;
        }

        AppLayoutPlan {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width,
                    height: y + 16.0,
                },
            }),
            children: plans,
            diagnostics: Vec::new(),
        }
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: slipway_core::LayoutOutputBuilder,
    ) -> LayoutOutput {
        let width = input.viewport.size.width.clamp(260.0, 640.0);
        let height = 16.0
            + [
                76.0,
                76.0,
                76.0,
                76.0,
                76.0,
                LIST_CARD_HEIGHT,
                228.0,
                276.0,
                292.0,
            ]
            .into_iter()
            .map(|height| height + 12.0)
            .sum::<f32>()
            + 16.0;
        output.finish(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size { width, height },
        }))
    }

    fn paint(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        vec![PaintOp::Fill {
            shape: rect_shape(
                "admission-app-bg",
                layout.bounds().into_rect(),
                ShapeKind::Rectangle,
            ),
            color: rgb(241, 245, 249),
        }]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct AdmissionAppLocal {
    root_scroll_y: f32,
}

impl AdmissionAppLocal {
    fn root_scroll_region_id() -> PresentationRegionId {
        PresentationRegionId::from("admission.app:root-scroll")
    }
}

#[derive(Clone, Debug, PartialEq)]
struct AdmissionRuntimeAppWidget {
    inner: SlipwayAppWidget<AdmissionApp>,
}

impl AdmissionRuntimeAppWidget {
    fn new(app: AdmissionApp) -> Self {
        Self {
            inner: SlipwayAppWidget::new(app),
        }
    }
}

impl SlipwayWidgetTypes for AdmissionRuntimeAppWidget {
    type ExternalState = <SlipwayAppWidget<AdmissionApp> as SlipwayWidgetTypes>::ExternalState;
    type LocalState = <SlipwayAppWidget<AdmissionApp> as SlipwayWidgetTypes>::LocalState;
    type AppMessage = <SlipwayAppWidget<AdmissionApp> as SlipwayWidgetTypes>::AppMessage;
}

impl SlipwaySsot for AdmissionRuntimeAppWidget {
    fn id(&self) -> WidgetId {
        self.inner.id()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.inner.capabilities()
    }

    fn topology(&self, external: &Self::ExternalState) -> TopologyNode {
        self.inner.topology(external)
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        self.inner.unsupported()
    }

    fn visit_authored_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        visitor: &mut V,
    ) where
        V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        self.inner.visit_authored_children(external, local, visitor);
    }
}

impl SlipwayLogic for AdmissionRuntimeAppWidget {
    fn handle_event(
        &self,
        external: &Self::ExternalState,
        local: &mut Self::LocalState,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        self.inner.handle_event(external, local, event)
    }
}

impl SlipwayView for AdmissionRuntimeAppWidget {
    fn initial_local_state(&self) -> Self::LocalState {
        self.inner.initial_local_state()
    }

    fn layout(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: LayoutInput,
        output: slipway_core::LayoutOutputBuilder,
    ) -> LayoutOutput {
        self.inner.layout(external, local, input, output)
    }

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        self.inner.paint(external, local, layout)
    }

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        self.inner.observe_state(external, local)
    }
}

impl SlipwayViewDefinition for AdmissionRuntimeAppWidget {
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let visible_viewport = input.layout_input.viewport.into_rect();
        let mut view = self.inner.view_definition(external, local, input);
        view.paint_order.allow_overlap = true;
        view.paint_order = view
            .paint_order
            .with_overflow_bounds(TargetLocalRect::new(Rect {
                origin: Point {
                    x: -1800.0,
                    y: -1400.0,
                },
                size: Size {
                    width: view.layout.bounds().as_rect().size.width + 3600.0,
                    height: view.layout.bounds().as_rect().size.height + 2800.0,
                },
            }));
        if view.layout.bounds().as_rect().size.height > visible_viewport.size.height + 0.5 {
            let root_slot = WidgetSlotAddress::new(self.id(), 0);
            let content_bounds = view.layout.bounds().into_rect();
            let terminal_region_index = view.scroll_regions.len();
            view.scroll_regions
                .push(root_scroll_region_for_admission_app(
                    self.id(),
                    root_slot,
                    visible_viewport,
                    content_bounds,
                    local.app.root_scroll_y,
                ));
            view.wheel_traversal_boundary.terminal_region_index = Some(terminal_region_index);
        }
        view
    }
}

fn root_scroll_region_for_admission_app(
    target: WidgetId,
    address: WidgetSlotAddress,
    viewport: Rect,
    content_bounds: Rect,
    offset_y: f32,
) -> ScrollRegionDeclaration {
    let content_size = Size {
        width: content_bounds.size.width.max(viewport.size.width),
        height: content_bounds.size.height.max(viewport.size.height),
    };
    let max_offset_y = (content_size.height - viewport.size.height).max(0.0);
    ScrollRegionDeclaration::explicit(
        AdmissionAppLocal::root_scroll_region_id(),
        target,
        Some(address),
        TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: viewport.size,
        }),
        TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: content_size,
        }),
        Point {
            x: 0.0,
            y: offset_y.clamp(0.0, max_offset_y),
        },
        slipway_core::ScrollAxes {
            horizontal: false,
            vertical: true,
        },
        // The default routing mode: wheel selection stays the front-most
        // containing consumable owner. Since the Step 200 revert
        // (902f99eae) EVERY region in this example — this root page region,
        // the List, the nested outer AND inners — authors the
        // `NearestScrollable` default; no non-default declaration remains
        // (see `AdmissionWidget::wheel_routing_policy` for the revert
        // record and the authoring breadcrumb).
        WheelRouting::NearestScrollable,
        HitRegionOrder {
            z_index: -1,
            paint_order: 0,
            traversal_order: 0,
        },
        slipway_core::ScrollConsumptionPolicy::exclusive_wheel(),
        true,
    )
}

impl slipway_core::SlipwayEventRoutingPolicy for AdmissionRuntimeAppWidget {
    fn event_routing_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        event: &InputEvent,
    ) -> slipway_core::EventRoutingPolicyDeclaration {
        self.inner.event_routing_policy(external, local, event)
    }
}

impl slipway_core::SlipwayEventDispositionPolicy for AdmissionRuntimeAppWidget {
    fn event_disposition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        event: &InputEvent,
        route: &EventRoute,
    ) -> slipway_core::EventPropagationEvidence {
        self.inner.event_disposition(external, local, event, route)
    }
}

impl SlipwayFontResolutionPolicy for AdmissionRuntimeAppWidget {
    fn resolve_font(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        request: FontResolutionRequest,
    ) -> FontResolutionEvidence {
        unresolved_example_font_evidence(self.id(), request)
    }
}

impl SlipwayIcedAuthoredChildren for AdmissionRuntimeAppWidget {
    fn visit_iced_authored_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        self.inner
            .visit_iced_authored_children(external, local, visitor);
    }

    fn visit_iced_authored_children_in_paint_order<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        frame: &slipway_core::FrameIdentity,
        placements: &[slipway_core::ChildPlacement],
        traversal_order: IcedChildTraversalOrder,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        self.inner.visit_iced_authored_children_in_paint_order(
            external,
            local,
            frame,
            placements,
            traversal_order,
            visitor,
        );
    }
}

impl SlipwayEguiAuthoredChildren for AdmissionRuntimeAppWidget {
    fn visit_egui_authored_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        self.inner
            .visit_egui_authored_children(external, local, visitor);
    }

    fn visit_egui_authored_children_in_paint_order<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_view: &ViewDefinition,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        self.inner.visit_egui_authored_children_in_paint_order(
            external,
            local,
            parent_view,
            visitor,
        );
    }
}

fn admission_widgets() -> Vec<AdmissionWidget> {
    vec![
        AdmissionWidget::Action(ActionWidget),
        AdmissionWidget::Segment(SegmentWidget),
        AdmissionWidget::Text(TextWidget),
        AdmissionWidget::Toggle(ToggleWidget),
        AdmissionWidget::Slider(SliderWidget),
        AdmissionWidget::List(ListWidget),
        AdmissionWidget::Overlay(OverlayWidget),
        AdmissionWidget::OverlayStack(OverlayStackWidget),
        AdmissionWidget::NestedScroll(NestedScrollWidget),
    ]
}

#[derive(Clone, Debug, PartialEq)]
struct AdmissionState {
    counter: u32,
    segment: Segment,
    draft: String,
    enabled: bool,
    intensity: f32,
    selected_item: usize,
}

impl Default for AdmissionState {
    fn default() -> Self {
        Self {
            counter: 0,
            segment: Segment::Today,
            draft: "edit me".to_string(),
            enabled: true,
            intensity: 0.42,
            selected_item: 0,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Segment {
    Today,
    Week,
    Month,
}

impl Segment {
    fn next(self) -> Self {
        match self {
            Self::Today => Self::Week,
            Self::Week => Self::Month,
            Self::Month => Self::Today,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Today => "today",
            Self::Week => "week",
            Self::Month => "month",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum AdmissionMessage {
    Increment,
    SelectNextSegment,
    UpdateDraft(String),
    ToggleEnabled,
    SetIntensity(f32),
    SelectItem(usize),
}

fn apply_messages(state: &mut AdmissionState, messages: Vec<AdmissionMessage>) {
    for message in messages {
        match message {
            AdmissionMessage::Increment => state.counter += 1,
            AdmissionMessage::SelectNextSegment => state.segment = state.segment.next(),
            AdmissionMessage::UpdateDraft(text) => state.draft = text,
            AdmissionMessage::ToggleEnabled => state.enabled = !state.enabled,
            AdmissionMessage::SetIntensity(value) => state.intensity = value.clamp(0.0, 1.0),
            AdmissionMessage::SelectItem(index) => state.selected_item = index.min(4),
        }
    }
}

fn apply_text_edit_to_draft(current: &str, edit: &slipway_core::TextEditEvent) -> String {
    match edit.kind {
        TextEditKind::ReplaceSelection | TextEditKind::ReplaceBuffer => {
            edit.text.clone().unwrap_or_else(|| current.to_string())
        }
        TextEditKind::InsertText => {
            let mut next = current.to_string();
            if let Some(text) = &edit.text {
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

// LIST card row geometry. The pointer-selection math
// (`list_row_from_pointer_y`), the declared per-row hit regions
// (`push_list_row_hit_regions`), the painted row labels (`paint`), and the
// row-quantized scroll conversions MUST all derive from these same
// constants: admission validates hit bounds against layout, but nothing
// can validate them against paint, so a drift here silently routes clicks
// to rows the user does not see.
const LIST_ROW_TOP: f32 = 44.0;
const LIST_ROW_STEP: f32 = 20.0;
const LIST_ROW_COUNT: usize = 5;

// Card geometry shared by `AdmissionWidget::layout`, the app's
// `layout_plan`, and the (RESERVED, evidence-only) viewport-observation
// declaration. The observation evidence MUST derive from the same
// constants as the presented layout or the probe reports a viewport the
// backend never presented.
const CARD_MAX_WIDTH: f32 = 560.0;
const LIST_CARD_HEIGHT: f32 = 168.0;

fn list_row_from_pointer_y(y: f32) -> Option<usize> {
    let row = ((y - LIST_ROW_TOP) / LIST_ROW_STEP).floor() as i32;
    (0..LIST_ROW_COUNT as i32)
        .contains(&row)
        .then_some(row as usize)
}

fn list_scroll_rows_after_wheel(current: i32, delta_y: f32) -> i32 {
    let direction = if delta_y < 0.0 {
        1
    } else if delta_y > 0.0 {
        -1
    } else {
        0
    };
    // Wheel travel is quantized to one row per notch, capped at one full
    // row-set of travel.
    (current + direction).clamp(0, LIST_ROW_COUNT as i32)
}

fn list_scroll_content_height(viewport_height: f32) -> f32 {
    260.0_f32.max(viewport_height)
}

fn list_scroll_offset_y(scroll_rows: i32, viewport_height: f32) -> f32 {
    let content_height = list_scroll_content_height(viewport_height);
    let max_offset = (content_height - viewport_height).max(0.0);
    (scroll_rows as f32 * LIST_ROW_STEP).clamp(0.0, max_offset)
}

const NESTED_SCROLL_ROW_STEP: f32 = 18.0;
const NESTED_SCROLL_MAX_ROWS: i32 = 7;
const NESTED_INNER_VIEWPORT_HEIGHT: f32 = 62.0;
// The OUTER region's travel is authored separately from the inner panels.
// Its total travel (step * max rows = 24px) must stay strictly under half a
// panel height (62 / 2 = 31px): whenever the outer consumes — a wheel over
// the outer body, or default chaining once the inner under the cursor is at
// its limit — it displaces the panels upward, and every later wheel resolves
// the inner under the cursor at its DISPLACED position. With the old travel
// (126px, two panel pitches) the anchored panel had scrolled out from under
// the cursor by the time the outer saturated: the bottom panel's band became
// bare outer body (a dead wheel), the top panel became unreachable, and a
// cursor over the first panel landed on the THIRD — the live symptom "the
// third nested panel never scrolls". Keeping the travel under half a panel
// height guarantees every panel-center anchor still sits over its own panel
// across the outer's whole travel.
const NESTED_OUTER_ROW_STEP: f32 = 8.0;
const NESTED_OUTER_MAX_ROWS: i32 = 3;
// Nested-demo panel geometry. The declared inner viewports
// (`nested_inner_content_viewport`), the region declarations built from
// them (`push_nested_scroll_regions`), and the painted panel field +
// outer-content tail markers (`push_nested_scroll_paint_ops`) MUST all
// derive from these same constants: a pitch or top-offset drift makes the
// wheel land on a panel the user is not pointing at, and admission cannot
// catch it (it validates geometry against layout, not against paint).
const NESTED_PANEL_COUNT: usize = 3;
const NESTED_PANEL_PITCH: f32 = 70.0;
// Content-local y of panel 0 inside the outer region; panels sit at
// NESTED_OUTER_VIEWPORT_TOP + NESTED_PANEL_FIELD_TOP in card-local space.
const NESTED_PANEL_FIELD_TOP: f32 = 14.0;
const NESTED_OUTER_VIEWPORT_TOP: f32 = 44.0;

fn nested_scroll_local(local: &AdmissionLocal) -> Option<(i32, [i32; 3])> {
    match local {
        AdmissionLocal::NestedScroll {
            outer_scroll_rows,
            inner_scroll_rows,
        } => Some((*outer_scroll_rows, *inner_scroll_rows)),
        _ => None,
    }
}

fn nested_scroll_rows_after_wheel(current: i32, delta_y: f32, max_rows: i32) -> i32 {
    let direction = if delta_y < 0.0 {
        1
    } else if delta_y > 0.0 {
        -1
    } else {
        0
    };
    (current + direction).clamp(0, max_rows)
}

fn nested_scroll_region_index(region_id: &PresentationRegionId) -> Option<Option<usize>> {
    let text = region_id.as_str();
    if text.ends_with(":outer") {
        Some(None)
    } else if text.ends_with(":inner-0") {
        Some(Some(0))
    } else if text.ends_with(":inner-1") {
        Some(Some(1))
    } else if text.ends_with(":inner-2") {
        Some(Some(2))
    } else {
        None
    }
}

#[derive(Clone, Debug, PartialEq)]
enum AdmissionWidget {
    Action(ActionWidget),
    Segment(SegmentWidget),
    Text(TextWidget),
    Toggle(ToggleWidget),
    Slider(SliderWidget),
    List(ListWidget),
    Overlay(OverlayWidget),
    OverlayStack(OverlayStackWidget),
    NestedScroll(NestedScrollWidget),
}

#[derive(Clone, Debug, PartialEq)]
enum AdmissionLocal {
    Action {
        pressed: bool,
    },
    Segment {
        hover_count: u32,
    },
    Text {
        focused: bool,
        local_edit_count: u32,
    },
    Toggle {
        armed: bool,
    },
    Slider {
        dragging: bool,
    },
    List {
        scroll_rows: i32,
    },
    Overlay {
        offset: Point,
        dragging: bool,
        drag_anchor: Point,
    },
    OverlayStack {
        offsets: [Point; 4],
        order: [usize; 4],
        dragging: Option<usize>,
        drag_anchor: Point,
    },
    NestedScroll {
        outer_scroll_rows: i32,
        inner_scroll_rows: [i32; 3],
    },
}

#[derive(Clone, Debug, PartialEq)]
struct ActionWidget;

#[derive(Clone, Debug, PartialEq)]
struct SegmentWidget;

#[derive(Clone, Debug, PartialEq)]
struct TextWidget;

#[derive(Clone, Debug, PartialEq)]
struct ToggleWidget;

#[derive(Clone, Debug, PartialEq)]
struct SliderWidget;

#[derive(Clone, Debug, PartialEq)]
struct ListWidget;

#[derive(Clone, Debug, PartialEq)]
struct OverlayWidget;

#[derive(Clone, Debug, PartialEq)]
struct OverlayStackWidget;

#[derive(Clone, Debug, PartialEq)]
struct NestedScrollWidget;

#[derive(Clone, Copy, Debug, PartialEq)]
struct OverlayStackLayerSpec {
    overlay: usize,
    key: PaintLayerKey,
}

const ADMISSION_OVERLAY_GLOBAL_Z: i32 = 10;
const ADMISSION_MOVABLE_OVERLAY_ORDER: usize = 10;
const ADMISSION_OVERLAY_STACK_ORDER_BASE: usize = 0;
const ADMISSION_OVERLAY_HIT_TRAVERSAL_BASE: usize = 10_000;

impl OverlayStackLayerSpec {
    fn new(overlay: usize, rank: usize) -> Self {
        Self {
            overlay,
            key: PaintLayerKey::ordered(
                ADMISSION_OVERLAY_GLOBAL_Z,
                ADMISSION_OVERLAY_STACK_ORDER_BASE + rank,
            ),
        }
    }

    fn paint_layer(self, ops: Vec<PaintOp>) -> PaintOp {
        // The overlay body stays pointer-opaque (clicks are occluded) but is
        // authored wheel-pass-through, so wheeling over or while dragging the
        // overlay scrolls the page behind it. See the explicit wheel-channel
        // API `PaintOp::with_wheel_transparency`.
        PaintOp::keyed_layer(self.key, ops)
            .with_layer_id(format!("overlay-stack-window-{}", self.overlay))
            .with_wheel_transparency(PaintInputTransparency::PassThrough)
    }

    fn absorb_hit_order(self) -> HitRegionOrder {
        let paint_order = self.key.order.unwrap_or(self.overlay);
        HitRegionOrder {
            z_index: self.key.z_index,
            paint_order,
            traversal_order: ADMISSION_OVERLAY_HIT_TRAVERSAL_BASE + paint_order * 2,
        }
    }

    fn drag_hit_order(self) -> HitRegionOrder {
        let paint_order = self.key.order.unwrap_or(self.overlay);
        HitRegionOrder {
            z_index: self.key.z_index,
            paint_order,
            traversal_order: ADMISSION_OVERLAY_HIT_TRAVERSAL_BASE + paint_order * 2 + 1,
        }
    }
}

impl AdmissionWidget {
    fn overlay_layer_key() -> PaintLayerKey {
        PaintLayerKey::ordered(ADMISSION_OVERLAY_GLOBAL_Z, ADMISSION_MOVABLE_OVERLAY_ORDER)
    }

    fn overlay_hit_order_for_key(key: PaintLayerKey) -> HitRegionOrder {
        let paint_order = key.order.unwrap_or(ADMISSION_MOVABLE_OVERLAY_ORDER);
        HitRegionOrder {
            z_index: key.z_index,
            paint_order,
            traversal_order: ADMISSION_OVERLAY_HIT_TRAVERSAL_BASE + paint_order * 2 + 1,
        }
    }

    fn overlay_default_offset() -> Point {
        Point { x: 220.0, y: -44.0 }
    }

    fn overlay_allowed_bounds(bounds: Rect) -> Rect {
        Rect {
            origin: Point {
                x: -1600.0,
                y: -1200.0,
            },
            size: Size {
                width: bounds.size.width + 3200.0,
                height: bounds.size.height + 2400.0,
            },
        }
    }

    fn clamp_overlay_offset_to_allowed(bounds: Rect, offset: Point) -> Point {
        let allowed = Self::overlay_allowed_bounds(bounds);
        Point {
            x: offset.x.clamp(
                allowed.origin.x,
                allowed.origin.x + (allowed.size.width - 224.0).max(0.0),
            ),
            y: offset.y.clamp(
                allowed.origin.y,
                allowed.origin.y + (allowed.size.height - 132.0).max(0.0),
            ),
        }
    }

    fn overlay_rect(bounds: Rect, offset: Point) -> Rect {
        let offset = Self::clamp_overlay_offset_to_allowed(bounds, offset);
        Rect {
            origin: offset,
            size: Size {
                width: 224.0,
                height: 132.0,
            },
        }
    }

    fn overlay_titlebar_rect(bounds: Rect, offset: Point) -> Rect {
        let overlay = Self::overlay_rect(bounds, offset);
        Rect {
            origin: overlay.origin,
            size: Size {
                width: overlay.size.width,
                height: 30.0,
            },
        }
    }

    fn overlay_stack_default_offsets() -> [Point; 4] {
        [
            Point { x: 236.0, y: 108.0 },
            Point { x: 300.0, y: 128.0 },
            Point { x: 252.0, y: 152.0 },
            Point { x: 364.0, y: 176.0 },
        ]
    }

    fn overlay_stack_default_order() -> [usize; 4] {
        [0, 1, 2, 3]
    }

    fn overlay_stack_allowed_bounds(bounds: Rect) -> Rect {
        Rect {
            origin: Point {
                x: -1600.0,
                y: -1200.0,
            },
            size: Size {
                width: bounds.size.width + 3200.0,
                height: bounds.size.height + 2400.0,
            },
        }
    }

    fn clamp_overlay_stack_offset_to_allowed(bounds: Rect, offset: Point) -> Point {
        let allowed = Self::overlay_stack_allowed_bounds(bounds);
        Point {
            x: offset.x.clamp(
                allowed.origin.x,
                allowed.origin.x + (allowed.size.width - 176.0).max(0.0),
            ),
            y: offset.y.clamp(
                allowed.origin.y,
                allowed.origin.y + (allowed.size.height - 104.0).max(0.0),
            ),
        }
    }

    fn overlay_stack_rect(bounds: Rect, offsets: [Point; 4], index: usize) -> Rect {
        Rect {
            origin: Self::clamp_overlay_stack_offset_to_allowed(bounds, offsets[index]),
            size: Size {
                width: 176.0,
                height: 104.0,
            },
        }
    }

    fn overlay_stack_titlebar_rect(bounds: Rect, offsets: [Point; 4], index: usize) -> Rect {
        let overlay = Self::overlay_stack_rect(bounds, offsets, index);
        Rect {
            origin: overlay.origin,
            size: Size {
                width: overlay.size.width,
                height: 28.0,
            },
        }
    }

    fn overlay_stack_drag_rect(bounds: Rect, offsets: [Point; 4], index: usize) -> Rect {
        Self::overlay_stack_rect(bounds, offsets, index)
    }

    fn overlay_stack_help_text_rect(bounds: Rect) -> Rect {
        Rect {
            origin: Point { x: 20.0, y: 44.0 },
            size: Size {
                width: (bounds.size.width - 40.0).clamp(1.0, 216.0),
                height: 18.0,
            },
        }
    }

    fn overlay_stack_order_button_rect(index: usize) -> Rect {
        Rect {
            origin: Point {
                x: 20.0 + index as f32 * 52.0,
                y: 68.0,
            },
            size: Size {
                width: 44.0,
                height: 24.0,
            },
        }
    }

    fn overlay_stack_rank(order: [usize; 4], overlay: usize) -> usize {
        order
            .iter()
            .position(|candidate| *candidate == overlay)
            .unwrap_or(overlay)
    }

    fn overlay_stack_bring_front(order: &mut [usize; 4], overlay: usize) {
        let mut next = Vec::with_capacity(4);
        for candidate in order.iter().copied() {
            if candidate != overlay {
                next.push(candidate);
            }
        }
        next.push(overlay);
        for (index, value) in next.into_iter().enumerate() {
            order[index] = value;
        }
    }

    fn overlay_stack_layer_spec(overlay: usize, rank: usize) -> OverlayStackLayerSpec {
        OverlayStackLayerSpec::new(overlay, rank)
    }

    fn overlay_stack_topmost_overlay_at(
        bounds: Rect,
        offsets: [Point; 4],
        order: [usize; 4],
        point: Point,
    ) -> Option<usize> {
        order
            .iter()
            .rev()
            .copied()
            .find(|index| point_in_rect(point, Self::overlay_stack_rect(bounds, offsets, *index)))
    }

    fn nested_outer_viewport(bounds: Rect) -> Rect {
        Rect {
            origin: Point {
                x: 18.0,
                y: NESTED_OUTER_VIEWPORT_TOP,
            },
            size: Size {
                width: (bounds.size.width - 36.0).max(1.0),
                height: (bounds.size.height - 68.0).max(1.0),
            },
        }
    }

    fn nested_inner_content_viewport(index: usize, outer_offset_y: f32) -> Rect {
        Rect {
            origin: Point {
                x: 32.0,
                y: NESTED_OUTER_VIEWPORT_TOP
                    + NESTED_PANEL_FIELD_TOP
                    + index as f32 * NESTED_PANEL_PITCH
                    - outer_offset_y,
            },
            size: Size {
                width: 156.0,
                height: NESTED_INNER_VIEWPORT_HEIGHT,
            },
        }
    }

    fn nested_inner_visible_viewport(
        index: usize,
        outer_offset_y: f32,
        outer_viewport: Rect,
    ) -> Option<Rect> {
        intersect_rect(
            Self::nested_inner_content_viewport(index, outer_offset_y),
            outer_viewport,
        )
    }

    fn nested_outer_content_height() -> f32 {
        // Aligned with the row-quantized wheel handler exactly like the inner
        // panels (`nested_inner_content_height`): the demo card is 292 tall
        // (`layout_plan`), so the outer viewport is 292 - 68 = 224, and the
        // declared content extent puts the offset at `NESTED_OUTER_MAX_ROWS`
        // rows exactly at the declared limit. Without this alignment the
        // declaration keeps reporting consumable room after the handler
        // saturates, so at-limit chaining to the page would never engage and
        // wheels over a saturated outer would be silently dead. The travel
        // itself is capped by the anchoring rule on
        // `NESTED_OUTER_ROW_STEP`/`NESTED_OUTER_MAX_ROWS`.
        224.0 + NESTED_OUTER_ROW_STEP * NESTED_OUTER_MAX_ROWS as f32
    }

    fn nested_inner_content_height(viewport_height: f32) -> f32 {
        viewport_height + NESTED_SCROLL_ROW_STEP * NESTED_SCROLL_MAX_ROWS as f32
    }

    fn nested_scroll_offset_y(
        rows: i32,
        row_step: f32,
        viewport_height: f32,
        content_height: f32,
    ) -> f32 {
        let max_offset = (content_height - viewport_height).max(0.0);
        (rows as f32 * row_step).clamp(0.0, max_offset)
    }

    fn text_input_focus_bounds(bounds: Rect) -> Rect {
        Rect {
            origin: Point { x: 12.0, y: 36.0 },
            size: Size {
                width: (bounds.size.width - 24.0).max(1.0),
                height: 28.0,
            },
        }
    }

    fn card_header_text_rect(&self, bounds: Rect) -> Rect {
        let width = match self {
            Self::OverlayStack(_) => 232.0,
            _ => bounds.size.width - 24.0,
        };
        Rect {
            origin: Point { x: 12.0, y: 12.0 },
            size: Size {
                width: width.max(1.0).min((bounds.size.width - 24.0).max(1.0)),
                height: 20.0,
            },
        }
    }

    fn role(&self) -> &'static str {
        match self {
            Self::Action(_) => "action",
            Self::Segment(_) => "segment",
            Self::Text(_) => "text-input",
            Self::Toggle(_) => "toggle",
            Self::Slider(_) => "slider",
            Self::List(_) => "scroll-list",
            Self::Overlay(_) => "movable-overlay",
            Self::OverlayStack(_) => "overlay-stack",
            Self::NestedScroll(_) => "nested-scroll",
        }
    }

    fn text_after_command(&self, state: &AdmissionState, command: &str) -> Option<String> {
        match (self, command) {
            (Self::Text(_), "probe") => Some(format!("{}*", state.draft)),
            _ => None,
        }
    }

    fn traversal_order(&self) -> usize {
        match self {
            Self::Action(_) => 0,
            Self::Segment(_) => 1,
            Self::Text(_) => 2,
            Self::Toggle(_) => 3,
            Self::Slider(_) => 4,
            Self::List(_) => 5,
            Self::Overlay(_) => 6,
            Self::OverlayStack(_) => 7,
            Self::NestedScroll(_) => 8,
        }
    }

    fn view_declarations(
        &self,
        external: &AdmissionState,
        local: &AdmissionLocal,
        input: &LayoutInput,
        layout: &LayoutOutput,
        bounds: Rect,
        address: Option<WidgetSlotAddress>,
    ) -> AdmissionViewDeclarations {
        let target = self.id();
        let mut declarations = AdmissionViewDeclarations {
            semantic_slots: vec![SemanticSlotDeclaration {
                target: target.clone(),
                node: SemanticNode {
                    id: target.clone(),
                    role: self.role().to_string(),
                    label: Some(self.semantic_label(external, local)),
                    value: Some(self.semantic_value(external, local)),
                    bounds: Some(bounds),
                    states: Vec::new(),
                    actions: Vec::new(),
                    relationships: Vec::new(),
                },
            }],
            probe_metadata: vec![ProbeMetadataDeclaration {
                target: target.clone(),
                name: "role".to_string(),
                value: self.role().to_string(),
            }],
            ..AdmissionViewDeclarations::default()
        };

        if matches!(self, Self::OverlayStack(_)) {
            self.push_overlay_stack_hit_regions(
                &mut declarations,
                external,
                local,
                address.clone(),
                bounds,
            );
        } else if self.has_pointer_region() {
            let hit_bounds = if matches!(self, Self::Text(_)) {
                bounds
            } else if matches!(self, Self::Overlay(_)) {
                let offset = match local {
                    AdmissionLocal::Overlay { offset, .. } => *offset,
                    _ => Self::overlay_default_offset(),
                };
                Self::overlay_titlebar_rect(bounds, offset)
            } else {
                bounds
            };
            let hit_order = if matches!(self, Self::Overlay(_)) {
                Self::overlay_hit_order_for_key(Self::overlay_layer_key())
            } else {
                HitRegionOrder {
                    z_index: 0,
                    paint_order: self.traversal_order(),
                    traversal_order: self.traversal_order(),
                }
            };
            declarations
                .hit_regions
                .push(slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    PresentationRegionId::from(format!("{}:hit", target.as_str())),
                    address.clone(),
                    TargetLocalRect::new(hit_bounds),
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    hit_order,
                    Some(format!("{}:hit", target.as_str())),
                    self.cursor(local),
                    true,
                    if matches!(self, Self::Slider(_) | Self::Overlay(_)) {
                        PointerCaptureIntent::DuringDrag
                    } else {
                        PointerCaptureIntent::OnPress
                    },
                ));
        }

        if matches!(self, Self::List(_)) {
            self.push_list_row_hit_regions(
                &mut declarations,
                external,
                local,
                address.clone(),
                bounds,
            );
        }

        if matches!(self, Self::NestedScroll(_)) {
            self.push_nested_scroll_regions(
                &mut declarations,
                external,
                local,
                layout,
                address.clone(),
                bounds,
            );
        }

        if matches!(self, Self::Text(_)) {
            declarations
                .focus_regions
                .push(slipway_core::text_edit_focus_region_from_capability(
                    self,
                    external,
                    local,
                    PresentationRegionId::from(format!("{}:focus", target.as_str())),
                    address.clone(),
                    TargetLocalRect::new(Self::text_input_focus_bounds(bounds)),
                    slipway_core::SlipwayFocusTraversal::focus_member(self, external, local),
                    true,
                    input,
                    None,
                ));
        }

        if matches!(self, Self::List(_)) {
            let terminal_region_index = declarations.scroll_regions.len();
            declarations.scroll_regions.push(
                slipway_core::scroll_region_from_scrollable_capability(
                    self,
                    external,
                    local,
                    layout,
                    Some(PresentationRegionId::from(format!(
                        "{}:scroll",
                        target.as_str()
                    ))),
                    address,
                    true,
                ),
            );
            declarations.wheel_traversal_boundary.terminal_region_index =
                Some(terminal_region_index);
        }

        declarations
    }

    fn has_pointer_region(&self) -> bool {
        matches!(
            self,
            Self::Action(_)
                | Self::Segment(_)
                | Self::Toggle(_)
                | Self::Slider(_)
                | Self::Overlay(_)
        )
    }

    fn push_list_row_hit_regions(
        &self,
        declarations: &mut AdmissionViewDeclarations,
        external: &AdmissionState,
        local: &AdmissionLocal,
        address: Option<WidgetSlotAddress>,
        bounds: Rect,
    ) {
        let target = self.id();
        // Hit bounds MUST derive from the same LIST_ROW_* constants as the
        // painted rows and the pointer math (see the constants' comment).
        for row in 0..LIST_ROW_COUNT {
            let id = PresentationRegionId::from(format!("{}:row-{row}", target.as_str()));
            declarations
                .hit_regions
                .push(slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    id,
                    address.clone(),
                    TargetLocalRect::new(Rect {
                        origin: Point {
                            x: 24.0,
                            y: LIST_ROW_TOP + row as f32 * LIST_ROW_STEP,
                        },
                        size: Size {
                            width: (bounds.size.width - 48.0).max(1.0),
                            height: 18.0,
                        },
                    }),
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    HitRegionOrder {
                        z_index: 1,
                        paint_order: row,
                        traversal_order: row,
                    },
                    Some(format!("{}:row-{row}", target.as_str())),
                    CursorCapability::Pointer,
                    true,
                    PointerCaptureIntent::OnPress,
                ));
        }
    }

    fn push_overlay_stack_hit_regions(
        &self,
        declarations: &mut AdmissionViewDeclarations,
        external: &AdmissionState,
        local: &AdmissionLocal,
        address: Option<WidgetSlotAddress>,
        bounds: Rect,
    ) {
        let target = self.id();
        let (offsets, order, dragging) = match local {
            AdmissionLocal::OverlayStack {
                offsets,
                order,
                dragging,
                ..
            } => (*offsets, *order, *dragging),
            _ => (
                Self::overlay_stack_default_offsets(),
                Self::overlay_stack_default_order(),
                None,
            ),
        };

        for index in 0..4 {
            declarations
                .hit_regions
                .push(slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    PresentationRegionId::from(format!("{}:order-{index}", target.as_str())),
                    address.clone(),
                    TargetLocalRect::new(Self::overlay_stack_order_button_rect(index)),
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    HitRegionOrder {
                        z_index: 0,
                        paint_order: index,
                        traversal_order: index,
                    },
                    Some(format!("{}:order-{index}", target.as_str())),
                    CursorCapability::Pointer,
                    true,
                    PointerCaptureIntent::OnPress,
                ));
        }

        for overlay in order {
            let rank = Self::overlay_stack_rank(order, overlay);
            let layer_spec = Self::overlay_stack_layer_spec(overlay, rank);
            let cursor = if dragging == Some(overlay) {
                CursorCapability::Grabbing
            } else {
                CursorCapability::Grab
            };
            declarations
                .hit_regions
                .push(slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    PresentationRegionId::from(format!(
                        "{}:overlay-{overlay}:absorb",
                        target.as_str()
                    )),
                    address.clone(),
                    TargetLocalRect::new(Self::overlay_stack_rect(bounds, offsets, overlay)),
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    layer_spec.absorb_hit_order(),
                    Some(format!("{}:overlay-{overlay}:absorb", target.as_str())),
                    CursorCapability::Pointer,
                    true,
                    PointerCaptureIntent::OnPress,
                ));
            declarations
                .hit_regions
                .push(slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    PresentationRegionId::from(format!(
                        "{}:overlay-{overlay}:drag",
                        target.as_str()
                    )),
                    address.clone(),
                    TargetLocalRect::new(Self::overlay_stack_drag_rect(bounds, offsets, overlay)),
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    layer_spec.drag_hit_order(),
                    Some(format!("{}:overlay-{overlay}:drag", target.as_str())),
                    cursor,
                    true,
                    PointerCaptureIntent::DuringDrag,
                ));
        }
    }

    fn push_nested_scroll_regions(
        &self,
        declarations: &mut AdmissionViewDeclarations,
        external: &AdmissionState,
        local: &AdmissionLocal,
        layout: &LayoutOutput,
        address: Option<WidgetSlotAddress>,
        bounds: Rect,
    ) {
        let Some((outer_scroll_rows, inner_scroll_rows)) = nested_scroll_local(local) else {
            return;
        };
        let outer_viewport = Self::nested_outer_viewport(bounds);
        let outer_offset_y = Self::nested_scroll_offset_y(
            outer_scroll_rows,
            NESTED_OUTER_ROW_STEP,
            outer_viewport.size.height,
            Self::nested_outer_content_height(),
        );
        let terminal_region_index = declarations.scroll_regions.len();
        declarations.scroll_regions.push(self.nested_scroll_region(
            external,
            local,
            layout,
            address.clone(),
            "outer",
            outer_viewport,
            Self::nested_outer_content_height(),
            outer_offset_y,
        ));
        declarations.wheel_traversal_boundary.terminal_region_index = Some(terminal_region_index);
        for index in 0..NESTED_PANEL_COUNT {
            let Some(viewport) =
                Self::nested_inner_visible_viewport(index, outer_offset_y, outer_viewport)
            else {
                continue;
            };
            let offset_y = Self::nested_scroll_offset_y(
                inner_scroll_rows[index],
                NESTED_SCROLL_ROW_STEP,
                viewport.size.height,
                Self::nested_inner_content_height(viewport.size.height),
            );
            declarations.scroll_regions.push(self.nested_scroll_region(
                external,
                local,
                layout,
                address.clone(),
                &format!("inner-{index}"),
                viewport,
                Self::nested_inner_content_height(viewport.size.height),
                offset_y,
            ));
        }
    }

    // Helper-then-patch idiom, and WHY the per-region override is
    // legitimate: `SlipwayScrollBehaviorPolicy::scroll_behavior_policy` is
    // a single-declaration surface (one geometry per widget), but this
    // widget declares FOUR overlapping regions (outer + 3 inners) with
    // distinct viewports, content heights, and offsets. The `_with_order`
    // helper still does the contract wiring worth keeping — region-id
    // resolution, the per-region wheel-routing snapshot (ADR-0002 B3), and
    // the consumption policy — and the geometry fields are then overridden
    // per region BEFORE the declaration is pushed, so admission validates
    // exactly what is declared. Do not copy this shape for a single-region
    // widget: there the policy itself should return the real geometry.
    fn nested_scroll_region(
        &self,
        external: &AdmissionState,
        local: &AdmissionLocal,
        layout: &LayoutOutput,
        address: Option<WidgetSlotAddress>,
        region: &str,
        viewport: Rect,
        content_height: f32,
        offset_y: f32,
    ) -> ScrollRegionDeclaration {
        // Overlapping wheel-consuming regions need distinct orders (the
        // `ambiguous_wheel_overlap` refusal): inners sit in front of the
        // outer, so the pointed-at inner wins under the default
        // `NearestScrollable` routing.
        let order = if let Some(index) = region
            .strip_prefix("inner-")
            .and_then(|value| value.parse::<usize>().ok())
        {
            HitRegionOrder {
                z_index: 1,
                paint_order: index + 1,
                traversal_order: index + 1,
            }
        } else {
            HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            }
        };
        let mut declaration = slipway_core::scroll_region_from_scrollable_capability_with_order(
            self,
            external,
            local,
            layout,
            Some(PresentationRegionId::from(format!(
                "{}:{region}",
                self.id().as_str()
            ))),
            address,
            true,
            order,
        );
        declaration.viewport = TargetLocalRect::new(viewport);
        declaration.content_bounds = TargetLocalRect::new(Rect {
            origin: viewport.origin,
            size: Size {
                width: viewport.size.width,
                height: content_height.max(viewport.size.height),
            },
        });
        declaration.offset = Point {
            x: 0.0,
            y: offset_y,
        };
        declaration
    }

    fn push_overlay_paint_ops(
        &self,
        out_ops: &mut Vec<PaintOp>,
        local: &AdmissionLocal,
        bounds: Rect,
    ) {
        let offset = match local {
            AdmissionLocal::Overlay { offset, .. } => *offset,
            _ => Self::overlay_default_offset(),
        };
        let overlay = Self::overlay_rect(bounds, offset);
        let titlebar = Self::overlay_titlebar_rect(bounds, offset);
        let mut window_ops = Vec::new();
        let ops = &mut window_ops;
        ops.push(PaintOp::Fill {
            shape: rect_shape(
                "overlay-window-shadow",
                overlay,
                ShapeKind::RoundedRectangle,
            ),
            color: rgba(15, 23, 42, 38),
        });
        ops.push(PaintOp::Fill {
            shape: rect_shape("overlay-window", overlay, ShapeKind::RoundedRectangle),
            color: rgb(255, 255, 255),
        });
        ops.push(PaintOp::Stroke {
            shape: rect_shape(
                "overlay-window-outline",
                overlay,
                ShapeKind::RoundedRectangle,
            ),
            color: rgb(124, 58, 237),
            width: 1.5,
        });
        ops.push(PaintOp::Fill {
            shape: rect_shape("overlay-titlebar", titlebar, ShapeKind::RoundedRectangle),
            color: rgb(237, 233, 254),
        });
        ops.push(PaintOp::Text {
            bounds: Rect {
                origin: Point {
                    x: titlebar.origin.x + 12.0,
                    y: titlebar.origin.y + 8.0,
                },
                size: Size {
                    width: titlebar.size.width - 24.0,
                    height: 16.0,
                },
            },
            content: "drag overlay".to_string(),
            color: rgb(76, 29, 149),
            style: TextStyle::plain().with_font_size(12.0),
        });
        let graph = Rect {
            origin: Point {
                x: overlay.origin.x + 14.0,
                y: overlay.origin.y + 44.0,
            },
            size: Size {
                width: overlay.size.width - 28.0,
                height: overlay.size.height - 58.0,
            },
        };
        ops.push(PaintOp::Fill {
            shape: rect_shape("overlay-complex-bg", graph, ShapeKind::RoundedRectangle),
            color: rgb(248, 250, 252),
        });
        ops.push(PaintOp::Stroke {
            shape: path_shape(
                "overlay-curve",
                graph,
                PathDeclaration {
                    commands: vec![
                        PathCommand::MoveTo(Point {
                            x: graph.origin.x + 12.0,
                            y: graph.origin.y + graph.size.height * 0.70,
                        }),
                        PathCommand::CubicTo {
                            control_1: Point {
                                x: graph.origin.x + graph.size.width * 0.28,
                                y: graph.origin.y + graph.size.height * 0.10,
                            },
                            control_2: Point {
                                x: graph.origin.x + graph.size.width * 0.56,
                                y: graph.origin.y + graph.size.height * 0.90,
                            },
                            to: Point {
                                x: graph.origin.x + graph.size.width - 14.0,
                                y: graph.origin.y + graph.size.height * 0.28,
                            },
                        },
                    ],
                },
            ),
            color: rgb(124, 58, 237),
            width: 2.0,
        });
        ops.push(PaintOp::Group {
            id: Some("overlay-curve-nodes".to_string()),
            clip: None,
            ops: (0..11)
                .map(|index| {
                    let t = index as f32 / 10.0;
                    let y = (t * std::f32::consts::PI * 1.75).sin() * 22.0;
                    PaintOp::Fill {
                        shape: rect_shape(
                            "overlay-curve-node",
                            Rect {
                                origin: Point {
                                    x: graph.origin.x + 10.0 + t * (graph.size.width - 28.0),
                                    y: graph.origin.y + graph.size.height * 0.45 - y,
                                },
                                size: Size {
                                    width: 8.0,
                                    height: 8.0,
                                },
                            },
                            ShapeKind::Circle,
                        ),
                        color: rgb(124, 58, 237),
                    }
                })
                .collect(),
        });
        for (index, (x, y, color)) in [
            (18.0, 34.0, rgb(20, 184, 166)),
            (64.0, 18.0, rgb(59, 130, 246)),
            (118.0, 56.0, rgb(245, 158, 11)),
            (166.0, 26.0, rgb(244, 63, 94)),
        ]
        .into_iter()
        .enumerate()
        {
            ops.push(PaintOp::Fill {
                shape: rect_shape(
                    "overlay-node",
                    Rect {
                        origin: Point {
                            x: graph.origin.x + x,
                            y: graph.origin.y + y,
                        },
                        size: Size {
                            width: 13.0 + index as f32 * 2.0,
                            height: 13.0 + index as f32 * 2.0,
                        },
                    },
                    ShapeKind::Circle,
                ),
                color,
            });
        }
        ops.push(PaintOp::Group {
            id: Some("overlay-bars".to_string()),
            clip: None,
            ops: (0..5)
                .map(|index| PaintOp::Fill {
                    shape: rect_shape(
                        "overlay-bar",
                        Rect {
                            origin: Point {
                                x: graph.origin.x + 16.0 + index as f32 * 30.0,
                                y: graph.origin.y + graph.size.height - 12.0 - index as f32 * 8.0,
                            },
                            size: Size {
                                width: 16.0,
                                height: 12.0 + index as f32 * 8.0,
                            },
                        },
                        ShapeKind::RoundedRectangle,
                    ),
                    color: rgba(124, 58, 237, 66),
                })
                .collect(),
        });
        out_ops.push(
            // Pointer-opaque (still catches clicks / drag on the titlebar) but
            // wheel-pass-through so wheeling over or while holding the movable
            // overlay scrolls the page behind it.
            PaintOp::keyed_layer(Self::overlay_layer_key(), window_ops)
                .with_layer_id("movable-overlay-window")
                .with_wheel_transparency(PaintInputTransparency::PassThrough),
        );
    }

    fn push_overlay_stack_paint_ops(
        &self,
        ops: &mut Vec<PaintOp>,
        local: &AdmissionLocal,
        bounds: Rect,
    ) {
        let (offsets, order, dragging) = match local {
            AdmissionLocal::OverlayStack {
                offsets,
                order,
                dragging,
                ..
            } => (*offsets, *order, *dragging),
            _ => (
                Self::overlay_stack_default_offsets(),
                Self::overlay_stack_default_order(),
                None,
            ),
        };

        ops.push(PaintOp::Text {
            bounds: Self::overlay_stack_help_text_rect(bounds),
            content: "front buttons; drag cards".to_string(),
            color: rgb(71, 85, 105),
            style: TextStyle::plain().with_font_size(12.0),
        });

        for index in 0..4 {
            let rect = Self::overlay_stack_order_button_rect(index);
            let is_front = order[3] == index;
            ops.push(PaintOp::Fill {
                shape: rect_shape(
                    "overlay-stack-order-button",
                    rect,
                    ShapeKind::RoundedRectangle,
                ),
                color: if is_front {
                    rgb(124, 58, 237)
                } else {
                    rgb(237, 233, 254)
                },
            });
            ops.push(PaintOp::Text {
                bounds: Rect {
                    origin: Point {
                        x: rect.origin.x + 13.0,
                        y: rect.origin.y + 6.0,
                    },
                    size: Size {
                        width: 24.0,
                        height: 14.0,
                    },
                },
                content: overlay_stack_label(index).to_string(),
                color: if is_front {
                    rgb(255, 255, 255)
                } else {
                    rgb(76, 29, 149)
                },
                style: TextStyle::plain().with_font_size(12.0),
            });
        }

        for overlay in order {
            let rank = Self::overlay_stack_rank(order, overlay);
            let layer_spec = Self::overlay_stack_layer_spec(overlay, rank);
            let mut window_ops = Vec::new();
            self.push_overlay_stack_window_paint_ops(
                &mut window_ops,
                bounds,
                offsets,
                overlay,
                rank,
                dragging == Some(overlay),
            );
            ops.push(layer_spec.paint_layer(window_ops));
        }
    }

    fn push_overlay_stack_window_paint_ops(
        &self,
        ops: &mut Vec<PaintOp>,
        bounds: Rect,
        offsets: [Point; 4],
        index: usize,
        rank: usize,
        dragging: bool,
    ) {
        let overlay = Self::overlay_stack_rect(bounds, offsets, index);
        let titlebar = Self::overlay_stack_titlebar_rect(bounds, offsets, index);
        let palette = [
            (rgb(237, 233, 254), rgb(124, 58, 237)),
            (rgb(219, 234, 254), rgb(37, 99, 235)),
            (rgb(204, 251, 241), rgb(15, 118, 110)),
            (rgb(254, 243, 199), rgb(180, 83, 9)),
        ][index];

        ops.push(PaintOp::Fill {
            shape: rect_shape("overlay-stack-shadow", overlay, ShapeKind::RoundedRectangle),
            color: rgba(15, 23, 42, 28 + rank as u8 * 8),
        });
        ops.push(PaintOp::Fill {
            shape: rect_shape("overlay-stack-window", overlay, ShapeKind::RoundedRectangle),
            color: rgb(255, 255, 255),
        });
        ops.push(PaintOp::Stroke {
            shape: rect_shape(
                "overlay-stack-outline",
                overlay,
                ShapeKind::RoundedRectangle,
            ),
            color: palette.1,
            width: if dragging { 2.5 } else { 1.5 },
        });
        ops.push(PaintOp::Fill {
            shape: rect_shape(
                "overlay-stack-titlebar",
                titlebar,
                ShapeKind::RoundedRectangle,
            ),
            color: palette.0,
        });
        ops.push(PaintOp::Text {
            bounds: Rect {
                origin: Point {
                    x: titlebar.origin.x + 10.0,
                    y: titlebar.origin.y + 7.0,
                },
                size: Size {
                    width: titlebar.size.width - 20.0,
                    height: 14.0,
                },
            },
            content: format!("{} layer {rank}", overlay_stack_label(index)),
            color: palette.1,
            style: TextStyle::plain().with_font_size(12.0),
        });

        let graph = Rect {
            origin: Point {
                x: overlay.origin.x + 12.0,
                y: overlay.origin.y + 40.0,
            },
            size: Size {
                width: overlay.size.width - 24.0,
                height: overlay.size.height - 52.0,
            },
        };
        ops.push(PaintOp::Stroke {
            shape: path_shape(
                "overlay-stack-line",
                graph,
                PathDeclaration {
                    commands: vec![
                        PathCommand::MoveTo(Point {
                            x: graph.origin.x + 4.0,
                            y: graph.origin.y + graph.size.height - 8.0,
                        }),
                        PathCommand::LineTo(Point {
                            x: graph.origin.x + graph.size.width * 0.33,
                            y: graph.origin.y + 10.0 + index as f32 * 4.0,
                        }),
                        PathCommand::LineTo(Point {
                            x: graph.origin.x + graph.size.width * 0.66,
                            y: graph.origin.y + graph.size.height * 0.62,
                        }),
                        PathCommand::LineTo(Point {
                            x: graph.origin.x + graph.size.width - 6.0,
                            y: graph.origin.y + 14.0,
                        }),
                    ],
                },
            ),
            color: palette.1,
            width: 2.0,
        });
        for point in 0..4 {
            ops.push(PaintOp::Fill {
                shape: rect_shape(
                    "overlay-stack-node",
                    Rect {
                        origin: Point {
                            x: graph.origin.x + 10.0 + point as f32 * 38.0,
                            y: graph.origin.y + 8.0 + ((point + index) % 3) as f32 * 14.0,
                        },
                        size: Size {
                            width: 8.0,
                            height: 8.0,
                        },
                    },
                    ShapeKind::Circle,
                ),
                color: palette.1,
            });
        }
    }

    fn push_nested_scroll_paint_ops(
        &self,
        ops: &mut Vec<PaintOp>,
        local: &AdmissionLocal,
        bounds: Rect,
    ) {
        let (outer_rows, inner_rows) = nested_scroll_local(local).unwrap_or((0, [0, 0, 0]));
        let outer = Self::nested_outer_viewport(bounds);
        let outer_offset = Self::nested_scroll_offset_y(
            outer_rows,
            NESTED_OUTER_ROW_STEP,
            outer.size.height,
            Self::nested_outer_content_height(),
        );
        ops.push(PaintOp::Fill {
            shape: rect_shape("nested-outer-bg", outer, ShapeKind::RoundedRectangle),
            color: rgb(241, 245, 249),
        });
        ops.push(PaintOp::Stroke {
            shape: rect_shape("nested-outer-outline", outer, ShapeKind::RoundedRectangle),
            color: rgb(148, 163, 184),
            width: 1.0,
        });
        let mut content_ops = Vec::new();
        for index in 0..NESTED_PANEL_COUNT {
            let viewport = Self::nested_inner_content_viewport(index, outer_offset);
            content_ops.push(PaintOp::Fill {
                shape: rect_shape("nested-inner-bg", viewport, ShapeKind::RoundedRectangle),
                color: rgb(255, 255, 255),
            });
            content_ops.push(PaintOp::Stroke {
                shape: rect_shape(
                    "nested-inner-outline",
                    viewport,
                    ShapeKind::RoundedRectangle,
                ),
                color: rgb(203, 213, 225),
                width: 1.0,
            });
            // The panel is a routed (non-native) scroll region, so the author
            // paints the VISIBLE window. The rows virtualize: the labels carry
            // the scroll offset (`row + inner_rows`), so the row positions stay
            // anchored to the panel. Subtracting the offset here as well would
            // apply it twice — the painted window would slide out of the clip,
            // showing labels that skip ahead two per notch and leaving the
            // panel completely blank from offset 4 on while it still consumes
            // wheels (the "collapsing" nested panel).
            let mut row_ops = Vec::new();
            for row in 0..4 {
                row_ops.push(PaintOp::Text {
                    bounds: Rect {
                        origin: Point {
                            x: viewport.origin.x + 12.0,
                            y: viewport.origin.y + 8.0 + row as f32 * NESTED_SCROLL_ROW_STEP,
                        },
                        size: Size {
                            width: viewport.size.width - 24.0,
                            height: 16.0,
                        },
                    },
                    content: format!("inner {} row {}", index + 1, row + inner_rows[index] + 1),
                    color: rgb(51, 65, 85),
                    style: TextStyle::plain().with_font_size(12.0),
                });
            }
            content_ops.push(PaintOp::Group {
                id: Some(format!("nested-inner-{index}-rows")),
                clip: Some(slipway_core::ClipDeclaration {
                    id: Some(format!("nested-inner-{index}-clip")),
                    bounds: viewport,
                    path: None,
                }),
                ops: row_ops,
            });
        }
        // The outer declares `nested_outer_content_height()` of content so
        // the outer's travel spans `NESTED_OUTER_MAX_ROWS` rows (the Step
        // 194 extent/handler alignment), but the panels end above that
        // extent. Paint tail markers through the declared remainder (below
        // the panel field, up to the declared extent) so deep outer offsets
        // show scrolling content instead of a blank band (the "collapsed"
        // outer). Marker placement derives from the panel field bottom so it
        // tracks the anchoring-capped travel.
        let outer_content_height = Self::nested_outer_content_height();
        // Derived from the same NESTED_PANEL_* constants as the declared
        // inner viewports (see the constants' comment): content-local
        // bottom edge of the last panel.
        let panel_field_bottom = NESTED_PANEL_FIELD_TOP
            + (NESTED_PANEL_COUNT - 1) as f32 * NESTED_PANEL_PITCH
            + NESTED_INNER_VIEWPORT_HEIGHT;
        let mut marker = 0;
        let mut content_y = panel_field_bottom + 8.0;
        while content_y + 16.0 <= outer_content_height {
            content_ops.push(PaintOp::Text {
                bounds: Rect {
                    origin: Point {
                        x: 32.0,
                        y: outer.origin.y + content_y - outer_offset,
                    },
                    size: Size {
                        width: 156.0,
                        height: 16.0,
                    },
                },
                content: format!("outer tail {}", marker + 1),
                color: rgb(100, 116, 139),
                style: TextStyle::plain().with_font_size(12.0),
            });
            marker += 1;
            content_y += 30.0;
        }
        content_ops.push(PaintOp::Text {
            bounds: Rect {
                origin: Point {
                    x: outer.origin.x + 212.0,
                    y: outer.origin.y + 18.0,
                },
                size: Size {
                    width: (outer.size.width - 236.0).max(1.0),
                    height: 32.0,
                },
            },
            content: "Wheel over outer or each inner panel; MCP should report distinct region ids."
                .to_string(),
            color: rgb(100, 116, 139),
            style: TextStyle::plain().with_font_size(12.0),
        });
        ops.push(PaintOp::Group {
            id: Some("nested-scroll-content".to_string()),
            clip: Some(slipway_core::ClipDeclaration {
                id: Some("nested-outer-clip".to_string()),
                bounds: outer,
                path: None,
            }),
            ops: content_ops,
        });
    }

    fn cursor(&self, local: &AdmissionLocal) -> CursorCapability {
        match (self, local) {
            (Self::Slider(_), AdmissionLocal::Slider { dragging: true }) => {
                CursorCapability::Grabbing
            }
            (Self::Slider(_), _) => CursorCapability::Grab,
            (Self::Overlay(_), AdmissionLocal::Overlay { dragging: true, .. }) => {
                CursorCapability::Grabbing
            }
            (Self::Overlay(_), _) => CursorCapability::Grab,
            _ => CursorCapability::Pointer,
        }
    }

    fn event_route(
        &self,
        target: &WidgetId,
        address: Option<WidgetSlotAddress>,
        suffix: &str,
    ) -> EventRoute {
        EventRoute {
            route_id: Some(format!("{}:{suffix}", target.as_str())),
            address,
            path: vec![target.clone()],
            phase: EventRoutePhase::Target,
        }
    }

    fn semantic_label(&self, external: &AdmissionState, _local: &AdmissionLocal) -> String {
        match self {
            Self::Action(_) => format!("Counter button {}", external.counter),
            Self::Segment(_) => format!("Segment {}", external.segment.label()),
            Self::Text(_) => format!("Draft {}", external.draft),
            Self::Toggle(_) => {
                if external.enabled {
                    "Notifications enabled".to_string()
                } else {
                    "Notifications disabled".to_string()
                }
            }
            Self::Slider(_) => format!("Intensity {:.0} percent", external.intensity * 100.0),
            Self::List(_) => format!("Selected row {}", external.selected_item + 1),
            Self::Overlay(_) => "Movable overlay window".to_string(),
            Self::OverlayStack(_) => "Ordered overlay stack".to_string(),
            Self::NestedScroll(_) => "Nested scroll verification".to_string(),
        }
        .to_string()
    }

    fn semantic_value(&self, external: &AdmissionState, local: &AdmissionLocal) -> String {
        match self {
            Self::Action(_) => external.counter.to_string(),
            Self::Segment(_) => external.segment.label().to_string(),
            Self::Text(_) => external.draft.clone(),
            Self::Toggle(_) => external.enabled.to_string(),
            Self::Slider(_) => format!("{:.2}", external.intensity),
            Self::List(_) => match local {
                AdmissionLocal::List { scroll_rows } => {
                    format!("row {}, scroll {}", external.selected_item + 1, scroll_rows)
                }
                _ => (external.selected_item + 1).to_string(),
            },
            Self::Overlay(_) => match local {
                AdmissionLocal::Overlay {
                    offset, dragging, ..
                } => {
                    format!(
                        "offset {:.0},{:.0}, dragging {}",
                        offset.x, offset.y, dragging
                    )
                }
                _ => "overlay".to_string(),
            },
            Self::OverlayStack(_) => match local {
                AdmissionLocal::OverlayStack {
                    order, dragging, ..
                } => {
                    let front = overlay_stack_label(order[3]);
                    let dragging = dragging.map(overlay_stack_label).unwrap_or("none");
                    format!("front {front}, dragging {dragging}")
                }
                _ => "overlay-stack".to_string(),
            },
            Self::NestedScroll(_) => match local {
                AdmissionLocal::NestedScroll {
                    outer_scroll_rows,
                    inner_scroll_rows,
                } => {
                    format!("outer {outer_scroll_rows}, inner {inner_scroll_rows:?}")
                }
                _ => "nested-scroll".to_string(),
            },
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct AdmissionViewDeclarations {
    hit_regions: Vec<HitRegionDeclaration>,
    focus_regions: Vec<FocusRegionDeclaration>,
    scroll_regions: Vec<ScrollRegionDeclaration>,
    wheel_traversal_boundary: slipway_core::DeclaredWheelTraversalBoundary,
    semantic_slots: Vec<SemanticSlotDeclaration>,
    probe_metadata: Vec<ProbeMetadataDeclaration>,
    diagnostics: Vec<Diagnostic>,
}

impl slipway_core::SlipwayTextBufferPolicy for AdmissionWidget {
    fn text_buffer(
        &self,
        external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> TextBufferSnapshot {
        TextBufferSnapshot {
            target: self.id(),
            text: external.draft.clone(),
            revision: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayTextSelectionPolicy for AdmissionWidget {
    fn text_selection(
        &self,
        external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> TextSelectionPolicyDeclaration {
        let caret = external.draft.chars().count();
        TextSelectionPolicyDeclaration::editable_text(self.id(), None, CaretSet::single(caret))
    }
}

impl slipway_core::SlipwayImeCompositionPolicy for AdmissionWidget {
    fn ime_composition(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> ImeCompositionPolicyDeclaration {
        ImeCompositionPolicyDeclaration {
            target: self.id(),
            active: false,
            preedit_text: None,
            cursor_range: None,
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayCaretGeometryPolicy for AdmissionWidget {
    fn caret_geometry(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _measurement: Option<&slipway_core::TextMeasurementEvidence>,
    ) -> CaretGeometryEvidence {
        CaretGeometryEvidence::measured(
            self.id(),
            slipway_core::NonEmptyTextRects::one(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 1.0,
                    height: 18.0,
                },
            }),
            slipway_core::TextSelectionGeometry::no_selection(),
        )
    }
}

impl slipway_core::SlipwayTextEditCommandPolicy for AdmissionWidget {
    fn text_edit_commands(
        &self,
        external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> Vec<TextEditCommandDeclaration> {
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

impl slipway_core::SlipwayTextInputVisualStylePolicy for AdmissionWidget {
    fn text_input_visual_style(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> slipway_core::TextInputVisualStyleDeclaration {
        slipway_core::TextInputVisualStyleDeclaration::explicit(
            self.id(),
            rgb(15, 23, 42),
            rgb(100, 116, 139),
            rgb(15, 23, 42),
            rgb(191, 219, 254),
            rgb(248, 250, 252),
            rgb(203, 213, 225),
            1.0,
            4.0,
            rgb(15, 23, 42),
        )
    }
}

impl slipway_core::SlipwayTextInputTypographyPolicy for AdmissionWidget {
    fn text_input_typography(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> TextInputTypographyDeclaration {
        admission_text_input_typography(self.id())
    }
}

impl slipway_core::SlipwayTextUndoRedoPolicy for AdmissionWidget {
    fn text_undo_redo(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> slipway_core::TextUndoRedoEvidence {
        slipway_core::TextUndoRedoEvidence {
            target: self.id(),
            can_undo: false,
            can_redo: false,
            undo_depth: Some(0),
            redo_depth: Some(0),
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayTextFlowPolicy for AdmissionWidget {
    fn text_flow_policy(
        &self,
        external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: &LayoutInput,
    ) -> slipway_core::TextFlowPolicy {
        slipway_core::TextFlowPolicy {
            target: self.id(),
            line_mode: TextLineMode::SingleLine,
            wrap: slipway_core::TextWrapMode::NoWrap,
            line_clamp: Some(1),
            allow_ellipsis: true,
            baseline: None,
            caret_bounds: slipway_core::TextCaretGeometry::unavailable(
                "text flow policy does not claim caret bounds",
            ),
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

impl slipway_core::SlipwayTextMeasurementPolicy for AdmissionWidget {
    fn text_measurement_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: &LayoutInput,
    ) -> slipway_core::TextMeasurementPolicyDeclaration {
        slipway_core::TextMeasurementPolicyDeclaration {
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
    ) -> slipway_core::TextMeasurementEvidence
    where
        P: slipway_core::SlipwayTextMetricProvider,
    {
        slipway_core::TextMeasurementEvidence {
            target: self.id(),
            policy: self.text_measurement_policy(external, local, input),
            receipts: Vec::new(),
            cache: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayTextMeasurementCachePolicy for AdmissionWidget {
    fn text_measurement_cache_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: &LayoutInput,
    ) -> Vec<slipway_core::TextMeasurementCachePolicyDeclaration> {
        Vec::new()
    }
}

impl slipway_core::SlipwayCachedTextMeasurementPolicy for AdmissionWidget {
    fn cached_text_measurement_evidence<P, C>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
        provider: &mut P,
        _cache: &mut C,
    ) -> slipway_core::TextMeasurementEvidence
    where
        P: slipway_core::SlipwayTextMetricProvider,
        C: slipway_core::SlipwayTextMeasurementCache,
    {
        slipway_core::SlipwayTextMeasurementPolicy::text_measurement_evidence(
            self, external, local, input, provider,
        )
    }
}

impl slipway_core::SlipwayFocusTraversal for AdmissionWidget {
    fn focus_member(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> Option<FocusTraversalMember> {
        Some(FocusTraversalMember {
            target: self.id(),
            scope: None,
            tab_order: Some(self.traversal_order() as i32),
        })
    }

    fn next_focus(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: slipway_core::FocusTraversalInput,
    ) -> Option<WidgetId> {
        None
    }

    fn previous_focus(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: slipway_core::FocusTraversalInput,
    ) -> Option<WidgetId> {
        None
    }
}

impl slipway_core::SlipwaySemantics for AdmissionWidget {
    fn semantics(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<SemanticNode> {
        vec![SemanticNode {
            id: self.id(),
            role: self.role().to_string(),
            label: Some(self.semantic_label(external, local)),
            value: Some(self.semantic_value(external, local)),
            bounds: None,
            states: Vec::new(),
            actions: Vec::new(),
            relationships: Vec::new(),
        }]
    }
}

impl slipway_core::SlipwayEventRoutingPolicy for AdmissionWidget {
    fn event_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        event: &InputEvent,
    ) -> slipway_core::EventRoutingPolicyDeclaration {
        let address = event.target_slot().cloned();
        slipway_core::EventRoutingPolicyDeclaration {
            target: self.id(),
            event_target: event.target().clone(),
            route: self.event_route(&self.id(), address, "policy"),
            capture: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayEventDispositionPolicy for AdmissionWidget {
    fn event_disposition(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        event: &InputEvent,
        route: &EventRoute,
    ) -> slipway_core::EventPropagationEvidence {
        let handled = event.target() == &self.id()
            && match (self, event) {
                (Self::Action(_), InputEvent::Pointer(pointer))
                | (Self::Segment(_), InputEvent::Pointer(pointer))
                | (Self::Toggle(_), InputEvent::Pointer(pointer)) => matches!(
                    pointer.kind,
                    slipway_core::PointerEventKind::Press | slipway_core::PointerEventKind::Release
                ),
                (Self::Slider(_), InputEvent::Pointer(pointer)) => matches!(
                    pointer.kind,
                    slipway_core::PointerEventKind::Press
                        | slipway_core::PointerEventKind::Move
                        | slipway_core::PointerEventKind::Release
                ),
                (Self::Text(_), InputEvent::Text(_))
                | (Self::Text(_), InputEvent::TextEdit(_))
                | (Self::Text(_), InputEvent::Focus(_))
                | (Self::Text(_), InputEvent::Command(_)) => true,
                (Self::Text(_), InputEvent::Keyboard(key)) => {
                    key.kind == slipway_core::KeyEventKind::Press
                        && key.key.to_ascii_lowercase().contains("backspace")
                }
                (Self::List(_), InputEvent::Wheel(_)) | (Self::List(_), InputEvent::Scroll(_)) => {
                    true
                }
                (Self::List(_), InputEvent::Pointer(pointer)) => {
                    matches!(
                        pointer.kind,
                        slipway_core::PointerEventKind::Press
                            | slipway_core::PointerEventKind::Release
                    ) && list_row_from_pointer_y(pointer.position.y).is_some()
                }
                (Self::Overlay(_), InputEvent::Pointer(pointer)) => match pointer.kind {
                    slipway_core::PointerEventKind::Press
                    | slipway_core::PointerEventKind::Release
                    | slipway_core::PointerEventKind::Cancel => true,
                    slipway_core::PointerEventKind::Move => {
                        matches!(_local, AdmissionLocal::Overlay { dragging: true, .. })
                    }
                    _ => false,
                },
                (Self::OverlayStack(_), InputEvent::Pointer(pointer)) => match pointer.kind {
                    slipway_core::PointerEventKind::Press
                    | slipway_core::PointerEventKind::Release
                    | slipway_core::PointerEventKind::Cancel => true,
                    slipway_core::PointerEventKind::Move => matches!(
                        _local,
                        AdmissionLocal::OverlayStack {
                            dragging: Some(_),
                            ..
                        }
                    ),
                    _ => false,
                },
                (Self::NestedScroll(_), InputEvent::Wheel(_))
                | (Self::NestedScroll(_), InputEvent::Scroll(_)) => true,
                _ => false,
            };
        let disposition = slipway_core::EventDisposition {
            handled,
            propagate: !handled,
            default_action_allowed: true,
        };
        slipway_core::EventPropagationEvidence {
            target: self.id(),
            event: event.clone(),
            steps: vec![slipway_core::EventPropagationStep {
                stage: slipway_core::EventPropagationStage::Target,
                node: route.path.last().cloned(),
                disposition,
                emitted_messages: Vec::new(),
                changes: Vec::new(),
            }],
            final_disposition: disposition,
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayDebugEventTracePolicy for AdmissionWidget {
    fn debug_event_trace_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> slipway_core::DebugEventTracePolicyDeclaration {
        slipway_core::DebugEventTracePolicyDeclaration {
            target: self.id(),
            request_only: true,
            include_route: true,
            include_messages: true,
            include_state_changes: true,
            include_repaint_request: true,
        }
    }
}

impl slipway_core::SlipwayContainerLayoutPolicy for AdmissionWidget {
    fn container_layout_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: &LayoutInput,
    ) -> slipway_core::ContainerLayoutPolicyDeclaration {
        slipway_core::ContainerLayoutPolicyDeclaration {
            target: self.id(),
            kind: slipway_core::ContainerLayoutKind::Column,
            child_order: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayChildConstraintPolicy for AdmissionWidget {
    fn child_constraints(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _input: &LayoutInput,
    ) -> Vec<slipway_core::ChildConstraintPolicyDeclaration> {
        Vec::new()
    }
}

impl slipway_core::SlipwayLayoutInvalidationPolicy for AdmissionWidget {
    fn layout_invalidation_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> slipway_core::LayoutInvalidationPolicyDeclaration {
        slipway_core::LayoutInvalidationPolicyDeclaration {
            target: self.id(),
            dependencies: Vec::new(),
            revisions: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayLayoutEvidencePolicy for AdmissionWidget {
    fn layout_evidence(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        output: &LayoutOutput,
    ) -> slipway_core::LayoutEvidence {
        slipway_core::LayoutEvidence {
            target: self.id(),
            bounds: *output.bounds(),
            child_placements: output.child_placements().to_vec(),
            invalidated: false,
            diagnostics: output.diagnostics().to_vec(),
        }
    }
}

impl slipway_core::SlipwayScrollBehaviorPolicy for AdmissionWidget {
    fn scroll_behavior_policy(
        &self,
        _external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> slipway_core::ScrollBehaviorPolicyDeclaration {
        let viewport = input.viewport.into_rect();
        let content_height = if matches!(self, Self::NestedScroll(_)) {
            Self::nested_outer_content_height().max(viewport.size.height)
        } else {
            list_scroll_content_height(viewport.size.height)
        };
        let scroll_y = match local {
            AdmissionLocal::List { scroll_rows } => {
                list_scroll_offset_y(*scroll_rows, viewport.size.height)
            }
            AdmissionLocal::NestedScroll {
                outer_scroll_rows, ..
            } => Self::nested_scroll_offset_y(
                *outer_scroll_rows,
                NESTED_OUTER_ROW_STEP,
                viewport.size.height,
                content_height,
            ),
            _ => 0.0,
        };
        slipway_core::ScrollBehaviorPolicyDeclaration {
            target: self.id(),
            region_id: Some(PresentationRegionId::from(format!(
                "{}:scroll",
                self.id().as_str()
            ))),
            address: None,
            axes: slipway_core::ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            extent: Size {
                width: viewport.size.width,
                height: content_height,
            },
            viewport: input.viewport,
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: viewport.size.width,
                    height: content_height,
                },
            }),
            offset: Point {
                x: 0.0,
                y: scroll_y,
            },
            consumption: slipway_core::ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayWheelRoutingPolicy for AdmissionWidget {
    fn wheel_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        region: &slipway_core::PresentationRegionId,
    ) -> slipway_core::WheelRoutingPolicyDeclaration {
        // The declaration-time call's `region` names the region being
        // declared (ADR-0002 B3 / Step 197), so this single impl can author
        // a different mode per region. A non-default route is authored by
        // matching the widget + region id, e.g. the Step 194 declaration was
        //
        //     matches!(self, Self::NestedScroll(_))
        //         && region.as_str().ends_with(":outer")
        //         => WheelRouting::SelfFirst
        //
        // This example now deliberately authors NOTHING non-default: every
        // region (the nested outer AND inners, the List, the root page
        // region) stays on the `NearestScrollable` default, so the region
        // under the cursor scrolls first — wheel over an inner panel scrolls
        // THAT inner, chains to the outer at the inner's limit, and to the
        // page at the outer's limit. The Step 194 `SelfFirst`-on-outer
        // authoring was reverted by an architect live-UX decision
        // (2026-07-11: pointing the wheel at an inner moved the outer). The
        // authored-routing API itself stays proven by the synthetic-fixture
        // suites (core `SelfFirst`/`ParentFirst` selection, iced/egui
        // end-to-end routing, and the dispatch-graph flip tests).
        let _ = region;
        slipway_core::WheelRoutingPolicyDeclaration {
            target: self.id(),
            routing: WheelRouting::NearestScrollable,
            modifiers: None,
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayViewportObservationPolicy for AdmissionWidget {
    fn viewport_observation(
        &self,
        _external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> slipway_core::ViewportObservationEvidence {
        // Evidence viewport derives from the same card constants as
        // `layout`/`layout_plan` (see CARD_MAX_WIDTH's comment).
        let viewport = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: CARD_MAX_WIDTH,
                height: LIST_CARD_HEIGHT,
            },
        };
        let offset_y = match local {
            AdmissionLocal::List { scroll_rows } => {
                list_scroll_offset_y(*scroll_rows, viewport.size.height)
            }
            AdmissionLocal::NestedScroll {
                outer_scroll_rows, ..
            } => Self::nested_scroll_offset_y(
                *outer_scroll_rows,
                NESTED_OUTER_ROW_STEP,
                viewport.size.height,
                Self::nested_outer_content_height(),
            ),
            _ => 0.0,
        };
        let content_height = if matches!(self, Self::NestedScroll(_)) {
            Self::nested_outer_content_height()
        } else {
            list_scroll_content_height(viewport.size.height)
        };
        slipway_core::ViewportObservationEvidence {
            target: self.id(),
            viewport: TargetLocalRect::new(viewport),
            visible_rect: TargetLocalRect::new(viewport),
            scroll: Some(slipway_core::ScrollState {
                target: self.id(),
                region_id: Some(PresentationRegionId::from(format!(
                    "{}:scroll",
                    self.id().as_str()
                ))),
                address: None,
                offset_x: 0.0,
                offset_y,
                axes: slipway_core::ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                extent: Size {
                    width: viewport.size.width,
                    height: content_height,
                },
                viewport,
                content_bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: viewport.size.width,
                        height: content_height,
                    },
                },
                consumption: Vec::new(),
            }),
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayVirtualCollectionPolicy for AdmissionWidget {
    fn virtual_collection_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> slipway_core::VirtualCollectionPolicyDeclaration {
        slipway_core::VirtualCollectionPolicyDeclaration {
            target: self.id(),
            item_count: 5,
            visible_range: Some(slipway_core::ItemRange { start: 0, end: 3 }),
            realization_hint: slipway_core::VirtualizationHint::None,
            diagnostics: Vec::new(),
        }
    }
}

impl slipway_core::SlipwayHitTesting for AdmissionWidget {
    fn hit_test(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: slipway_core::HitTestInput,
    ) -> slipway_core::HitTestOutput {
        slipway_core::HitTestOutput {
            target: Some(self.id()),
            local_point: Some(input.point),
            route: self.event_route(&self.id(), None, "hit-test"),
            diagnostics: Vec::new(),
        }
    }
}

impl SlipwayWidgetTypes for AdmissionWidget {
    type ExternalState = AdmissionState;
    type LocalState = AdmissionLocal;
    type AppMessage = AdmissionMessage;
}

impl SlipwaySsot for AdmissionWidget {
    fn id(&self) -> WidgetId {
        match self {
            Self::Action(_) => WidgetId::from("admission.action"),
            Self::Segment(_) => WidgetId::from("admission.segment"),
            Self::Text(_) => WidgetId::from("admission.text"),
            Self::Toggle(_) => WidgetId::from("admission.toggle"),
            Self::Slider(_) => WidgetId::from("admission.slider"),
            Self::List(_) => WidgetId::from("admission.list"),
            Self::Overlay(_) => WidgetId::from("admission.overlay"),
            Self::OverlayStack(_) => WidgetId::from("admission.overlay-stack"),
            Self::NestedScroll(_) => WidgetId::from("admission.nested-scroll"),
        }
    }

    fn capabilities(&self) -> Vec<Capability> {
        match self {
            Self::Action(_) => vec![Capability::PointerInput, Capability::Paint],
            Self::Segment(_) => vec![Capability::PointerInput, Capability::Paint],
            Self::Text(_) => vec![
                Capability::TextInput,
                Capability::KeyboardInput,
                Capability::FocusInput,
                Capability::Paint,
            ],
            Self::Toggle(_) => vec![Capability::PointerInput, Capability::Paint],
            Self::Slider(_) => vec![Capability::PointerInput, Capability::Paint],
            Self::List(_) => vec![
                Capability::WheelInput,
                Capability::PointerInput,
                Capability::ScrollRegionPresentation,
                Capability::Paint,
            ],
            Self::Overlay(_) => vec![
                Capability::PointerInput,
                Capability::ShapePathClipPresentation,
                Capability::Paint,
            ],
            Self::OverlayStack(_) => vec![
                Capability::PointerInput,
                Capability::ShapePathClipPresentation,
                Capability::Paint,
            ],
            Self::NestedScroll(_) => vec![
                Capability::WheelInput,
                Capability::ScrollRegionPresentation,
                Capability::Paint,
            ],
        }
    }

    fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
        TopologyNode {
            id: self.id(),
            children: Vec::new(),
            local_state_slot: Some(WidgetSlotAddress::new(self.id(), 0)),
        }
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        Vec::new()
    }
}

impl SlipwayLogic for AdmissionWidget {
    fn handle_event(
        &self,
        external: &Self::ExternalState,
        local: &mut Self::LocalState,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        if event.target() != &self.id() {
            return EventOutcome::ignored();
        }

        match (self, event) {
            (Self::Action(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Press =>
            {
                *local = AdmissionLocal::Action { pressed: true };
                message_outcome(
                    self.id(),
                    "increment",
                    AdmissionMessage::Increment,
                    "pressed",
                    "true",
                )
            }
            (Self::Action(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Release =>
            {
                *local = AdmissionLocal::Action { pressed: false };
                handled_no_change_outcome(self.id())
            }
            (Self::Segment(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Press =>
            {
                if let AdmissionLocal::Segment { hover_count } = local {
                    *hover_count += 1;
                }
                message_outcome(
                    self.id(),
                    "select-next-segment",
                    AdmissionMessage::SelectNextSegment,
                    "segment",
                    external.segment.next().label(),
                )
            }
            (Self::Segment(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Release =>
            {
                handled_no_change_outcome(self.id())
            }
            (Self::Text(_), InputEvent::Text(text)) => {
                if let AdmissionLocal::Text {
                    local_edit_count, ..
                } = local
                {
                    *local_edit_count += 1;
                }
                let next_text = format!("{}{}", external.draft, text.text);
                message_outcome(
                    self.id(),
                    "update-draft",
                    AdmissionMessage::UpdateDraft(next_text.clone()),
                    "draft",
                    next_text,
                )
            }
            (Self::Text(_), InputEvent::TextEdit(edit)) => {
                if let AdmissionLocal::Text {
                    local_edit_count, ..
                } = local
                {
                    *local_edit_count += 1;
                }
                let next_text = apply_text_edit_to_draft(&external.draft, &edit);
                message_outcome(
                    self.id(),
                    "update-draft",
                    AdmissionMessage::UpdateDraft(next_text.clone()),
                    "draft",
                    next_text,
                )
            }
            (Self::Text(_), InputEvent::Keyboard(key))
                if key.kind == slipway_core::KeyEventKind::Press
                    && key.key.to_ascii_lowercase().contains("backspace") =>
            {
                let mut next_text = external.draft.clone();
                next_text.pop();
                message_outcome(
                    self.id(),
                    "update-draft",
                    AdmissionMessage::UpdateDraft(next_text.clone()),
                    "draft",
                    next_text,
                )
            }
            (Self::Text(_), InputEvent::Focus(focus)) => {
                if let AdmissionLocal::Text { focused, .. } = local {
                    *focused = focus.focused;
                }
                local_change_outcome(
                    self.id(),
                    "text-focus",
                    "focused",
                    focus.focused.to_string(),
                )
            }
            (Self::Text(_), InputEvent::Command(command)) => {
                if let Some(text) = self.text_after_command(external, &command.command) {
                    message_outcome(
                        self.id(),
                        "update-draft",
                        AdmissionMessage::UpdateDraft(text),
                        "draft",
                        "probe-command",
                    )
                } else {
                    EventOutcome::ignored()
                }
            }
            (Self::Toggle(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Press =>
            {
                *local = AdmissionLocal::Toggle { armed: true };
                message_outcome(
                    self.id(),
                    "toggle-enabled",
                    AdmissionMessage::ToggleEnabled,
                    "enabled",
                    (!external.enabled).to_string(),
                )
            }
            (Self::Toggle(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Release =>
            {
                *local = AdmissionLocal::Toggle { armed: false };
                local_change_outcome(self.id(), "toggle-release", "armed", "false")
            }
            (Self::Slider(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Press =>
            {
                *local = AdmissionLocal::Slider { dragging: true };
                let value = intensity_from_pointer(pointer.position.x);
                message_outcome(
                    self.id(),
                    "set-intensity",
                    AdmissionMessage::SetIntensity(value),
                    "intensity",
                    format!("{value:.2}"),
                )
            }
            (Self::Slider(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Move =>
            {
                if matches!(local, AdmissionLocal::Slider { dragging: true }) {
                    let value = intensity_from_pointer(pointer.position.x);
                    message_outcome(
                        self.id(),
                        "set-intensity",
                        AdmissionMessage::SetIntensity(value),
                        "intensity",
                        format!("{value:.2}"),
                    )
                } else {
                    EventOutcome::ignored()
                }
            }
            (Self::Slider(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Release =>
            {
                if let AdmissionLocal::Slider { dragging } = local {
                    *dragging = false;
                }
                local_change_outcome(self.id(), "slider-release", "dragging", "false".to_string())
            }
            (Self::List(_), InputEvent::Wheel(wheel)) => {
                let next_rows = if let AdmissionLocal::List { scroll_rows } = local {
                    let next = list_scroll_rows_after_wheel(*scroll_rows, wheel.delta_y);
                    *scroll_rows = next;
                    next
                } else {
                    0
                };
                local_change_outcome(
                    self.id(),
                    "scroll-list-wheel",
                    "scroll-rows",
                    next_rows.to_string(),
                )
            }
            (Self::List(_), InputEvent::Scroll(scroll)) => {
                // Inverse of `list_scroll_offset_y`: same LIST_ROW_*
                // constants convert a declared offset back into rows.
                let next_rows = (scroll.offset_y / LIST_ROW_STEP)
                    .round()
                    .clamp(0.0, LIST_ROW_COUNT as f32) as i32;
                if let AdmissionLocal::List { scroll_rows } = local {
                    *scroll_rows = next_rows;
                }
                local_change_outcome(
                    self.id(),
                    "scroll-list",
                    "scroll-offset-y",
                    format!("{:.1}", scroll.offset_y),
                )
            }
            (Self::List(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Press =>
            {
                let Some(row) = list_row_from_pointer_y(pointer.position.y) else {
                    return EventOutcome::ignored();
                };
                message_outcome(
                    self.id(),
                    "select-list-row",
                    AdmissionMessage::SelectItem(row),
                    "selected-item",
                    (row + 1).to_string(),
                )
            }
            (Self::List(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Release =>
            {
                if list_row_from_pointer_y(pointer.position.y).is_some() {
                    handled_no_change_outcome(self.id())
                } else {
                    EventOutcome::ignored()
                }
            }
            (Self::Overlay(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Press =>
            {
                let bounds = pointer
                    .target_bounds
                    .map(TargetLocalRect::into_rect)
                    .unwrap_or(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 560.0,
                            height: 228.0,
                        },
                    });
                let offset = match local {
                    AdmissionLocal::Overlay { offset, .. } => *offset,
                    _ => Self::overlay_default_offset(),
                };
                let titlebar = Self::overlay_titlebar_rect(bounds, offset);
                if point_in_rect(pointer.position, titlebar) {
                    *local = AdmissionLocal::Overlay {
                        offset,
                        dragging: true,
                        drag_anchor: Point {
                            x: pointer.position.x - offset.x,
                            y: pointer.position.y - offset.y,
                        },
                    };
                    local_change_outcome(self.id(), "overlay-drag-start", "dragging", "true")
                } else {
                    handled_no_change_outcome(self.id())
                }
            }
            (Self::Overlay(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Move =>
            {
                let bounds = pointer
                    .target_bounds
                    .map(TargetLocalRect::into_rect)
                    .unwrap_or(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 560.0,
                            height: 228.0,
                        },
                    });
                let Some((current_offset, drag_anchor, dragging)) = (match local {
                    AdmissionLocal::Overlay {
                        offset,
                        drag_anchor,
                        dragging,
                        ..
                    } => Some((*offset, *drag_anchor, *dragging)),
                    _ => None,
                }) else {
                    return EventOutcome::ignored();
                };
                if !dragging {
                    return EventOutcome::ignored();
                }
                if !pointer.details.buttons.primary {
                    *local = AdmissionLocal::Overlay {
                        offset: current_offset,
                        dragging: false,
                        drag_anchor,
                    };
                    return local_change_outcome(
                        self.id(),
                        "overlay-drag-cancel-missing-button",
                        "dragging",
                        "false".to_string(),
                    );
                }
                let next = Self::clamp_overlay_offset_to_allowed(
                    bounds,
                    Point {
                        x: pointer.position.x - drag_anchor.x,
                        y: pointer.position.y - drag_anchor.y,
                    },
                );
                if current_offset == next {
                    return handled_no_change_outcome(self.id());
                }
                *local = AdmissionLocal::Overlay {
                    offset: next,
                    dragging: true,
                    drag_anchor,
                };
                local_change_outcome(
                    self.id(),
                    "overlay-drag-move",
                    "offset",
                    format!("{:.0},{:.0}", next.x, next.y),
                )
            }
            (Self::Overlay(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Release =>
            {
                if let AdmissionLocal::Overlay {
                    offset,
                    drag_anchor,
                    ..
                } = local
                {
                    *local = AdmissionLocal::Overlay {
                        offset: *offset,
                        dragging: false,
                        drag_anchor: *drag_anchor,
                    };
                }
                local_change_outcome(self.id(), "overlay-drag-release", "dragging", "false")
            }
            (Self::Overlay(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Cancel =>
            {
                if let AdmissionLocal::Overlay {
                    offset,
                    drag_anchor,
                    ..
                } = local
                {
                    *local = AdmissionLocal::Overlay {
                        offset: *offset,
                        dragging: false,
                        drag_anchor: *drag_anchor,
                    };
                }
                local_change_outcome(self.id(), "overlay-drag-cancel", "dragging", "false")
            }
            (Self::OverlayStack(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Press =>
            {
                let bounds = pointer
                    .target_bounds
                    .map(TargetLocalRect::into_rect)
                    .unwrap_or(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 560.0,
                            height: 276.0,
                        },
                    });
                let AdmissionLocal::OverlayStack {
                    offsets,
                    order,
                    dragging,
                    drag_anchor,
                } = local
                else {
                    return EventOutcome::ignored();
                };

                for index in 0..4 {
                    if point_in_rect(
                        pointer.position,
                        Self::overlay_stack_order_button_rect(index),
                    ) {
                        Self::overlay_stack_bring_front(order, index);
                        *dragging = None;
                        *drag_anchor = Point { x: 0.0, y: 0.0 };
                        return local_change_outcome(
                            self.id(),
                            "overlay-stack-bring-front",
                            "front",
                            overlay_stack_label(index).to_string(),
                        );
                    }
                }

                if let Some(index) = Self::overlay_stack_topmost_overlay_at(
                    bounds,
                    *offsets,
                    *order,
                    pointer.position,
                ) {
                    *dragging = Some(index);
                    *drag_anchor = Point {
                        x: pointer.position.x - offsets[index].x,
                        y: pointer.position.y - offsets[index].y,
                    };
                    local_change_outcome(
                        self.id(),
                        "overlay-stack-drag-start",
                        "dragging",
                        overlay_stack_label(index).to_string(),
                    )
                } else {
                    EventOutcome::ignored()
                }
            }
            (Self::OverlayStack(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Move =>
            {
                let bounds = pointer
                    .target_bounds
                    .map(TargetLocalRect::into_rect)
                    .unwrap_or(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 560.0,
                            height: 276.0,
                        },
                    });
                let AdmissionLocal::OverlayStack {
                    offsets,
                    dragging,
                    drag_anchor,
                    ..
                } = local
                else {
                    return EventOutcome::ignored();
                };
                let Some(index) = *dragging else {
                    return EventOutcome::ignored();
                };
                if !pointer.details.buttons.primary {
                    *dragging = None;
                    return local_change_outcome(
                        self.id(),
                        "overlay-stack-drag-cancel-missing-button",
                        "dragging",
                        "false".to_string(),
                    );
                }
                let next_offset = Self::clamp_overlay_stack_offset_to_allowed(
                    bounds,
                    Point {
                        x: pointer.position.x - drag_anchor.x,
                        y: pointer.position.y - drag_anchor.y,
                    },
                );
                if offsets[index] == next_offset {
                    return handled_no_change_outcome(self.id());
                }
                offsets[index] = next_offset;
                local_change_outcome(
                    self.id(),
                    "overlay-stack-drag-move",
                    "offset",
                    format!("{:.0},{:.0}", offsets[index].x, offsets[index].y),
                )
            }
            (Self::OverlayStack(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Release =>
            {
                if let AdmissionLocal::OverlayStack { dragging, .. } = local {
                    *dragging = None;
                }
                local_change_outcome(
                    self.id(),
                    "overlay-stack-drag-release",
                    "dragging",
                    "false".to_string(),
                )
            }
            (Self::OverlayStack(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Cancel =>
            {
                if let AdmissionLocal::OverlayStack { dragging, .. } = local {
                    *dragging = None;
                }
                local_change_outcome(
                    self.id(),
                    "overlay-stack-drag-cancel",
                    "dragging",
                    "false".to_string(),
                )
            }
            (Self::NestedScroll(_), InputEvent::Wheel(wheel)) => {
                // Which region id arrives here is decided entirely by the
                // declared routing (ADR-0002 B2): every region declares the
                // `NearestScrollable` default, so wheel over an inner panel
                // delivers THAT inner's region id while it can consume, and
                // the outer's id once that inner is at its limit (default
                // at-limit chaining). The handler stays region-driven and
                // encodes no selection-order assumptions of its own.
                let selected_region = wheel
                    .region_id
                    .as_ref()
                    .and_then(nested_scroll_region_index)
                    .unwrap_or(None);
                if let AdmissionLocal::NestedScroll {
                    outer_scroll_rows,
                    inner_scroll_rows,
                } = local
                {
                    match selected_region {
                        Some(index) => {
                            inner_scroll_rows[index] = nested_scroll_rows_after_wheel(
                                inner_scroll_rows[index],
                                wheel.delta_y,
                                NESTED_SCROLL_MAX_ROWS,
                            );
                            local_change_outcome(
                                self.id(),
                                "nested-scroll-wheel-inner",
                                "inner-scroll-rows",
                                format!("{inner_scroll_rows:?}"),
                            )
                        }
                        None => {
                            *outer_scroll_rows = nested_scroll_rows_after_wheel(
                                *outer_scroll_rows,
                                wheel.delta_y,
                                NESTED_OUTER_MAX_ROWS,
                            );
                            local_change_outcome(
                                self.id(),
                                "nested-scroll-wheel-outer",
                                "outer-scroll-rows",
                                (*outer_scroll_rows).to_string(),
                            )
                        }
                    }
                } else {
                    EventOutcome::ignored()
                }
            }
            (Self::NestedScroll(_), InputEvent::Scroll(scroll)) => {
                let Some(region) = nested_scroll_region_index(&scroll.region_id) else {
                    return EventOutcome::ignored();
                };
                // The outer and inner regions quantize on their own row steps
                // (the outer travel is anchoring-capped; see the constants).
                let (row_step, max_rows) = match region {
                    None => (NESTED_OUTER_ROW_STEP, NESTED_OUTER_MAX_ROWS),
                    Some(_) => (NESTED_SCROLL_ROW_STEP, NESTED_SCROLL_MAX_ROWS),
                };
                let next_rows = (scroll.offset_y / row_step)
                    .round()
                    .clamp(0.0, max_rows as f32) as i32;
                if let AdmissionLocal::NestedScroll {
                    outer_scroll_rows,
                    inner_scroll_rows,
                } = local
                {
                    match region {
                        None => {
                            *outer_scroll_rows = next_rows;
                            local_change_outcome(
                                self.id(),
                                "nested-scroll-outer",
                                "outer-scroll-rows",
                                (*outer_scroll_rows).to_string(),
                            )
                        }
                        Some(index) => {
                            inner_scroll_rows[index] = next_rows.min(NESTED_SCROLL_MAX_ROWS);
                            local_change_outcome(
                                self.id(),
                                "nested-scroll-inner",
                                "inner-scroll-rows",
                                format!("{inner_scroll_rows:?}"),
                            )
                        }
                    }
                } else {
                    EventOutcome::ignored()
                }
            }
            _ => EventOutcome::ignored(),
        }
    }
}

impl SlipwayViewDefinition for AdmissionWidget {
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let layout_input = input.layout_input.clone();
        let (frame, layout) = slipway_core::layout_view_definition(self, external, local, input);
        let paint = self.paint(external, local, &layout);
        let mut declarations = self.view_declarations(
            external,
            local,
            &layout_input,
            &layout,
            layout.bounds().into_rect(),
            Some(WidgetSlotAddress::new(self.id(), 0)),
        );
        declarations
            .diagnostics
            .extend(layout.diagnostics().to_vec());
        let mut paint_order = PaintOrderDeclaration::source_order(self.id());
        if matches!(self, Self::Overlay(_) | Self::OverlayStack(_)) {
            paint_order.allow_overlap = true;
            paint_order = paint_order.with_overflow_bounds(TargetLocalRect::new(
                if matches!(self, Self::OverlayStack(_)) {
                    Self::overlay_stack_allowed_bounds(layout.bounds().into_rect())
                } else {
                    Self::overlay_allowed_bounds(layout.bounds().into_rect())
                },
            ));
        }

        ViewDefinition {
            target: self.id(),
            frame,
            layout,
            paint,
            paint_order,
            hit_regions: declarations.hit_regions,
            focus_regions: declarations.focus_regions,
            scroll_regions: declarations.scroll_regions,
            wheel_traversal_boundary: declarations.wheel_traversal_boundary,
            semantic_slots: declarations.semantic_slots,
            probe_metadata: declarations.probe_metadata,
            diagnostics: declarations.diagnostics,
        }
    }
}

impl SlipwayFontResolutionPolicy for AdmissionWidget {
    fn resolve_font(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        request: FontResolutionRequest,
    ) -> FontResolutionEvidence {
        unresolved_example_font_evidence(self.id(), request)
    }
}

impl SlipwayIcedAuthoredChildren for AdmissionWidget {
    fn visit_iced_authored_children<V>(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
    }
}

impl SlipwayEguiAuthoredChildren for AdmissionWidget {
    fn visit_egui_authored_children<V>(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
    }
}

fn unresolved_example_font_evidence(
    target: WidgetId,
    request: FontResolutionRequest,
) -> FontResolutionEvidence {
    let mut fallback_chain = Vec::with_capacity(1 + request.fallback_families.len());
    fallback_chain.push(request.family.clone());
    fallback_chain.extend(request.fallback_families.clone());
    let source = request.source.clone();

    if let Some(source) = source {
        let resolved_ref = source
            .family
            .clone()
            .unwrap_or_else(|| source.source_id.clone());
        return FontResolutionEvidence {
            request,
            resolved_ref: Some(resolved_ref.clone()),
            fallback_chain,
            installation: Some(ResourceInstallationEvidence {
                resource_id: resolved_ref,
                source: Some(source.clone()),
                status: ResourceInstallationStatus::NotRequested,
                evidence_source: EvidenceSource {
                    label: "example_authored_font_source".to_string(),
                    backend_id: None,
                    provider_id: Some("slipway-example-admission".to_string()),
                    pass_id: None,
                },
                diagnostics: Vec::new(),
            }),
            refusal: None,
            valid_source: Some(SourceValidityEvidence {
                source_id: source.source_id,
                validity: SourceValidityKind::Valid,
                diagnostics: Vec::new(),
            }),
            diagnostics: Vec::new(),
        };
    }

    let diagnostic = Diagnostic::unsupported(
        Some(target),
        "example-font-unresolved",
        "the admission example does not load or verify system fonts; visible backends must provide their own font evidence",
    );

    FontResolutionEvidence {
        request,
        resolved_ref: None,
        fallback_chain,
        installation: None,
        refusal: Some(ResourceRefusalEvidence {
            resource_id: source
                .as_ref()
                .and_then(|source| source.family.clone())
                .unwrap_or_else(|| "font-request".to_string()),
            source: source.clone(),
            reason: "no example-side loadable font source was provided or verified".to_string(),
            evidence_source: EvidenceSource {
                label: "example_authored_refusal".to_string(),
                backend_id: None,
                provider_id: Some("slipway-example-admission".to_string()),
                pass_id: None,
            },
            diagnostics: vec![diagnostic.clone()],
        }),
        valid_source: source.map(|source| SourceValidityEvidence {
            source_id: source.source_id,
            validity: SourceValidityKind::Unknown,
            diagnostics: vec![diagnostic.clone()],
        }),
        diagnostics: vec![diagnostic],
    }
}

impl SlipwayView for AdmissionWidget {
    fn initial_local_state(&self) -> Self::LocalState {
        match self {
            Self::Action(_) => AdmissionLocal::Action { pressed: false },
            Self::Segment(_) => AdmissionLocal::Segment { hover_count: 0 },
            Self::Text(_) => AdmissionLocal::Text {
                focused: false,
                local_edit_count: 0,
            },
            Self::Toggle(_) => AdmissionLocal::Toggle { armed: false },
            Self::Slider(_) => AdmissionLocal::Slider { dragging: false },
            Self::List(_) => AdmissionLocal::List { scroll_rows: 0 },
            Self::Overlay(_) => AdmissionLocal::Overlay {
                offset: Self::overlay_default_offset(),
                dragging: false,
                drag_anchor: Point { x: 0.0, y: 0.0 },
            },
            Self::OverlayStack(_) => AdmissionLocal::OverlayStack {
                offsets: Self::overlay_stack_default_offsets(),
                order: Self::overlay_stack_default_order(),
                dragging: None,
                drag_anchor: Point { x: 0.0, y: 0.0 },
            },
            Self::NestedScroll(_) => AdmissionLocal::NestedScroll {
                outer_scroll_rows: 0,
                inner_scroll_rows: [0, 1, 2],
            },
        }
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: slipway_core::LayoutOutputBuilder,
    ) -> LayoutOutput {
        let width = input
            .constraints
            .max
            .width
            .min(CARD_MAX_WIDTH)
            .max(input.constraints.min.width)
            .max(220.0);
        let height = match self {
            Self::List(_) => LIST_CARD_HEIGHT,
            Self::Overlay(_) => 228.0,
            Self::OverlayStack(_) => 276.0,
            Self::NestedScroll(_) => 292.0,
            _ => 76.0,
        };
        output.finish(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size { width, height },
        }))
    }

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        let bounds = layout.bounds().into_rect();
        let title = match self {
            Self::Action(_) => format!("Counter button: {}", external.counter),
            Self::Segment(_) => format!("Segment: {}", external.segment.label()),
            Self::Text(_) => "Draft".to_string(),
            Self::Toggle(_) => format!(
                "Notifications: {}",
                if external.enabled {
                    "enabled"
                } else {
                    "disabled"
                }
            ),
            Self::Slider(_) => format!("Intensity: {:.0}%", external.intensity * 100.0),
            Self::List(_) => {
                let scroll = match local {
                    AdmissionLocal::List { scroll_rows } => *scroll_rows,
                    _ => 0,
                };
                format!(
                    "List selected: row {} | local scroll rows: {}",
                    external.selected_item + 1,
                    scroll
                )
            }
            Self::Overlay(_) => match local {
                AdmissionLocal::Overlay {
                    offset, dragging, ..
                } => {
                    format!(
                        "Overlay offset: {:.0},{:.0}{}",
                        offset.x,
                        offset.y,
                        if *dragging { " dragging" } else { "" }
                    )
                }
                _ => "Overlay".to_string(),
            },
            Self::OverlayStack(_) => match local {
                AdmissionLocal::OverlayStack {
                    order, dragging, ..
                } => format!(
                    "Overlay stack front {}{}",
                    overlay_stack_label(order[3]),
                    dragging
                        .map(|index| format!(" dragging {}", overlay_stack_label(index)))
                        .unwrap_or_default()
                ),
                _ => "Overlay stack".to_string(),
            },
            Self::NestedScroll(_) => match local {
                AdmissionLocal::NestedScroll {
                    outer_scroll_rows,
                    inner_scroll_rows,
                } => {
                    format!("Nested scroll outer {outer_scroll_rows} inner {inner_scroll_rows:?}")
                }
                _ => "Nested scroll".to_string(),
            },
        };

        let panel_color = match self {
            Self::Action(_) => rgb(219, 234, 254),
            Self::Segment(_) => rgb(254, 249, 195),
            Self::Text(_) => rgb(220, 252, 231),
            _ => rgb(248, 250, 252),
        };
        let outline_color = match self {
            Self::Action(_) => rgb(29, 78, 216),
            Self::Segment(_) => rgb(161, 98, 7),
            Self::Text(_) => rgb(21, 128, 61),
            _ => rgb(203, 213, 225),
        };
        let text_style = match self {
            Self::Action(_) => TextStyle::plain()
                .with_align_x(TextAlignX::Center)
                .with_align_y(TextAlignY::Center),
            Self::Segment(_) => TextStyle::plain()
                .with_align_x(TextAlignX::End)
                .with_align_y(TextAlignY::Bottom),
            _ => TextStyle::plain(),
        };
        let featured = matches!(self, Self::Action(_) | Self::Segment(_) | Self::Text(_));
        let content_bounds = featured.then(|| {
            slipway_core::derive_target_box(bounds.size, admission_demo_spacing(self.id().as_str()))
                .expect("static admission spacing must be valid")
                .content
                .into_rect()
        });
        let text_bounds = if matches!(self, Self::Action(_) | Self::Segment(_)) {
            content_bounds.expect("featured alignment card has content geometry")
        } else {
            self.card_header_text_rect(bounds)
        };

        let mut ops = vec![
            PaintOp::Fill {
                shape: rect_shape("panel", bounds, ShapeKind::RoundedRectangle),
                color: panel_color,
            },
            PaintOp::Stroke {
                shape: rect_shape("outline", bounds, ShapeKind::RoundedRectangle),
                color: outline_color,
                width: if featured { 3.0 } else { 1.0 },
            },
            PaintOp::Fill {
                shape: rect_shape(
                    "content-guide",
                    content_bounds.unwrap_or(bounds),
                    ShapeKind::Rectangle,
                ),
                color: if featured {
                    rgba(255, 255, 255, 196)
                } else {
                    rgba(248, 250, 252, 0)
                },
            },
            PaintOp::Text {
                bounds: text_bounds,
                content: format!("{} | {}", self.role(), title),
                color: rgb(15, 23, 42),
                style: text_style,
            },
        ];

        match self {
            Self::Toggle(_) => {
                let track = Rect {
                    origin: Point { x: 24.0, y: 42.0 },
                    size: Size {
                        width: 64.0,
                        height: 24.0,
                    },
                };
                ops.push(PaintOp::Fill {
                    shape: rect_shape("toggle-track", track, ShapeKind::RoundedRectangle),
                    color: if external.enabled {
                        rgb(15, 118, 110)
                    } else {
                        rgb(148, 163, 184)
                    },
                });
                ops.push(PaintOp::Fill {
                    shape: rect_shape(
                        "toggle-thumb",
                        Rect {
                            origin: Point {
                                x: if external.enabled { 62.0 } else { 28.0 },
                                y: 46.0,
                            },
                            size: Size {
                                width: 16.0,
                                height: 16.0,
                            },
                        },
                        ShapeKind::Circle,
                    ),
                    color: rgb(255, 255, 255),
                });
            }
            Self::Slider(_) => {
                let track_width = (bounds.size.width - 64.0).max(1.0);
                let filled = track_width * external.intensity.clamp(0.0, 1.0);
                ops.push(PaintOp::Fill {
                    shape: rect_shape(
                        "slider-track",
                        Rect {
                            origin: Point { x: 24.0, y: 52.0 },
                            size: Size {
                                width: track_width,
                                height: 8.0,
                            },
                        },
                        ShapeKind::RoundedRectangle,
                    ),
                    color: rgb(226, 232, 240),
                });
                ops.push(PaintOp::Fill {
                    shape: rect_shape(
                        "slider-fill",
                        Rect {
                            origin: Point { x: 24.0, y: 52.0 },
                            size: Size {
                                width: filled,
                                height: 8.0,
                            },
                        },
                        ShapeKind::RoundedRectangle,
                    ),
                    color: rgb(37, 99, 235),
                });
                ops.push(PaintOp::Fill {
                    shape: rect_shape(
                        "slider-thumb",
                        Rect {
                            origin: Point {
                                x: 18.0 + filled,
                                y: 46.0,
                            },
                            size: Size {
                                width: 20.0,
                                height: 20.0,
                            },
                        },
                        ShapeKind::Circle,
                    ),
                    color: rgb(30, 64, 175),
                });
            }
            _ => {}
        }

        if matches!(self, Self::List(_)) {
            // Painted row geometry MUST derive from the same LIST_ROW_*
            // constants as the hit regions and the pointer math (see the
            // constants' comment).
            for i in 0..LIST_ROW_COUNT {
                ops.push(PaintOp::Text {
                    bounds: Rect {
                        origin: Point {
                            x: 24.0,
                            y: LIST_ROW_TOP + i as f32 * LIST_ROW_STEP,
                        },
                        size: Size {
                            width: bounds.size.width - 48.0,
                            height: 18.0,
                        },
                    },
                    content: format!(
                        "row {}{}",
                        i + 1,
                        if i == external.selected_item {
                            " selected"
                        } else {
                            ""
                        }
                    ),
                    color: if i == external.selected_item {
                        rgb(37, 99, 235)
                    } else {
                        rgb(51, 65, 85)
                    },
                    style: TextStyle::plain(),
                });
            }
        }

        if matches!(self, Self::Overlay(_)) {
            self.push_overlay_paint_ops(&mut ops, local, bounds);
        }

        if matches!(self, Self::OverlayStack(_)) {
            self.push_overlay_stack_paint_ops(&mut ops, local, bounds);
        }

        if matches!(self, Self::NestedScroll(_)) {
            self.push_nested_scroll_paint_ops(&mut ops, local, bounds);
        }

        ops
    }

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        let id = self.id();
        let slot = Some(WidgetSlotAddress::new(id.clone(), 0));
        let mut observations = vec![StateObservation {
            target: id.clone(),
            slot: slot.clone(),
            name: "external".to_string(),
            value: match self {
                Self::Action(_) => external.counter.to_string(),
                Self::Segment(_) => external.segment.label().to_string(),
                Self::Text(_) => external.draft.clone(),
                Self::Toggle(_) => external.enabled.to_string(),
                Self::Slider(_) => format!("{:.2}", external.intensity),
                Self::List(_) => external.selected_item.to_string(),
                Self::Overlay(_) => "local-overlay".to_string(),
                Self::OverlayStack(_) => "local-overlay-stack".to_string(),
                Self::NestedScroll(_) => "local-nested-scroll".to_string(),
            },
        }];

        observations.push(StateObservation {
            target: id,
            slot,
            name: "local".to_string(),
            value: format!("{local:?}"),
        });
        observations
    }
}

fn message_outcome(
    target: WidgetId,
    name: &'static str,
    message: AdmissionMessage,
    field: &'static str,
    after: impl Into<String>,
) -> EventOutcome<AdmissionMessage> {
    let change = ChangeEvidence {
        target: target.clone(),
        slot: Some(WidgetSlotAddress::new(target.clone(), 0)),
        field: field.to_string(),
        before: None,
        after: Some(after.into()),
    };
    EventOutcome {
        handled: true,
        propagate: false,
        emitted_messages: vec![EmittedMessage {
            target: target.clone(),
            name: name.to_string(),
            message,
        }],
        changes: vec![change.clone()],
        observations: Vec::new(),
        probes: vec![ProbeProduct::Change(slipway_core::ChangeProbe {
            target,
            changes: vec![change],
        })],
        diagnostics: Vec::new(),
    }
}

fn local_change_outcome(
    target: WidgetId,
    _name: &'static str,
    field: &'static str,
    after: impl Into<String>,
) -> EventOutcome<AdmissionMessage> {
    let change = ChangeEvidence {
        target: target.clone(),
        slot: Some(WidgetSlotAddress::new(target.clone(), 0)),
        field: field.to_string(),
        before: None,
        after: Some(after.into()),
    };
    EventOutcome {
        handled: true,
        propagate: false,
        emitted_messages: Vec::new(),
        changes: vec![change.clone()],
        observations: Vec::new(),
        probes: vec![ProbeProduct::Change(slipway_core::ChangeProbe {
            target,
            changes: vec![change],
        })],
        diagnostics: Vec::new(),
    }
}

fn handled_no_change_outcome(_target: WidgetId) -> EventOutcome<AdmissionMessage> {
    EventOutcome {
        handled: true,
        propagate: false,
        emitted_messages: Vec::new(),
        changes: Vec::new(),
        observations: Vec::new(),
        probes: Vec::new(),
        diagnostics: Vec::new(),
    }
}

fn intensity_from_pointer(x: f32) -> f32 {
    ((x - 24.0) / 496.0).clamp(0.0, 1.0)
}

fn admission_text_input_style_token() -> TextStyle {
    TextStyle::plain()
        .with_font_family("system-ui")
        .with_font_size(14.0)
}

fn admission_text_input_typography(target: WidgetId) -> TextInputTypographyDeclaration {
    let declaration =
        TextInputTypographyDeclaration::explicit(target, admission_text_input_style_token());
    if let Some(source) = admission_cjk_font_source() {
        declaration.with_source(source)
    } else {
        declaration
    }
}

fn admission_cjk_font_source() -> Option<ResourceSourceDeclaration> {
    #[cfg(target_os = "windows")]
    {
        const CANDIDATES: &[(&str, &str)] = &[
            ("Noto Sans KR", "C:\\Windows\\Fonts\\NotoSansKR-VF.ttf"),
            ("Malgun Gothic", "C:\\Windows\\Fonts\\malgun.ttf"),
            ("Gulim", "C:\\Windows\\Fonts\\gulim.ttc"),
        ];

        for (family, path) in CANDIDATES {
            if std::path::Path::new(path).exists() {
                return Some(ResourceSourceDeclaration {
                    source_id: format!("windows-cjk-font:{family}"),
                    kind: ResourceSourceKind::Asset,
                    family: Some((*family).to_string()),
                    asset_ref: Some((*path).to_string()),
                    revision: Vec::new(),
                });
            }
        }
    }

    None
}

fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color {
        red: f32::from(red) / 255.0,
        green: f32::from(green) / 255.0,
        blue: f32::from(blue) / 255.0,
        alpha: 1.0,
    }
}

fn rgba(red: u8, green: u8, blue: u8, alpha: u8) -> Color {
    Color {
        red: f32::from(red) / 255.0,
        green: f32::from(green) / 255.0,
        blue: f32::from(blue) / 255.0,
        alpha: f32::from(alpha) / 255.0,
    }
}

fn rect_shape(id: &'static str, bounds: Rect, kind: ShapeKind) -> ShapeDeclaration {
    ShapeDeclaration {
        id: Some(id.to_string()),
        kind,
        bounds,
        path: None,
        clip: None,
    }
}

fn path_shape(id: &'static str, bounds: Rect, path: PathDeclaration) -> ShapeDeclaration {
    ShapeDeclaration {
        id: Some(id.to_string()),
        kind: ShapeKind::Path,
        bounds,
        path: Some(path),
        clip: None,
    }
}

fn point_in_rect(point: Point, rect: Rect) -> bool {
    point.x >= rect.origin.x
        && point.x <= rect.origin.x + rect.size.width
        && point.y >= rect.origin.y
        && point.y <= rect.origin.y + rect.size.height
}

fn intersect_rect(left: Rect, right: Rect) -> Option<Rect> {
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

fn overlay_stack_label(index: usize) -> &'static str {
    ["A", "B", "C", "D"].get(index).copied().unwrap_or("?")
}

#[cfg(test)]
mod tests {
    use super::*;
    use slipway_backend_egui::{
        SlipwayEguiBackendChildWidget, SlipwayEguiNativeChildWidget, egui_backend_admission,
    };
    use slipway_core::{FrameIdentity, LayoutConstraints, PaintInputTransparency};

    fn admission_test_frame(width: f32, height: f32) -> FrameIdentity {
        FrameIdentity {
            surface_id: "iced-visible".to_string(),
            surface_instance_id: "admission.app".to_string(),
            revision: 1,
            frame_index: 1,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size { width, height },
            },
        }
    }

    fn admission_view(
        runtime: &SlipwayRuntime<AdmissionRuntimeAppWidget>,
        frame: FrameIdentity,
    ) -> ViewDefinition {
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(frame.viewport),
            content: TargetLocalRect::new(frame.viewport),
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

    fn widget_layout_input(width: f32, height: f32) -> LayoutInput {
        LayoutInput {
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size { width, height },
            }),
            content: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size { width, height },
            }),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: Size { width, height },
            },
        }
    }

    #[test]
    fn spacing_demo_has_asymmetric_responsive_content_geometry() {
        let center = admission_demo_spacing("admission.action");
        assert_eq!(center.margin, EdgeInsets::trbl(8.0, 24.0, 12.0, 4.0));
        assert_eq!(center.padding, EdgeInsets::trbl(6.0, 28.0, 18.0, 10.0));

        let wide = slipway_core::derive_target_box(
            Size {
                width: 560.0,
                height: 76.0,
            },
            center,
        )
        .expect("wide demo geometry");
        assert_eq!(wide.content.origin, Point { x: 10.0, y: 6.0 });
        assert_eq!(wide.content.size.width, 522.0);
        assert_eq!(wide.content.size.height, 52.0);

        let narrow = slipway_core::derive_target_box(
            Size {
                width: 32.0,
                height: 20.0,
            },
            center,
        )
        .expect("finite excess padding remains valid");
        assert_eq!(narrow.content.origin, Point { x: 10.0, y: 6.0 });
        assert_eq!(
            narrow.content.size,
            Size {
                width: 0.0,
                height: 0.0,
            }
        );
    }

    #[test]
    fn spacing_demo_text_uses_text_edit_focus_region_without_pointer_hit() {
        let widget = AdmissionWidget::Text(TextWidget);
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let spacing = admission_demo_spacing(widget.id().as_str());
        let target = slipway_core::derive_target_box(
            Size {
                width: 240.0,
                height: 76.0,
            },
            spacing,
        )
        .expect("nested demo geometry");
        let input = LayoutInput {
            viewport: target.border,
            content: target.content,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: target.border.size,
            },
        };
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(admission_test_frame(240.0, 76.0), input),
        );
        let focus = view
            .focus_regions
            .iter()
            .find(|region| region.target == widget.id())
            .expect("text demo explicitly declares a text edit focus region");

        assert!(
            view.hit_regions.is_empty(),
            "text input must not add a separate pointer hit region over the native TextEdit"
        );
        assert!(focus.text_edit.is_some());
        assert_eq!(
            focus.bounds,
            TargetLocalRect::new(AdmissionWidget::text_input_focus_bounds(
                view.layout.bounds().into_rect()
            ))
        );
        assert!(target.content.origin.x > target.border.origin.x);
        assert!(target.content.origin.y > target.border.origin.y);
        assert!(spacing.margin.left > 0.0 && spacing.margin.top > 0.0);
        assert!(view.diagnostics.is_empty(), "{:?}", view.diagnostics);
    }

    #[test]
    fn spacing_demo_text_passes_visible_contract_admission() {
        let widget = AdmissionWidget::Text(TextWidget);
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let spacing = admission_demo_spacing(widget.id().as_str());
        let target = slipway_core::derive_target_box(
            Size {
                width: 560.0,
                height: 76.0,
            },
            spacing,
        )
        .expect("text demo geometry");
        let input = LayoutInput {
            viewport: target.border,
            content: target.content,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: target.border.size,
            },
        };
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(admission_test_frame(560.0, 76.0), input),
        );
        let diagnostics = slipway_core::view_definition_contract_diagnostics_for_capabilities(
            &view,
            &widget.capabilities(),
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    fn pointer_backend_input_for_view(
        view: &ViewDefinition,
        point: Point,
    ) -> slipway_core::BackendInputEvent {
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            slipway_core::EvidenceSource::backend_presented("iced", "test"),
            view.frame.clone(),
            &view.layout,
            &view.hit_regions,
            point,
            slipway_core::PointerEventKind::Press,
            Some(slipway_core::PointerButton::Primary),
            slipway_core::PointerDetails::default(),
            true,
        );
        let dispatch = dispatch.expect("point should resolve to a declared hit region");
        slipway_core::BackendInputEvent::declared(dispatch.input, evidence)
    }

    fn text_edit_backend_input_for_view(
        view: &ViewDefinition,
        text: &str,
    ) -> slipway_core::BackendInputEvent {
        let region = view
            .focus_regions
            .iter()
            .find(|region| region.target == WidgetId::from("admission.text"))
            .expect("text widget should expose a text edit focus region");
        let event = InputEvent::TextEdit(slipway_core::TextEditEvent {
            target: WidgetId::from("admission.text"),
            target_slot: region.address.clone(),
            kind: TextEditKind::ReplaceBuffer,
            text: Some(text.to_string()),
            selection_before: None,
            selection_after: None,
        });
        let evidence = slipway_core::declared_focus_text_dispatch_evidence(
            slipway_core::EvidenceSource::backend_presented("iced", "test"),
            view.frame.clone(),
            &view.focus_regions,
            Some(region),
            slipway_core::DeclaredEventDispatchKind::Text,
            None,
            event.clone(),
        );
        slipway_core::BackendInputEvent::declared(event, evidence)
    }

    fn wheel_backend_input_for_view(
        view: &ViewDefinition,
        point: Point,
        delta_y: f32,
    ) -> slipway_core::BackendInputEvent {
        let (dispatch, evidence) = slipway_core::resolve_declared_wheel_dispatch_with_evidence(
            slipway_core::EvidenceSource::backend_presented("iced", "test"),
            view.frame.clone(),
            &view.layout,
            &view.scroll_regions,
            point,
            0.0,
            delta_y,
        );
        let dispatch = dispatch.expect("point should resolve to a declared scroll region");
        slipway_core::BackendInputEvent::declared(dispatch.input, evidence)
    }

    fn apply_test_backend_input(
        runtime: &mut SlipwayRuntime<AdmissionRuntimeAppWidget>,
        input: slipway_core::BackendInputEvent,
    ) -> slipway_runtime::SlipwayBackendInputApplyReport {
        let mut reducer = apply_messages;
        runtime.apply_backend_input_event_for_backend_with_app_reducer(input, "iced", &mut reducer)
    }

    fn clipped_group_count(ops: &[PaintOp], prefix: &str) -> usize {
        let mut count = 0;
        for op in ops {
            if let PaintOp::Group { id, clip, ops } = op {
                if id.as_deref().is_some_and(|id| id.starts_with(prefix)) && clip.is_some() {
                    count += 1;
                }
                count += clipped_group_count(ops, prefix);
            }
        }
        count
    }

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

    fn group_text_ops(ops: &[PaintOp], group_id: &str) -> Vec<(Rect, String)> {
        let mut texts = Vec::new();
        for op in ops {
            if let PaintOp::Group { id, ops, .. } = op {
                if id.as_deref() == Some(group_id) {
                    collect_text_ops(ops, &mut texts);
                } else {
                    texts.extend(group_text_ops(ops, group_id));
                }
            } else if let PaintOp::Layer { ops, .. } = op {
                texts.extend(group_text_ops(ops, group_id));
            }
        }
        texts
    }

    fn overlay_stack_layer_labels(ops: &[PaintOp]) -> Vec<String> {
        let mut labels = Vec::new();
        for op in ops {
            match op {
                PaintOp::Text { content, .. } if content.contains(" layer ") => {
                    labels.push(content.clone());
                }
                PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                    labels.extend(overlay_stack_layer_labels(ops));
                }
                _ => {}
            }
        }
        labels
    }

    fn first_overlay_stack_layer_label(ops: &[PaintOp]) -> Option<String> {
        for op in ops {
            match op {
                PaintOp::Text { content, .. } if content.contains(" layer ") => {
                    return Some(content.clone());
                }
                PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                    if let Some(label) = first_overlay_stack_layer_label(ops) {
                        return Some(label);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn overlay_stack_paint_layers(
        ops: &[PaintOp],
    ) -> Vec<(String, PaintLayerKey, PaintInputTransparency)> {
        let mut layers = Vec::new();
        for op in ops {
            match op {
                PaintOp::Layer {
                    key,
                    input_transparency,
                    ops,
                    ..
                } => {
                    if let Some(label) = first_overlay_stack_layer_label(ops) {
                        layers.push((label, *key, *input_transparency));
                    }
                }
                PaintOp::Group { ops, .. } => layers.extend(overlay_stack_paint_layers(ops)),
                _ => {}
            }
        }
        layers
    }

    fn paint_layer_by_id<'a>(
        ops: &'a [PaintOp],
        wanted_id: &str,
    ) -> Option<(PaintLayerKey, PaintInputTransparency, &'a [PaintOp])> {
        for op in ops {
            match op {
                PaintOp::Layer {
                    id,
                    key,
                    input_transparency,
                    ops,
                    ..
                } if id.as_deref() == Some(wanted_id) => {
                    return Some((*key, *input_transparency, ops));
                }
                PaintOp::Layer { ops, .. } | PaintOp::Group { ops, .. } => {
                    if let Some(layer) = paint_layer_by_id(ops, wanted_id) {
                        return Some(layer);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn text_labels(ops: &[PaintOp]) -> Vec<String> {
        let mut labels = Vec::new();
        for op in ops {
            match op {
                PaintOp::Text { content, .. } => labels.push(content.clone()),
                PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                    labels.extend(text_labels(ops));
                }
                _ => {}
            }
        }
        labels
    }

    fn assert_overlay_stack_layers_match_order(view: &ViewDefinition, order: [usize; 4]) {
        let layers = overlay_stack_paint_layers(&view.paint);
        assert_eq!(layers.len(), 4);
        for (rank, overlay) in order.into_iter().enumerate() {
            let spec = AdmissionWidget::overlay_stack_layer_spec(overlay, rank);
            let expected_label = format!("{} layer {rank}", overlay_stack_label(overlay));
            assert_eq!(layers[rank].0, expected_label);
            assert_eq!(layers[rank].1, spec.key);
            assert_eq!(layers[rank].2, PaintInputTransparency::Opaque);

            let drag_hit = view
                .hit_regions
                .iter()
                .find(|region| {
                    region
                        .id
                        .as_str()
                        .contains(&format!(":overlay-{overlay}:drag"))
                })
                .expect("overlay drag hit region is declared");
            let absorb_hit = view
                .hit_regions
                .iter()
                .find(|region| {
                    region
                        .id
                        .as_str()
                        .contains(&format!(":overlay-{overlay}:absorb"))
                })
                .expect("overlay absorb hit region is declared");
            assert_eq!(drag_hit.order, spec.drag_hit_order());
            assert_eq!(absorb_hit.order, spec.absorb_hit_order());
            assert!(
                drag_hit.order.traversal_order > absorb_hit.order.traversal_order,
                "titlebar drag must win over same-card body absorption"
            );
        }
    }

    fn overlay_stack_control_rects(bounds: Rect) -> Vec<Rect> {
        let mut rects = vec![AdmissionWidget::overlay_stack_help_text_rect(bounds)];
        rects.extend((0..4).map(AdmissionWidget::overlay_stack_order_button_rect));
        rects
    }

    fn text_bounds_for_content_prefix(ops: &[PaintOp], prefix: &str) -> Option<Rect> {
        for op in ops {
            match op {
                PaintOp::Text {
                    bounds, content, ..
                } if content.starts_with(prefix) => return Some(*bounds),
                PaintOp::Group { ops, .. } => {
                    if let Some(bounds) = text_bounds_for_content_prefix(ops, prefix) {
                        return Some(bounds);
                    }
                }
                _ => {}
            }
        }
        None
    }

    fn rects_overlap(left: Rect, right: Rect) -> bool {
        intersect_rect(left, right).is_some()
    }

    #[test]
    fn example_uses_core_traits_for_all_widgets() {
        let widgets = admission_widgets();
        assert_eq!(widgets.len(), 9);
        for widget in widgets {
            assert!(!widget.capabilities().is_empty());
            let local = widget.initial_local_state();
            assert!(
                !widget
                    .observe_state(&AdmissionState::default(), &local)
                    .is_empty()
            );
        }
    }

    #[test]
    fn probe_collection_is_request_scoped() {
        let mut collector = ProbeCollector::new();
        collector.push(ProbeProduct::State(slipway_core::StateProbe {
            target: WidgetId::from("x"),
            observations: Vec::new(),
        }));
        assert_eq!(collector.len(), 1);
        assert_eq!(collector.take().len(), 1);
        assert!(collector.is_empty());
    }

    #[test]
    fn pointer_controls_emit_external_state_messages() {
        let state = AdmissionState::default();
        let toggle = AdmissionWidget::Toggle(ToggleWidget);
        let mut toggle_local = toggle.initial_local_state();
        let toggle_outcome = toggle.handle_event(
            &state,
            &mut toggle_local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: toggle.id(),
                target_slot: None,
                position: Point { x: 4.0, y: 4.0 },
                target_bounds: None,
                kind: slipway_core::PointerEventKind::Press,
                button: None,
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(toggle_outcome.handled);
        assert_eq!(toggle_outcome.emitted_messages.len(), 1);

        let slider = AdmissionWidget::Slider(SliderWidget);
        let mut slider_local = slider.initial_local_state();
        let slider_outcome = slider.handle_event(
            &state,
            &mut slider_local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: slider.id(),
                target_slot: None,
                position: Point { x: 272.0, y: 4.0 },
                target_bounds: None,
                kind: slipway_core::PointerEventKind::Press,
                button: None,
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(slider_outcome.handled);
        assert_eq!(slider_outcome.emitted_messages.len(), 1);
    }

    #[test]
    fn action_release_clears_pressed_local_state_after_click() {
        let state = AdmissionState::default();
        let action = AdmissionWidget::Action(ActionWidget);
        let mut local = action.initial_local_state();

        let press = action.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: action.id(),
                target_slot: None,
                position: Point { x: 4.0, y: 4.0 },
                target_bounds: None,
                kind: slipway_core::PointerEventKind::Press,
                button: None,
                details: slipway_core::PointerDetails::default(),
            }),
        );

        assert!(press.handled);
        assert_eq!(press.emitted_messages.len(), 1);
        assert_eq!(local, AdmissionLocal::Action { pressed: true });

        let release = action.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: action.id(),
                target_slot: None,
                position: Point { x: 4.0, y: 4.0 },
                target_bounds: None,
                kind: slipway_core::PointerEventKind::Release,
                button: None,
                details: slipway_core::PointerDetails::default(),
            }),
        );

        assert!(release.handled);
        assert!(release.emitted_messages.is_empty());
        assert_eq!(local, AdmissionLocal::Action { pressed: false });
    }

    #[test]
    fn text_edit_event_updates_draft_message_and_local_edit_count() {
        let state = AdmissionState {
            draft: "edit me".to_string(),
            ..AdmissionState::default()
        };
        let text = AdmissionWidget::Text(TextWidget);
        let mut local = text.initial_local_state();

        let outcome = text.handle_event(
            &state,
            &mut local,
            InputEvent::TextEdit(slipway_core::TextEditEvent {
                target: text.id(),
                target_slot: None,
                kind: TextEditKind::ReplaceSelection,
                text: Some("edited".to_string()),
                selection_before: None,
                selection_after: None,
            }),
        );

        assert!(outcome.handled);
        assert_eq!(outcome.emitted_messages.len(), 1);
        assert_eq!(
            outcome.emitted_messages[0].message,
            AdmissionMessage::UpdateDraft("edited".to_string())
        );
        assert_eq!(
            local,
            AdmissionLocal::Text {
                focused: false,
                local_edit_count: 1,
            }
        );
    }

    #[test]
    fn text_copy_command_does_not_mutate_draft() {
        let state = AdmissionState {
            draft: "copy me".to_string(),
            ..AdmissionState::default()
        };
        let text = AdmissionWidget::Text(TextWidget);
        let mut local = text.initial_local_state();

        let outcome = text.handle_event(
            &state,
            &mut local,
            InputEvent::Command(slipway_core::CommandEvent {
                target: text.id(),
                target_slot: None,
                command: "copy".to_string(),
                payload_ref: None,
                source: None,
            }),
        );

        assert!(!outcome.handled);
        assert!(outcome.emitted_messages.is_empty());
        assert_eq!(state.draft, "copy me");
        assert_eq!(local, text.initial_local_state());
    }

    #[test]
    fn text_probe_command_is_the_only_demo_command_that_mutates_draft() {
        let state = AdmissionState {
            draft: "probe".to_string(),
            ..AdmissionState::default()
        };
        let text = AdmissionWidget::Text(TextWidget);
        let mut local = text.initial_local_state();

        let outcome = text.handle_event(
            &state,
            &mut local,
            InputEvent::Command(slipway_core::CommandEvent {
                target: text.id(),
                target_slot: None,
                command: "probe".to_string(),
                payload_ref: None,
                source: None,
            }),
        );

        assert!(outcome.handled);
        assert_eq!(
            outcome.emitted_messages[0].message,
            AdmissionMessage::UpdateDraft("probe*".to_string())
        );
    }

    #[test]
    fn scroll_event_updates_list_local_scroll_and_scroll_region_is_declared() {
        let state = AdmissionState::default();
        let list = AdmissionWidget::List(ListWidget);
        let mut local = list.initial_local_state();
        let layout_input = widget_layout_input(560.0, 168.0);

        let view = list.view_definition(
            &state,
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "test-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: layout_input.viewport.into_rect(),
                },
                layout_input.clone(),
            ),
        );

        assert_eq!(view.scroll_regions.len(), 1);
        assert!(
            view.scroll_regions[0].content_bounds.size.height
                > view.scroll_regions[0].viewport.size.height
        );

        let outcome = list.handle_event(
            &state,
            &mut local,
            InputEvent::Scroll(slipway_core::ScrollEvent {
                target: list.id(),
                target_slot: None,
                region_id: view.scroll_regions[0].id.clone(),
                offset_x: 0.0,
                offset_y: 60.0,
                viewport: view.scroll_regions[0].viewport,
                content_bounds: view.scroll_regions[0].content_bounds,
            }),
        );

        assert!(outcome.handled);
        assert_eq!(local, AdmissionLocal::List { scroll_rows: 3 });
        assert!(outcome.emitted_messages.is_empty());
    }

    #[test]
    fn overlay_widget_declares_layered_complex_paint_and_draggable_local_state() {
        let state = AdmissionState::default();
        let overlay = AdmissionWidget::Overlay(OverlayWidget);
        let mut local = overlay.initial_local_state();
        let layout_input = widget_layout_input(560.0, 228.0);
        let view = overlay.view_definition(
            &state,
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "test-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: layout_input.viewport.into_rect(),
                },
                layout_input.clone(),
            ),
        );

        assert_eq!(
            view.paint_order.mode,
            slipway_core::PaintOrderMode::SourceOrder
        );
        assert!(view.paint_order.allow_overlap);
        assert!(view.paint_order.allow_overflow_paint);
        let (overlay_key, overlay_transparency, overlay_ops) =
            paint_layer_by_id(&view.paint, "movable-overlay-window")
                .expect("movable overlay window must be the only ordered overlay layer");
        assert_eq!(overlay_key, AdmissionWidget::overlay_layer_key());
        assert_eq!(overlay_transparency, PaintInputTransparency::Opaque);
        assert!(overlay_ops.iter().any(|op| matches!(
            op,
            PaintOp::Stroke { shape, .. }
                if shape.id.as_deref() == Some("overlay-curve")
                    && shape.kind == ShapeKind::Path
                    && shape.path.is_some()
        )));
        assert!(overlay_ops.iter().any(|op| matches!(
            op,
            PaintOp::Group { id, ops, .. }
                if id.as_deref() == Some("overlay-curve-nodes") && ops.len() >= 8
        )));
        assert!(view.hit_regions.iter().any(|region| {
            region.id.as_str().ends_with(":hit")
                && region.capture == PointerCaptureIntent::DuringDrag
                && region.order.z_index == overlay_key.z_index
                && region.order.paint_order == overlay_key.order.unwrap_or_default()
                && region.bounds.origin.y < 0.0
        }));

        let press = overlay.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: overlay.id(),
                target_slot: None,
                position: Point { x: 244.0, y: -32.0 },
                target_bounds: Some(TargetLocalRect::new(view.layout.bounds().into_rect())),
                kind: slipway_core::PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(press.handled);

        let moved = overlay.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: overlay.id(),
                target_slot: None,
                position: Point { x: 304.0, y: 10.0 },
                target_bounds: Some(TargetLocalRect::new(view.layout.bounds().into_rect())),
                kind: slipway_core::PointerEventKind::Move,
                button: Some(slipway_core::PointerButton::Primary),
                details: primary_pointer_details(),
            }),
        );
        assert!(moved.handled);
        assert!(matches!(
            local,
            AdmissionLocal::Overlay {
                offset,
                dragging: true,
                ..
            } if offset.x > 220.0 && offset.y > -44.0
        ));

        let cancel = overlay.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: overlay.id(),
                target_slot: None,
                position: Point { x: 640.0, y: 360.0 },
                target_bounds: Some(TargetLocalRect::new(view.layout.bounds().into_rect())),
                kind: slipway_core::PointerEventKind::Cancel,
                button: None,
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(cancel.handled);
        assert!(matches!(
            local,
            AdmissionLocal::Overlay {
                dragging: false,
                ..
            }
        ));
    }

    fn pointer_event_for_widget(
        widget: &AdmissionWidget,
        kind: slipway_core::PointerEventKind,
    ) -> InputEvent {
        InputEvent::Pointer(slipway_core::PointerEvent {
            target: widget.id(),
            target_slot: None,
            position: Point { x: 24.0, y: 24.0 },
            target_bounds: Some(TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 240.0,
                    height: 160.0,
                },
            })),
            kind,
            button: Some(slipway_core::PointerButton::Primary),
            details: primary_pointer_details(),
        })
    }

    fn primary_pointer_details() -> slipway_core::PointerDetails {
        slipway_core::PointerDetails {
            buttons: slipway_core::PointerButtons {
                primary: true,
                ..slipway_core::PointerButtons::default()
            },
            ..slipway_core::PointerDetails::default()
        }
    }

    fn target_route_for_widget(widget: &AdmissionWidget) -> EventRoute {
        EventRoute {
            route_id: Some("test-route".to_string()),
            address: None,
            path: vec![widget.id()],
            phase: EventRoutePhase::Target,
        }
    }

    fn disposition_handled(
        widget: &AdmissionWidget,
        state: &AdmissionState,
        local: &AdmissionLocal,
        event: &InputEvent,
    ) -> bool {
        let evidence = slipway_core::SlipwayEventDispositionPolicy::event_disposition(
            widget,
            state,
            local,
            event,
            &target_route_for_widget(widget),
        );
        evidence
            .steps
            .last()
            .expect("event disposition should record a target step")
            .disposition
            .handled
    }

    #[test]
    fn overlay_move_is_declared_handled_only_while_dragging() {
        let state = AdmissionState::default();
        let overlay = AdmissionWidget::Overlay(OverlayWidget);
        let event = pointer_event_for_widget(&overlay, slipway_core::PointerEventKind::Move);

        let idle = AdmissionLocal::Overlay {
            offset: AdmissionWidget::overlay_default_offset(),
            dragging: false,
            drag_anchor: Point { x: 0.0, y: 0.0 },
        };
        assert!(!disposition_handled(&overlay, &state, &idle, &event));

        let dragging = AdmissionLocal::Overlay {
            offset: AdmissionWidget::overlay_default_offset(),
            dragging: true,
            drag_anchor: Point { x: 0.0, y: 0.0 },
        };
        assert!(disposition_handled(&overlay, &state, &dragging, &event));
    }

    #[test]
    fn overlay_stack_move_is_declared_handled_only_while_dragging() {
        let state = AdmissionState::default();
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let event = pointer_event_for_widget(&stack, slipway_core::PointerEventKind::Move);
        let offsets = AdmissionWidget::overlay_stack_default_offsets();
        let order = [0, 1, 2, 3];

        let idle = AdmissionLocal::OverlayStack {
            offsets,
            order,
            dragging: None,
            drag_anchor: Point { x: 0.0, y: 0.0 },
        };
        assert!(!disposition_handled(&stack, &state, &idle, &event));

        let dragging = AdmissionLocal::OverlayStack {
            offsets,
            order,
            dragging: Some(0),
            drag_anchor: Point { x: 0.0, y: 0.0 },
        };
        assert!(disposition_handled(&stack, &state, &dragging, &event));
    }

    #[test]
    fn overlay_stack_stale_drag_move_without_primary_button_cancels_dragging() {
        let state = AdmissionState::default();
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let offsets = AdmissionWidget::overlay_stack_default_offsets();
        let order = AdmissionWidget::overlay_stack_default_order();
        let mut local = AdmissionLocal::OverlayStack {
            offsets,
            order,
            dragging: Some(0),
            drag_anchor: Point { x: 12.0, y: 8.0 },
        };

        let outcome = stack.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: stack.id(),
                target_slot: None,
                position: Point { x: 340.0, y: 180.0 },
                target_bounds: Some(TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 560.0,
                        height: 276.0,
                    },
                })),
                kind: slipway_core::PointerEventKind::Move,
                button: None,
                details: slipway_core::PointerDetails::default(),
            }),
        );

        assert!(outcome.handled);
        assert_eq!(
            local,
            AdmissionLocal::OverlayStack {
                offsets,
                order,
                dragging: None,
                drag_anchor: Point { x: 12.0, y: 8.0 },
            }
        );
    }

    #[test]
    fn overlay_stack_declares_multiple_ordered_overlays_and_changes_front() {
        let state = AdmissionState::default();
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let mut local = stack.initial_local_state();
        let layout_input = widget_layout_input(560.0, 276.0);
        let view = stack.view_definition(
            &state,
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "test-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: layout_input.viewport.into_rect(),
                },
                layout_input.clone(),
            ),
        );

        assert_eq!(
            view.paint_order.mode,
            slipway_core::PaintOrderMode::SourceOrder
        );
        assert_eq!(view.paint_order.z_index, 0);
        assert_eq!(view.paint_order.order, None);
        assert!(view.paint_order.allow_overlap);
        assert!(view.paint_order.allow_overflow_paint);
        assert!(view.paint_order.overflow_bounds.is_some());
        assert_eq!(
            view.hit_regions
                .iter()
                .filter(|region| region.id.as_str().contains(":overlay-")
                    && region.id.as_str().contains(":drag"))
                .count(),
            4
        );
        assert_eq!(
            view.hit_regions
                .iter()
                .filter(|region| region.id.as_str().contains(":overlay-")
                    && region.id.as_str().contains(":absorb"))
                .count(),
            4
        );
        assert_eq!(
            view.hit_regions
                .iter()
                .filter(|region| region.id.as_str().contains(":order-"))
                .count(),
            4
        );
        let overlay_0_order = view
            .hit_regions
            .iter()
            .find(|region| region.id.as_str().contains(":overlay-0:drag"))
            .expect("overlay 0 drag region is declared")
            .order
            .clone();
        let overlay_0_absorb_order = view
            .hit_regions
            .iter()
            .find(|region| region.id.as_str().contains(":overlay-0:absorb"))
            .expect("overlay 0 absorb region is declared")
            .order
            .clone();
        let overlay_3_order = view
            .hit_regions
            .iter()
            .find(|region| region.id.as_str().contains(":overlay-3:drag"))
            .expect("overlay 3 drag region is declared")
            .order
            .clone();
        assert_eq!(
            overlay_0_order,
            AdmissionWidget::overlay_stack_layer_spec(0, 0).drag_hit_order()
        );
        assert_eq!(
            overlay_0_absorb_order,
            AdmissionWidget::overlay_stack_layer_spec(0, 0).absorb_hit_order()
        );
        assert_eq!(
            overlay_3_order,
            AdmissionWidget::overlay_stack_layer_spec(3, 3).drag_hit_order()
        );
        assert!(overlay_0_order.traversal_order > overlay_0_absorb_order.traversal_order);
        assert!(overlay_3_order.paint_order > overlay_0_order.paint_order);
        for index in 0..4 {
            let button_order = view
                .hit_regions
                .iter()
                .find(|region| region.id.as_str().contains(&format!(":order-{index}")))
                .expect("overlay-stack order button is declared")
                .order
                .clone();
            assert_eq!(
                button_order,
                HitRegionOrder {
                    z_index: 0,
                    paint_order: index,
                    traversal_order: index,
                },
                "overlay-stack control buttons are local/default controls, not overlay layers"
            );
        }

        let press_a_order = stack.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: stack.id(),
                target_slot: None,
                position: Point { x: 32.0, y: 78.0 },
                target_bounds: Some(TargetLocalRect::new(view.layout.bounds().into_rect())),
                kind: slipway_core::PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(press_a_order.handled);
        assert!(matches!(
            local,
            AdmissionLocal::OverlayStack {
                order,
                ..
            } if order[3] == 0
        ));

        let drag_bounds = view.layout.bounds().into_rect();
        let drag_offsets = match &local {
            AdmissionLocal::OverlayStack { offsets, .. } => *offsets,
            _ => AdmissionWidget::overlay_stack_default_offsets(),
        };
        let drag_card = AdmissionWidget::overlay_stack_rect(drag_bounds, drag_offsets, 0);
        let body_press = stack.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: stack.id(),
                target_slot: None,
                position: Point {
                    x: drag_card.origin.x + 40.0,
                    y: drag_card.origin.y + 52.0,
                },
                target_bounds: Some(TargetLocalRect::new(drag_bounds)),
                kind: slipway_core::PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(body_press.handled);
        assert!(matches!(
            local,
            AdmissionLocal::OverlayStack {
                dragging: Some(0),
                ..
            }
        ));
        if let AdmissionLocal::OverlayStack { dragging, .. } = &mut local {
            *dragging = None;
        }
        let drag_start_position = Point {
            x: drag_card.origin.x + 24.0,
            y: drag_card.origin.y + 12.0,
        };
        let drag_start = stack.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: stack.id(),
                target_slot: None,
                position: drag_start_position,
                target_bounds: Some(TargetLocalRect::new(drag_bounds)),
                kind: slipway_core::PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(drag_start.handled);
        let drag_move = stack.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: stack.id(),
                target_slot: None,
                position: Point { x: 470.0, y: 48.0 },
                target_bounds: Some(TargetLocalRect::new(drag_bounds)),
                kind: slipway_core::PointerEventKind::Move,
                button: Some(slipway_core::PointerButton::Primary),
                details: primary_pointer_details(),
            }),
        );
        assert!(drag_move.handled);
        assert!(matches!(
            local,
            AdmissionLocal::OverlayStack {
                offsets,
                dragging: Some(0),
                ..
            } if offsets[0].x > drag_offsets[0].x && offsets[0].y < drag_offsets[0].y
        ));
    }

    #[test]
    fn overlay_stack_drag_preserves_order_and_moves_only_offset() {
        let state = AdmissionState::default();
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let mut local = stack.initial_local_state();
        let initial_order = AdmissionWidget::overlay_stack_default_order();
        let initial_offsets = AdmissionWidget::overlay_stack_default_offsets();
        let layout_input = widget_layout_input(560.0, 276.0);
        let view = stack.view_definition(
            &state,
            &local,
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
        );
        let bounds = view.layout.bounds().into_rect();
        let overlay_0 = AdmissionWidget::overlay_stack_titlebar_rect(bounds, initial_offsets, 0);

        let drag_start = stack.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: stack.id(),
                target_slot: None,
                position: Point {
                    x: overlay_0.origin.x + 24.0,
                    y: overlay_0.origin.y + 12.0,
                },
                target_bounds: Some(TargetLocalRect::new(bounds)),
                kind: slipway_core::PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(drag_start.handled);
        assert!(matches!(
            local,
            AdmissionLocal::OverlayStack {
                order,
                dragging: Some(0),
                ..
            } if order == initial_order
        ));

        let drag_move = stack.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: stack.id(),
                target_slot: None,
                position: Point {
                    x: overlay_0.origin.x + 64.0,
                    y: overlay_0.origin.y + 36.0,
                },
                target_bounds: Some(TargetLocalRect::new(bounds)),
                kind: slipway_core::PointerEventKind::Move,
                button: Some(slipway_core::PointerButton::Primary),
                details: primary_pointer_details(),
            }),
        );
        assert!(drag_move.handled);
        assert!(matches!(
            local,
            AdmissionLocal::OverlayStack {
                offsets,
                order,
                dragging: Some(0),
                ..
            } if order == initial_order && offsets[0] != initial_offsets[0]
        ));
    }

    #[test]
    fn overlay_stack_paint_layers_follow_authored_order_and_match_hit_order() {
        let state = AdmissionState::default();
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let mut local = stack.initial_local_state();
        let layout_input = widget_layout_input(560.0, 276.0);
        let frame = FrameIdentity {
            surface_id: "test-surface".to_string(),
            surface_instance_id: "test-instance".to_string(),
            revision: 0,
            frame_index: 0,
            viewport: layout_input.viewport.into_rect(),
        };
        let view = stack.view_definition(
            &state,
            &local,
            ViewDefinitionInput::new(frame.clone(), layout_input.clone()),
        );

        assert_eq!(
            overlay_stack_layer_labels(&view.paint),
            vec!["A layer 0", "B layer 1", "C layer 2", "D layer 3"]
        );
        assert_overlay_stack_layers_match_order(&view, [0, 1, 2, 3]);
        let labels = text_labels(&slipway_core::flatten_ordered_paint_units(vec![
            slipway_core::PaintUnit::from_view(view.clone(), stack.traversal_order()),
        ]));
        let control_text = labels
            .iter()
            .position(|label| label == "front buttons; drag cards")
            .expect("overlay-stack default control text is painted");
        let first_layer = labels
            .iter()
            .position(|label| label == "A layer 0")
            .expect("first overlay-stack layer is painted");
        let front_layer = labels
            .iter()
            .position(|label| label == "D layer 3")
            .expect("front overlay-stack layer is painted");
        assert!(
            control_text < first_layer && control_text < front_layer,
            "overlay-stack default/card controls must stay below ordered overlay cards"
        );

        let bring_a_front = stack.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: stack.id(),
                target_slot: None,
                position: Point { x: 32.0, y: 78.0 },
                target_bounds: Some(TargetLocalRect::new(view.layout.bounds().into_rect())),
                kind: slipway_core::PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
            }),
        );
        assert!(bring_a_front.handled);

        let view = stack.view_definition(
            &state,
            &local,
            ViewDefinitionInput::new(frame, layout_input),
        );

        assert_eq!(
            overlay_stack_layer_labels(&view.paint),
            vec!["B layer 0", "C layer 1", "D layer 2", "A layer 3"]
        );
        assert_overlay_stack_layers_match_order(&view, [1, 2, 3, 0]);
    }

    #[test]
    fn overlay_stack_front_key_sorts_above_lower_key_sibling_paint() {
        let state = AdmissionState::default();
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let local = stack.initial_local_state();
        let layout_input = widget_layout_input(560.0, 276.0);
        let stack_view = stack.view_definition(
            &state,
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "stack-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: layout_input.viewport.into_rect(),
                },
                layout_input,
            ),
        );
        let lower_key_sibling = slipway_core::PaintUnit {
            target: WidgetId::from("admission.lower-key-sibling"),
            address: None,
            order: PaintOrderDeclaration::layered_order(
                WidgetId::from("admission.lower-key-sibling"),
                10,
                2,
            ),
            traversal_order: 99,
            paint: vec![PaintOp::Text {
                bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 20.0,
                    },
                },
                content: "lower-key sibling".to_string(),
                color: rgb(0, 0, 0),
                style: TextStyle::plain(),
            }],
        };

        let labels = text_labels(&slipway_core::flatten_ordered_paint_units(vec![
            slipway_core::PaintUnit::from_view(stack_view, stack.traversal_order()),
            lower_key_sibling,
        ]));
        let sibling_index = labels
            .iter()
            .position(|label| label == "lower-key sibling")
            .expect("lower-key sibling text is painted");
        let front_index = labels
            .iter()
            .position(|label| label == "D layer 3")
            .expect("front overlay-stack layer is painted");
        assert!(
            sibling_index < front_index,
            "front overlay-stack keyed layer must paint above a lower-key sibling"
        );
    }

    #[test]
    fn overlay_stack_text_control_layout_has_non_overlapping_bounds() {
        let state = AdmissionState::default();
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let local = stack.initial_local_state();
        let layout_input = widget_layout_input(560.0, 276.0);
        let view = stack.view_definition(
            &state,
            &local,
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
        );
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 560.0,
                height: 276.0,
            },
        };
        let header = text_bounds_for_content_prefix(&view.paint, "overlay-stack |")
            .expect("overlay-stack card header text is painted");
        let mut controls = vec![header];
        controls.extend(overlay_stack_control_rects(bounds));

        for (index, left) in controls.iter().enumerate() {
            for right in controls.iter().skip(index + 1) {
                assert!(
                    !rects_overlap(*left, *right),
                    "overlay-stack text/control bounds must not overlap: {left:?} {right:?}"
                );
            }
        }
    }

    #[test]
    fn overlay_stack_default_overlay_bounds_do_not_unintentionally_overlap_controls() {
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 560.0,
                height: 276.0,
            },
        };
        let offsets = AdmissionWidget::overlay_stack_default_offsets();
        let mut controls = vec![stack.card_header_text_rect(bounds)];
        controls.extend(overlay_stack_control_rects(bounds));
        let overlays = (0..4)
            .map(|index| AdmissionWidget::overlay_stack_rect(bounds, offsets, index))
            .collect::<Vec<_>>();

        assert!(
            overlays
                .iter()
                .any(|overlay| overlay.origin.y + overlay.size.height
                    > bounds.origin.y + bounds.size.height),
            "default overlay stack should still provide bounded overflow evidence"
        );
        for overlay in overlays {
            for control in &controls {
                assert!(
                    !rects_overlap(overlay, *control),
                    "default overlay bounds must not accidentally cover text/control bounds: {overlay:?} {control:?}"
                );
            }
        }
    }

    #[test]
    fn overlay_and_overlay_stack_layers_keep_drag_overlay_visibly_above_stack() {
        let state = AdmissionState::default();
        let overlay = AdmissionWidget::Overlay(OverlayWidget);
        let stack = AdmissionWidget::OverlayStack(OverlayStackWidget);
        let overlay_local = overlay.initial_local_state();
        let stack_local = stack.initial_local_state();
        let overlay_input = widget_layout_input(560.0, 228.0);
        let stack_input = widget_layout_input(560.0, 276.0);
        let overlay_view = overlay.view_definition(
            &state,
            &overlay_local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "overlay-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: overlay_input.viewport.into_rect(),
                },
                overlay_input,
            ),
        );
        let stack_view = stack.view_definition(
            &state,
            &stack_local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "stack-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: stack_input.viewport.into_rect(),
                },
                stack_input,
            ),
        );

        assert_eq!(
            overlay_view.paint_order.mode,
            slipway_core::PaintOrderMode::SourceOrder
        );
        assert_eq!(
            stack_view.paint_order.mode,
            slipway_core::PaintOrderMode::SourceOrder
        );
        assert!(overlay_view.paint_order.allow_overflow_paint);
        assert!(stack_view.paint_order.allow_overflow_paint);
        let (overlay_key, _, _) = paint_layer_by_id(&overlay_view.paint, "movable-overlay-window")
            .expect("movable overlay window is the ordered layer, not its card view");
        let stack_front_key = AdmissionWidget::overlay_stack_layer_spec(3, 3).key;
        assert!(
            (overlay_key.z_index, overlay_key.order)
                > (stack_front_key.z_index, stack_front_key.order),
            "drag overlay must paint above the overlay-stack ordered cards"
        );

        let labels = text_labels(&slipway_core::flatten_ordered_paint_units(vec![
            slipway_core::PaintUnit::from_view(overlay_view, overlay.traversal_order()),
            slipway_core::PaintUnit::from_view(stack_view, stack.traversal_order()),
        ]));
        let stack_front = labels
            .iter()
            .position(|label| label == "D layer 3")
            .expect("overlay-stack front layer is painted");
        let movable_overlay = labels
            .iter()
            .position(|label| label == "drag overlay")
            .expect("movable overlay window is painted");
        assert!(
            stack_front < movable_overlay,
            "movable overlay keyed window must paint above overlay-stack cards"
        );
    }

    #[test]
    fn nested_scroll_widget_declares_outer_and_inner_scroll_regions() {
        let state = AdmissionState::default();
        let nested = AdmissionWidget::NestedScroll(NestedScrollWidget);
        let mut local = nested.initial_local_state();
        let layout_input = widget_layout_input(560.0, 292.0);
        let view = nested.view_definition(
            &state,
            &local,
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
        );

        assert_eq!(view.scroll_regions.len(), 4);
        assert!(
            view.scroll_regions
                .iter()
                .any(|region| region.id.as_str().ends_with(":outer"))
        );
        assert!(
            view.scroll_regions
                .iter()
                .filter(|region| region.id.as_str().contains(":inner-"))
                .count()
                == 3
        );
        for region in &view.scroll_regions {
            assert!(region.enabled);
            assert!(region.axes.vertical);
            assert!(region.consumption.wheel);
            assert!(
                region.content_bounds.size.height > region.viewport.size.height,
                "nested scroll regions must declare enough content extent for scrollbar evidence"
            );
        }
        let inner_viewports = view
            .scroll_regions
            .iter()
            .filter(|region| region.id.as_str().contains(":inner-"))
            .map(|region| region.viewport.into_rect())
            .collect::<Vec<_>>();
        assert_eq!(inner_viewports.len(), 3);
        assert!(
            inner_viewports
                .windows(2)
                .all(|pair| pair[0].origin.y + pair[0].size.height <= pair[1].origin.y),
            "inner scroll regions must stay as distinct visible owners"
        );
        let inner_one = view
            .scroll_regions
            .iter()
            .find(|region| region.id.as_str().ends_with(":inner-1"))
            .expect("inner scroll region 1 is declared");
        let outcome = nested.handle_event(
            &state,
            &mut local,
            InputEvent::Scroll(slipway_core::ScrollEvent {
                target: nested.id(),
                target_slot: None,
                region_id: inner_one.id.clone(),
                offset_x: 0.0,
                offset_y: 54.0,
                viewport: inner_one.viewport,
                content_bounds: inner_one.content_bounds,
            }),
        );

        assert!(outcome.handled);
        assert!(matches!(
            local,
            AdmissionLocal::NestedScroll {
                inner_scroll_rows, ..
            } if inner_scroll_rows[1] == 3
        ));
        assert_eq!(
            clipped_group_count(&view.paint, "nested-inner-"),
            3,
            "each inner scroll panel must clip its own rows"
        );
    }

    // The inner panels are routed (non-native) scroll regions whose rows
    // virtualize: the labels carry the scroll offset, so the painted row
    // positions must stay anchored to the panel. The pre-fix painting also
    // subtracted the raw offset (applying the scroll twice), which slid the
    // painted window out of the clip: visible labels skipped ahead two per
    // notch and the panel went completely blank from offset 4 on while it
    // still consumed wheels — the intermittently "collapsing" nested panel.
    #[test]
    fn nested_inner_panel_rows_stay_visible_across_all_inner_offsets() {
        let state = AdmissionState::default();
        let nested = AdmissionWidget::NestedScroll(NestedScrollWidget);
        let layout_input = widget_layout_input(560.0, 292.0);
        let panel = AdmissionWidget::nested_inner_content_viewport(0, 0.0);
        for rows in 0..=NESTED_SCROLL_MAX_ROWS {
            let local = AdmissionLocal::NestedScroll {
                outer_scroll_rows: 0,
                inner_scroll_rows: [rows, 0, 0],
            };
            let layout = slipway_core::layout_view(&nested, &state, &local, layout_input.clone());
            let paint = nested.paint(&state, &local, &layout);
            let visible = group_text_ops(&paint, "nested-inner-0-rows")
                .into_iter()
                .filter(|(bounds, _)| intersect_rect(*bounds, panel).is_some())
                .collect::<Vec<_>>();
            assert!(
                visible.len() >= 3,
                "inner panel must keep showing rows at offset {rows}, saw {}",
                visible.len()
            );
            assert_eq!(
                visible[0].1,
                format!("inner 1 row {}", rows + 1),
                "visible rows must advance exactly one row per scrolled row"
            );
        }
    }

    // The outer declares `nested_outer_content_height()` of content so the
    // outer's travel spans `NESTED_OUTER_MAX_ROWS` rows (the Step 194
    // extent/handler alignment), so the painted content must actually span
    // that extent: at every outer offset the bottom band of the outer
    // viewport shows painted content. Pre-fix the panels ended well above
    // the declared extent, so deeper offsets scrolled into a blank band — the
    // "collapsed" outer region.
    #[test]
    fn nested_outer_paints_content_through_declared_extent() {
        let state = AdmissionState::default();
        let nested = AdmissionWidget::NestedScroll(NestedScrollWidget);
        let layout_input = widget_layout_input(560.0, 292.0);
        let outer = AdmissionWidget::nested_outer_viewport(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 560.0,
                height: 292.0,
            },
        });
        let bottom_band = Rect {
            origin: Point {
                x: outer.origin.x,
                y: outer.origin.y + outer.size.height - 56.0,
            },
            size: Size {
                width: outer.size.width,
                height: 56.0,
            },
        };
        for rows in 0..=NESTED_OUTER_MAX_ROWS {
            let local = AdmissionLocal::NestedScroll {
                outer_scroll_rows: rows,
                inner_scroll_rows: [0, 0, 0],
            };
            let layout = slipway_core::layout_view(&nested, &state, &local, layout_input.clone());
            let paint = nested.paint(&state, &local, &layout);
            let in_band = group_text_ops(&paint, "nested-scroll-content")
                .into_iter()
                .filter(|(bounds, _)| intersect_rect(*bounds, bottom_band).is_some())
                .count();
            assert!(
                in_band > 0,
                "outer viewport bottom band must show painted content at outer offset {rows}"
            );
        }
    }

    // The demo-contract pin for the DEFAULT wheel routing (NearestScrollable
    // everywhere): wheel over an inner panel scrolls THAT inner first, chains
    // to the outer exactly at the inner's limit, and an up-wheel returns to
    // the inner as soon as it has room again. Step 194 authored `SelfFirst`
    // on the outer and this test then encoded the outer-first contract; the
    // architect's live UX check REJECTED that authoring (2026-07-11: pointing
    // the wheel at an inner panel moved the outer), so the demo is back on
    // the routed default. The authored modes stay proven by the
    // synthetic-fixture suites, not by this example.
    #[test]
    fn nested_scroll_wheel_routes_inner_first_and_chains_to_outer_at_inner_limit() {
        let state = AdmissionState::default();
        let nested = AdmissionWidget::NestedScroll(NestedScrollWidget);
        // Pin the starting state (the initial local staggers the inner rows).
        let mut local = AdmissionLocal::NestedScroll {
            outer_scroll_rows: 0,
            inner_scroll_rows: [0, 0, 0],
        };
        let layout_input = widget_layout_input(560.0, 292.0);
        let view_for = |local: &AdmissionLocal, revision: u64| {
            nested.view_definition(
                &state,
                local,
                ViewDefinitionInput::new(
                    FrameIdentity {
                        surface_id: "test-surface".to_string(),
                        surface_instance_id: "test-instance".to_string(),
                        revision,
                        frame_index: revision,
                        viewport: layout_input.viewport.into_rect(),
                    },
                    layout_input.clone(),
                ),
            )
        };
        let selected = |input: &slipway_core::BackendInputEvent| {
            input
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref())
                .map(PresentationRegionId::as_str)
                .map(str::to_owned)
        };

        // Phase 1: wheel down OVER INNER-0 with everything at rest — the
        // fronter inner panel under the cursor wins by default order and the
        // outer does not move.
        let view = view_for(&local, 0);
        let over_inner_down = wheel_backend_input_for_view(&view, Point { x: 42.0, y: 70.0 }, -1.0);
        assert_eq!(
            selected(&over_inner_down).as_deref(),
            Some("admission.nested-scroll:inner-0"),
            "default routing must scroll the inner panel under the cursor first"
        );
        let outcome = nested.handle_event(&state, &mut local, over_inner_down.event);
        assert!(outcome.handled);
        assert!(matches!(
            local,
            AdmissionLocal::NestedScroll {
                outer_scroll_rows: 0,
                inner_scroll_rows: [1, 0, 0],
            }
        ));

        // Phase 2: wheel up over the same point — the inner has room upward,
        // so it consumes the reverse direction too.
        let view = view_for(&local, 1);
        let over_inner_up = wheel_backend_input_for_view(&view, Point { x: 42.0, y: 70.0 }, 1.0);
        assert_eq!(
            selected(&over_inner_up).as_deref(),
            Some("admission.nested-scroll:inner-0")
        );
        let outcome = nested.handle_event(&state, &mut local, over_inner_up.event);
        assert!(outcome.handled);
        assert!(matches!(
            local,
            AdmissionLocal::NestedScroll {
                outer_scroll_rows: 0,
                inner_scroll_rows: [0, 0, 0],
            }
        ));

        // Phase 3: wheel over the outer body (outside every inner panel)
        // scrolls the outer itself.
        let view = view_for(&local, 2);
        let outer_down = wheel_backend_input_for_view(&view, Point { x: 260.0, y: 70.0 }, -1.0);
        assert_eq!(
            selected(&outer_down).as_deref(),
            Some("admission.nested-scroll:outer")
        );
        let outcome = nested.handle_event(&state, &mut local, outer_down.event);
        assert!(outcome.handled);
        assert!(matches!(
            local,
            AdmissionLocal::NestedScroll {
                outer_scroll_rows: 1,
                ..
            }
        ));

        // Phase 4: with INNER-0 AT ITS LIMIT the at-limit inner drops out of
        // the eligible pool and the down-wheel over it chains to the outer
        // (nothing is black-holed at the inner's limit).
        local = AdmissionLocal::NestedScroll {
            outer_scroll_rows: 0,
            inner_scroll_rows: [NESTED_SCROLL_MAX_ROWS, 0, 0],
        };
        let view = view_for(&local, 3);
        let at_inner_limit_down =
            wheel_backend_input_for_view(&view, Point { x: 42.0, y: 70.0 }, -1.0);
        assert_eq!(
            selected(&at_inner_limit_down).as_deref(),
            Some("admission.nested-scroll:outer"),
            "at the inner's limit the wheel must chain to the outer scroll owner"
        );
        let outcome = nested.handle_event(&state, &mut local, at_inner_limit_down.event);
        assert!(outcome.handled);
        assert!(matches!(
            local,
            AdmissionLocal::NestedScroll {
                outer_scroll_rows: 1,
                inner_scroll_rows: [NESTED_SCROLL_MAX_ROWS, 0, 0],
            }
        ));

        // Phase 5: wheel UP at the same point — the inner can consume upward
        // again, so default routing immediately returns to the inner under
        // the cursor and the outer keeps its position. The anchoring-capped
        // outer travel (at most 24px, under half a panel height) keeps the
        // SAME panel under the (42, 70) anchor at its displaced position.
        let view = view_for(&local, 4);
        let after_chain_up = wheel_backend_input_for_view(&view, Point { x: 42.0, y: 70.0 }, 1.0);
        assert_eq!(
            selected(&after_chain_up).as_deref(),
            Some("admission.nested-scroll:inner-0"),
            "an up-wheel must return to the inner as soon as it has room again"
        );
        let outcome = nested.handle_event(&state, &mut local, after_chain_up.event);
        assert!(outcome.handled);
        let reclaimed_rows = NESTED_SCROLL_MAX_ROWS - 1;
        assert!(matches!(
            local,
            AdmissionLocal::NestedScroll {
                outer_scroll_rows: 1,
                inner_scroll_rows: [rows, 0, 0],
            } if rows == reclaimed_rows
        ));
    }

    // The regression pin for the live symptom "the third nested panel never
    // scrolls" (Step 198): the outer's travel displaces the panels while it
    // consumes (a wheel over the outer body, or default chaining at an
    // inner's limit), and every later wheel resolves the inner at its
    // DISPLACED position. The travel must therefore stay under half a panel
    // height: at the outer's limit, the wheel anchored at EACH panel's
    // fresh-view center must select THAT panel (identity preserved) and
    // scroll its rows. With the old two-pitch travel (126px) the center
    // anchor of panel 0 landed on panel 2, and the anchors of panels 1 and 2
    // resolved no inner at all — a silently dead wheel over the
    // visually-third panel.
    #[test]
    fn nested_outer_limit_keeps_wheel_anchored_to_each_panel_center() {
        let state = AdmissionState::default();
        let nested = AdmissionWidget::NestedScroll(NestedScrollWidget);
        let layout_input = widget_layout_input(560.0, 292.0);
        for index in 0..3usize {
            let mut local = AdmissionLocal::NestedScroll {
                outer_scroll_rows: NESTED_OUTER_MAX_ROWS,
                inner_scroll_rows: [0, 0, 0],
            };
            let view = nested.view_definition(
                &state,
                &local,
                ViewDefinitionInput::new(
                    FrameIdentity {
                        surface_id: "test-surface".to_string(),
                        surface_instance_id: "test-instance".to_string(),
                        revision: index as u64,
                        frame_index: index as u64,
                        viewport: layout_input.viewport.into_rect(),
                    },
                    layout_input.clone(),
                ),
            );
            // The fresh-view (outer offset 0) center of panel `index`.
            let fresh_center = AdmissionWidget::nested_inner_content_viewport(index, 0.0);
            let anchor = Point {
                x: fresh_center.origin.x + fresh_center.size.width / 2.0,
                y: fresh_center.origin.y + fresh_center.size.height / 2.0,
            };
            let down = wheel_backend_input_for_view(&view, anchor, -1.0);
            assert_eq!(
                down.dispatch_evidence
                    .as_ref()
                    .and_then(|evidence| evidence.selected_region.as_ref())
                    .map(PresentationRegionId::as_str),
                Some(format!("admission.nested-scroll:inner-{index}").as_str()),
                "the panel-{index} center anchor must stay over panel {index} at the outer's limit"
            );
            let outcome = nested.handle_event(&state, &mut local, down.event);
            assert!(outcome.handled);
            let AdmissionLocal::NestedScroll {
                inner_scroll_rows, ..
            } = local
            else {
                panic!("nested local state expected");
            };
            assert_eq!(
                inner_scroll_rows[index], 1,
                "the anchored panel {index} must scroll its own rows"
            );
        }
    }

    #[test]
    fn composed_view_carries_only_default_wheel_routing_declarations() {
        let mut runtime = SlipwayRuntime::new(
            AdmissionRuntimeAppWidget::new(AdmissionApp {
                widgets: admission_widget_tuple(),
            }),
            AdmissionState::default(),
        );
        // A frame tall enough (1300) to keep the nested card (root-local
        // y 1164..1456) inside the root page region's band while the page
        // content (~1472) still overflows it: this harness resolves in
        // composed content space, so the declared chain-to-page hop below is
        // only expressible when the anchor sits inside the root region's
        // viewport. The live 700-tall window reaches the same chain through
        // the backend's presented-space translation (live-verified).
        let frame = admission_test_frame(640.0, 1300.0);
        let view = admission_view(&runtime, frame.clone());

        // The declaration snapshot: EVERY scroll region in the composed app
        // (nested outer AND inners, list, root page) declares the
        // `NearestScrollable` default — the example authors no non-default
        // wheel routing at all. This is the control that the demo runs on
        // the routed default after the Step 194 `SelfFirst`-on-outer
        // authoring was reverted (architect live-UX decision 2026-07-11:
        // the region under the cursor scrolls first); the authored modes
        // stay covered by the synthetic-fixture suites.
        let non_default: Vec<_> = view
            .scroll_regions
            .iter()
            .filter(|region| region.wheel_routing != WheelRouting::NearestScrollable)
            .map(|region| (region.id.as_str(), region.wheel_routing))
            .collect();
        assert_eq!(
            non_default,
            Vec::new(),
            "the example must carry only default wheel-routing declarations"
        );

        let selected = |input: &slipway_core::BackendInputEvent| {
            input
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref())
                .map(PresentationRegionId::as_str)
                .map(str::to_owned)
        };

        // End-to-end selection over the real composed view: wheel over an
        // inner panel picks THAT inner. The nested card sits at root-local
        // y 1164 (layout_plan); inner-0 maps to (48, 1222)-(204, 1284) and
        // the outer to (34, 1208)-(558, 1432).
        let over_inner = wheel_backend_input_for_view(&view, Point { x: 58.0, y: 1234.0 }, -1.0);
        assert_eq!(
            selected(&over_inner).as_deref(),
            Some("admission.nested-scroll:inner-0"),
            "composed-view wheel over an inner panel must pick that inner first"
        );

        // Control: wheel over the list picks the list itself (front-most
        // containing consumable owner, unchanged default order).
        let over_list = wheel_backend_input_for_view(&view, Point { x: 300.0, y: 500.0 }, -1.0);
        assert_eq!(
            selected(&over_list).as_deref(),
            Some("admission.list:scroll"),
            "the list control case must keep default front-most selection"
        );

        // Chain hop 1: with inner-0 at its limit the down-wheel over it
        // chains to the outer.
        runtime.local_state_mut().widgets.8 = AdmissionLocal::NestedScroll {
            outer_scroll_rows: 0,
            inner_scroll_rows: [NESTED_SCROLL_MAX_ROWS, 1, 2],
        };
        let view = admission_view(&runtime, frame.clone());
        let at_inner_limit =
            wheel_backend_input_for_view(&view, Point { x: 58.0, y: 1234.0 }, -1.0);
        assert_eq!(
            selected(&at_inner_limit).as_deref(),
            Some("admission.nested-scroll:outer"),
            "at the inner's limit the composed-view wheel must chain to the outer"
        );

        // Chain hop 2: with the outer ALSO at its limit the down-wheel
        // chains to the page (the root scroll owner). The anchoring-capped
        // outer travel (24px, under half a panel height) keeps inner-0 under
        // the (58, 1234) anchor at its displaced position, so this proves
        // the at-limit chain and not a hit-test miss.
        runtime.local_state_mut().widgets.8 = AdmissionLocal::NestedScroll {
            outer_scroll_rows: NESTED_OUTER_MAX_ROWS,
            inner_scroll_rows: [NESTED_SCROLL_MAX_ROWS, 1, 2],
        };
        let view = admission_view(&runtime, frame);
        let at_outer_limit =
            wheel_backend_input_for_view(&view, Point { x: 58.0, y: 1234.0 }, -1.0);
        assert_eq!(
            selected(&at_outer_limit).as_deref(),
            Some("admission.app:root-scroll"),
            "at the outer's limit the composed-view wheel must chain to the page"
        );
    }

    #[test]
    fn nested_outer_scroll_declared_extent_matches_handler_boundary() {
        // Mirror of `nested_inner_scroll_declared_extent_matches_handler_
        // boundary` for the outer region: the declared content extent must
        // put `NESTED_OUTER_MAX_ROWS` exactly at the declared limit, or the
        // outer would keep reporting consumable room after the handler
        // saturates and the at-limit chaining to the page would never
        // engage — wheels over a saturated outer would be silently dead.
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 560.0,
                height: 292.0,
            },
        };
        let viewport_height = AdmissionWidget::nested_outer_viewport(bounds).size.height;
        let content_height = AdmissionWidget::nested_outer_content_height();
        let max_offset = (content_height - viewport_height).max(0.0);
        let bottom_offset = AdmissionWidget::nested_scroll_offset_y(
            NESTED_OUTER_MAX_ROWS,
            NESTED_OUTER_ROW_STEP,
            viewport_height,
            content_height,
        );
        let before_bottom_offset = AdmissionWidget::nested_scroll_offset_y(
            NESTED_OUTER_MAX_ROWS - 1,
            NESTED_OUTER_ROW_STEP,
            viewport_height,
            content_height,
        );

        assert_eq!(bottom_offset, max_offset);
        assert!(before_bottom_offset < max_offset);
        // The anchoring rule itself: the whole outer travel stays under
        // half a panel height, so no panel-center anchor can escape its
        // panel anywhere across the outer's travel.
        assert!(max_offset < NESTED_INNER_VIEWPORT_HEIGHT / 2.0);
    }

    #[test]
    fn wheel_event_scrolls_list_by_delta_direction_without_selecting_item() {
        let state = AdmissionState::default();
        let list = AdmissionWidget::List(ListWidget);
        let mut local = AdmissionLocal::List { scroll_rows: 2 };

        let down = list.handle_event(
            &state,
            &mut local,
            InputEvent::Wheel(slipway_core::WheelEvent {
                target: list.id(),
                target_slot: None,
                region_id: None,
                delta_x: 0.0,
                delta_y: -1.0,
            }),
        );

        assert!(down.handled);
        assert_eq!(local, AdmissionLocal::List { scroll_rows: 3 });
        assert!(down.emitted_messages.is_empty());

        let up = list.handle_event(
            &state,
            &mut local,
            InputEvent::Wheel(slipway_core::WheelEvent {
                target: list.id(),
                target_slot: None,
                region_id: None,
                delta_x: 0.0,
                delta_y: 1.0,
            }),
        );

        assert!(up.handled);
        assert_eq!(local, AdmissionLocal::List { scroll_rows: 2 });
        assert!(up.emitted_messages.is_empty());
    }

    #[test]
    fn list_bottom_scroll_offset_stays_inside_declared_bounds() {
        let state = AdmissionState::default();
        let list = AdmissionWidget::List(ListWidget);
        let local = AdmissionLocal::List { scroll_rows: 5 };
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 560.0,
                    height: 168.0,
                },
            }),
            content: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 560.0,
                    height: 168.0,
                },
            }),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: Size {
                    width: 560.0,
                    height: 168.0,
                },
            },
        };

        let view = list.view_definition(
            &state,
            &local,
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
        );
        let scroll = view
            .scroll_regions
            .first()
            .expect("list exposes a scroll region");
        let max_offset = (scroll.content_bounds.size.height - scroll.viewport.size.height).max(0.0);

        assert!(scroll.offset.y <= max_offset);
        let diagnostics = slipway_core::view_definition_contract_diagnostics_for_capabilities(
            &view,
            &list.capabilities(),
        );
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.scroll_offset_out_of_range"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn runtime_list_bottom_wheel_dispatches_to_root_scroll() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let mut runtime = SlipwayRuntime::new(widget, AdmissionState::default());
        runtime.local_state_mut().widgets.5 = AdmissionLocal::List { scroll_rows: 5 };
        runtime.record_presented_viewport(admission_test_frame(640.0, 480.0).viewport);

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame);
        let input = wheel_backend_input_for_view(&view, Point { x: 42.0, y: 464.0 }, -1.0);

        let InputEvent::Wheel(wheel) = &input.event else {
            panic!("expected wheel backend input");
        };
        assert_eq!(
            wheel.region_id.as_ref(),
            Some(&AdmissionAppLocal::root_scroll_region_id()),
            "down-wheel over a list already at its bottom must bubble to the root scroll owner"
        );

        let report = apply_test_backend_input(&mut runtime, input);
        assert!(report.handled, "{:?}", report.diagnostics);
        assert!(
            runtime.local_state().app.root_scroll_y > 0.0,
            "root scroll local state must move when the list cannot consume the down-wheel"
        );
    }

    #[test]
    fn runtime_refreshes_stale_wheel_evidence_at_list_boundary() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let mut runtime = SlipwayRuntime::new(widget, AdmissionState::default());
        runtime.record_presented_viewport(admission_test_frame(640.0, 480.0).viewport);

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame);
        let stale_list_wheel =
            wheel_backend_input_for_view(&view, Point { x: 42.0, y: 464.0 }, -1.0);

        for _ in 0..6 {
            let report = apply_test_backend_input(&mut runtime, stale_list_wheel.clone());
            assert!(report.handled, "{:?}", report.diagnostics);
        }

        assert_eq!(
            runtime.local_state().widgets.5,
            AdmissionLocal::List { scroll_rows: 5 }
        );
        assert!(
            runtime.local_state().app.root_scroll_y > 0.0,
            "stale backend wheel evidence must be refreshed against current declarations and bubble to root at list bottom"
        );
    }

    #[test]
    fn runtime_refreshes_stale_native_scroll_evidence_before_apply() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let mut runtime = SlipwayRuntime::new(widget, AdmissionState::default());
        runtime.record_presented_viewport(admission_test_frame(640.0, 480.0).viewport);

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame.clone());
        let root_scroll = view
            .scroll_regions
            .iter()
            .find(|region| region.id == AdmissionAppLocal::root_scroll_region_id())
            .expect("root scroll exists");
        let stale_event = InputEvent::Scroll(slipway_core::ScrollEvent {
            target: root_scroll.target.clone(),
            target_slot: root_scroll.address.clone(),
            region_id: root_scroll.id.clone(),
            offset_x: 0.0,
            offset_y: 60.0,
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 1.0,
                    height: 1.0,
                },
            }),
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 1.0,
                    height: 1.0,
                },
            }),
        });
        let stale_evidence = slipway_core::declared_scroll_dispatch_evidence(
            slipway_core::EvidenceSource::backend_presented("iced", "native-scroll"),
            frame,
            &view.scroll_regions,
            Some(root_scroll),
            stale_event.clone(),
        );

        let report = apply_test_backend_input(
            &mut runtime,
            slipway_core::BackendInputEvent::declared(stale_event, stale_evidence),
        );

        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.local_state().app.root_scroll_y, 60.0);
    }

    #[test]
    fn nested_inner_scroll_declared_extent_matches_handler_boundary() {
        let viewport_height = NESTED_INNER_VIEWPORT_HEIGHT;
        let content_height = AdmissionWidget::nested_inner_content_height(viewport_height);
        let max_offset = (content_height - viewport_height).max(0.0);
        let bottom_offset = AdmissionWidget::nested_scroll_offset_y(
            NESTED_SCROLL_MAX_ROWS,
            NESTED_SCROLL_ROW_STEP,
            viewport_height,
            content_height,
        );
        let before_bottom_offset = AdmissionWidget::nested_scroll_offset_y(
            NESTED_SCROLL_MAX_ROWS - 1,
            NESTED_SCROLL_ROW_STEP,
            viewport_height,
            content_height,
        );

        assert_eq!(bottom_offset, max_offset);
        assert!(before_bottom_offset < max_offset);
        assert_eq!(
            nested_scroll_rows_after_wheel(NESTED_SCROLL_MAX_ROWS, -1.0, NESTED_SCROLL_MAX_ROWS),
            NESTED_SCROLL_MAX_ROWS
        );
        assert_eq!(
            nested_scroll_rows_after_wheel(
                NESTED_SCROLL_MAX_ROWS - 1,
                -1.0,
                NESTED_SCROLL_MAX_ROWS
            ),
            NESTED_SCROLL_MAX_ROWS
        );
    }

    #[test]
    fn list_release_is_consumed_without_changing_selection_or_scroll() {
        let state = AdmissionState::default();
        let list = AdmissionWidget::List(ListWidget);
        let mut local = AdmissionLocal::List { scroll_rows: 2 };

        let outcome = list.handle_event(
            &state,
            &mut local,
            InputEvent::Pointer(slipway_core::PointerEvent {
                target: list.id(),
                target_slot: None,
                position: Point { x: 48.0, y: 64.0 },
                target_bounds: None,
                kind: slipway_core::PointerEventKind::Release,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
            }),
        );

        assert!(outcome.handled);
        assert!(outcome.emitted_messages.is_empty());
        assert_eq!(local, AdmissionLocal::List { scroll_rows: 2 });
    }

    #[test]
    fn runtime_app_view_definition_keeps_child_paint_out_of_root_paint() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "test-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    },
                },
                LayoutInput {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    }),
                    content: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    },
                },
            ),
        );

        assert!(
            view.paint
                .iter()
                .all(|op| !matches!(op, PaintOp::Text { .. })),
            "root view paint must not duplicate child text paint"
        );
        assert_eq!(view.layout.child_placements().len(), 9);
        assert!(
            view.hit_regions
                .iter()
                .any(|region| region.target == WidgetId::from("admission.toggle"))
        );
    }

    #[test]
    fn runtime_app_declares_root_scroll_when_content_exceeds_viewport() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "test-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    },
                },
                LayoutInput {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    }),
                    content: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    },
                },
            ),
        );

        let root_scroll = view
            .scroll_regions
            .iter()
            .find(|region| region.id == AdmissionAppLocal::root_scroll_region_id())
            .expect("root overflow must be an explicit scroll declaration");
        assert_eq!(root_scroll.target, widget.id());
        assert_eq!(
            root_scroll.address,
            Some(WidgetSlotAddress::new(widget.id(), 0))
        );
        assert!(root_scroll.content_bounds.size.height > root_scroll.viewport.size.height);
        assert_eq!(root_scroll.order.z_index, -1);
        let terminal_region_index = view
            .wheel_traversal_boundary
            .terminal_region_index
            .expect("overflowing app declares its root as terminal");
        assert_eq!(
            view.scroll_regions[terminal_region_index].id,
            AdmissionAppLocal::root_scroll_region_id()
        );
        assert_eq!(
            view.scroll_regions
                .iter()
                .filter(|region| region
                    .id
                    .as_str()
                    .contains("admission.nested-scroll:inner-"))
                .count(),
            3,
            "runtime app view must preserve nested inner scroll declarations"
        );
    }

    #[test]
    fn mounted_producer_root_bottom_wheel_is_consumed_no_op() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let mut local = widget.initial_local_state();
        let frame = admission_test_frame(640.0, 480.0);
        let initial = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame.clone(), widget_layout_input(640.0, 480.0)),
        );
        let initial_index = initial
            .wheel_traversal_boundary
            .terminal_region_index
            .expect("root terminal index");
        let initial_root = &initial.scroll_regions[initial_index];
        local.app.root_scroll_y =
            initial_root.content_bounds.size.height - initial_root.viewport.size.height;

        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame, widget_layout_input(640.0, 480.0)),
        );
        let terminal_region_index = view
            .wheel_traversal_boundary
            .terminal_region_index
            .expect("mounted root terminal index");
        assert_eq!(terminal_region_index, initial_index);
        let root = &view.scroll_regions[terminal_region_index];
        assert_eq!(root.id, AdmissionAppLocal::root_scroll_region_id());
        assert_eq!(
            root.address,
            Some(WidgetSlotAddress::new(widget.id(), 0)),
            "mounting must not erase producer ownership"
        );

        let geometry_index = slipway_core::PresentationGeometryIndex::from_layout(&view.layout);
        let disposition =
            slipway_core::declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &view.scroll_regions,
                view.wheel_traversal_boundary,
                Point { x: 10.0, y: 470.0 },
                0.0,
                -1.0,
            );
        assert!(matches!(
            disposition,
            slipway_core::DeclaredWheelDisposition::ConsumedNoOp(region)
                if region.id == AdmissionAppLocal::root_scroll_region_id()
        ));
    }

    #[test]
    fn mounted_producer_clamps_overscrolled_root_before_contract_admission() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let mut local = widget.initial_local_state();
        let frame = admission_test_frame(640.0, 480.0);
        let initial = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame.clone(), widget_layout_input(640.0, 480.0)),
        );
        let initial_index = initial
            .wheel_traversal_boundary
            .terminal_region_index
            .expect("root terminal index");
        let initial_root = &initial.scroll_regions[initial_index];
        let max_offset =
            (initial_root.content_bounds.size.height - initial_root.viewport.size.height).max(0.0);
        local.app.root_scroll_y = max_offset + 48.0;

        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame, widget_layout_input(640.0, 480.0)),
        );
        let terminal_region_index = view
            .wheel_traversal_boundary
            .terminal_region_index
            .expect("mounted root terminal index");
        let root = &view.scroll_regions[terminal_region_index];

        assert_eq!(root.id, AdmissionAppLocal::root_scroll_region_id());
        assert_eq!(root.offset.y, max_offset);
        let diagnostics = slipway_core::view_definition_contract_diagnostics_for_capabilities(
            &view,
            &widget.capabilities(),
        );
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.scroll_offset_out_of_range"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn runtime_render_packet_applies_root_scroll_to_canonical_viewport() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let mut runtime = SlipwayRuntime::new(widget, AdmissionState::default());
        runtime.local_state_mut().app.root_scroll_y = 220.0;

        let packet = runtime
            .render_packet_for_frame(FrameIdentity {
                surface_id: "test-surface".to_string(),
                surface_instance_id: "test-instance".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 640.0,
                        height: 480.0,
                    },
                },
            })
            .expect("valid admission view must render a packet before root-scroll assertions");

        let PaintOp::Group { clip, ops, .. } = packet
            .paint
            .first()
            .expect("canonical root scroll wraps paint in a viewport clip")
        else {
            panic!("canonical root scroll must clip the visible viewport");
        };
        assert_eq!(
            clip.as_ref().map(|clip| clip.bounds.size.height),
            Some(480.0)
        );
        let action_label_y = first_text_y_containing(ops, "action | Counter button")
            .expect("action card label remains present in scrolled canonical paint");
        assert!(
            action_label_y < 0.0,
            "root scroll must translate top content out of the visible viewport, got y={action_label_y}"
        );
    }

    fn first_text_y_containing(ops: &[PaintOp], needle: &str) -> Option<f32> {
        for op in ops {
            match op {
                PaintOp::Text {
                    bounds, content, ..
                } if content.contains(needle) => return Some(bounds.origin.y),
                PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                    if let Some(y) = first_text_y_containing(ops, needle) {
                        return Some(y);
                    }
                }
                _ => {}
            }
        }
        None
    }

    #[test]
    fn runtime_app_wrapper_exposes_authored_children_to_backends() {
        struct CountingVisitor {
            ids: Vec<WidgetId>,
        }

        impl SlipwayWidgetListVisitor<AdmissionState, AdmissionMessage> for CountingVisitor {
            fn visit_child<W>(
                &mut self,
                widget: &W,
                _external: &AdmissionState,
                _local: &W::LocalState,
                _slot: WidgetSlotAddress,
            ) where
                W: slipway_core::SlipwayWidget<
                        ExternalState = AdmissionState,
                        AppMessage = AdmissionMessage,
                    > + SlipwayViewDefinition,
            {
                self.ids.push(widget.id());
            }
        }

        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let mut visitor = CountingVisitor { ids: Vec::new() };

        widget.visit_authored_children(&external, &local, &mut visitor);

        assert_eq!(
            visitor.ids,
            vec![
                WidgetId::from("admission.action"),
                WidgetId::from("admission.segment"),
                WidgetId::from("admission.text"),
                WidgetId::from("admission.toggle"),
                WidgetId::from("admission.slider"),
                WidgetId::from("admission.list"),
                WidgetId::from("admission.overlay"),
                WidgetId::from("admission.overlay-stack"),
                WidgetId::from("admission.nested-scroll"),
            ]
        );
    }

    #[test]
    fn runtime_app_wrapper_forwards_egui_paint_ordered_children() {
        struct CountingEguiVisitor {
            ids: Vec<WidgetId>,
        }

        impl SlipwayEguiWidgetListVisitor<AdmissionState, AdmissionMessage> for CountingEguiVisitor {
            fn visit_egui_child<W>(
                &mut self,
                widget: &W,
                _external: &AdmissionState,
                _local: &W::LocalState,
                _slot: WidgetSlotAddress,
            ) where
                W: SlipwayEguiBackendChildWidget<
                        ExternalState = AdmissionState,
                        AppMessage = AdmissionMessage,
                    >,
            {
                self.ids.push(widget.id());
            }

            fn visit_egui_native_child<N>(
                &mut self,
                widget: &N,
                _external: &AdmissionState,
                _local: &N::LocalState,
                _slot: WidgetSlotAddress,
            ) where
                N: SlipwayEguiNativeChildWidget<
                        ExternalState = AdmissionState,
                        AppMessage = AdmissionMessage,
                    >,
            {
                self.ids.push(widget.id());
            }
        }

        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let frame = FrameIdentity {
            surface_id: "egui-wrapper-test".to_string(),
            surface_instance_id: "egui-wrapper-test-instance".to_string(),
            revision: 0,
            frame_index: 0,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 640.0,
                    height: 520.0,
                },
            },
        };
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(frame.viewport),
            content: TargetLocalRect::new(frame.viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame.viewport.size,
            },
        };
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame, layout_input),
        );
        let mut visitor = CountingEguiVisitor { ids: Vec::new() };

        widget.visit_egui_authored_children_in_paint_order(&external, &local, &view, &mut visitor);

        assert_eq!(
            visitor.ids,
            vec![
                WidgetId::from("admission.action"),
                WidgetId::from("admission.segment"),
                WidgetId::from("admission.text"),
                WidgetId::from("admission.toggle"),
                WidgetId::from("admission.slider"),
                WidgetId::from("admission.list"),
                WidgetId::from("admission.nested-scroll"),
                WidgetId::from("admission.overlay-stack"),
                WidgetId::from("admission.overlay"),
            ]
        );
    }

    #[test]
    fn runtime_app_visible_egui_root_admission_keeps_children() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let frame = FrameIdentity {
            surface_id: "egui-root-admission-test".to_string(),
            surface_instance_id: "egui-root-admission-test-instance".to_string(),
            revision: 0,
            frame_index: 0,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 904.0,
                    height: 721.0,
                },
            },
        };
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(frame.viewport),
            content: TargetLocalRect::new(frame.viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame.viewport.size,
            },
        };
        let view = widget.visible_backend_view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame, layout_input),
        );
        let admission = egui_backend_admission()
            .admit_view_definition_with_capabilities(&widget.capabilities(), &view);

        assert_eq!(view.layout.child_placements().len(), 9);
        assert!(
            admission.accepted,
            "root admission refused: {:?}",
            admission.unsupported
        );
    }

    #[test]
    fn runtime_app_visible_egui_text_child_passes_mounted_admission() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let frame = FrameIdentity {
            surface_id: "egui-text-child-admission-test".to_string(),
            surface_instance_id: "egui-text-child-admission-test-instance".to_string(),
            revision: 0,
            frame_index: 0,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 800.0,
                    height: 600.0,
                },
            },
        };
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(frame.viewport),
            content: TargetLocalRect::new(frame.viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame.viewport.size,
            },
        };
        let parent_view = widget.visible_backend_view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame, layout_input),
        );
        let text_placement = parent_view
            .layout
            .child_placements()
            .iter()
            .find(|placement| placement.child == WidgetId::from("admission.text"))
            .expect("app places the text child");
        let text_input = slipway_core::child_layout_input_for_placement(text_placement);
        let text_widget = AdmissionWidget::Text(TextWidget);
        let text_local = &local.widgets.2;
        let mut text_view = text_widget.visible_backend_view_definition(
            &external,
            text_local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "egui-text-child".to_string(),
                    surface_instance_id: "egui-text-child-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: text_input.viewport.into_rect(),
                },
                text_input,
            ),
        );
        let text_slot = WidgetSlotAddress::new(widget.id(), 0).child(text_widget.id(), 2);
        for region in &mut text_view.hit_regions {
            region.address = Some(text_slot.clone());
            region.route.address = Some(text_slot.clone());
            region.route.path = text_slot.path.clone();
        }
        for region in &mut text_view.focus_regions {
            region.address = Some(text_slot.clone());
        }

        let diagnostics = slipway_core::validate_and_index_view(&text_view)
            .err()
            .unwrap_or_default();
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let admission = egui_backend_admission()
            .admit_view_definition_with_capabilities(&text_widget.capabilities(), &text_view);
        assert!(
            admission.accepted,
            "{:?}",
            admission
                .unsupported
                .iter()
                .flat_map(|unsupported| unsupported.diagnostics.iter())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn runtime_app_child_hit_regions_use_parent_runtime_slots() {
        let widget = AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        });
        let external = AdmissionState::default();
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "test-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    },
                },
                LayoutInput {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    }),
                    content: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 640.0,
                            height: 480.0,
                        },
                    },
                },
            ),
        );

        let expected =
            WidgetSlotAddress::new(widget.id(), 0).child(WidgetId::from("admission.toggle"), 3);
        let toggle = view
            .hit_regions
            .iter()
            .find(|region| region.target == WidgetId::from("admission.toggle"))
            .expect("toggle hit region must be mounted");

        assert_eq!(toggle.address, Some(expected.clone()));
        assert_eq!(toggle.route.address, Some(expected));
    }

    #[test]
    fn runtime_app_backend_pointer_event_reaches_child_and_app_reducer() {
        let mut runtime = SlipwayRuntime::new(
            AdmissionRuntimeAppWidget::new(AdmissionApp {
                widgets: admission_widget_tuple(),
            }),
            AdmissionState::default(),
        );
        runtime.record_presented_viewport(admission_test_frame(640.0, 700.0).viewport);
        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame);

        let backend_input = pointer_backend_input_for_view(&view, Point { x: 24.0, y: 24.0 });
        assert_eq!(
            backend_input.event.target(),
            &WidgetId::from("admission.action")
        );

        let report = apply_test_backend_input(&mut runtime, backend_input);

        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(report.applied_messages, 1, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().counter, 1);
    }

    #[test]
    fn runtime_app_backend_inputs_cover_all_admission_interactions() {
        let mut runtime = SlipwayRuntime::new(
            AdmissionRuntimeAppWidget::new(AdmissionApp {
                widgets: admission_widget_tuple(),
            }),
            AdmissionState::default(),
        );
        runtime.record_presented_viewport(admission_test_frame(640.0, 700.0).viewport);

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            pointer_backend_input_for_view(&view, Point { x: 24.0, y: 112.0 }),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().segment, Segment::Week);

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            text_edit_backend_input_for_view(&view, "edited through backend"),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().draft, "edited through backend");

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            pointer_backend_input_for_view(&view, Point { x: 24.0, y: 288.0 }),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert!(!runtime.external().enabled);

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            pointer_backend_input_for_view(&view, Point { x: 520.0, y: 376.0 }),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert!(runtime.external().intensity > 0.9);

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            wheel_backend_input_for_view(&view, Point { x: 24.0, y: 464.0 }, -1.0),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().selected_item, 0);

        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame);
        let report = apply_test_backend_input(
            &mut runtime,
            pointer_backend_input_for_view(&view, Point { x: 48.0, y: 520.0 }),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().selected_item, 1);
    }

    fn admission_dispatch_graphs() -> (
        slipway_core::DispatchGraph,
        slipway_core::DispatchGraph,
        ViewDefinition,
    ) {
        let mut runtime = SlipwayRuntime::new(
            AdmissionRuntimeAppWidget::new(AdmissionApp {
                widgets: admission_widget_tuple(),
            }),
            AdmissionState::default(),
        );
        runtime.record_presented_viewport(admission_test_frame(640.0, 700.0).viewport);
        let frame = runtime.last_frame_identity();
        let view = admission_view(&runtime, frame.clone());

        let iced_graph = slipway_backend_iced::iced_dispatch_graph_for_widget(
            runtime.widget(),
            runtime.external(),
            runtime.local_state(),
            widget_layout_input(640.0, 700.0),
            Some(&frame),
        );
        let egui_graph = slipway_backend_egui::egui_dispatch_graph_for_widget(
            runtime.widget(),
            runtime.external(),
            runtime.local_state(),
            widget_layout_input(640.0, 700.0),
            Some(&frame),
        );
        (iced_graph, egui_graph, view)
    }

    #[test]
    fn dispatch_graph_parity_between_iced_and_egui_presentation_pipelines() {
        let (iced_graph, egui_graph, view) = admission_dispatch_graphs();

        assert!(
            iced_graph.nodes.iter().any(|node| node.kind
                == slipway_core::DispatchGraphNodeKind::Scroll
                && node.id == "admission.app:root-scroll"),
            "iced graph must include the root scroll node"
        );
        assert!(
            !view.hit_regions.is_empty(),
            "admission view must declare hit regions"
        );

        // Golden parity: both backend presentation pipelines derive the SAME
        // dispatch graph from the admission view. Raw equality is intended;
        // any divergence here is a parity finding, not test noise.
        assert_eq!(iced_graph, egui_graph);
    }

    #[test]
    fn dispatch_graph_wheel_transparent_overlays_occlude_pointer_but_not_wheel() {
        let (iced_graph, egui_graph, _view) = admission_dispatch_graphs();

        for graph in [&iced_graph, &egui_graph] {
            // The composed root view attributes paint units to the app
            // target, so overlay-window occluders are identified by the
            // admission overlay z-layer (ADMISSION_OVERLAY_GLOBAL_Z).
            let overlay_occluders: Vec<_> = graph
                .nodes
                .iter()
                .filter(|node| {
                    node.kind == slipway_core::DispatchGraphNodeKind::Occlusion
                        && node.order.z_index == ADMISSION_OVERLAY_GLOBAL_Z
                })
                .collect();
            assert!(
                !overlay_occluders.is_empty(),
                "overlay windows must materialize occlusion nodes"
            );
            // de24dda0a semantics: every admission overlay window is authored
            // pointer-opaque + wheel-pass-through.
            for occluder in &overlay_occluders {
                assert_eq!(occluder.blocks_pointer, Some(true));
                assert_eq!(
                    occluder.blocks_wheel,
                    Some(false),
                    "overlay occluder {} must be wheel-transparent",
                    occluder.id
                );
            }

            // Pointer channel: the overlay-stack occluders block the hit
            // regions beneath them (lower cards' absorb/drag regions).
            let pointer_occlusion_from_overlays = graph
                .edges
                .iter()
                .filter(|edge| {
                    edge.kind == slipway_core::DispatchGraphEdgeKind::Occlusion
                        && edge.channel == slipway_core::DispatchGraphChannel::Pointer
                        && overlay_occluders.iter().any(|node| node.id == edge.from)
                })
                .count();
            assert!(
                pointer_occlusion_from_overlays > 0,
                "overlay occluders must have pointer occlusion edges to regions beneath"
            );

            // Wheel channel: wheel-transparent occluders must have NO wheel
            // occlusion edges; structurally, every wheel occlusion edge must
            // originate from a blocks_wheel occluder.
            for edge in &graph.edges {
                if edge.kind == slipway_core::DispatchGraphEdgeKind::Occlusion
                    && edge.channel == slipway_core::DispatchGraphChannel::Wheel
                {
                    let from = graph
                        .nodes
                        .iter()
                        .find(|node| node.id == edge.from)
                        .expect("wheel occlusion edge source node exists");
                    assert_eq!(
                        from.blocks_wheel,
                        Some(true),
                        "wheel occlusion edges may only originate from wheel-blocking occluders"
                    );
                }
            }
            assert!(
                !graph.edges.iter().any(|edge| {
                    edge.kind == slipway_core::DispatchGraphEdgeKind::Occlusion
                        && edge.channel == slipway_core::DispatchGraphChannel::Wheel
                        && overlay_occluders.iter().any(|node| node.id == edge.from)
                }),
                "wheel-transparent overlays must not block the wheel to the scroll owners beneath"
            );
        }
    }

    #[test]
    fn dispatch_graph_chaining_edges_reach_ancestor_scroll_owners() {
        let (iced_graph, egui_graph, _view) = admission_dispatch_graphs();

        for graph in [&iced_graph, &egui_graph] {
            let chaining: Vec<_> = graph
                .edges
                .iter()
                .filter(|edge| {
                    edge.kind == slipway_core::DispatchGraphEdgeKind::Chaining
                        && edge.channel == slipway_core::DispatchGraphChannel::Wheel
                })
                .collect();

            // The list scroll region chains to the root scroll owner: the
            // 177-191 bug-class scenario "at-limit list wheel bubbles to the
            // root" as structure.
            assert!(
                chaining.iter().any(|edge| {
                    edge.from == "admission.list:scroll" && edge.to == "admission.app:root-scroll"
                }),
                "list scroll must chain to the root scroll owner; edges: {chaining:?}"
            );

            // The nested inner scroll chains to its outer ancestor.
            assert!(
                chaining.iter().any(|edge| {
                    edge.from == "admission.nested-scroll:inner-0"
                        && edge.to == "admission.nested-scroll:outer"
                }),
                "nested inner scroll must chain to the outer scroll; edges: {chaining:?}"
            );
        }
    }

    #[test]
    fn dispatch_graph_wheel_hit_order_keeps_default_inner_in_front_of_outer() {
        // The derived graph (ADR-0002 B1) over the routing-aware selector
        // (B2) on an all-default view: with every declaration on
        // `NearestScrollable` the fronter inner panels win their wheel
        // HitOrder pairs against the outer, so the edges run inner -> outer
        // in BOTH backend graphs. (Step 194's authored SelfFirst flipped
        // these to outer -> inner; that authoring was reverted per the
        // architect's live UX decision, 2026-07-11, and the flip contract
        // stays proven by the core/backend synthetic-fixture graph tests.)
        let (iced_graph, egui_graph, _view) = admission_dispatch_graphs();

        for graph in [&iced_graph, &egui_graph] {
            let wheel_hit_order: Vec<_> = graph
                .edges
                .iter()
                .filter(|edge| {
                    edge.kind == slipway_core::DispatchGraphEdgeKind::HitOrder
                        && edge.channel == slipway_core::DispatchGraphChannel::Wheel
                })
                .collect();
            for inner in ["inner-0", "inner-1", "inner-2"] {
                let inner_id = format!("admission.nested-scroll:{inner}");
                assert!(
                    wheel_hit_order.iter().any(|edge| {
                        edge.from == inner_id && edge.to == "admission.nested-scroll:outer"
                    }),
                    "the fronter {inner_id} must win the default wheel HitOrder pair over the outer; edges: {wheel_hit_order:?}"
                );
                assert!(
                    !wheel_hit_order.iter().any(|edge| {
                        edge.from == "admission.nested-scroll:outer" && edge.to == inner_id
                    }),
                    "the Step 194 authored outer-first edge direction must be gone for {inner_id}"
                );
            }

            // Control: the list region keeps the default front-most edge
            // over the root scroll owner.
            assert!(
                wheel_hit_order.iter().any(|edge| {
                    edge.from == "admission.list:scroll" && edge.to == "admission.app:root-scroll"
                }),
                "list control must keep default front-most wheel order; edges: {wheel_hit_order:?}"
            );
        }
    }

    #[test]
    fn dispatch_graph_capture_edges_cover_overlay_drag_regions() {
        let (iced_graph, egui_graph, _view) = admission_dispatch_graphs();

        for graph in [&iced_graph, &egui_graph] {
            for overlay in 0..4 {
                let drag_id = format!("admission.overlay-stack:overlay-{overlay}:drag");
                assert!(
                    graph.edges.iter().any(|edge| {
                        edge.kind == slipway_core::DispatchGraphEdgeKind::Capture
                            && edge.channel == slipway_core::DispatchGraphChannel::Pointer
                            && edge.from == drag_id
                            && edge.to == "admission.overlay-stack"
                    }),
                    "overlay drag region {drag_id} must have a capture edge"
                );
                let drag_node = graph
                    .nodes
                    .iter()
                    .find(|node| node.id == drag_id)
                    .expect("overlay drag hit node exists");
                assert_eq!(
                    drag_node.capture,
                    Some(slipway_core::PointerCaptureIntent::DuringDrag)
                );
            }

            // The movable overlay titlebar drag region also captures.
            assert!(
                graph.edges.iter().any(|edge| {
                    edge.kind == slipway_core::DispatchGraphEdgeKind::Capture
                        && edge.from == "admission.overlay:hit"
                }),
                "movable overlay titlebar must have a capture edge"
            );
        }
    }
}
