use slipway_backend_egui::{
    SlipwayEguiAuthoredChildren, SlipwayEguiWidgetListVisitor,
    run_slipway_egui_runtime_app_with_default_bridge,
};
use slipway_backend_iced::{
    SlipwayIcedAuthoredChildren, SlipwayIcedWidgetListVisitor, run_slipway_iced_runtime_app,
};
use slipway_core::{
    AppLayoutPlan, Capability, CaretGeometryEvidence, CaretSet, ChangeEvidence, ChildLayoutPlan,
    ChildLayoutSeed, Color, CursorCapability, Diagnostic, EdgeInsets, EmittedMessage, EventOutcome,
    EventRoute, EventRoutePhase, EvidenceSource, FocusRegionDeclaration, FocusTraversalMember,
    FontResolutionEvidence, FontResolutionRequest, HitRegionDeclaration, HitRegionOrder,
    ImeCompositionPolicyDeclaration, InputEvent, LayoutConstraints, LayoutInput, LayoutOutput,
    PaintOp, PaintOrderDeclaration, ParentLocalRect, Point, PointerCaptureIntent,
    PresentationRegionId, ProbeCollector, ProbeMetadataDeclaration, ProbeProduct, Rect,
    ResourceRefusalEvidence, ScrollRegionDeclaration, SemanticNode, SemanticSlotDeclaration,
    ShapeDeclaration, ShapeKind, Size, SlipwayApp, SlipwayAppWidget, SlipwayFontResolutionPolicy,
    SlipwayLogic, SlipwaySsot, SlipwayView, SlipwayViewDefinition, SlipwayWidgetListVisitor,
    SlipwayWidgetTypes, SourceValidityEvidence, SourceValidityKind, StateObservation,
    TargetLocalRect, TextBufferSnapshot, TextEditCommandDeclaration, TextEditKind, TextLineMode,
    TextSelectionPolicyDeclaration, TextSelectionRange, TextStyle, TextViewport, TopologyNode,
    ViewDefinition, ViewDefinitionInput, WheelRouting, WidgetId, WidgetSlotAddress,
};
use slipway_runtime::SlipwayRuntime;

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
    run_slipway_iced_runtime_app(
        AdmissionRuntimeAppWidget::new(AdmissionApp {
            widgets: admission_widget_tuple(),
        }),
        AdmissionState::default(),
        apply_messages,
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
    ),
}

fn admission_widget_tuple() -> (
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
    )
}

impl SlipwayApp for AdmissionApp {
    type ExternalState = AdmissionState;
    type LocalState = ();
    type AppMessage = AdmissionMessage;
    type Widgets = (
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

    fn initial_local_state(&self) -> Self::LocalState {}

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
            let height = if seed.child.as_str() == "admission.list" {
                168.0
            } else {
                76.0
            };
            let bounds = Rect {
                origin: Point { x: 16.0, y },
                size: Size {
                    width: (width - 32.0).max(1.0),
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
        _input: LayoutInput,
        children: Vec<slipway_core::ChildPlacement>,
    ) -> LayoutOutput {
        let height = children
            .iter()
            .map(|child| child.bounds.origin.y + child.bounds.size.height)
            .fold(0.0, f32::max)
            + 16.0;
        let width = children
            .iter()
            .map(|child| child.bounds.origin.x + child.bounds.size.width)
            .fold(0.0, f32::max)
            + 16.0;

        LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size { width, height },
            }),
            child_placements: children,
            diagnostics: Vec::new(),
        }
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
                layout.bounds.into_rect(),
                ShapeKind::Rectangle,
            ),
            color: rgb(241, 245, 249),
        }]
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
    ) -> LayoutOutput {
        self.inner.layout(external, local, input)
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
        self.inner.view_definition(external, local, input)
    }
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
}

fn admission_widgets() -> Vec<AdmissionWidget> {
    vec![
        AdmissionWidget::Action(ActionWidget),
        AdmissionWidget::Segment(SegmentWidget),
        AdmissionWidget::Text(TextWidget),
        AdmissionWidget::Toggle(ToggleWidget),
        AdmissionWidget::Slider(SliderWidget),
        AdmissionWidget::List(ListWidget),
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
    SelectNextItem,
}

fn apply_messages(state: &mut AdmissionState, messages: Vec<AdmissionMessage>) {
    for message in messages {
        match message {
            AdmissionMessage::Increment => state.counter += 1,
            AdmissionMessage::SelectNextSegment => state.segment = state.segment.next(),
            AdmissionMessage::UpdateDraft(text) => state.draft = text,
            AdmissionMessage::ToggleEnabled => state.enabled = !state.enabled,
            AdmissionMessage::SetIntensity(value) => state.intensity = value.clamp(0.0, 1.0),
            AdmissionMessage::SelectNextItem => state.selected_item = (state.selected_item + 1) % 5,
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

#[derive(Clone, Debug, PartialEq)]
enum AdmissionWidget {
    Action(ActionWidget),
    Segment(SegmentWidget),
    Text(TextWidget),
    Toggle(ToggleWidget),
    Slider(SliderWidget),
    List(ListWidget),
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

impl AdmissionWidget {
    fn role(&self) -> &'static str {
        match self {
            Self::Action(_) => "action",
            Self::Segment(_) => "segment",
            Self::Text(_) => "text-input",
            Self::Toggle(_) => "toggle",
            Self::Slider(_) => "slider",
            Self::List(_) => "scroll-list",
        }
    }

    fn text_after_command(&self, state: &AdmissionState) -> Option<String> {
        match self {
            Self::Text(_) => Some(format!("{}*", state.draft)),
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
        }
    }

    fn view_declarations(
        &self,
        external: &AdmissionState,
        local: &AdmissionLocal,
        input: &LayoutInput,
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

        if self.has_pointer_region() {
            declarations
                .hit_regions
                .push(slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    PresentationRegionId::from(format!("{}:hit", target.as_str())),
                    address.clone(),
                    TargetLocalRect::new(bounds),
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    HitRegionOrder {
                        z_index: 0,
                        paint_order: self.traversal_order(),
                        traversal_order: self.traversal_order(),
                    },
                    Some(format!("{}:hit", target.as_str())),
                    self.cursor(local),
                    true,
                    if matches!(self, Self::Slider(_)) {
                        PointerCaptureIntent::DuringDrag
                    } else {
                        PointerCaptureIntent::OnPress
                    },
                ));
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
                    TargetLocalRect::new(bounds),
                    slipway_core::SlipwayFocusTraversal::focus_member(self, external, local),
                    true,
                    input,
                    None,
                ));
        }

        if matches!(self, Self::List(_)) {
            declarations.scroll_regions.push(
                slipway_core::scroll_region_from_scrollable_capability(
                    self,
                    external,
                    local,
                    input,
                    Some(PresentationRegionId::from(format!(
                        "{}:scroll",
                        target.as_str()
                    ))),
                    address,
                    true,
                ),
            );
        }

        declarations
    }

    fn has_pointer_region(&self) -> bool {
        matches!(
            self,
            Self::Action(_) | Self::Segment(_) | Self::Toggle(_) | Self::Slider(_) | Self::List(_)
        )
    }

    fn cursor(&self, local: &AdmissionLocal) -> CursorCapability {
        match (self, local) {
            (Self::Slider(_), AdmissionLocal::Slider { dragging: true }) => {
                CursorCapability::Grabbing
            }
            (Self::Slider(_), _) => CursorCapability::Grab,
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
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
struct AdmissionViewDeclarations {
    hit_regions: Vec<HitRegionDeclaration>,
    focus_regions: Vec<FocusRegionDeclaration>,
    scroll_regions: Vec<ScrollRegionDeclaration>,
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
        CaretGeometryEvidence {
            target: self.id(),
            caret_bounds: Vec::new(),
            selection_bounds: Vec::new(),
            measurement_request_ids: Vec::new(),
            diagnostics: vec![Diagnostic::unsupported(
                Some(self.id()),
                "example-caret-geometry-unmeasured",
                "the admission example declares editable text but does not claim backend text metrics",
            )],
        }
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
        ]
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
        let disposition = slipway_core::EventDisposition {
            handled: true,
            propagate: false,
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
            bounds: output.bounds,
            child_placements: output.child_placements.clone(),
            invalidated: false,
            diagnostics: output.diagnostics.clone(),
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
        let scroll_y = match local {
            AdmissionLocal::List { scroll_rows } => (*scroll_rows as f32 * 20.0).max(0.0),
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
                height: 260.0_f32.max(viewport.size.height),
            },
            viewport: input.viewport,
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: viewport.size.width,
                    height: 260.0_f32.max(viewport.size.height),
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
        _wheel: &slipway_core::WheelEvent,
    ) -> slipway_core::WheelRoutingPolicyDeclaration {
        slipway_core::WheelRoutingPolicyDeclaration {
            target: self.id(),
            routing: WheelRouting::SelfFirst,
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
        let viewport = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 560.0,
                height: 168.0,
            },
        };
        let offset_y = match local {
            AdmissionLocal::List { scroll_rows } => (*scroll_rows as f32 * 20.0).max(0.0),
            _ => 0.0,
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
                    height: 260.0,
                },
                viewport,
                content_bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: viewport.size.width,
                        height: 260.0,
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
                EventOutcome::ignored()
            }
            (Self::Text(_), InputEvent::Command(_)) => {
                if let Some(text) = self.text_after_command(external) {
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
                EventOutcome::ignored()
            }
            (Self::List(_), InputEvent::Wheel(wheel)) => {
                if let AdmissionLocal::List { scroll_rows } = local {
                    *scroll_rows += wheel.delta_y.signum() as i32;
                }
                message_outcome(
                    self.id(),
                    "select-next-item",
                    AdmissionMessage::SelectNextItem,
                    "selected-item",
                    "wheel",
                )
            }
            (Self::List(_), InputEvent::Scroll(scroll)) => {
                if let AdmissionLocal::List { scroll_rows } = local {
                    *scroll_rows = (scroll.offset_y / 20.0).round().max(0.0) as i32;
                }
                message_outcome(
                    self.id(),
                    "scroll-list",
                    AdmissionMessage::SelectNextItem,
                    "scroll-offset-y",
                    format!("{:.1}", scroll.offset_y),
                )
            }
            (Self::List(_), InputEvent::Pointer(pointer))
                if pointer.kind == slipway_core::PointerEventKind::Press =>
            {
                message_outcome(
                    self.id(),
                    "select-next-item",
                    AdmissionMessage::SelectNextItem,
                    "selected-item",
                    "pointer",
                )
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
        let layout_input = input.layout_input;
        let layout = self.layout(external, local, layout_input.clone());
        let paint = self.paint(external, local, &layout);
        let mut declarations = self.view_declarations(
            external,
            local,
            &layout_input,
            layout.bounds.into_rect(),
            Some(WidgetSlotAddress::new(self.id(), 0)),
        );
        declarations.diagnostics.extend(layout.diagnostics.clone());

        ViewDefinition {
            target: self.id(),
            frame: input.frame,
            layout,
            paint,
            paint_order: PaintOrderDeclaration::source_order(self.id()),
            hit_regions: declarations.hit_regions,
            focus_regions: declarations.focus_regions,
            scroll_regions: declarations.scroll_regions,
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
        }
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
    ) -> LayoutOutput {
        let width = input
            .constraints
            .max
            .width
            .min(560.0)
            .max(input.constraints.min.width)
            .max(220.0);
        let height = match self {
            Self::List(_) => 168.0,
            _ => 76.0,
        };
        LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size { width, height },
            }),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        let bounds = layout.bounds.into_rect();
        let title = match self {
            Self::Action(_) => format!("Counter button: {}", external.counter),
            Self::Segment(_) => format!("Segment: {}", external.segment.label()),
            Self::Text(_) => format!("Draft: {}", external.draft),
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
        };

        let mut ops = vec![
            PaintOp::Fill {
                shape: rect_shape("panel", bounds, ShapeKind::RoundedRectangle),
                color: rgb(248, 250, 252),
            },
            PaintOp::Stroke {
                shape: rect_shape("outline", bounds, ShapeKind::RoundedRectangle),
                color: rgb(203, 213, 225),
                width: 1.0,
            },
            PaintOp::Text {
                bounds: inset(
                    bounds,
                    EdgeInsets {
                        top: 12.0,
                        right: 12.0,
                        bottom: 12.0,
                        left: 12.0,
                    },
                ),
                content: format!("{} | {}", self.role(), title),
                color: rgb(15, 23, 42),
                style: TextStyle::default(),
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
            for i in 0..5 {
                ops.push(PaintOp::Text {
                    bounds: Rect {
                        origin: Point {
                            x: 24.0,
                            y: 44.0 + i as f32 * 20.0,
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
                    style: TextStyle::default(),
                });
            }
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

fn intensity_from_pointer(x: f32) -> f32 {
    ((x - 24.0) / 496.0).clamp(0.0, 1.0)
}

fn rgb(red: u8, green: u8, blue: u8) -> Color {
    Color {
        red: f32::from(red) / 255.0,
        green: f32::from(green) / 255.0,
        blue: f32::from(blue) / 255.0,
        alpha: 1.0,
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

fn inset(rect: Rect, insets: EdgeInsets) -> Rect {
    Rect {
        origin: Point {
            x: rect.origin.x + insets.left,
            y: rect.origin.y + insets.top,
        },
        size: Size {
            width: (rect.size.width - insets.left - insets.right).max(0.0),
            height: (rect.size.height - insets.top - insets.bottom).max(0.0),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slipway_core::FrameIdentity;

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
            ViewDefinitionInput {
                frame,
                layout_input,
            },
        )
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
        runtime.apply_backend_input_event_with_app_reducer(input, &mut reducer)
    }

    #[test]
    fn example_uses_core_traits_for_all_widgets() {
        let widgets = admission_widgets();
        assert_eq!(widgets.len(), 6);
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
    fn scroll_event_updates_list_local_scroll_and_scroll_region_is_declared() {
        let state = AdmissionState::default();
        let list = AdmissionWidget::List(ListWidget);
        let mut local = list.initial_local_state();
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(Rect {
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
            ViewDefinitionInput {
                frame: FrameIdentity {
                    surface_id: "test-surface".to_string(),
                    surface_instance_id: "test-instance".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: layout_input.viewport.into_rect(),
                },
                layout_input: layout_input.clone(),
            },
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
        assert_eq!(outcome.emitted_messages.len(), 1);
        assert_eq!(
            outcome.emitted_messages[0].message,
            AdmissionMessage::SelectNextItem
        );
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
            ViewDefinitionInput {
                frame: FrameIdentity {
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
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(Rect {
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
            },
        );

        assert!(
            view.paint
                .iter()
                .all(|op| !matches!(op, PaintOp::Text { .. })),
            "root view paint must not duplicate child text paint"
        );
        assert_eq!(view.layout.child_placements.len(), 6);
        assert!(
            view.hit_regions
                .iter()
                .any(|region| region.target == WidgetId::from("admission.toggle"))
        );
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
            ]
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
            ViewDefinitionInput {
                frame: FrameIdentity {
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
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(Rect {
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
            },
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
        let frame = admission_test_frame(640.0, 700.0);
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
        let frame = admission_test_frame(640.0, 700.0);

        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            pointer_backend_input_for_view(&view, Point { x: 24.0, y: 112.0 }),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().segment, Segment::Week);

        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            text_edit_backend_input_for_view(&view, "edited through backend"),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().draft, "edited through backend");

        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            pointer_backend_input_for_view(&view, Point { x: 24.0, y: 288.0 }),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert!(!runtime.external().enabled);

        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            pointer_backend_input_for_view(&view, Point { x: 520.0, y: 376.0 }),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert!(runtime.external().intensity > 0.9);

        let view = admission_view(&runtime, frame.clone());
        let report = apply_test_backend_input(
            &mut runtime,
            wheel_backend_input_for_view(&view, Point { x: 24.0, y: 464.0 }, 1.0),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().selected_item, 1);

        let view = admission_view(&runtime, frame);
        let report = apply_test_backend_input(
            &mut runtime,
            pointer_backend_input_for_view(&view, Point { x: 24.0, y: 464.0 }),
        );
        assert!(report.handled, "{:?}", report.diagnostics);
        assert_eq!(runtime.external().selected_item, 2);
    }
}
