use eframe::egui;
use slipway_core::{
    BackendCapabilityReport, BackendInputEvent, BackendInputTrace, BackendParityAdmission,
    BackendVisibleCapability, BackendVisibleCapabilityRequirement, BaselineShift, Capability,
    CapabilityProfileKind, ChangeEvidence, ChildPlacement, CursorCapability,
    DeclaredEventDispatchKind, Diagnostic, EmittedMessageEvidence, EventOutcome, EvidenceSource,
    FocusRegionDeclaration, FocusTraversalMember, FontResolutionRequest, FontStyle, FontWeight,
    FrameIdentity, HitRegionDeclaration, HitRegionOrder, HitTestInput, InputEvent, KeyEventKind,
    KeyLocation, KeyboardDetails, KeyboardEvent, LayoutConstraints, LayoutInput, LayoutIntentProbe,
    LayoutOutput, Modifiers, PaintInputTransparency, PaintOp, PaintOrderMode, PaintUnit,
    PathCommand, PathDeclaration, Point, PointerButton, PointerButtons, PointerCaptureIntent,
    PointerDetails, PointerDeviceKind, PointerEventCoordinateSpace, PointerEventKind,
    PresentationGeometryIndex, PresentationRegionId, ProbeCollector, ProbeProduct,
    ProviderHitTestEvidence, ProviderSnapshotEvidence, ProviderSnapshotRequest,
    ProviderSurfaceKind, ProviderSurfaceRequest, Rect, RenderSurfaceDeclaration,
    ResourceSourceDeclaration, ResourceSourceKind, ScrollAxes, ScrollEvent,
    ScrollRegionDeclaration, ShapeDeclaration, ShapeKind, Size, SlipwayAuthoredWidget,
    SlipwayBackendCapabilityProbe, SlipwayBackendParityAdmission, SlipwayCanvasProvider,
    SlipwayEventDispositionPolicy, SlipwayEventRoutingPolicy, SlipwayFontResolutionPolicy,
    SlipwayGpuSurfaceProvider, SlipwayLayoutIntent, SlipwayLogic, SlipwayMediaProvider,
    SlipwayPlotProvider, SlipwayProviderHitTestPolicy, SlipwayProviderSnapshotPolicy,
    SlipwayRenderSurfaces, SlipwayScrollableContainerCapability, SlipwaySsot,
    SlipwayTextInputCapability, SlipwayUnsupportedCapabilityEvidence, SlipwayView,
    SlipwayViewDefinition, SlipwayWidget, SlipwayWidgetTypes, SourceValidityKind, StateObservation,
    StateProbe, TargetLocalRect, TextCompositionEvent, TextCompositionPhase, TextEditEvent,
    TextEditKind, TextEditRegionDeclaration, TextInputEvent, TextInputVisualStyleDeclaration,
    TextMeasurementEvidence, TextSelectionRange, TextStyle, TopologyNode, TopologyProbe,
    UnsupportedCapabilityEvidence, ViewDefinition, ViewDefinitionInput, WidgetId, WidgetSlot,
    WidgetSlotAddress, expand_paint_unit_layers, mount_widget_slot_address, paint_unit_sort_key,
    scroll_region_from_scrollable_capability, scroll_region_from_scrollable_capability_with_order,
    text_edit_focus_region_from_capability, validate_and_index_view,
    view_definition_contract_diagnostics_for_capabilities,
    view_definition_has_blocking_contract_diagnostic,
};
use slipway_debug_bridge::{
    CompositionPhaseProvenance, DebugCommand, DebugCommandKind, DebugCompositionCommitMutation,
    DebugCompositionIngressObservation, DebugCompositionPhaseTrace, DebugCompositionTrace,
    DebugControlMode, DebugControlTrace, DebugFailure, DebugPhysicalControl, DebugReplyProduct,
    PresentedScreenshotProduct, PresentedScreenshotRefusal, PresentedScreenshotSelector,
    VisibleFrameTimingRecorder,
};
use slipway_runtime::{
    DebugEguiCompositionIngressCustody, SlipwayRuntime, SlipwayRuntimeDrainBudget,
    SlipwayRuntimeMcpTransport, SlipwayRuntimePendingNativeMcpCall, SlipwayServiceMcpTransport,
};
use std::fs;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

mod native_runner;

pub const EGUI_BACKEND_ID: &str = "slipway-backend-egui";
const EGUI_PROVIDER_SURFACE_REQUIREMENT: &str = "egui.provider_surface.native_wrapper";
const EGUI_NATIVE_OS_INPUT_PASS: &str = "physical-input/native-os";
const EGUI_DEBUG_INPUT_PASS: &str = "physical-input/debug-injected";
const EGUI_DEBUG_COMPOSITION_PASS: &str = "physical-input/debug-injected/composition";
#[derive(Clone, Debug, Default)]
pub struct EguiBackendAdmission;

pub fn egui_backend_admission() -> EguiBackendAdmission {
    EguiBackendAdmission
}

/// Derives the ADR-0002 B1 dispatch graph through egui's presentation
/// pipeline: the same view assembly and scroll-region normalization the
/// visible widget path applies before dispatch
/// (`normalize_egui_visible_scroll_regions`), occlusion inputs composed
/// through the shared core collector (the declared analogue of egui's
/// per-widget paint-occlusion allocation, which resolves the wheel channel
/// through the same `paint_layer_blocks_wheel` filter), then the shared core
/// builder (`slipway_core::derive_dispatch_graph_with_geometry_index`)
/// derives nodes and edges. Read-only: nothing consumes the returned graph
/// for dispatch.
pub fn egui_dispatch_graph_for_widget<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    layout_input: LayoutInput,
    frame_seed: Option<&FrameIdentity>,
) -> slipway_core::DispatchGraph
where
    W: SlipwayEguiBackendChildWidget,
{
    let target = widget.id();
    let frame = frame_seed.cloned().unwrap_or_else(|| FrameIdentity {
        surface_id: "egui-visible".to_string(),
        surface_instance_id: target.as_str().to_string(),
        revision: 0,
        frame_index: 0,
        viewport: layout_input.viewport.into_rect(),
    });
    let mut view = widget.visible_backend_view_definition(
        external,
        local,
        ViewDefinitionInput::new(frame, layout_input),
    );
    let occlusions = slipway_core::dispatch_graph_occlusion_regions_for_composed_view(
        widget, external, local, &view,
    );
    let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
    normalize_egui_visible_scroll_regions(&mut view, &geometry_index);

    slipway_core::derive_dispatch_graph_with_geometry_index(
        &view.target,
        &geometry_index,
        &view.hit_regions,
        &view.focus_regions,
        &view.scroll_regions,
        &occlusions,
    )
}

impl EguiBackendAdmission {
    pub fn admit_view_definition(&self, view: &ViewDefinition) -> BackendParityAdmission {
        self.admit_view_definition_with_capabilities(&[], view)
    }

    pub fn admit_view_definition_with_capabilities(
        &self,
        capabilities: &[Capability],
        view: &ViewDefinition,
    ) -> BackendParityAdmission {
        let mut visible_requirements = Vec::new();
        let mut unsupported = Vec::new();
        let contract_diagnostics =
            view_definition_contract_diagnostics_for_capabilities(view, capabilities);

        push_view_requirement(
            &mut visible_requirements,
            "view.hit_regions",
            Some(view.target.clone()),
            BackendVisibleCapability::HitRegions,
        );
        push_view_requirement(
            &mut visible_requirements,
            "view.backend_presented_evidence",
            Some(view.target.clone()),
            BackendVisibleCapability::BackendPresentedEvidence,
        );

        if view_definition_has_blocking_contract_diagnostic(&contract_diagnostics) {
            push_view_requirement(
                &mut visible_requirements,
                "view.contract",
                Some(view.target.clone()),
                BackendVisibleCapability::Custom("view_contract".to_string()),
            );
            unsupported.push(UnsupportedCapabilityEvidence {
                backend_id: EGUI_BACKEND_ID.to_string(),
                target: Some(view.target.clone()),
                capability: Capability::CapabilityAdmission,
                visible_capability: Some(BackendVisibleCapability::Custom(
                    "view_contract".to_string(),
                )),
                requirement_id: Some("view.contract".to_string()),
                reason: "view definition contract diagnostics contain blocking errors".to_string(),
                source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "visible-admission"),
                diagnostics: contract_diagnostics.clone(),
            });
        }

        if view
            .hit_regions
            .iter()
            .any(|region| region.capture != PointerCaptureIntent::None)
        {
            push_view_requirement(
                &mut visible_requirements,
                "view.pointer_capture",
                Some(view.target.clone()),
                BackendVisibleCapability::PointerCapture,
            );
        }

        if !view.focus_regions.is_empty() {
            push_view_requirement(
                &mut visible_requirements,
                "view.focus_regions",
                Some(view.target.clone()),
                BackendVisibleCapability::FocusRegions,
            );
        }

        if view
            .focus_regions
            .iter()
            .any(|region| region.text_edit.is_some())
        {
            push_view_requirement(
                &mut visible_requirements,
                "view.text_edit_regions",
                Some(view.target.clone()),
                BackendVisibleCapability::TextEditRegions,
            );
        }

        if !view.scroll_regions.is_empty() {
            push_view_requirement(
                &mut visible_requirements,
                "view.scroll_regions",
                Some(view.target.clone()),
                BackendVisibleCapability::ScrollRegions,
            );
        }

        if view
            .paint
            .iter()
            .any(|op| paint_op_uses_shape_path_or_clip(op))
        {
            push_view_requirement(
                &mut visible_requirements,
                "view.shape_path_clip",
                Some(view.target.clone()),
                BackendVisibleCapability::ShapePathClip,
            );
        }

        let paint_diagnostics =
            unsupported_egui_visible_paint_diagnostics(&view.target, &view.paint);
        if !paint_diagnostics.is_empty() {
            push_view_requirement(
                &mut visible_requirements,
                "view.shape_path_clip",
                Some(view.target.clone()),
                BackendVisibleCapability::ShapePathClip,
            );
            unsupported.push(UnsupportedCapabilityEvidence {
                backend_id: EGUI_BACKEND_ID.to_string(),
                target: Some(view.target.clone()),
                capability: Capability::Paint,
                visible_capability: Some(BackendVisibleCapability::ShapePathClip),
                requirement_id: Some("view.shape_path_clip".to_string()),
                reason:
                    "egui backend refuses visible shape/path/clip declarations it cannot present"
                        .to_string(),
                source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "visible-admission"),
                diagnostics: paint_diagnostics,
            });
        }

        if view_requires_font_installation(view) {
            push_view_requirement(
                &mut visible_requirements,
                "view.font_resource_installation",
                Some(view.target.clone()),
                BackendVisibleCapability::FontInstallation,
            );
        }

        BackendParityAdmission {
            backend_id: EGUI_BACKEND_ID.to_string(),
            accepted: unsupported.is_empty(),
            required_profiles: Vec::new(),
            visible_requirements,
            unsupported,
            source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "visible-admission"),
            diagnostics: contract_diagnostics,
        }
    }
}

impl SlipwayBackendCapabilityProbe for EguiBackendAdmission {
    fn backend_capabilities(&self) -> BackendCapabilityReport {
        BackendCapabilityReport {
            backend_id: EGUI_BACKEND_ID.to_string(),
            capabilities: vec![
                Capability::PointerInput,
                Capability::KeyboardInput,
                Capability::TextInput,
                Capability::WheelInput,
                Capability::FocusInput,
                Capability::HitRegionPresentation,
                Capability::FocusRegionPresentation,
                Capability::TextEditRegionPresentation,
                Capability::ScrollRegionPresentation,
                Capability::ShapePathClipPresentation,
                Capability::FontResourceInstallation,
                Capability::BackendPresentedEvidence,
                Capability::CapabilityAdmission,
                Capability::BackendCapabilityNegotiation,
                Capability::RenderSurface,
                Capability::ProviderSurfacePolicy,
                Capability::Layout,
                Capability::Paint,
            ],
            profiles: vec![
                CapabilityProfileKind::TextInput,
                CapabilityProfileKind::ScrollableContainer,
                CapabilityProfileKind::ProviderSurface,
                CapabilityProfileKind::BackendAdapter,
            ],
            visible_capabilities: vec![
                BackendVisibleCapability::HitRegions,
                BackendVisibleCapability::Cursor,
                BackendVisibleCapability::PointerCapture,
                BackendVisibleCapability::FocusRegions,
                BackendVisibleCapability::TextEditRegions,
                BackendVisibleCapability::ScrollRegions,
                BackendVisibleCapability::ShapePathClip,
                BackendVisibleCapability::FontInstallation,
                BackendVisibleCapability::BackendPresentedEvidence,
                egui_provider_surface_visible_capability(),
            ],
        }
    }
}

impl SlipwayUnsupportedCapabilityEvidence for EguiBackendAdmission {
    fn unsupported_capabilities(
        &self,
        required: &[Capability],
    ) -> Vec<UnsupportedCapabilityEvidence> {
        let report = self.backend_capabilities();
        required
            .iter()
            .filter(|capability| !report.capabilities.iter().any(|owned| owned == *capability))
            .map(|capability| UnsupportedCapabilityEvidence {
                backend_id: EGUI_BACKEND_ID.to_string(),
                target: None,
                capability: capability.clone(),
                visible_capability: None,
                requirement_id: Some(format!("capability::{capability:?}")),
                reason: "capability is not declared by the egui visible backend".to_string(),
                source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "capability-admission"),
                diagnostics: Vec::new(),
            })
            .collect()
    }
}

impl SlipwayBackendParityAdmission for EguiBackendAdmission {
    fn backend_parity_admission(
        &self,
        required_profiles: &[CapabilityProfileKind],
    ) -> BackendParityAdmission {
        backend_profile_admission(
            EGUI_BACKEND_ID,
            &self.backend_capabilities(),
            required_profiles,
        )
    }
}

pub trait SlipwayEguiWidgetListVisitor<ExternalState, AppMessage> {
    fn visit_egui_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayEguiBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>;

    fn visit_egui_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayEguiNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>;
}

struct EguiMountedVisitor<'a, V> {
    mounted_parent: Option<&'a WidgetSlotAddress>,
    visitor: &'a mut V,
}

impl<V> EguiMountedVisitor<'_, V> {
    fn mount(&self, slot: WidgetSlotAddress) -> WidgetSlotAddress {
        match self.mounted_parent {
            Some(parent) => mount_widget_slot_address(slot, parent),
            None => slot,
        }
    }
}

impl<ExternalState, AppMessage, V> SlipwayEguiWidgetListVisitor<ExternalState, AppMessage>
    for EguiMountedVisitor<'_, V>
where
    V: SlipwayEguiWidgetListVisitor<ExternalState, AppMessage>,
{
    fn visit_egui_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayEguiBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let slot = self.mount(slot);
        self.visitor.visit_egui_child(widget, external, local, slot);
    }

    fn visit_egui_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayEguiNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let slot = self.mount(slot);
        self.visitor
            .visit_egui_native_child(widget, external, local, slot);
    }
}

#[derive(Clone, Debug)]
pub struct EguiNativeWidgetContext<'a> {
    pub slot: &'a WidgetSlotAddress,
    pub frame: &'a FrameIdentity,
    pub placement: ChildPlacement,
    pub rect: egui::Rect,
}

#[derive(Clone, Debug, Default)]
pub struct EguiNativeWidgetOutput {
    pub input_events: Vec<BackendInputEvent>,
    pub state: Vec<StateObservation>,
    pub diagnostics: Vec<Diagnostic>,
    pub request_repaint: bool,
}

/// Backend-specific escape hatch for an already-owned egui UI fragment.
///
/// This is not a backend-neutral parity guarantee. Implementors must still
/// expose Slipway layout/view/debug evidence through the surrounding
/// `SlipwayViewDefinition` contract, and any behavior that cannot be expressed
/// there should be reported as an unsupported backend-specific gap.
pub trait SlipwayEguiNativeChildWidget: SlipwayWidget + SlipwayViewDefinition {
    fn egui_native_ui(
        &self,
        ui: &mut egui::Ui,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        context: EguiNativeWidgetContext<'_>,
    ) -> EguiNativeWidgetOutput;
}

/// Contract for a backend-specific egui native wrapper.
///
/// This wrapper may mount native egui UI, but Slipway will not infer its
/// internal drawing, hit testing, or cross-backend parity automatically.
pub trait SlipwayEguiNativeWidgetSpec: SlipwayWidgetTypes {
    fn id(&self) -> WidgetId;

    fn capabilities(&self) -> Vec<Capability> {
        Vec::new()
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        Vec::new()
    }

    fn initial_local_state(&self) -> Self::LocalState;

    fn handle_event(
        &self,
        _external: &Self::ExternalState,
        _local: &mut Self::LocalState,
        _event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        EventOutcome::ignored()
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: slipway_core::LayoutOutputBuilder,
    ) -> LayoutOutput {
        output.finish(input.viewport)
    }

    fn observe_state(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        Vec::new()
    }

    fn egui_native_ui(
        &self,
        ui: &mut egui::Ui,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        context: EguiNativeWidgetContext<'_>,
    ) -> EguiNativeWidgetOutput;
}

pub trait SlipwayEguiProviderSurfaceSpec: SlipwayEguiNativeWidgetSpec {
    fn provider_surface_request(&self) -> ProviderSurfaceRequest;

    fn provider_surface_capabilities(&self) -> Vec<String> {
        match self.provider_surface_request().kind {
            ProviderSurfaceKind::Canvas => vec!["canvas".to_string()],
            ProviderSurfaceKind::Gpu => vec!["gpu".to_string()],
            ProviderSurfaceKind::Media => vec!["media".to_string()],
            ProviderSurfaceKind::Plot => vec!["plot".to_string()],
            ProviderSurfaceKind::Map => vec!["map".to_string()],
            ProviderSurfaceKind::Terminal => vec!["terminal".to_string()],
            ProviderSurfaceKind::RasterImage => vec!["raster-image".to_string()],
            ProviderSurfaceKind::Custom(name) => vec![name],
        }
    }

    fn render_surface_declaration(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> RenderSurfaceDeclaration {
        let request = self.provider_surface_request();
        RenderSurfaceDeclaration {
            id: request.target,
            provider_id: request.provider_id,
            bounds: request.bounds,
            payload_ref: request.payload_ref,
            dirty_regions: request.dirty_regions,
            capabilities: self.provider_surface_capabilities(),
        }
    }

    fn provider_hit_test(&self, request: HitTestInput) -> ProviderHitTestEvidence {
        let surface = self.provider_surface_request();
        let diagnostic = Diagnostic::unsupported(
            Some(request.target.clone()),
            "egui.provider_surface.hit_test_unsupported",
            "egui provider surface did not implement backend-specific provider_hit_test",
        );
        ProviderHitTestEvidence {
            target: request.target,
            provider_id: surface.provider_id,
            point: request.point,
            hit: None,
            diagnostics: vec![diagnostic],
        }
    }

    fn provider_snapshot(&mut self, request: ProviderSnapshotRequest) -> ProviderSnapshotEvidence {
        let diagnostic = Diagnostic::unsupported(
            Some(request.target.clone()),
            "egui.provider_surface.snapshot_unsupported",
            "egui provider surface did not implement backend-specific provider_snapshot",
        );
        ProviderSnapshotEvidence {
            target: request.target,
            provider_id: request.provider_id,
            snapshot_ref: None,
            frame: request.frame,
            diagnostics: vec![diagnostic],
        }
    }
}

pub struct EguiGpuSurfacePrepareContext<'a> {
    pub target: &'a WidgetId,
    pub provider_id: &'a str,
    pub frame: Option<&'a FrameIdentity>,
    pub bounds: Rect,
    pub viewport: Rect,
    pub device: &'a egui_wgpu::wgpu::Device,
    pub queue: &'a egui_wgpu::wgpu::Queue,
    pub format: egui_wgpu::wgpu::TextureFormat,
}

pub struct EguiGpuSurfacePaintContext<'a, 'pass, Prepared> {
    pub target: &'a WidgetId,
    pub provider_id: &'a str,
    pub frame: Option<&'a FrameIdentity>,
    pub bounds: Rect,
    pub viewport: Rect,
    pub prepared: &'a Prepared,
    pub render_pass: &'a mut egui_wgpu::wgpu::RenderPass<'pass>,
}

pub trait SlipwayEguiSplitGpuProviderSurfaceSpec: SlipwayEguiProviderSurfaceSpec {
    type PreparedFrame;

    fn prepare_egui_gpu_surface(
        &mut self,
        context: EguiGpuSurfacePrepareContext<'_>,
    ) -> Result<Self::PreparedFrame, Vec<Diagnostic>>;

    fn paint_prepared_egui_gpu_surface(
        &self,
        context: EguiGpuSurfacePaintContext<'_, '_, Self::PreparedFrame>,
    ) -> Result<(), Vec<Diagnostic>>;
}

pub trait SlipwayEguiSplitGpuSurfaceProvider: SlipwayGpuSurfaceProvider {
    type PreparedFrame;

    fn prepare_egui_gpu_surface(
        &mut self,
        context: EguiGpuSurfacePrepareContext<'_>,
    ) -> Result<Self::PreparedFrame, Vec<Diagnostic>>;

    fn paint_prepared_egui_gpu_surface(
        &self,
        context: EguiGpuSurfacePaintContext<'_, '_, Self::PreparedFrame>,
    ) -> Result<(), Vec<Diagnostic>>;
}

#[derive(Clone, Debug)]
pub struct SlipwayEguiNativeWidget<N> {
    native: N,
}

impl<N> SlipwayEguiNativeWidget<N> {
    pub fn new(native: N) -> Self {
        Self { native }
    }

    pub fn native(&self) -> &N {
        &self.native
    }

    pub fn native_mut(&mut self) -> &mut N {
        &mut self.native
    }

    pub fn into_inner(self) -> N {
        self.native
    }
}

impl<N> SlipwayWidgetTypes for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiNativeWidgetSpec,
{
    type ExternalState = N::ExternalState;
    type LocalState = N::LocalState;
    type AppMessage = N::AppMessage;
}

impl<N> SlipwaySsot for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiNativeWidgetSpec,
{
    fn id(&self) -> WidgetId {
        self.native.id()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.native.capabilities()
    }

    fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
        TopologyNode {
            id: self.id(),
            children: Vec::new(),
            local_state_slot: None,
        }
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        self.native.unsupported()
    }
}

impl<N> SlipwayLogic for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiNativeWidgetSpec,
{
    fn handle_event(
        &self,
        external: &Self::ExternalState,
        local: &mut Self::LocalState,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        self.native.handle_event(external, local, event)
    }
}

impl<N> SlipwayView for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiNativeWidgetSpec,
{
    fn initial_local_state(&self) -> Self::LocalState {
        self.native.initial_local_state()
    }

    fn layout(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: LayoutInput,
        output: slipway_core::LayoutOutputBuilder,
    ) -> LayoutOutput {
        self.native.layout(external, local, input, output)
    }

    fn paint(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        Vec::new()
    }

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        self.native.observe_state(external, local)
    }
}

impl<N> SlipwayViewDefinition for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiNativeWidgetSpec,
{
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let (frame, layout) = slipway_core::layout_view_definition(self, external, local, input);
        ViewDefinition {
            target: self.id(),
            frame,
            layout,
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: self.unsupported(),
        }
    }
}

impl<N> SlipwayEguiNativeChildWidget for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiNativeWidgetSpec,
{
    fn egui_native_ui(
        &self,
        ui: &mut egui::Ui,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        context: EguiNativeWidgetContext<'_>,
    ) -> EguiNativeWidgetOutput {
        self.native.egui_native_ui(ui, external, local, context)
    }
}

impl<N> SlipwayRenderSurfaces for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiProviderSurfaceSpec,
{
    fn render_surfaces(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<RenderSurfaceDeclaration> {
        vec![self.native.render_surface_declaration(external, local)]
    }
}

impl<N> SlipwayCanvasProvider for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiProviderSurfaceSpec,
{
    fn canvas_surfaces(&self) -> Vec<ProviderSurfaceRequest> {
        let request = self.native.provider_surface_request();
        if request.kind == ProviderSurfaceKind::Canvas {
            vec![request]
        } else {
            Vec::new()
        }
    }
}

impl<N> SlipwayGpuSurfaceProvider for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiProviderSurfaceSpec,
{
    fn gpu_surfaces(&self) -> Vec<ProviderSurfaceRequest> {
        let request = self.native.provider_surface_request();
        if request.kind == ProviderSurfaceKind::Gpu {
            vec![request]
        } else {
            Vec::new()
        }
    }
}

impl<N> SlipwayEguiSplitGpuSurfaceProvider for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiSplitGpuProviderSurfaceSpec,
{
    type PreparedFrame = N::PreparedFrame;

    fn prepare_egui_gpu_surface(
        &mut self,
        context: EguiGpuSurfacePrepareContext<'_>,
    ) -> Result<Self::PreparedFrame, Vec<Diagnostic>> {
        self.native.prepare_egui_gpu_surface(context)
    }

    fn paint_prepared_egui_gpu_surface(
        &self,
        context: EguiGpuSurfacePaintContext<'_, '_, Self::PreparedFrame>,
    ) -> Result<(), Vec<Diagnostic>> {
        self.native.paint_prepared_egui_gpu_surface(context)
    }
}

impl<N> SlipwayMediaProvider for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiProviderSurfaceSpec,
{
    fn media_surfaces(&self) -> Vec<ProviderSurfaceRequest> {
        let request = self.native.provider_surface_request();
        if request.kind == ProviderSurfaceKind::Media {
            vec![request]
        } else {
            Vec::new()
        }
    }
}

impl<N> SlipwayPlotProvider for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiProviderSurfaceSpec,
{
    fn plot_surfaces(&self) -> Vec<ProviderSurfaceRequest> {
        let request = self.native.provider_surface_request();
        if request.kind == ProviderSurfaceKind::Plot {
            vec![request]
        } else {
            Vec::new()
        }
    }
}

impl<N> SlipwayProviderHitTestPolicy for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiProviderSurfaceSpec,
{
    fn provider_hit_test(&self, request: HitTestInput) -> ProviderHitTestEvidence {
        self.native.provider_hit_test(request)
    }
}

impl<N> SlipwayProviderSnapshotPolicy for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiProviderSurfaceSpec,
{
    fn provider_snapshot(&mut self, request: ProviderSnapshotRequest) -> ProviderSnapshotEvidence {
        self.native.provider_snapshot(request)
    }
}

pub trait SlipwayEguiWidgetListEntry: SlipwayWidget + SlipwayViewDefinition {
    fn visit_egui_entry<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        slot: WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>;
}

impl<W> SlipwayEguiWidgetListEntry for W
where
    W: SlipwayEguiBackendChildWidget,
{
    fn visit_egui_entry<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        slot: WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        visitor.visit_egui_child(self, external, local, slot);
    }
}

impl<N> SlipwayEguiWidgetListEntry for SlipwayEguiNativeWidget<N>
where
    N: SlipwayEguiNativeWidgetSpec,
{
    fn visit_egui_entry<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        slot: WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        visitor.visit_egui_native_child(self, external, local, slot);
    }
}

pub trait SlipwayEguiWidgetList {
    type ExternalState;
    type LocalState;
    type AppMessage;

    fn visit_egui_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>;

    fn visit_egui_children_in_paint_order<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        parent_view: &ViewDefinition,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>;
}

impl SlipwayEguiWidgetList for () {
    type ExternalState = ();
    type LocalState = ();
    type AppMessage = ();

    fn visit_egui_children<V>(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
    }

    fn visit_egui_children_in_paint_order<V>(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _parent_view: &ViewDefinition,
        _visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
    }
}

macro_rules! impl_egui_widget_list_tuple {
    ($first:ident $first_index:tt $(, $widget:ident $index:tt)*) => {
        impl<$first $(, $widget)*> SlipwayEguiWidgetList for ($first, $($widget,)*)
        where
            $first: SlipwayEguiWidgetListEntry,
            $(
                $widget: SlipwayEguiWidgetListEntry<
                    ExternalState = <$first as SlipwayWidgetTypes>::ExternalState,
                    AppMessage = <$first as SlipwayWidgetTypes>::AppMessage,
                >,
            )*
        {
            type ExternalState = <$first as SlipwayWidgetTypes>::ExternalState;
            type LocalState = (
                <$first as SlipwayWidgetTypes>::LocalState,
                $(<$widget as SlipwayWidgetTypes>::LocalState,)*
            );
            type AppMessage = <$first as SlipwayWidgetTypes>::AppMessage;

            fn visit_egui_children<V>(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                visitor: &mut V,
            ) where
                V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
            {
                self.$first_index.visit_egui_entry(
                    external,
                    &local.$first_index,
                    parent_slot.child(self.$first_index.id(), $first_index),
                    visitor,
                );
                $(
                    self.$index.visit_egui_entry(
                        external,
                        &local.$index,
                        parent_slot.child(self.$index.id(), $index),
                        visitor,
                    );
                )*
            }

            fn visit_egui_children_in_paint_order<V>(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                parent_view: &ViewDefinition,
                visitor: &mut V,
            ) where
                V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
            {
                let mut order = vec![(
                    egui_child_paint_sort_key(
                        &self.$first_index,
                        external,
                        &local.$first_index,
                        parent_slot,
                        $first_index,
                        parent_view,
                    ),
                    $first_index,
                )];
                $(
                    order.push((
                        egui_child_paint_sort_key(
                            &self.$index,
                            external,
                            &local.$index,
                            parent_slot,
                            $index,
                            parent_view,
                        ),
                        $index,
                    ));
                )*
                order.sort_by_key(|(key, _)| *key);

                for (_, index) in order {
                    match index {
                        $first_index => self.$first_index.visit_egui_entry(
                            external,
                            &local.$first_index,
                            parent_slot.child(self.$first_index.id(), $first_index),
                            visitor,
                        ),
                        $(
                            $index => self.$index.visit_egui_entry(
                                external,
                                &local.$index,
                                parent_slot.child(self.$index.id(), $index),
                                visitor,
                            ),
                        )*
                        _ => {}
                    }
                }
            }
        }
    };
}

impl_egui_widget_list_tuple!(A 0);
impl_egui_widget_list_tuple!(A 0, B 1);
impl_egui_widget_list_tuple!(A 0, B 1, C 2);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13, O 14);
impl_egui_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13, O 14, P 15);

pub trait SlipwayEguiAuthoredChildren: SlipwayAuthoredWidget {
    fn visit_egui_authored_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>;

    fn visit_egui_authored_children_in_paint_order<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        _parent_view: &ViewDefinition,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        self.visit_egui_authored_children(external, local, visitor);
    }
}

pub trait SlipwayEguiBackendContract:
    SlipwayWidget
    + SlipwayViewDefinition
    + SlipwayEventRoutingPolicy
    + SlipwayEventDispositionPolicy
    + SlipwayFontResolutionPolicy
    + SlipwayEguiAuthoredChildren
{
}

impl<W> SlipwayEguiBackendContract for W where
    W: SlipwayWidget
        + SlipwayViewDefinition
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy
        + SlipwayFontResolutionPolicy
        + SlipwayEguiAuthoredChildren
{
}

fn visit_egui_authored_children_mounted<W, V>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    mounted_parent: Option<&WidgetSlotAddress>,
    visitor: &mut V,
) where
    W: SlipwayEguiBackendChildWidget,
    V: SlipwayEguiWidgetListVisitor<W::ExternalState, W::AppMessage>,
{
    let mut mounted = EguiMountedVisitor {
        mounted_parent,
        visitor,
    };
    widget.visit_egui_authored_children(external, local, &mut mounted);
}

fn visit_egui_authored_children_in_paint_order_mounted<W, V>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    parent_view: &ViewDefinition,
    mounted_parent: Option<&WidgetSlotAddress>,
    visitor: &mut V,
) where
    W: SlipwayEguiBackendChildWidget,
    V: SlipwayEguiWidgetListVisitor<W::ExternalState, W::AppMessage>,
{
    let mut mounted = EguiMountedVisitor {
        mounted_parent,
        visitor,
    };
    widget.visit_egui_authored_children_in_paint_order(external, local, parent_view, &mut mounted);
}

impl<A> SlipwayEguiAuthoredChildren for slipway_core::SlipwayAppWidget<A>
where
    A: slipway_core::SlipwayApp,
    A::Widgets: SlipwayEguiWidgetList<
            ExternalState = A::ExternalState,
            LocalState = <A::Widgets as slipway_core::SlipwayWidgetList>::LocalState,
            AppMessage = A::AppMessage,
        >,
{
    fn visit_egui_authored_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        visitor: &mut V,
    ) where
        V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        self.app
            .widgets()
            .visit_egui_children(external, &local.widgets, &root_slot, visitor);
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
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        self.app.widgets().visit_egui_children_in_paint_order(
            external,
            &local.widgets,
            &root_slot,
            parent_view,
            visitor,
        );
    }
}

pub trait SlipwayEguiBackendChildWidget: SlipwayEguiBackendContract {}

impl<W> SlipwayEguiBackendChildWidget for W where W: SlipwayEguiBackendContract {}

pub trait SlipwayEguiTextInputBackendWidget:
    SlipwayEguiBackendChildWidget + SlipwayTextInputCapability
{
}

impl<W> SlipwayEguiTextInputBackendWidget for W where
    W: SlipwayEguiBackendChildWidget + SlipwayTextInputCapability
{
}

pub trait SlipwayEguiScrollableContainerBackendWidget:
    SlipwayEguiBackendChildWidget + SlipwayScrollableContainerCapability
{
}

impl<W> SlipwayEguiScrollableContainerBackendWidget for W where
    W: SlipwayEguiBackendChildWidget + SlipwayScrollableContainerCapability
{
}

#[allow(clippy::too_many_arguments)]
pub fn egui_text_edit_focus_region_from_capability<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    id: PresentationRegionId,
    address: Option<WidgetSlotAddress>,
    bounds: TargetLocalRect,
    member: Option<FocusTraversalMember>,
    enabled: bool,
    layout_input: &LayoutInput,
    measurement: Option<&TextMeasurementEvidence>,
) -> FocusRegionDeclaration
where
    W: SlipwayEguiTextInputBackendWidget,
{
    text_edit_focus_region_from_capability(
        widget,
        external,
        local,
        id,
        address,
        bounds,
        member,
        enabled,
        layout_input,
        measurement,
    )
}

pub fn egui_scroll_region_from_capability<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    layout: &LayoutOutput,
    region_id: Option<PresentationRegionId>,
    address: Option<WidgetSlotAddress>,
    enabled: bool,
) -> ScrollRegionDeclaration
where
    W: SlipwayEguiScrollableContainerBackendWidget,
{
    scroll_region_from_scrollable_capability(
        widget, external, local, layout, region_id, address, enabled,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn egui_scroll_region_from_capability_with_order<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    layout: &LayoutOutput,
    region_id: Option<PresentationRegionId>,
    address: Option<WidgetSlotAddress>,
    enabled: bool,
    order: HitRegionOrder,
) -> ScrollRegionDeclaration
where
    W: SlipwayEguiScrollableContainerBackendWidget,
{
    scroll_region_from_scrollable_capability_with_order(
        widget, external, local, layout, region_id, address, enabled, order,
    )
}

pub trait SlipwayEguiBackendWidget: SlipwayAuthoredWidget + SlipwayEguiBackendChildWidget {}

impl<W> SlipwayEguiBackendWidget for W where W: SlipwayAuthoredWidget + SlipwayEguiBackendChildWidget
{}

pub trait SlipwayEguiLayoutIntentBackendWidget:
    SlipwayEguiBackendWidget + SlipwayLayoutIntent
{
}

impl<W> SlipwayEguiLayoutIntentBackendWidget for W where
    W: SlipwayEguiBackendWidget + SlipwayLayoutIntent
{
}

pub trait SlipwayEguiLayoutIntentBackendChildWidget:
    SlipwayEguiBackendChildWidget + SlipwayLayoutIntent
{
}

impl<W> SlipwayEguiLayoutIntentBackendChildWidget for W where
    W: SlipwayEguiBackendChildWidget + SlipwayLayoutIntent
{
}

/// Egui-facing context used to create a backend-neutral Slipway layout input.
pub struct EguiLayoutContext<'a> {
    pub ui: &'a egui::Ui,
    pub available_size: egui::Vec2,
    pub pixels_per_point: f32,
}

/// Egui-facing context used to translate host interaction into Slipway input events.
pub struct EguiInputContext<'a> {
    pub ui: &'a egui::Ui,
    pub widget_id: WidgetId,
    pub frame: &'a FrameIdentity,
    pub rect: egui::Rect,
    pub layout: &'a LayoutOutput,
    pub geometry_index: &'a PresentationGeometryIndex,
    pub hit_regions: &'a [HitRegionDeclaration],
    pub focus_regions: &'a [FocusRegionDeclaration],
    pub scroll_regions: &'a [ScrollRegionDeclaration],
    pub wheel_traversal_boundary: slipway_core::DeclaredWheelTraversalBoundary,
    pub response: &'a egui::Response,
    pub regions: &'a [EguiPresentedRegion],
    pub native_physical_operation: Option<&'a DebugPhysicalControl>,
}

#[derive(Clone, Debug)]
struct EguiRawInputSnapshot {
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
    hover_pos: Option<egui::Pos2>,
    pointer_any_down: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EguiDebugInputSpan {
    pub token: u64,
    pub start_event_index: usize,
    pub end_event_index: usize,
}

fn egui_raw_input_snapshot(ui: &egui::Ui) -> EguiRawInputSnapshot {
    ui.input(|input| EguiRawInputSnapshot {
        events: input.events.clone(),
        modifiers: input.modifiers,
        hover_pos: input.pointer.hover_pos(),
        pointer_any_down: input.pointer.any_down(),
    })
}

fn egui_raw_event_source(
    span: Option<EguiDebugInputSpan>,
    composition: bool,
    event_index: usize,
    event: &egui::Event,
) -> EvidenceSource {
    let in_debug_span = span.is_some_and(|span| {
        span.start_event_index <= event_index && event_index < span.end_event_index
    });
    let pass = if !in_debug_span {
        EGUI_NATIVE_OS_INPUT_PASS
    } else if composition && matches!(event, egui::Event::Ime(_)) {
        EGUI_DEBUG_COMPOSITION_PASS
    } else {
        EGUI_DEBUG_INPUT_PASS
    };
    EvidenceSource::backend_presented(EGUI_BACKEND_ID, pass)
}

fn egui_accepted_composition_commit_key(
    events: &[egui::Event],
    span: Option<EguiDebugInputSpan>,
    composition: bool,
    operation: Option<&DebugPhysicalControl>,
) -> Option<(EguiAcceptedCommitKey, usize)> {
    if !composition {
        return None;
    }
    let span = span?;
    let DebugPhysicalControl::TextComposition {
        updates, commit, ..
    } = operation?
    else {
        return None;
    };
    let event_count = updates.len().checked_add(1)?;
    if span.token == 0
        || span.end_event_index.checked_sub(span.start_event_index) != Some(event_count)
        || span.end_event_index > events.len()
    {
        return None;
    }
    if events.iter().enumerate().any(|(index, event)| {
        (index < span.start_event_index || index >= span.end_event_index)
            && egui_event_may_mutate_focused_text(event)
    }) {
        return None;
    }
    for (ordinal, event) in events[span.start_event_index..span.end_event_index]
        .iter()
        .enumerate()
    {
        let matches_request = if let Some(update) = updates.get(ordinal) {
            matches!(
                event,
                egui::Event::Ime(egui::ImeEvent::Preedit {
                    text,
                    active_range_chars,
                }) if text == &update.preedit_text
                    && active_range_chars.as_ref().map(|range| TextSelectionRange {
                        anchor: range.start,
                        focus: range.end,
                    }) == update.cursor_range
            )
        } else {
            matches!(event, egui::Event::Ime(egui::ImeEvent::Commit(text)) if text == commit)
        };
        if !matches_request {
            return None;
        }
    }
    let event_index = span.end_event_index.checked_sub(1)?;
    Some((
        EguiAcceptedCommitKey {
            token: span.token,
            start_event_index: span.start_event_index,
            end_event_index: span.end_event_index,
            event_index,
        },
        event_count,
    ))
}

fn egui_event_may_mutate_focused_text(event: &egui::Event) -> bool {
    match event {
        egui::Event::Key { pressed, .. } => *pressed,
        egui::Event::Text(_) | egui::Event::Paste(_) | egui::Event::Cut => true,
        egui::Event::Ime(egui::ImeEvent::Commit(_)) => true,
        egui::Event::Copy
        | egui::Event::PointerMoved(_)
        | egui::Event::MouseMoved(_)
        | egui::Event::PointerButton { .. }
        | egui::Event::PointerGone
        | egui::Event::Zoom(_)
        | egui::Event::Rotate(_)
        | egui::Event::MouseWheel { .. }
        | egui::Event::WindowFocused(_)
        | egui::Event::AccessKitActionRequest(_)
        | egui::Event::Screenshot { .. }
        | egui::Event::Ime(_)
        | egui::Event::Touch { .. } => false,
    }
}

fn push_egui_backend_input_with_source(
    events: &mut Vec<BackendInputEvent>,
    mut event: BackendInputEvent,
    source: EvidenceSource,
) {
    if let Some(evidence) = event.dispatch_evidence.as_mut() {
        evidence.source = source;
    }
    events.push(event);
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum EguiPresentedRegionKind {
    Hit,
    Focus,
    TextEdit,
    Scroll,
    Occlusion,
}

#[derive(Clone, Debug)]
pub struct EguiTextEditChange {
    pub before: String,
    pub after: String,
    pub selection_before: Option<TextSelectionRange>,
    pub selection_after: Option<TextSelectionRange>,
}

#[derive(Clone, Debug)]
pub struct EguiScrollRegionState {
    pub declared_offset: Point,
    pub egui_offset: Point,
    pub content_size: Size,
    pub inner_rect: Rect,
    /// ABSOLUTE track/thumb rects of the declared indicator painted for
    /// this region this frame (Step 212, interactive indicators): `None`
    /// when no indicator is painted (`Hidden` mode or no vertical
    /// overflow). Computed by `declared_scroll_indicator_geometry` — the
    /// same source the painter uses — at allocation time, so the input
    /// path's hit surface always matches the pixels.
    pub indicator: Option<(egui::Rect, egui::Rect)>,
}

#[derive(Clone, Debug)]
pub struct EguiPresentedRegion {
    pub kind: EguiPresentedRegionKind,
    pub region_id: PresentationRegionId,
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    pub paint_sort_key: (i32, usize, usize),
    /// AUTHORED within-z order of the occluding layer's paint unit
    /// (`slipway_core::paint_unit_authored_z_order`), only meaningful for
    /// `EguiPresentedRegionKind::Occlusion` regions; `None` when the sort
    /// key's tie-break fields were defaulted from the unit traversal (the
    /// mounted slot ordinal) and for every non-occlusion region. Same-owner
    /// occlusion comparisons may only consult this authored component
    /// (NC-2) — see `egui_occlusion_blocks_region`.
    pub authored_z_order: Option<usize>,
    pub event_target: WidgetId,
    pub event_target_slot: Option<WidgetSlotAddress>,
    pub declared_bounds: Rect,
    pub target_origin: egui::Pos2,
    pub target_bounds: Rect,
    pub event_coordinate_space: PointerEventCoordinateSpace,
    pub response: egui::Response,
    pub cursor: CursorCapability,
    pub enabled: bool,
    pub text_edit_change: Option<EguiTextEditChange>,
    pub scroll_state: Option<EguiScrollRegionState>,
    /// Resolved wheel-channel opacity, only meaningful for
    /// `EguiPresentedRegionKind::Occlusion` regions. `true` (the default for an
    /// opaque layer) means the occluder blocks wheel input like pointer input;
    /// `false` means the layer was authored wheel-pass-through, so it still
    /// occludes the pointer but lets the wheel reach the scroll region behind
    /// it. Derived via `slipway_core::paint_layer_blocks_wheel`. Non-occlusion
    /// regions set `true` and never consult this field.
    pub blocks_wheel: bool,
}

#[derive(Clone, Debug)]
pub struct EguiCompositionPreflight {
    target: WidgetId,
    target_slot: Option<WidgetSlotAddress>,
    selected_region: PresentationRegionId,
    focused: bool,
    editable: bool,
}

#[derive(Clone, Debug)]
pub struct EguiNativeTextMutationEvidence {
    target: WidgetId,
    target_slot: Option<WidgetSlotAddress>,
    selected_region: PresentationRegionId,
    before: String,
    after: String,
    commit: EguiAcceptedCommitKey,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[doc(hidden)]
pub struct EguiAcceptedCompositionEvent {
    token: u64,
    start_event_index: usize,
    end_event_index: usize,
    event_index: usize,
    phase: TextCompositionPhase,
    dispatch_ordinal: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct EguiAcceptedCommitKey {
    token: u64,
    start_event_index: usize,
    end_event_index: usize,
    event_index: usize,
}

/// Egui-facing context used to translate Slipway paint declarations into egui paint calls.
pub struct EguiPaintContext<'a> {
    pub ui: &'a egui::Ui,
    pub painter: egui::Painter,
    pub rect: egui::Rect,
    pub layout: &'a LayoutOutput,
}

/// Egui-facing context for explicit state/topology observation.
pub struct EguiObservationContext {
    pub widget_id: WidgetId,
    pub capabilities: Vec<Capability>,
    pub topology: TopologyNode,
    pub unsupported: Vec<Diagnostic>,
    pub state: Vec<StateObservation>,
    pub layout_intent: Option<LayoutIntentProbe>,
}

/// Backend-owned conversion boundary between egui callbacks and Slipway core I/O.
///
/// This trait is deliberately mechanical: it converts host framework data to
/// backend-neutral core declarations and renders returned declarations. It does
/// not define widget semantics, local state transitions, or app messages.
pub trait EguiSlipwayBridge<W: SlipwayAuthoredWidget> {
    fn layout_input(&mut self, context: EguiLayoutContext<'_>) -> LayoutInput;

    fn desired_size(&mut self, layout: &LayoutOutput) -> egui::Vec2;

    fn input_events(&mut self, context: EguiInputContext<'_>) -> Vec<BackendInputEvent>;

    fn paint(&mut self, context: EguiPaintContext<'_>, ops: &[PaintOp]);

    fn messages(&mut self, outcome: EventOutcome<W::AppMessage>) -> Vec<W::AppMessage>;

    fn set_slipway_debug_input_span(
        &mut self,
        _span: Option<EguiDebugInputSpan>,
        _composition: bool,
    ) {
    }

    fn take_slipway_composition_preflight(&mut self) -> Option<EguiCompositionPreflight> {
        None
    }

    fn take_slipway_native_text_mutation(&mut self) -> Option<EguiNativeTextMutationEvidence> {
        None
    }

    fn drain_slipway_accepted_composition_events(&mut self) -> Vec<EguiAcceptedCompositionEvent> {
        Vec::new()
    }

    fn visible_admission_refused(&mut self, _admission: BackendParityAdmission) {}

    /// Drains the refused admissions this bridge retained so the caller can
    /// feed them into the runtime diagnostics surface. Bridges that do not
    /// retain refused admissions return an empty list.
    fn drain_refused_admissions(&mut self) -> Vec<BackendParityAdmission> {
        Vec::new()
    }

    /// Drains the no-consumer dispatch refusal evidence this bridge retained
    /// (audit finding MF-H3) so the caller can feed it into the runtime's
    /// bounded refusal ring. Bridges that do not retain refusals return an
    /// empty list.
    fn drain_dispatch_refusals(&mut self) -> Vec<slipway_core::DeclaredEventDispatchEvidence> {
        Vec::new()
    }

    fn take_terminal_wheel_consumed(&mut self) -> bool {
        false
    }

    fn wants_observation(&mut self) -> bool {
        false
    }

    fn observe(&mut self, _context: EguiObservationContext) {}
}

/// Backend-internal capture for a declared-indicator thumb drag
/// (Step 212). Deliberately NOT a declared hit region injected into the
/// author's view: the indicator is backend presentation, so its hit
/// surface lives in the bridge, invisible to authored hit-region
/// resolution and to admission. The drag maps track-space pointer travel
/// back to a declared offset and synthesizes `InputEvent::Scroll` through
/// the same declared scroll machinery the offset-sync path uses.
#[derive(Debug)]
struct EguiIndicatorDragState {
    region_id: PresentationRegionId,
    /// Absolute y of the press that started the drag.
    press_y: f32,
    /// Offset anchor at press time; cumulative pointer travel maps from
    /// here so per-frame offset quantization by the author's handler
    /// cannot feed back into the mapping.
    start_offset_y: f32,
    /// Track travel (track height minus thumb height) captured at press.
    track_travel: f32,
    /// Declared travel (content minus viewport) captured at press.
    max_offset: f32,
}

#[derive(Debug, Default)]
pub struct DefaultEguiBridge {
    probes: ProbeCollector,
    observe_next_frame: bool,
    focused_target: Option<WidgetId>,
    hovered_region: Option<PresentationRegionId>,
    pointer_capture_region: Option<PresentationRegionId>,
    /// Button that began the active pointer capture. Captured (held) moves carry
    /// no per-event button from egui, so this is threaded into the move's
    /// pointer details to preserve the held-button state drag handlers require.
    pointer_capture_button: Option<egui::PointerButton>,
    /// Active declared-indicator thumb drag (backend-internal capture).
    indicator_drag: Option<EguiIndicatorDragState>,
    refused_admissions: Vec<BackendParityAdmission>,
    dispatch_refusals: Vec<slipway_core::DeclaredEventDispatchEvidence>,
    terminal_wheel_consumed: bool,
    debug_input_span: Option<EguiDebugInputSpan>,
    debug_composition: bool,
    composition_preflight: Option<EguiCompositionPreflight>,
    native_text_mutation: Option<EguiNativeTextMutationEvidence>,
    accepted_composition_events: Vec<EguiAcceptedCompositionEvent>,
}

impl DefaultEguiBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_observation(&mut self) {
        self.observe_next_frame = true;
    }

    pub fn take_probe_products(&mut self) -> Vec<ProbeProduct> {
        self.probes.take()
    }

    pub fn take_refused_admissions(&mut self) -> Vec<BackendParityAdmission> {
        std::mem::take(&mut self.refused_admissions)
    }

    pub fn take_dispatch_refusals(&mut self) -> Vec<slipway_core::DeclaredEventDispatchEvidence> {
        std::mem::take(&mut self.dispatch_refusals)
    }

    pub fn take_terminal_wheel_consumed(&mut self) -> bool {
        std::mem::take(&mut self.terminal_wheel_consumed)
    }
}

impl<W> EguiSlipwayBridge<W> for DefaultEguiBridge
where
    W: SlipwayEguiBackendChildWidget,
{
    fn set_slipway_debug_input_span(
        &mut self,
        span: Option<EguiDebugInputSpan>,
        composition: bool,
    ) {
        self.debug_input_span = span;
        self.debug_composition = composition;
    }

    fn take_slipway_composition_preflight(&mut self) -> Option<EguiCompositionPreflight> {
        self.composition_preflight.take()
    }

    fn take_slipway_native_text_mutation(&mut self) -> Option<EguiNativeTextMutationEvidence> {
        self.native_text_mutation.take()
    }

    fn drain_slipway_accepted_composition_events(&mut self) -> Vec<EguiAcceptedCompositionEvent> {
        std::mem::take(&mut self.accepted_composition_events)
    }

    fn layout_input(&mut self, context: EguiLayoutContext<'_>) -> LayoutInput {
        let width = context.available_size.x.max(0.0);
        let height = context.available_size.y.max(0.0);

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

    fn desired_size(&mut self, layout: &LayoutOutput) -> egui::Vec2 {
        egui::vec2(
            layout.bounds().into_rect().size.width.max(0.0),
            layout.bounds().into_rect().size.height.max(0.0),
        )
    }

    fn input_events(&mut self, context: EguiInputContext<'_>) -> Vec<BackendInputEvent> {
        let mut events = Vec::new();
        let root_target = context.widget_id.clone();
        let raw_input = egui_raw_input_snapshot(context.ui);
        self.composition_preflight = None;
        self.native_text_mutation = None;
        self.accepted_composition_events.clear();
        let accepted_commit = egui_accepted_composition_commit_key(
            &raw_input.events,
            self.debug_input_span,
            self.debug_composition,
            context.native_physical_operation,
        );
        if let Some((_, event_count)) = accepted_commit {
            self.accepted_composition_events = Vec::with_capacity(event_count);
        }
        if let Some(DebugPhysicalControl::TextComposition { selector, .. }) =
            context.native_physical_operation
            && let Some(region) =
                egui_text_edit_region_for_native_selector(context.regions, selector)
        {
            self.composition_preflight = Some(EguiCompositionPreflight {
                target: region.target.clone(),
                target_slot: region.address.clone(),
                selected_region: region.region_id.clone(),
                focused: region.response.has_focus(),
                editable: region.enabled
                    && region.response.enabled()
                    && region.response.sense.is_focusable(),
            });
        }
        let has_mouse_wheel = raw_input
            .events
            .iter()
            .any(|event| matches!(event, egui::Event::MouseWheel { .. }));

        for region in context.regions {
            let native_effect_source = if context.native_physical_operation.is_some() {
                EvidenceSource::backend_presented(EGUI_BACKEND_ID, EGUI_DEBUG_INPUT_PASS)
            } else {
                EvidenceSource::backend_presented(EGUI_BACKEND_ID, EGUI_NATIVE_OS_INPUT_PASS)
            };
            if region.response.gained_focus() {
                self.focused_target = Some(region.target.clone());
                let event = InputEvent::Focus(slipway_core::FocusEvent {
                    target: region.target.clone(),
                    target_slot: region.address.clone(),
                    focused: true,
                });
                push_egui_backend_input_with_source(
                    &mut events,
                    egui_focus_backend_input_event(
                        &context,
                        region,
                        DeclaredEventDispatchKind::Focus,
                        event,
                    ),
                    native_effect_source.clone(),
                );
            }

            if region.response.lost_focus() {
                if self.focused_target.as_ref() == Some(&region.target) {
                    self.focused_target = None;
                }
                let event = InputEvent::Focus(slipway_core::FocusEvent {
                    target: region.target.clone(),
                    target_slot: region.address.clone(),
                    focused: false,
                });
                push_egui_backend_input_with_source(
                    &mut events,
                    egui_focus_backend_input_event(
                        &context,
                        region,
                        DeclaredEventDispatchKind::Focus,
                        event,
                    ),
                    native_effect_source.clone(),
                );
            }

            if region.response.clicked() && egui_region_can_request_focus(region) {
                region.response.request_focus();
                self.focused_target = Some(region.target.clone());
            }

            if let Some(change) = &region.text_edit_change {
                let text_change_source = if self.debug_composition
                    && self.debug_input_span.is_some()
                {
                    EvidenceSource::backend_presented(EGUI_BACKEND_ID, EGUI_DEBUG_COMPOSITION_PASS)
                } else {
                    native_effect_source.clone()
                };
                let event = InputEvent::TextEdit(TextEditEvent {
                    target: region.target.clone(),
                    target_slot: region.address.clone(),
                    kind: TextEditKind::ReplaceBuffer,
                    text: Some(change.after.clone()),
                    selection_before: change.selection_before.clone(),
                    selection_after: change.selection_after.clone(),
                });
                if let Some((commit, _)) = accepted_commit
                    && self
                        .composition_preflight
                        .as_ref()
                        .is_some_and(|preflight| {
                            preflight.target == region.target
                                && preflight.target_slot == region.address
                                && preflight.selected_region == region.region_id
                                && preflight.focused == region.response.has_focus()
                                && preflight.editable
                                    == (region.enabled
                                        && region.response.enabled()
                                        && region.response.sense.is_focusable())
                        })
                    && change.before != change.after
                {
                    self.native_text_mutation = Some(EguiNativeTextMutationEvidence {
                        target: region.target.clone(),
                        target_slot: region.address.clone(),
                        selected_region: region.region_id.clone(),
                        before: change.before.clone(),
                        after: change.after.clone(),
                        commit,
                    });
                }
                push_egui_backend_input_with_source(
                    &mut events,
                    egui_focus_backend_input_event(
                        &context,
                        region,
                        DeclaredEventDispatchKind::Text,
                        event,
                    ),
                    text_change_source,
                );
            }

            if let Some(scroll) = &region.scroll_state
                && !has_mouse_wheel
            {
                let delta_x = scroll.egui_offset.x - scroll.declared_offset.x;
                let delta_y = scroll.egui_offset.y - scroll.declared_offset.y;
                if delta_x.abs() > f32::EPSILON || delta_y.abs() > f32::EPSILON {
                    let event = InputEvent::Scroll(ScrollEvent {
                        target: region.target.clone(),
                        target_slot: region.address.clone(),
                        region_id: region.region_id.clone(),
                        offset_x: scroll.egui_offset.x,
                        offset_y: scroll.egui_offset.y,
                        viewport: TargetLocalRect::new(scroll.inner_rect),
                        content_bounds: TargetLocalRect::new(Rect {
                            origin: Point { x: 0.0, y: 0.0 },
                            size: scroll.content_size,
                        }),
                    });
                    if let Some(event) = egui_scroll_backend_input_event(&context, region, event) {
                        push_egui_backend_input_with_source(
                            &mut events,
                            event,
                            native_effect_source.clone(),
                        );
                    }
                }
            }
        }

        let mut focus_request_after_input = None;
        let focused_native_text_edit = (context.native_physical_operation.is_none()
            || matches!(
                context.native_physical_operation,
                Some(DebugPhysicalControl::TextComposition { .. })
            ))
            && focused_region(context.regions, self.focused_target.as_ref())
                .is_some_and(|region| region.kind == EguiPresentedRegionKind::TextEdit);

        for (event_index, event) in raw_input.events.iter().enumerate() {
            let event_source = egui_raw_event_source(
                self.debug_input_span,
                self.debug_composition,
                event_index,
                event,
            );
            match event {
                egui::Event::PointerMoved(position) => {
                    // Active declared-indicator thumb drag (Step 212): the
                    // captured thumb follows the pointer; every move
                    // synthesizes a declared Scroll event (the same
                    // machinery as the offset-sync path above).
                    if let Some(drag) = self.indicator_drag.as_ref() {
                        let region = egui_region_by_id(context.regions, &drag.region_id);
                        match region {
                            Some(region) => {
                                let offset_y = egui_indicator_drag_offset(drag, position.y);
                                if let Some(event) =
                                    egui_indicator_scroll_event_for_offset(region, offset_y)
                                    && let Some(event) =
                                        egui_scroll_backend_input_event(&context, region, event)
                                {
                                    push_egui_backend_input_with_source(
                                        &mut events,
                                        event,
                                        event_source.clone(),
                                    );
                                }
                            }
                            None => {
                                // The dragged region left the view.
                                self.indicator_drag = None;
                            }
                        }
                        continue;
                    }
                    if let Some(captured) = self
                        .pointer_capture_region
                        .as_ref()
                        .and_then(|id| egui_region_by_id(context.regions, id))
                    {
                        if let Some(event) = egui_backend_captured_pointer_input_event(
                            &context,
                            captured,
                            *position,
                            PointerEventKind::Move,
                            None,
                            egui_pointer_details(raw_input.modifiers, self.pointer_capture_button),
                            true,
                        ) {
                            push_egui_backend_input_with_source(
                                &mut events,
                                event,
                                event_source.clone(),
                            );
                        }
                        continue;
                    }

                    let region = egui_region_at_position(context.regions, *position);
                    let next_hovered = region.map(|region| region.region_id.clone());
                    if self.hovered_region != next_hovered {
                        if let Some(previous_id) = self.hovered_region.take() {
                            if let Some(previous) = egui_region_by_id(context.regions, &previous_id)
                            {
                                let leave_position =
                                    egui_region_anchor_position(&context, previous);
                                if let Some(event) = egui_backend_pointer_input_event(
                                    &context,
                                    previous,
                                    leave_position,
                                    PointerEventKind::Leave,
                                    None,
                                    egui_pointer_details(raw_input.modifiers, None),
                                    false,
                                ) {
                                    push_egui_backend_input_with_source(
                                        &mut events,
                                        event,
                                        event_source.clone(),
                                    );
                                }
                            }
                        }

                        if let Some(region) = region {
                            if let Some(event) = egui_backend_pointer_input_event(
                                &context,
                                region,
                                *position,
                                PointerEventKind::Enter,
                                None,
                                egui_pointer_details(raw_input.modifiers, None),
                                false,
                            ) {
                                push_egui_backend_input_with_source(
                                    &mut events,
                                    event,
                                    event_source.clone(),
                                );
                            }
                        }
                        self.hovered_region = next_hovered;
                    }

                    if let Some(region) = region {
                        if let Some(event) = egui_backend_pointer_input_event(
                            &context,
                            region,
                            *position,
                            PointerEventKind::Move,
                            None,
                            egui_pointer_details(raw_input.modifiers, None),
                            false,
                        ) {
                            push_egui_backend_input_with_source(
                                &mut events,
                                event,
                                event_source.clone(),
                            );
                        }
                    }
                }
                egui::Event::PointerButton {
                    pos,
                    button,
                    pressed,
                    ..
                } => {
                    if !*pressed && self.indicator_drag.take().is_some() {
                        // End of a declared-indicator drag: consume the
                        // release; the declared offset already tracks the
                        // last drag move.
                        continue;
                    }
                    // A primary press on a painted indicator track/thumb is
                    // claimed by the backend (Step 212): thumb press ->
                    // captured drag; track press -> JUMP-TO-POSITION (the
                    // thumb centers on the click and the press keeps
                    // dragging from there). Points covered by a fronting
                    // authored region (overlay panel occlusion or a
                    // fronting hit region) are refused inside the helper,
                    // so pointer selection agrees with visible stacking.
                    if *pressed
                        && *button == egui::PointerButton::Primary
                        && let Some((drag, jump)) =
                            egui_indicator_press_interaction(context.regions, *pos)
                    {
                        if let Some(offset_y) = jump
                            && let Some(region) =
                                egui_region_by_id(context.regions, &drag.region_id)
                            && let Some(event) =
                                egui_indicator_scroll_event_for_offset(region, offset_y)
                            && let Some(event) =
                                egui_scroll_backend_input_event(&context, region, event)
                        {
                            push_egui_backend_input_with_source(
                                &mut events,
                                event,
                                event_source.clone(),
                            );
                        }
                        self.indicator_drag = Some(drag);
                        continue;
                    }
                    if !*pressed {
                        if let Some(captured) = self
                            .pointer_capture_region
                            .as_ref()
                            .and_then(|id| egui_region_by_id(context.regions, id))
                        {
                            if let Some(event) = egui_backend_captured_pointer_input_event(
                                &context,
                                captured,
                                *pos,
                                PointerEventKind::Release,
                                Some(egui_pointer_button(*button)),
                                egui_pointer_details(raw_input.modifiers, Some(*button)),
                                false,
                            ) {
                                push_egui_backend_input_with_source(
                                    &mut events,
                                    event,
                                    event_source.clone(),
                                );
                            }
                            self.pointer_capture_region = None;
                            self.pointer_capture_button = None;
                            continue;
                        }
                    }

                    let Some(region) = egui_region_at_position(context.regions, *pos) else {
                        // No-silence contract (NC-2, roadmap Phase 6 item 1)
                        // — egui parity with the iced press/release arms: a
                        // press or release consumed by a pointer-opaque
                        // paint layer with no reachable hit region leaves
                        // inspectable refusal evidence in the bridge's
                        // refusal ring, never silence.
                        if let Some(occlusion) =
                            egui_occlusion_region_at_position(context.regions, *pos)
                        {
                            let mut refusal = egui_blocked_pointer_refusal_evidence(
                                &context,
                                occlusion,
                                *pos,
                                if *pressed {
                                    PointerEventKind::Press
                                } else {
                                    PointerEventKind::Release
                                },
                                Some(egui_pointer_button(*button)),
                            );
                            refusal.source = event_source.clone();
                            self.dispatch_refusals.push(refusal);
                        }
                        continue;
                    };
                    if *pressed {
                        self.focused_target = Some(region.target.clone());
                        if egui_region_can_request_focus(region) {
                            focus_request_after_input = Some(region.region_id.clone());
                        }
                    }
                    if let Some(event) = egui_backend_pointer_input_event(
                        &context,
                        region,
                        *pos,
                        if *pressed {
                            PointerEventKind::Press
                        } else {
                            PointerEventKind::Release
                        },
                        Some(egui_pointer_button(*button)),
                        egui_pointer_details(raw_input.modifiers, Some(*button)),
                        *pressed,
                    ) {
                        if *pressed
                            && event
                                .dispatch_evidence
                                .as_ref()
                                .is_some_and(|evidence| evidence.capture_event)
                            && egui_region_requires_stateful_pointer_capture(
                                context.hit_regions,
                                &region.region_id,
                            )
                        {
                            self.pointer_capture_region = Some(region.region_id.clone());
                            self.pointer_capture_button = Some(*button);
                        }
                        push_egui_backend_input_with_source(
                            &mut events,
                            event,
                            event_source.clone(),
                        );
                    }
                }
                egui::Event::PointerGone => {
                    if self.pointer_capture_region.is_some() && raw_input.pointer_any_down {
                        continue;
                    }
                    let region = self
                        .pointer_capture_region
                        .as_ref()
                        .and_then(|id| egui_region_by_id(context.regions, id))
                        .or_else(|| {
                            self.hovered_region
                                .as_ref()
                                .and_then(|id| egui_region_by_id(context.regions, id))
                        })
                        .or_else(|| focused_region(context.regions, self.focused_target.as_ref()));
                    if let Some(region) = region {
                        let position = egui_region_anchor_position(&context, region);
                        if let Some(event) = egui_backend_captured_pointer_input_event(
                            &context,
                            region,
                            position,
                            PointerEventKind::Cancel,
                            None,
                            egui_pointer_details(raw_input.modifiers, None),
                            false,
                        ) {
                            push_egui_backend_input_with_source(
                                &mut events,
                                event,
                                event_source.clone(),
                            );
                        }
                    }
                    self.pointer_capture_region = None;
                    self.pointer_capture_button = None;
                    self.indicator_drag = None;
                    self.hovered_region = None;
                }
                egui::Event::Text(text)
                    if self.focused_target.is_some() && !focused_native_text_edit =>
                {
                    let (target, target_slot) = focused_event_target(
                        context.regions,
                        self.focused_target.as_ref(),
                        &root_target,
                    );
                    let event = InputEvent::Text(TextInputEvent {
                        target,
                        target_slot,
                        text: text.clone(),
                    });
                    if let Some(event) = egui_focused_backend_input_event(
                        &context,
                        self.focused_target.as_ref(),
                        DeclaredEventDispatchKind::Text,
                        event,
                    ) {
                        push_egui_backend_input_with_source(
                            &mut events,
                            event,
                            event_source.clone(),
                        );
                    }
                }
                egui::Event::Paste(text)
                    if self.focused_target.is_some() && !focused_native_text_edit =>
                {
                    let (target, target_slot) = focused_event_target(
                        context.regions,
                        self.focused_target.as_ref(),
                        &root_target,
                    );
                    let event = InputEvent::Text(TextInputEvent {
                        target,
                        target_slot,
                        text: text.clone(),
                    });
                    if let Some(event) = egui_focused_backend_input_event(
                        &context,
                        self.focused_target.as_ref(),
                        DeclaredEventDispatchKind::Text,
                        event,
                    ) {
                        push_egui_backend_input_with_source(
                            &mut events,
                            event,
                            event_source.clone(),
                        );
                    }
                }
                egui::Event::Key {
                    key,
                    physical_key,
                    pressed,
                    repeat,
                    modifiers,
                    ..
                } if self.focused_target.is_some() && !focused_native_text_edit => {
                    let (target, target_slot) = focused_event_target(
                        context.regions,
                        self.focused_target.as_ref(),
                        &root_target,
                    );
                    let event = InputEvent::Keyboard(KeyboardEvent {
                        target,
                        target_slot,
                        key: format!("{key:?}"),
                        kind: if *pressed {
                            KeyEventKind::Press
                        } else {
                            KeyEventKind::Release
                        },
                        modifiers: egui_modifiers(*modifiers),
                        details: KeyboardDetails {
                            logical_key: Some(format!("{key:?}")),
                            physical_key: physical_key.map(|key| format!("{key:?}")),
                            text: None,
                            repeat: *repeat,
                            location: KeyLocation::Unknown,
                        },
                    });
                    if let Some(event) = egui_focused_backend_input_event(
                        &context,
                        self.focused_target.as_ref(),
                        DeclaredEventDispatchKind::Keyboard,
                        event,
                    ) {
                        push_egui_backend_input_with_source(
                            &mut events,
                            event,
                            event_source.clone(),
                        );
                    }
                }
                egui::Event::Ime(ime) if self.focused_target.is_some() => {
                    let (target, target_slot) = focused_event_target(
                        context.regions,
                        self.focused_target.as_ref(),
                        &root_target,
                    );
                    if let egui::ImeEvent::Commit(text) = ime
                        && !focused_native_text_edit
                    {
                        let event = InputEvent::Text(TextInputEvent {
                            target: target.clone(),
                            target_slot: target_slot.clone(),
                            text: text.clone(),
                        });
                        if let Some(event) = egui_focused_backend_input_event(
                            &context,
                            self.focused_target.as_ref(),
                            DeclaredEventDispatchKind::Text,
                            event,
                        ) {
                            push_egui_backend_input_with_source(
                                &mut events,
                                event,
                                event_source.clone(),
                            );
                        }
                    }
                    if let Some(composition_event) =
                        egui_composition_event(Some(target), target_slot, ime)
                    {
                        if let Some(event) = egui_focused_backend_input_event(
                            &context,
                            self.focused_target.as_ref(),
                            DeclaredEventDispatchKind::Text,
                            InputEvent::TextComposition(composition_event.clone()),
                        ) {
                            let dispatch_ordinal = events.len();
                            push_egui_backend_input_with_source(
                                &mut events,
                                event,
                                event_source.clone(),
                            );
                            if let Some(span) = self.debug_input_span
                                && accepted_commit.is_some()
                                && span.start_event_index <= event_index
                                && event_index < span.end_event_index
                                && matches!(
                                    composition_event.phase,
                                    TextCompositionPhase::Update | TextCompositionPhase::Commit
                                )
                            {
                                self.accepted_composition_events.push(
                                    EguiAcceptedCompositionEvent {
                                        token: span.token,
                                        start_event_index: span.start_event_index,
                                        end_event_index: span.end_event_index,
                                        event_index,
                                        phase: composition_event.phase,
                                        dispatch_ordinal,
                                    },
                                );
                            }
                        }
                    }
                }
                egui::Event::MouseWheel { delta, .. } => {
                    let Some(position) = raw_input.hover_pos else {
                        continue;
                    };
                    let refusal_start = self.dispatch_refusals.len();
                    let resolution = egui_backend_wheel_input_resolution(
                        &context,
                        position,
                        delta.x,
                        delta.y,
                        &mut self.dispatch_refusals,
                    );
                    if resolution.terminal_consumed {
                        self.terminal_wheel_consumed = true;
                    }
                    if let Some(event) = resolution.event {
                        push_egui_backend_input_with_source(
                            &mut events,
                            event,
                            event_source.clone(),
                        );
                    }
                    for refusal in &mut self.dispatch_refusals[refusal_start..] {
                        refusal.source = event_source.clone();
                    }
                }
                egui::Event::Copy if self.focused_target.is_some() => {
                    let (target, target_slot) = focused_event_target(
                        context.regions,
                        self.focused_target.as_ref(),
                        &root_target,
                    );
                    let event = InputEvent::Command(slipway_core::CommandEvent {
                        target,
                        target_slot,
                        command: "copy".to_string(),
                        payload_ref: None,
                        source: None,
                    });
                    if let Some(event) = egui_focused_backend_input_event(
                        &context,
                        self.focused_target.as_ref(),
                        DeclaredEventDispatchKind::Command,
                        event,
                    ) {
                        push_egui_backend_input_with_source(
                            &mut events,
                            event,
                            event_source.clone(),
                        );
                    }
                }
                egui::Event::Cut if self.focused_target.is_some() => {
                    let (target, target_slot) = focused_event_target(
                        context.regions,
                        self.focused_target.as_ref(),
                        &root_target,
                    );
                    let event = InputEvent::Command(slipway_core::CommandEvent {
                        target,
                        target_slot,
                        command: "cut".to_string(),
                        payload_ref: None,
                        source: None,
                    });
                    if let Some(event) = egui_focused_backend_input_event(
                        &context,
                        self.focused_target.as_ref(),
                        DeclaredEventDispatchKind::Command,
                        event,
                    ) {
                        push_egui_backend_input_with_source(
                            &mut events,
                            event,
                            event_source.clone(),
                        );
                    }
                }
                _ => {}
            }
        }

        if let Some(region_id) = focus_request_after_input {
            if let Some(region) = context
                .regions
                .iter()
                .find(|region| region.region_id == region_id)
            {
                region.response.request_focus();
            }
        }

        events
    }

    fn paint(&mut self, context: EguiPaintContext<'_>, ops: &[PaintOp]) {
        for op in ops {
            paint_op(&context.painter, context.rect.min, op);
        }
    }

    fn messages(&mut self, outcome: EventOutcome<W::AppMessage>) -> Vec<W::AppMessage> {
        self.probes.extend(outcome.probes);
        self.probes.extend(
            outcome
                .diagnostics
                .into_iter()
                .map(ProbeProduct::Diagnostic),
        );
        outcome
            .emitted_messages
            .into_iter()
            .map(|message| message.message)
            .collect()
    }

    fn visible_admission_refused(&mut self, admission: BackendParityAdmission) {
        if let Some(existing) = self
            .refused_admissions
            .iter_mut()
            .find(|existing| backend_admission_same_observation(existing, &admission))
        {
            *existing = admission;
        } else {
            self.refused_admissions.push(admission);
        }
    }

    fn drain_refused_admissions(&mut self) -> Vec<BackendParityAdmission> {
        self.take_refused_admissions()
    }

    fn drain_dispatch_refusals(&mut self) -> Vec<slipway_core::DeclaredEventDispatchEvidence> {
        self.take_dispatch_refusals()
    }

    fn take_terminal_wheel_consumed(&mut self) -> bool {
        self.take_terminal_wheel_consumed()
    }

    fn wants_observation(&mut self) -> bool {
        let requested = self.observe_next_frame;
        self.observe_next_frame = false;
        requested
    }

    fn observe(&mut self, context: EguiObservationContext) {
        let traversal = context.topology.traverse_depth_first();
        self.probes.push(ProbeProduct::Topology(TopologyProbe {
            root: context.topology,
            traversal,
        }));
        self.probes.push(ProbeProduct::State(StateProbe {
            target: context.widget_id,
            observations: context.state,
        }));
        if let Some(layout_intent) = context.layout_intent {
            self.probes.push(ProbeProduct::LayoutIntent(layout_intent));
        }
        self.probes.extend(
            context
                .unsupported
                .into_iter()
                .map(ProbeProduct::Diagnostic),
        );
    }
}

fn backend_admission_same_observation(
    existing: &BackendParityAdmission,
    incoming: &BackendParityAdmission,
) -> bool {
    existing == incoming
}

/// A single authored Slipway widget lifted into egui's custom widget path.
///
/// The wrapper preserves the authored widget identity and local state slot it
/// was given. It does not merge the application into one backend-visible widget.
pub struct SlipwayEguiWidget<'a, W, B>
where
    W: SlipwayEguiBackendWidget,
    B: EguiSlipwayBridge<W>,
{
    widget: &'a W,
    external: &'a W::ExternalState,
    local: &'a mut W::LocalState,
    bridge: &'a mut B,
    messages: &'a mut Vec<W::AppMessage>,
    backend_traces: Option<&'a mut Vec<BackendInputTrace>>,
    sense: egui::Sense,
    presented_viewport: Option<&'a mut Option<Rect>>,
    presented_content_size: Option<&'a mut Option<Size>>,
    native_physical_operation: Option<&'a DebugPhysicalControl>,
    frame_seed: Option<FrameIdentity>,
    layout_input_override: Option<LayoutInput>,
    allocated_size_override: Option<egui::Vec2>,
    timing_samples: Option<&'a mut Vec<EguiFrameTimingSample>>,
}

struct EguiFrameTimingSample {
    kind: &'static str,
    duration: Duration,
    event_count: usize,
}

impl<'a, W, B> SlipwayEguiWidget<'a, W, B>
where
    W: SlipwayEguiBackendWidget,
    B: EguiSlipwayBridge<W>,
{
    pub fn new(
        widget: &'a W,
        external: &'a W::ExternalState,
        local: &'a mut W::LocalState,
        bridge: &'a mut B,
        messages: &'a mut Vec<W::AppMessage>,
    ) -> Self {
        Self {
            widget,
            external,
            local,
            bridge,
            messages,
            backend_traces: None,
            sense: egui::Sense::hover(),
            presented_viewport: None,
            presented_content_size: None,
            native_physical_operation: None,
            frame_seed: None,
            layout_input_override: None,
            allocated_size_override: None,
            timing_samples: None,
        }
    }

    pub fn sense(mut self, sense: egui::Sense) -> Self {
        self.sense = sense;
        self
    }

    pub fn record_presented_viewport(mut self, viewport: &'a mut Option<Rect>) -> Self {
        self.presented_viewport = Some(viewport);
        self
    }

    pub fn record_presented_content_size(mut self, content_size: &'a mut Option<Size>) -> Self {
        self.presented_content_size = Some(content_size);
        self
    }

    pub fn record_backend_traces(mut self, backend_traces: &'a mut Vec<BackendInputTrace>) -> Self {
        self.backend_traces = Some(backend_traces);
        self
    }

    fn native_physical_operation(mut self, operation: Option<&'a DebugPhysicalControl>) -> Self {
        self.native_physical_operation = operation;
        self
    }

    fn frame_seed(mut self, frame: FrameIdentity) -> Self {
        self.frame_seed = Some(frame);
        self
    }

    fn layout_input_override(mut self, layout_input: LayoutInput) -> Self {
        self.layout_input_override = Some(layout_input);
        self
    }

    fn allocated_size_override(mut self, allocated_size: egui::Vec2) -> Self {
        self.allocated_size_override = Some(allocated_size);
        self
    }

    fn record_frame_timing(mut self, timing_samples: &'a mut Vec<EguiFrameTimingSample>) -> Self {
        self.timing_samples = Some(timing_samples);
        self
    }
}

fn push_egui_frame_timing(
    timing_samples: &mut Option<&mut Vec<EguiFrameTimingSample>>,
    kind: &'static str,
    duration: Duration,
    event_count: usize,
) {
    if let Some(samples) = timing_samples.as_deref_mut() {
        samples.push(EguiFrameTimingSample {
            kind,
            duration,
            event_count,
        });
    }
}

#[derive(Clone, Copy)]
struct EguiPresentationRegionTimingLabels {
    scroll: &'static str,
    focus: &'static str,
    hit: &'static str,
}

const EGUI_WIDGET_PRESENTATION_REGION_TIMING_LABELS: EguiPresentationRegionTimingLabels =
    EguiPresentationRegionTimingLabels {
        scroll: "egui.widget.presentation_regions.scroll",
        focus: "egui.widget.presentation_regions.focus",
        hit: "egui.widget.presentation_regions.hit",
    };

const EGUI_CHILD_PRESENTATION_REGION_TIMING_LABELS: EguiPresentationRegionTimingLabels =
    EguiPresentationRegionTimingLabels {
        scroll: "egui.child.presentation_regions.scroll",
        focus: "egui.child.presentation_regions.focus",
        hit: "egui.child.presentation_regions.hit",
    };

impl<W, B> egui::Widget for SlipwayEguiWidget<'_, W, B>
where
    W: SlipwayEguiBackendWidget,
    B: EguiSlipwayBridge<W>,
{
    fn ui(mut self, ui: &mut egui::Ui) -> egui::Response {
        let mut timing_samples = self.timing_samples.take();
        let view_definition_start = Instant::now();
        let layout_input = self.layout_input_override.unwrap_or_else(|| {
            self.bridge.layout_input(EguiLayoutContext {
                ui,
                available_size: ui.available_size(),
                pixels_per_point: ui.ctx().pixels_per_point(),
            })
        });
        if let Some(presented_viewport) = self.presented_viewport {
            *presented_viewport = Some(layout_input.viewport.into_rect());
        }
        let frame = self.frame_seed.take().unwrap_or_else(|| {
            egui_frame_identity(ui, &self.widget.id(), layout_input.viewport.into_rect())
        });
        let mut view = self.widget.visible_backend_view_definition(
            self.external,
            self.local,
            ViewDefinitionInput::new(frame, layout_input),
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.view_definition",
            view_definition_start.elapsed(),
            1,
        );
        let admission_geometry_start = Instant::now();
        let capabilities = self.widget.capabilities();
        let prepared_geometry = match validate_and_index_view(&view) {
            Ok(prepared) => prepared,
            Err(diagnostics) => {
                view.diagnostics.extend(diagnostics);
                let admission = egui_backend_admission()
                    .admit_view_definition_with_capabilities(&capabilities, &view);
                let desired_size = self.bridge.desired_size(&view.layout);
                let allocated_size = self.allocated_size_override.unwrap_or(desired_size);
                let (rect, response) = ui.allocate_exact_size(allocated_size, self.sense);
                paint_visible_admission_refusal(ui, rect, &admission);
                self.bridge.visible_admission_refused(admission);
                return response;
            }
        };
        let geometry_index = &prepared_geometry.index;
        normalize_egui_visible_scroll_regions(&mut view, geometry_index);
        let admission =
            egui_backend_admission().admit_view_definition_with_capabilities(&capabilities, &view);
        let desired_size = self.bridge.desired_size(&view.layout);
        if let Some(presented_content_size) = self.presented_content_size {
            *presented_content_size = Some(view.layout.bounds().into_rect().size);
        }
        let allocated_size = self.allocated_size_override.unwrap_or(desired_size);
        let (rect, response) = ui.allocate_exact_size(allocated_size, self.sense);
        if !admission.accepted {
            push_egui_frame_timing(
                &mut timing_samples,
                "egui.widget.admission_geometry",
                admission_geometry_start.elapsed(),
                0,
            );
            paint_visible_admission_refusal(ui, rect, &admission);
            self.bridge.visible_admission_refused(admission);
            return response;
        }
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.admission_geometry",
            admission_geometry_start.elapsed(),
            1,
        );

        let font_install_start = Instant::now();
        let (font_admissions, font_metrics) =
            install_declared_fonts_with_metrics(ui, self.widget, self.external, self.local, &view);
        let font_refusal_count = font_admissions.len();
        for admission in font_admissions {
            self.bridge.visible_admission_refused(admission);
        }
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install",
            font_install_start.elapsed(),
            font_metrics.total(),
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install.queued",
            Duration::ZERO,
            font_metrics.queued,
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install.installed",
            Duration::ZERO,
            font_metrics.installed,
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install.already_installed",
            Duration::ZERO,
            font_metrics.already_installed,
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install.missing_source",
            Duration::ZERO,
            font_metrics.missing_source,
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install.missing_asset_ref",
            Duration::ZERO,
            font_metrics.missing_asset_ref,
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install.read_failed",
            Duration::ZERO,
            font_metrics.read_failed,
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install.unsupported_source",
            Duration::ZERO,
            font_metrics.unsupported_source,
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.font_install.refused",
            Duration::ZERO,
            font_refusal_count,
        );
        let paint_region_start = Instant::now();
        let child_slot_start = Instant::now();
        let child_slots = authored_child_slots(self.widget, self.external, self.local);
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.authored_child_slots",
            child_slot_start.elapsed(),
            child_slots.len(),
        );
        let mut child_assembly = EguiChildAssembly::default();
        if ui.is_rect_visible(rect) {
            let paint_clip = egui_view_paint_clip_rect(rect.min, rect, &view);
            let first_job = child_assembly.paint_jobs.len();
            let paint_local_start = Instant::now();
            let paint_records = paint_egui_default_jobs_and_push_explicit_layer_jobs(
                ui,
                &mut child_assembly.paint_jobs,
                PaintUnit::from_view_ref(&view, 0),
                rect.min,
                paint_clip,
            );
            let queued_explicit_jobs = child_assembly.paint_jobs.len().saturating_sub(first_job);
            push_egui_frame_timing(
                &mut timing_samples,
                "egui.widget.paint_local_jobs",
                paint_local_start.elapsed(),
                paint_records.len() + queued_explicit_jobs,
            );
            let occlusion_start = Instant::now();
            let occlusion_regions =
                allocate_paint_occlusion_regions(ui, &child_assembly.paint_jobs[first_job..]);
            let occlusion_region_count = occlusion_regions.len();
            child_assembly.regions.extend(occlusion_regions);
            push_egui_frame_timing(
                &mut timing_samples,
                "egui.widget.paint_occlusion_regions",
                occlusion_start.elapsed(),
                occlusion_region_count,
            );
        }
        let presentation_region_start = Instant::now();
        let root_slot = WidgetSlotAddress::new(self.widget.id(), 0);
        let mut regions = allocate_presentation_regions_with_timing(
            ui,
            self.widget,
            self.external,
            self.local,
            Some(&root_slot),
            rect.min,
            &view,
            geometry_index,
            &child_slots,
            &mut child_assembly,
            self.native_physical_operation,
            timing_samples.as_deref_mut(),
            Some(EGUI_WIDGET_PRESENTATION_REGION_TIMING_LABELS),
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.presentation_regions",
            presentation_region_start.elapsed(),
            regions.len(),
        );
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.paint_region_construction",
            paint_region_start.elapsed(),
            regions.len(),
        );
        let child_presentation_start = Instant::now();
        let authored_children = present_authored_children_mounted(
            ui,
            self.widget,
            self.external,
            self.local,
            &view,
            Some(&root_slot),
            geometry_index,
            rect.min,
            &child_assembly.claimed_slots,
            None,
            self.native_physical_operation,
            timing_samples.as_deref_mut(),
        );
        child_assembly.extend(authored_children);
        paint_egui_jobs(ui, &mut child_assembly.paint_jobs);
        paint_declared_scroll_indicators(ui, &mut child_assembly.scroll_indicators);
        for admission in child_assembly.refused_admissions.drain(..) {
            self.bridge.visible_admission_refused(admission);
        }
        regions.extend(child_assembly.regions);
        apply_egui_native_physical_region_effect(self.native_physical_operation, &regions);
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.child_presentation",
            child_presentation_start.elapsed(),
            child_assembly.presented_slots.len(),
        );
        let input_bridge_start = Instant::now();
        let mut input_events = child_assembly.input_events;
        input_events.extend(self.bridge.input_events(EguiInputContext {
            ui,
            widget_id: self.widget.id(),
            frame: &view.frame,
            rect,
            layout: &view.layout,
            geometry_index,
            hit_regions: &view.hit_regions,
            focus_regions: &view.focus_regions,
            scroll_regions: &view.scroll_regions,
            wheel_traversal_boundary: view.wheel_traversal_boundary,
            response: &response,
            regions: &regions,
            native_physical_operation: self.native_physical_operation,
        }));
        let input_event_count = input_events.len();
        for event in input_events {
            if let Some(outcome) =
                egui_backend_input_contract_refusal::<W::AppMessage>(&view, &event)
            {
                if let Some(backend_traces) = self.backend_traces.as_deref_mut() {
                    backend_traces.push(egui_backend_input_trace(
                        self.widget,
                        self.external,
                        self.local,
                        event,
                        &outcome,
                    ));
                }
                continue;
            }
            let input = event.event.clone();
            let declaration = slipway_core::declared_event_handling(
                self.widget,
                self.external,
                &*self.local,
                &input,
            );
            if !declaration.disposition.final_disposition.handled {
                let outcome = slipway_core::refuse_event_declared_unhandled(declaration);
                if let Some(backend_traces) = self.backend_traces.as_deref_mut() {
                    backend_traces.push(egui_backend_input_trace(
                        self.widget,
                        self.external,
                        self.local,
                        event,
                        &outcome,
                    ));
                }
                continue;
            }
            let raw_outcome = self
                .widget
                .handle_event(self.external, self.local, input.clone());
            let outcome =
                slipway_core::apply_physical_event_handling_declaration(declaration, raw_outcome);
            if let Some(backend_traces) = self.backend_traces.as_deref_mut() {
                backend_traces.push(egui_backend_input_trace(
                    self.widget,
                    self.external,
                    self.local,
                    event,
                    &outcome,
                ));
            }
            if outcome.handled {
                ui.ctx().request_repaint();
            }
            self.messages.extend(self.bridge.messages(outcome));
        }
        push_egui_frame_timing(
            &mut timing_samples,
            "egui.widget.input_bridge",
            input_bridge_start.elapsed(),
            input_event_count,
        );

        if self.bridge.wants_observation() {
            self.bridge.observe(EguiObservationContext {
                widget_id: self.widget.id(),
                capabilities: self.widget.capabilities(),
                topology: self.widget.topology(self.external),
                unsupported: self.widget.unsupported(),
                state: self.widget.observe_state(self.external, self.local),
                layout_intent: None,
            });
        }

        response
    }
}

/// A layout-intent-aware egui lift for authored widgets that explicitly opt in.
///
/// This wrapper does not infer or calculate policy. It only calls
/// `SlipwayLayoutIntent::layout_intent` during explicit observation requests and
/// forwards the returned probe product to the bridge.
pub struct SlipwayEguiLayoutIntentWidget<'a, W, B>
where
    W: SlipwayEguiLayoutIntentBackendWidget,
    B: EguiSlipwayBridge<W>,
{
    widget: &'a W,
    external: &'a W::ExternalState,
    local: &'a mut W::LocalState,
    bridge: &'a mut B,
    messages: &'a mut Vec<W::AppMessage>,
    backend_traces: Option<&'a mut Vec<BackendInputTrace>>,
    sense: egui::Sense,
}

impl<'a, W, B> SlipwayEguiLayoutIntentWidget<'a, W, B>
where
    W: SlipwayEguiLayoutIntentBackendWidget,
    B: EguiSlipwayBridge<W>,
{
    pub fn new(
        widget: &'a W,
        external: &'a W::ExternalState,
        local: &'a mut W::LocalState,
        bridge: &'a mut B,
        messages: &'a mut Vec<W::AppMessage>,
    ) -> Self {
        Self {
            widget,
            external,
            local,
            bridge,
            messages,
            backend_traces: None,
            sense: egui::Sense::hover(),
        }
    }

    pub fn sense(mut self, sense: egui::Sense) -> Self {
        self.sense = sense;
        self
    }

    pub fn record_backend_traces(mut self, backend_traces: &'a mut Vec<BackendInputTrace>) -> Self {
        self.backend_traces = Some(backend_traces);
        self
    }
}

impl<W, B> egui::Widget for SlipwayEguiLayoutIntentWidget<'_, W, B>
where
    W: SlipwayEguiLayoutIntentBackendWidget,
    B: EguiSlipwayBridge<W>,
{
    fn ui(mut self, ui: &mut egui::Ui) -> egui::Response {
        let layout_input = self.bridge.layout_input(EguiLayoutContext {
            ui,
            available_size: ui.available_size(),
            pixels_per_point: ui.ctx().pixels_per_point(),
        });
        let frame = egui_frame_identity(ui, &self.widget.id(), layout_input.viewport.into_rect());
        let mut view = self.widget.visible_backend_view_definition(
            self.external,
            self.local,
            ViewDefinitionInput::new(frame, layout_input.clone()),
        );
        let capabilities = self.widget.capabilities();
        let prepared_geometry = match validate_and_index_view(&view) {
            Ok(prepared) => prepared,
            Err(diagnostics) => {
                view.diagnostics.extend(diagnostics);
                let admission = egui_backend_admission()
                    .admit_view_definition_with_capabilities(&capabilities, &view);
                let desired_size = self.bridge.desired_size(&view.layout);
                let (rect, response) = ui.allocate_exact_size(desired_size, self.sense);
                paint_visible_admission_refusal(ui, rect, &admission);
                self.bridge.visible_admission_refused(admission);
                return response;
            }
        };
        let geometry_index = &prepared_geometry.index;
        normalize_egui_visible_scroll_regions(&mut view, geometry_index);
        let admission =
            egui_backend_admission().admit_view_definition_with_capabilities(&capabilities, &view);
        let desired_size = self.bridge.desired_size(&view.layout);
        let (rect, response) = ui.allocate_exact_size(desired_size, self.sense);
        if !admission.accepted {
            paint_visible_admission_refusal(ui, rect, &admission);
            self.bridge.visible_admission_refused(admission);
            return response;
        }

        for admission in install_declared_fonts(ui, self.widget, self.external, self.local, &view) {
            self.bridge.visible_admission_refused(admission);
        }
        let child_slots = authored_child_slots(self.widget, self.external, self.local);
        let mut child_assembly = EguiChildAssembly::default();
        if ui.is_rect_visible(rect) {
            let paint_clip = egui_view_paint_clip_rect(rect.min, rect, &view);
            let first_job = child_assembly.paint_jobs.len();
            paint_egui_default_jobs_and_push_explicit_layer_jobs(
                ui,
                &mut child_assembly.paint_jobs,
                PaintUnit::from_view_ref(&view, 0),
                rect.min,
                paint_clip,
            );
            child_assembly
                .regions
                .extend(allocate_paint_occlusion_regions(
                    ui,
                    &child_assembly.paint_jobs[first_job..],
                ));
        }
        let mut regions = allocate_presentation_regions(
            ui,
            self.widget,
            self.external,
            self.local,
            rect.min,
            &view,
            geometry_index,
            &child_slots,
            &mut child_assembly,
            None,
        );
        let root_slot = WidgetSlotAddress::new(self.widget.id(), 0);
        let authored_children = present_authored_children_mounted(
            ui,
            self.widget,
            self.external,
            self.local,
            &view,
            Some(&root_slot),
            geometry_index,
            rect.min,
            &child_assembly.claimed_slots,
            None,
            None,
            None,
        );
        child_assembly.extend(authored_children);
        paint_egui_jobs(ui, &mut child_assembly.paint_jobs);
        paint_declared_scroll_indicators(ui, &mut child_assembly.scroll_indicators);
        for admission in child_assembly.refused_admissions.drain(..) {
            self.bridge.visible_admission_refused(admission);
        }
        regions.extend(child_assembly.regions);
        let mut input_events = child_assembly.input_events;
        input_events.extend(self.bridge.input_events(EguiInputContext {
            ui,
            widget_id: self.widget.id(),
            frame: &view.frame,
            rect,
            layout: &view.layout,
            geometry_index,
            hit_regions: &view.hit_regions,
            focus_regions: &view.focus_regions,
            scroll_regions: &view.scroll_regions,
            wheel_traversal_boundary: view.wheel_traversal_boundary,
            response: &response,
            regions: &regions,
            native_physical_operation: None,
        }));
        for event in input_events {
            if let Some(outcome) =
                egui_backend_input_contract_refusal::<W::AppMessage>(&view, &event)
            {
                if let Some(backend_traces) = self.backend_traces.as_deref_mut() {
                    backend_traces.push(egui_backend_input_trace(
                        self.widget,
                        self.external,
                        self.local,
                        event,
                        &outcome,
                    ));
                }
                continue;
            }
            let input = event.event.clone();
            let declaration = slipway_core::declared_event_handling(
                self.widget,
                self.external,
                &*self.local,
                &input,
            );
            if !declaration.disposition.final_disposition.handled {
                let outcome = slipway_core::refuse_event_declared_unhandled(declaration);
                if let Some(backend_traces) = self.backend_traces.as_deref_mut() {
                    backend_traces.push(egui_backend_input_trace(
                        self.widget,
                        self.external,
                        self.local,
                        event,
                        &outcome,
                    ));
                }
                continue;
            }
            let raw_outcome = self
                .widget
                .handle_event(self.external, self.local, input.clone());
            let outcome =
                slipway_core::apply_physical_event_handling_declaration(declaration, raw_outcome);
            if let Some(backend_traces) = self.backend_traces.as_deref_mut() {
                backend_traces.push(egui_backend_input_trace(
                    self.widget,
                    self.external,
                    self.local,
                    event,
                    &outcome,
                ));
            }
            if outcome.handled {
                ui.ctx().request_repaint();
            }
            self.messages.extend(self.bridge.messages(outcome));
        }

        if self.bridge.wants_observation() {
            self.bridge.observe(EguiObservationContext {
                widget_id: self.widget.id(),
                capabilities: self.widget.capabilities(),
                topology: self.widget.topology(self.external),
                unsupported: self.widget.unsupported(),
                state: self.widget.observe_state(self.external, self.local),
                layout_intent: Some(self.widget.layout_intent(
                    self.external,
                    self.local,
                    &layout_input,
                )),
            });
        }

        response
    }
}

/// Generic eframe application shell for N authored Slipway widget slots.
pub struct SlipwayEguiApp<W, B, F>
where
    W: SlipwayEguiBackendWidget,
    B: EguiSlipwayBridge<W>,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
{
    external: W::ExternalState,
    slots: Vec<WidgetSlot<W>>,
    bridge: B,
    on_messages: F,
    sense: egui::Sense,
}

impl<W, B, F> SlipwayEguiApp<W, B, F>
where
    W: SlipwayEguiBackendWidget,
    B: EguiSlipwayBridge<W>,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
{
    pub fn new(
        external: W::ExternalState,
        widgets: impl IntoIterator<Item = W>,
        bridge: B,
        on_messages: F,
    ) -> Self {
        let slots = widgets
            .into_iter()
            .map(|widget| {
                let local_state = widget.initial_local_state();
                WidgetSlot {
                    widget,
                    local_state,
                }
            })
            .collect();

        Self {
            external,
            slots,
            bridge,
            on_messages,
            sense: egui::Sense::hover(),
        }
    }

    pub fn sense(mut self, sense: egui::Sense) -> Self {
        self.sense = sense;
        self
    }

    pub fn widget_count(&self) -> usize {
        self.slots.len()
    }
}

impl<W, B, F> eframe::App for SlipwayEguiApp<W, B, F>
where
    W: SlipwayEguiBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'static,
    B: EguiSlipwayBridge<W> + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        fill_egui_host_background(ui);
        let mut messages = Vec::new();

        for slot in &mut self.slots {
            ui.add(
                SlipwayEguiWidget::new(
                    &slot.widget,
                    &self.external,
                    &mut slot.local_state,
                    &mut self.bridge,
                    &mut messages,
                )
                .sense(self.sense),
            );
        }

        if !messages.is_empty() {
            (self.on_messages)(&mut self.external, messages);
        }
    }
}

/// Launch a generic egui application for authored Slipway widgets.
pub fn run_slipway_egui_app<W, B, F>(
    title: impl Into<String>,
    external: W::ExternalState,
    widgets: impl IntoIterator<Item = W>,
    bridge: B,
    on_messages: F,
) -> eframe::Result<()>
where
    W: SlipwayEguiBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'static,
    B: EguiSlipwayBridge<W> + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    let title = title.into();
    let app = SlipwayEguiApp::new(external, widgets, bridge, on_messages);

    eframe::run_native(
        &title,
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(|_creation_context| Ok(Box::new(app))),
    )
}

pub fn run_slipway_egui_app_with_default_bridge<W, F>(
    title: impl Into<String>,
    external: W::ExternalState,
    widgets: impl IntoIterator<Item = W>,
    on_messages: F,
) -> eframe::Result<()>
where
    W: SlipwayEguiBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    run_slipway_egui_app(
        title,
        external,
        widgets,
        DefaultEguiBridge::new(),
        on_messages,
    )
}

/// Generic eframe application shell for one assembled Slipway runtime.
///
/// Unlike `SlipwayEguiApp`, this type does not own a separate widget slot. The
/// authored widget, external state, local state, frame identity, and debug
/// bridge live inside `SlipwayRuntime`, so visible backend events and MCP/debug
/// commands route through the same app owner.
pub struct SlipwayEguiRuntimeApp<W, B, F>
where
    W: SlipwayEguiBackendWidget,
    W::LocalState: Clone,
    B: EguiSlipwayBridge<W>,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
{
    runtime: SlipwayRuntime<W>,
    bridge: B,
    on_messages: F,
    sense: egui::Sense,
    debug_mcp_transport: Option<SlipwayRuntimeMcpTransport>,
    service_mcp_transport: Option<SlipwayServiceMcpTransport>,
    egui_mcp_wake_rx: Option<mpsc::Receiver<()>>,
    native_mcp_wake_pending: bool,
    pending_native_physical: Option<PendingEguiNativePhysicalControl>,
    pending_presented_capture: Option<PendingEguiPresentedCapture>,
    #[allow(dead_code)]
    next_native_debug_token: u64,
    last_successfully_presented: Option<FrameIdentity>,
    rendered_frame_candidate: Option<FrameIdentity>,
    root_scroll_offset: egui::Vec2,
    frame_timing: VisibleFrameTimingRecorder,
    native_create_started_at: Option<Instant>,
    #[cfg(test)]
    live_continuous_repaint: bool,
    /// Most recent dispatch refusal drained THIS frame (None when the frame
    /// drained none). Feeds the post-hoc diagnosis attached to the no-match
    /// physical-control failure (audit finding MF-H5).
    last_frame_dispatch_refusal: Option<slipway_core::DeclaredEventDispatchEvidence>,
}

#[allow(dead_code)]
enum PendingEguiNativePhysicalControl {
    WaitingForDebugLease(SlipwayRuntimePendingNativeMcpCall),
    WaitingForCompositionPreflight {
        pending: SlipwayRuntimePendingNativeMcpCall,
        lease: slipway_debug_bridge::DebugCommandLease,
    },
    WaitingForIngress {
        pending: SlipwayRuntimePendingNativeMcpCall,
        lease: slipway_debug_bridge::DebugCommandLease,
        token: u64,
        composition: Option<PendingEguiComposition>,
    },
    WaitingForBackendTrace {
        pending: SlipwayRuntimePendingNativeMcpCall,
        lease: slipway_debug_bridge::DebugCommandLease,
        origin: PendingEguiTraceOrigin,
    },
}

#[derive(Clone, Debug)]
struct PendingEguiComposition {
    preflight: EguiCompositionPreflight,
}

#[allow(dead_code)]
#[derive(Clone, Debug)]
enum PendingEguiTraceOrigin {
    BackendNativeMutation,
    DebugInput {
        token: u64,
        span: EguiDebugInputSpan,
    },
    DebugComposition {
        token: u64,
        span: EguiDebugInputSpan,
        composition: PendingEguiComposition,
    },
}

struct PendingEguiPresentedCapture {
    pending: SlipwayRuntimePendingNativeMcpCall,
    lease: slipway_debug_bridge::DebugCommandLease,
    selector: PresentedScreenshotSelector,
}

impl<W, B, F> SlipwayEguiRuntimeApp<W, B, F>
where
    W: SlipwayEguiBackendWidget,
    W::LocalState: Clone,
    B: EguiSlipwayBridge<W>,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
{
    pub fn new(runtime: SlipwayRuntime<W>, bridge: B, on_messages: F) -> Self {
        Self {
            runtime,
            bridge,
            on_messages,
            sense: egui::Sense::hover(),
            debug_mcp_transport: None,
            service_mcp_transport: None,
            egui_mcp_wake_rx: None,
            native_mcp_wake_pending: false,
            pending_native_physical: None,
            pending_presented_capture: None,
            next_native_debug_token: 1,
            last_successfully_presented: None,
            rendered_frame_candidate: None,
            root_scroll_offset: egui::Vec2::ZERO,
            frame_timing: VisibleFrameTimingRecorder::from_env("egui"),
            native_create_started_at: None,
            #[cfg(test)]
            live_continuous_repaint: false,
            last_frame_dispatch_refusal: None,
        }
    }

    pub fn runtime(&self) -> &SlipwayRuntime<W> {
        &self.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut SlipwayRuntime<W> {
        &mut self.runtime
    }

    pub fn with_debug_mcp_transport(mut self, transport: SlipwayRuntimeMcpTransport) -> Self {
        self.debug_mcp_transport = Some(transport);
        self
    }

    pub fn with_service_mcp_transport(mut self, transport: SlipwayServiceMcpTransport) -> Self {
        self.service_mcp_transport = Some(transport);
        self
    }

    pub fn debug_mcp_transport_addr(&self) -> Option<SocketAddr> {
        self.debug_mcp_transport
            .as_ref()
            .map(SlipwayRuntimeMcpTransport::local_addr)
    }

    pub fn service_mcp_transport_addr(&self) -> Option<SocketAddr> {
        self.service_mcp_transport
            .as_ref()
            .map(SlipwayServiceMcpTransport::local_addr)
    }

    fn mark_native_create_started(&mut self) {
        self.native_create_started_at = Some(Instant::now());
    }

    fn record_native_create_phase(&mut self) {
        let Some(started_at) = self.native_create_started_at.take() else {
            return;
        };
        self.frame_timing
            .record("egui.native.create_native", started_at.elapsed(), 0, None);
    }

    fn prewarm_native_visible_cache(&mut self, ctx: &egui::Context) {
        let mut visible_frame_timing = VisibleFrameTimingRecorder::disabled("egui");
        std::mem::swap(&mut self.frame_timing, &mut visible_frame_timing);
        for _ in 0..3 {
            let raw_input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run_ui(raw_input, |ui| self.render_ui(ui));
        }
        std::mem::swap(&mut self.frame_timing, &mut visible_frame_timing);
    }

    pub fn drain_debug_pending(&mut self) -> (usize, Option<String>) {
        self.runtime.record_presenting_backend(EGUI_BACKEND_ID);
        match self.runtime.drain_live_debug_turn_with_app_reducer(
            SlipwayRuntimeDrainBudget::default(),
            &mut self.on_messages,
        ) {
            Ok(report) => (
                report.debug_replies_drained
                    + report.runtime_mcp_replies_drained
                    + report.service_mcp_replies_drained,
                None,
            ),
            Err(error) => (0, Some(format!("{error:?}"))),
        }
    }

    #[allow(dead_code)]
    fn next_native_debug_token(&mut self) -> u64 {
        let token = self.next_native_debug_token;
        self.next_native_debug_token = self.next_native_debug_token.wrapping_add(1).max(1);
        token
    }

    fn complete_native_request(
        pending: SlipwayRuntimePendingNativeMcpCall,
        lease: slipway_debug_bridge::DebugCommandLease,
        product: DebugReplyProduct,
    ) -> Result<(), String> {
        lease
            .complete(product)
            .map_err(|error| format!("{error:?}"))?;
        pending
            .try_finish_and_respond()
            .map_err(|error| format!("{error:?}"))?;
        Ok(())
    }

    fn complete_native_refusal(
        pending: SlipwayRuntimePendingNativeMcpCall,
        lease: slipway_debug_bridge::DebugCommandLease,
        code: &str,
        message: impl Into<String>,
    ) -> Result<(), String> {
        Self::complete_native_request(
            pending,
            lease,
            DebugReplyProduct::Error(DebugFailure {
                code: code.to_string(),
                message: message.into(),
                dispatch_evidence: None,
            }),
        )
    }

    fn intake_pending_native_command(&mut self, ctx: &egui::Context) -> (usize, Option<String>) {
        self.runtime.record_presenting_backend(EGUI_BACKEND_ID);
        if self.pending_presented_capture.is_some() {
            return (0, None);
        }
        let mut drained = 0usize;
        loop {
            let pending = match self.pending_native_physical.take() {
                Some(
                    state @ PendingEguiNativePhysicalControl::WaitingForCompositionPreflight {
                        ..
                    },
                )
                | Some(state @ PendingEguiNativePhysicalControl::WaitingForIngress { .. })
                | Some(state @ PendingEguiNativePhysicalControl::WaitingForBackendTrace { .. }) => {
                    self.pending_native_physical = Some(state);
                    break;
                }
                Some(PendingEguiNativePhysicalControl::WaitingForDebugLease(pending)) => pending,
                None => {
                    let pending = match self.runtime.take_pending_native_mcp_call() {
                        Ok(Some(pending)) => pending,
                        Ok(None) => break,
                        Err(error) => return (drained, Some(format!("{error:?}"))),
                    };
                    drained += 1;
                    pending
                }
            };

            let lease = match self.runtime.take_debug_command_lease() {
                Ok(Some(lease)) => lease,
                Ok(None) => {
                    self.pending_native_physical = Some(
                        PendingEguiNativePhysicalControl::WaitingForDebugLease(pending),
                    );
                    break;
                }
                Err(error) => {
                    self.pending_native_physical = Some(
                        PendingEguiNativePhysicalControl::WaitingForDebugLease(pending),
                    );
                    return (drained, Some(format!("{error:?}")));
                }
            };

            let command = lease.command().clone();
            if matches!(command.kind, DebugCommandKind::Screenshot { .. }) {
                if let Err(error) = self.arm_presented_capture(pending, lease) {
                    return (drained, Some(error));
                }
                break;
            }

            let DebugCommandKind::PhysicalControl { operation, .. } = &command.kind else {
                let product = self
                    .runtime
                    .handle_debug_command_with_app_reducer(command, &mut self.on_messages);
                if let Err(error) = lease.complete(product) {
                    return (drained, Some(format!("{error:?}")));
                }
                if let Err(error) = pending.try_finish_and_respond() {
                    return (drained, Some(format!("{error:?}")));
                }
                continue;
            };

            if matches!(operation, DebugPhysicalControl::TextComposition { .. }) {
                self.pending_native_physical = Some(
                    PendingEguiNativePhysicalControl::WaitingForCompositionPreflight {
                        pending,
                        lease,
                    },
                );
                break;
            }

            let plan = match native_runner::egui_events_for_native_physical_operation(
                operation,
                ctx.pixels_per_point(),
            ) {
                Ok(plan) => plan,
                Err(unsupported) => {
                    if let Err(error) = Self::complete_native_refusal(
                        pending,
                        lease,
                        unsupported.code,
                        unsupported.message,
                    ) {
                        return (drained, Some(error));
                    }
                    continue;
                }
            };

            match plan {
                native_runner::NativePhysicalControlPlan::BackendNativeMutation => {
                    self.pending_native_physical =
                        Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
                            pending,
                            lease,
                            origin: PendingEguiTraceOrigin::BackendNativeMutation,
                        });
                }
            }
            break;
        }

        (drained, None)
    }

    #[cfg(test)]
    fn inject_pending_native_physical_into_raw_input(
        &mut self,
        raw_input: &mut egui::RawInput,
    ) -> (usize, Option<String>) {
        self.runtime.record_presenting_backend(EGUI_BACKEND_ID);
        if self.pending_native_physical.is_some() {
            return (0, None);
        }
        let pending = match self.runtime.take_pending_native_mcp_call() {
            Ok(Some(pending)) => pending,
            Ok(None) => return (0, None),
            Err(error) => return (0, Some(format!("{error:?}"))),
        };
        let lease = match self.runtime.take_debug_command_lease() {
            Ok(Some(lease)) => lease,
            Ok(None) => {
                self.pending_native_physical = Some(
                    PendingEguiNativePhysicalControl::WaitingForDebugLease(pending),
                );
                return (1, None);
            }
            Err(error) => return (1, Some(format!("{error:?}"))),
        };
        let operation = match &lease.command().kind {
            DebugCommandKind::PhysicalControl { operation, .. } => operation,
            _ => {
                let product = self.runtime.handle_debug_command_with_app_reducer(
                    lease.command().clone(),
                    &mut self.on_messages,
                );
                return match Self::complete_native_request(pending, lease, product) {
                    Ok(()) => (1, None),
                    Err(error) => (1, Some(error)),
                };
            }
        };
        let events = match native_runner::egui_test_events_for_native_physical_operation(
            operation, raw_input,
        ) {
            Ok(events) => events,
            Err(error) => {
                return match Self::complete_native_refusal(
                    pending,
                    lease,
                    error.code,
                    error.message,
                ) {
                    Ok(()) => (1, None),
                    Err(error) => (1, Some(error)),
                };
            }
        };
        if events.is_empty() {
            self.pending_native_physical =
                Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
                    pending,
                    lease,
                    origin: PendingEguiTraceOrigin::BackendNativeMutation,
                });
            return (1, None);
        }
        let start_event_index = raw_input.events.len();
        for event in events {
            raw_input.events.push(event);
        }
        let token = self.next_native_debug_token();
        let span = EguiDebugInputSpan {
            token,
            start_event_index,
            end_event_index: raw_input.events.len(),
        };
        self.pending_native_physical =
            Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
                pending,
                lease,
                origin: PendingEguiTraceOrigin::DebugInput { token, span },
            });
        (1, None)
    }

    fn arm_presented_capture(
        &mut self,
        pending: SlipwayRuntimePendingNativeMcpCall,
        lease: slipway_debug_bridge::DebugCommandLease,
    ) -> Result<(), String> {
        let selector = match &lease.command().kind {
            DebugCommandKind::Screenshot { request } => request.selector.clone(),
            _ => {
                return Self::complete_native_refusal(
                    pending,
                    lease,
                    "screenshot-command-required",
                    "egui presented capture only accepts screenshot commands",
                );
            }
        };
        Self::complete_native_request(
            pending,
            lease,
            egui_screenshot_refusal(
                selector,
                self.last_successfully_presented.clone(),
                "screenshot-presented-capture-unavailable",
                "standard eframe/egui-wgpu does not expose request-scoped presented-surface readback; use canonical/offscreen debug rendering instead",
            ),
        )
    }

    fn drain_pending_presented_capture(&mut self) -> (bool, Option<String>) {
        let Some(capture) = self.pending_presented_capture.take() else {
            return (false, None);
        };
        let product = egui_screenshot_refusal(
            capture.selector,
            self.last_successfully_presented.clone(),
            "screenshot-presented-capture-unavailable",
            "standard eframe/egui-wgpu does not expose request-scoped presented-surface readback",
        );
        let result = Self::complete_native_request(capture.pending, capture.lease, product);
        (true, result.err())
    }

    fn record_egui_post_presented_frame(&mut self) {
        let Some(mut presented) = self.rendered_frame_candidate.take() else {
            return;
        };
        let frame_index = self
            .last_successfully_presented
            .as_ref()
            .and_then(|previous| previous.frame_index.checked_add(1))
            .unwrap_or(presented.frame_index);
        presented.frame_index = frame_index;
        self.last_successfully_presented = Some(presented);
    }

    fn terminate_pending_presented_capture_for_teardown(&mut self) {
        let Some(capture) = self.pending_presented_capture.take() else {
            return;
        };
        let product = egui_screenshot_refusal(
            capture.selector,
            self.last_successfully_presented.clone(),
            "screenshot-teardown",
            "the egui window closed before presented capture completed",
        );
        let _ = Self::complete_native_request(capture.pending, capture.lease, product);
    }
    pub fn handle_backend_presented_physical_control(
        &mut self,
        command: DebugCommand,
        backend_input: BackendInputEvent,
    ) -> DebugReplyProduct {
        self.runtime
            .handle_backend_presented_physical_control_for_backend_with_app_reducer(
                command,
                backend_input,
                EGUI_BACKEND_ID,
                &mut self.on_messages,
            )
    }

    pub fn sense(mut self, sense: egui::Sense) -> Self {
        self.sense = sense;
        self
    }

    fn render_ui(&mut self, ui: &mut egui::Ui) {
        let timing_start = Instant::now();
        #[cfg(test)]
        if self.live_continuous_repaint {
            ui.ctx().request_repaint();
        }
        let background_layout_start = Instant::now();
        fill_egui_host_background(ui);
        let mut messages = Vec::new();
        let mut backend_traces = Vec::new();
        let mut widget_timing_samples = self.frame_timing.is_enabled().then(Vec::new);
        let mut presented_viewport = None;
        let mut presented_content_size = None;
        let sense = self.sense;
        let revision_before = self.runtime.last_frame_identity().revision;
        let native_physical_operation = self.pending_native_physical_operation().cloned();
        let (debug_input_span, debug_composition) = self
            .pending_native_physical
            .as_ref()
            .and_then(|pending| match pending {
                PendingEguiNativePhysicalControl::WaitingForBackendTrace { origin, .. } => {
                    match origin {
                        PendingEguiTraceOrigin::DebugInput { span, .. } => Some((*span, false)),
                        PendingEguiTraceOrigin::DebugComposition { span, .. } => {
                            Some((*span, true))
                        }
                        PendingEguiTraceOrigin::BackendNativeMutation => None,
                    }
                }
                _ => None,
            })
            .map_or((None, false), |(span, composition)| {
                (Some(span), composition)
            });
        self.bridge
            .set_slipway_debug_input_span(debug_input_span, debug_composition);

        let available_size = ui.available_size();
        let timing_viewport = Some(Size {
            width: available_size.x.max(0.0),
            height: available_size.y.max(0.0),
        });
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: available_size.x.max(0.0),
                    height: available_size.y.max(0.0),
                },
            }),
            content: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: available_size.x.max(0.0),
                    height: available_size.y.max(0.0),
                },
            }),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: Size {
                    width: available_size.x.max(0.0),
                    height: available_size.y.max(0.0),
                },
            },
        };
        let root_wheel_delta = egui_root_wheel_delta(ui, available_size);
        self.frame_timing.record(
            "egui.render_ui.background_layout_setup",
            background_layout_start.elapsed(),
            0,
            timing_viewport,
        );
        let frame_seed = self
            .runtime
            .frame_identity(layout_input.viewport.into_rect());
        let root_scroll_show_start = Instant::now();
        let mut ui_add_elapsed = Duration::ZERO;
        let ui_add_start = Instant::now();
        self.runtime
            .with_widget_state_mut(|widget, external, local| {
                let widget = SlipwayEguiWidget::new(
                    widget,
                    external,
                    local,
                    &mut self.bridge,
                    &mut messages,
                )
                .sense(sense)
                .record_backend_traces(&mut backend_traces)
                .record_presented_viewport(&mut presented_viewport)
                .record_presented_content_size(&mut presented_content_size)
                .native_physical_operation(native_physical_operation.as_ref())
                .frame_seed(frame_seed.clone())
                .layout_input_override(layout_input.clone())
                .allocated_size_override(available_size);
                let widget = if let Some(samples) = widget_timing_samples.as_mut() {
                    widget.record_frame_timing(samples)
                } else {
                    widget
                };
                ui.add(widget);
            });
        ui_add_elapsed += ui_add_start.elapsed();
        self.frame_timing.record(
            "egui.render_ui.root_scroll_show",
            root_scroll_show_start.elapsed(),
            backend_traces.len(),
            timing_viewport,
        );
        self.frame_timing.record(
            "egui.render_ui.ui_add",
            ui_add_elapsed,
            backend_traces.len(),
            timing_viewport,
        );
        if let Some(samples) = widget_timing_samples {
            for sample in samples {
                self.frame_timing.record(
                    sample.kind,
                    sample.duration,
                    sample.event_count,
                    timing_viewport,
                );
            }
        }
        let root_content_size = presented_content_size
            .map(|size| egui::vec2(size.width.max(0.0), size.height.max(0.0)))
            .unwrap_or(available_size);
        self.root_scroll_offset = clamp_egui_root_scroll_offset(
            self.root_scroll_offset,
            root_content_size,
            available_size,
        );
        if let Some(viewport) = presented_viewport {
            self.runtime.record_presented_viewport(viewport);
        }
        let composition_preflight = self.bridge.take_slipway_composition_preflight();
        let native_text_mutation = self.bridge.take_slipway_native_text_mutation();
        let accepted_composition_events = self.bridge.drain_slipway_accepted_composition_events();
        // Measurement projection (Phase 6 item 3b slice (iii), NC-4): on
        // the same cadence as the viewport projection, hand the app hook
        // the backend's REAL text layout so authored geometry can size
        // itself to laid-out text. No-op (and free) for apps that never
        // override `project_text_metrics`.
        let metrics_ctx = ui.ctx().clone();
        self.runtime
            .project_text_metrics(&mut EguiTextMetricProvider::new(&metrics_ctx));

        let app_message_count = messages.len();
        let revision_after = revision_before + u64::from(app_message_count > 0);
        let handled_wheel = backend_traces_handled_wheel(&backend_traces);
        let terminal_wheel_consumed = self.bridge.take_terminal_wheel_consumed();
        let backend_trace_count = backend_traces.len();
        let retain_validated_traces = self.pending_native_physical.is_some();
        let mut validated_pending_traces = Vec::new();

        let trace_recording_start = Instant::now();
        for mut trace in backend_traces {
            trace.revision_before = trace.revision_before.or(Some(revision_before));
            trace.revision_after = trace.revision_after.or(Some(revision_after));
            self.runtime
                .record_backend_input_trace_for_backend(trace, EGUI_BACKEND_ID);
            if retain_validated_traces {
                validated_pending_traces.push(
                    self.runtime
                        .last_backend_input_trace()
                        .expect("the trace was just recorded")
                        .clone(),
                );
            }
        }
        self.frame_timing.record(
            "egui.render_ui.trace_recording",
            trace_recording_start.elapsed(),
            backend_trace_count,
            timing_viewport,
        );
        let app_message_apply_start = Instant::now();
        self.runtime
            .apply_app_messages(messages, &mut self.on_messages);
        self.frame_timing.record(
            "egui.render_ui.app_message_apply",
            app_message_apply_start.elapsed(),
            app_message_count,
            timing_viewport,
        );
        debug_assert_eq!(self.runtime.last_frame_identity().revision, revision_after);
        if matches!(
            self.pending_native_physical,
            Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
                origin: PendingEguiTraceOrigin::DebugComposition { .. },
                ..
            })
        ) {
            self.complete_pending_native_composition(
                &validated_pending_traces,
                &accepted_composition_events,
                composition_preflight.clone(),
                native_text_mutation,
            );
        } else {
            for trace in &validated_pending_traces {
                self.try_complete_pending_native_physical(trace);
            }
        }
        for admission in self.bridge.drain_refused_admissions() {
            self.runtime.record_backend_admission(&admission);
        }
        let drained_refusals = self.bridge.drain_dispatch_refusals();
        self.last_frame_dispatch_refusal = drained_refusals.last().cloned();
        for refusal in drained_refusals {
            self.runtime
                .record_dispatch_refusal_for_backend(refusal, EGUI_BACKEND_ID);
        }
        if root_wheel_delta != egui::Vec2::ZERO && !handled_wheel && !terminal_wheel_consumed {
            self.root_scroll_offset = clamp_egui_root_scroll_offset(
                self.root_scroll_offset - root_wheel_delta,
                root_content_size,
                available_size,
            );
            ui.ctx().request_repaint();
        }
        if matches!(
            self.pending_native_physical,
            Some(PendingEguiNativePhysicalControl::WaitingForCompositionPreflight { .. })
        ) {
            self.arm_pending_native_composition(composition_preflight, ui.ctx().pixels_per_point());
        } else {
            self.fail_unmatched_pending_native_physical();
        }
        self.rendered_frame_candidate = Some(frame_seed);
        self.frame_timing.record(
            "egui.render_ui",
            timing_start.elapsed(),
            backend_trace_count,
            timing_viewport,
        );
    }

    fn arm_pending_native_composition(
        &mut self,
        _preflight: Option<EguiCompositionPreflight>,
        _pixels_per_point: f32,
    ) {
        let Some(PendingEguiNativePhysicalControl::WaitingForCompositionPreflight {
            pending,
            lease,
        }) = self.pending_native_physical.take()
        else {
            return;
        };
        let _ = Self::complete_native_refusal(
            pending,
            lease,
            "native-physical-control-ingress-unavailable",
            "standard eframe does not expose request-scoped IME composition ingress",
        );
    }

    fn complete_pending_native_composition(
        &mut self,
        traces: &[BackendInputTrace],
        accepted_events: &[EguiAcceptedCompositionEvent],
        focused_after: Option<EguiCompositionPreflight>,
        mutation: Option<EguiNativeTextMutationEvidence>,
    ) {
        let Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
            pending,
            lease,
            origin:
                PendingEguiTraceOrigin::DebugComposition {
                    token,
                    span,
                    composition,
                },
        }) = self.pending_native_physical.take()
        else {
            return;
        };
        debug_assert_eq!(span.token, token);
        let command = lease.command().clone();
        let frame = command.frame_identity().clone();
        let request_id = command.request_id.clone();
        let selected = &composition.preflight;
        let DebugCommandKind::PhysicalControl {
            operation:
                DebugPhysicalControl::TextComposition {
                    updates, commit, ..
                },
            ..
        } = &command.kind
        else {
            unreachable!("composition completion retains a composition command")
        };
        let expected_record_count = updates.len().checked_add(1);
        let records_valid = expected_record_count == Some(accepted_events.len())
            && span.token == token
            && span.end_event_index.checked_sub(span.start_event_index) == expected_record_count
            && accepted_events
                .iter()
                .enumerate()
                .all(|(ordinal, accepted)| {
                    let expected_phase = if ordinal < updates.len() {
                        TextCompositionPhase::Update
                    } else {
                        TextCompositionPhase::Commit
                    };
                    accepted.token == token
                        && accepted.start_event_index == span.start_event_index
                        && accepted.end_event_index == span.end_event_index
                        && span.start_event_index.checked_add(ordinal) == Some(accepted.event_index)
                        && accepted.phase == expected_phase
                        && (ordinal == 0
                            || accepted_events[ordinal - 1].dispatch_ordinal
                                < accepted.dispatch_ordinal)
                });

        let mut native_phases = Vec::with_capacity(accepted_events.len());
        let mut trace_cursor = 0usize;
        if records_valid {
            for (ordinal, accepted) in accepted_events.iter().enumerate() {
                let expected_phase = accepted.phase;
                let expected_text = updates
                    .get(ordinal)
                    .map_or(commit.as_str(), |update| update.preedit_text.as_str());
                let expected_range = updates
                    .get(ordinal)
                    .and_then(|update| update.cursor_range.as_ref());
                let matched = traces
                    .iter()
                    .enumerate()
                    .skip(trace_cursor)
                    .find(|(_, trace)| {
                        let Some(dispatch) = trace.input.dispatch_evidence.as_ref() else {
                            return false;
                        };
                        let InputEvent::TextComposition(event) = &trace.input.event else {
                            return false;
                        };
                        dispatch.source.pass_id.as_deref() == Some(EGUI_DEBUG_COMPOSITION_PASS)
                            && dispatch.frame == frame
                            && dispatch.selected_region.as_ref() == Some(&selected.selected_region)
                            && dispatch.generated_event.as_ref() == Some(&trace.input.event)
                            && event.target == selected.target
                            && event.target_slot == selected.target_slot
                            && event.phase == expected_phase
                            && event.preedit_text == expected_text
                            && event.cursor_range.as_ref() == expected_range
                            && trace.event_probe().result_identity.handled == Some(trace.handled)
                    });
                let Some((trace_index, trace)) = matched else {
                    native_phases.clear();
                    break;
                };
                trace_cursor = trace_index + 1;
                let dispatch = trace
                    .input
                    .dispatch_evidence
                    .as_ref()
                    .expect("the matched composition trace has dispatch evidence");
                let InputEvent::TextComposition(event) = &trace.input.event else {
                    unreachable!("the matched trace is a composition event")
                };
                native_phases.push(DebugCompositionPhaseTrace {
                    phase: event.phase,
                    backend_event: match event.phase {
                        TextCompositionPhase::Update => "egui::ImeEvent::Preedit".to_string(),
                        TextCompositionPhase::Commit => "egui::ImeEvent::Commit".to_string(),
                        _ => unreachable!(),
                    },
                    provenance: CompositionPhaseProvenance::Native,
                    event: event.clone(),
                    ingress_observation: DebugCompositionIngressObservation::EguiRawInputSpan {
                        event_index: accepted.event_index,
                    },
                    dispatch_evidence: dispatch.clone(),
                    app_handled: trace.handled,
                    result_identity: Some(trace.event_probe().result_identity),
                });
            }
        }
        let debug_composition_trace_count = traces
            .iter()
            .filter(|trace| {
                trace
                    .input
                    .dispatch_evidence
                    .as_ref()
                    .is_some_and(|dispatch| {
                        dispatch.source.pass_id.as_deref() == Some(EGUI_DEBUG_COMPOSITION_PASS)
                    })
                    && matches!(
                        trace.input.event,
                        InputEvent::TextComposition(TextCompositionEvent {
                            phase: TextCompositionPhase::Update | TextCompositionPhase::Commit,
                            ..
                        })
                    )
            })
            .count();
        if debug_composition_trace_count != accepted_events.len() {
            native_phases.clear();
        }
        let consumed_commit = (native_phases.len() == accepted_events.len())
            .then(|| {
                accepted_events
                    .iter()
                    .find(|accepted| accepted.phase == TextCompositionPhase::Commit)
                    .map(|accepted| EguiAcceptedCommitKey {
                        token: accepted.token,
                        start_event_index: accepted.start_event_index,
                        end_event_index: accepted.end_event_index,
                        event_index: accepted.event_index,
                    })
            })
            .flatten();

        let mut phases = Vec::with_capacity(native_phases.len().saturating_add(2));
        if let Some(first_update) = native_phases
            .iter()
            .find(|phase| phase.phase == TextCompositionPhase::Update)
        {
            let mut start = first_update.clone();
            start.phase = TextCompositionPhase::Start;
            start.backend_event = "derived egui composition start".to_string();
            start.provenance = CompositionPhaseProvenance::Derived {
                from: TextCompositionPhase::Update,
            };
            start.event.phase = TextCompositionPhase::Start;
            start.event.preedit_text.clear();
            start.event.cursor_range = None;
            start.ingress_observation = DebugCompositionIngressObservation::Derived {
                from_sequence_index: 1,
            };
            phases.push(start);
        }
        phases.extend(native_phases);
        if let Some(commit_index) = phases
            .iter()
            .position(|phase| phase.phase == TextCompositionPhase::Commit)
        {
            let mut end = phases[commit_index].clone();
            end.phase = TextCompositionPhase::End;
            end.backend_event = "derived egui composition end".to_string();
            end.provenance = CompositionPhaseProvenance::Derived {
                from: TextCompositionPhase::Commit,
            };
            end.event.phase = TextCompositionPhase::End;
            end.event.preedit_text.clear();
            end.event.cursor_range = None;
            end.ingress_observation = DebugCompositionIngressObservation::Derived {
                from_sequence_index: commit_index,
            };
            phases.push(end);
        }

        let commit_mutation = mutation.and_then(|mutation| {
            if mutation.target != selected.target
                || mutation.target_slot != selected.target_slot
                || mutation.selected_region != selected.selected_region
                || Some(mutation.commit) != consumed_commit
            {
                return None;
            }
            let mut matching_traces = traces.iter().filter(|trace| {
                matches!(
                    &trace.input.event,
                    InputEvent::TextEdit(event)
                        if event.target == mutation.target
                            && event.target_slot == mutation.target_slot
                ) && trace
                    .input
                    .dispatch_evidence
                    .as_ref()
                    .is_some_and(|evidence| {
                        evidence.source.pass_id.as_deref() == Some(EGUI_DEBUG_COMPOSITION_PASS)
                            && evidence.selected_region.as_ref() == Some(&mutation.selected_region)
                            && evidence.frame == frame
                            && evidence.generated_event.as_ref() == Some(&trace.input.event)
                    })
                    && trace.event_probe().result_identity.handled == Some(trace.handled)
            });
            let trace = matching_traces.next()?;
            if matching_traces.next().is_some() {
                return None;
            }
            let dispatch = trace.input.dispatch_evidence.clone();
            let control = DebugControlTrace::new(
                request_id.clone(),
                frame.clone(),
                &trace.input.event,
                trace.handled,
                trace.revision_before.unwrap_or(frame.revision),
                trace.revision_after.unwrap_or(frame.revision),
                trace.diagnostics.clone(),
            )
            .with_mode(DebugControlMode::PhysicalEquivalent)
            .with_dispatch_evidence(dispatch)
            .with_result_identity(trace.event_probe().result_identity);
            Some(DebugCompositionCommitMutation {
                trace: control,
                before: mutation.before,
                after: mutation.after,
            })
        });
        let focused_after = focused_after.is_some_and(|after| {
            after.target == selected.target
                && after.target_slot == selected.target_slot
                && after.selected_region == selected.selected_region
                && after.focused
                && after.editable
        });
        let trace = DebugCompositionTrace {
            request_id,
            frame,
            backend_id: EGUI_BACKEND_ID.to_string(),
            target: selected.target.clone(),
            selected_region: selected.selected_region.clone(),
            focused_before: selected.focused,
            focused_after,
            phases,
            commit_mutation,
            completed: false,
            failure: None,
        };
        let product = DebugEguiCompositionIngressCustody::new(
            span.token,
            span.start_event_index,
            span.end_event_index,
        )
        .map_or_else(
            || {
                DebugReplyProduct::Error(DebugFailure {
                    code: "native-physical-control-text-composition-provenance-invalid".to_string(),
                    message: "egui composition accepted span could not establish custody"
                        .to_string(),
                    dispatch_evidence: None,
                })
            },
            |custody| {
                self.runtime
                    .backend_presented_text_composition_product_from_traces_for_backend_with_egui_custody(
                        command,
                        trace,
                        EGUI_BACKEND_ID,
                        custody,
                    )
            },
        );
        let _ = Self::complete_native_request(pending, lease, product);
    }

    fn ensure_mcp_wake_forwarder(&mut self, ctx: &egui::Context) {
        if self.egui_mcp_wake_rx.is_some() {
            return;
        }

        if self.debug_mcp_transport.is_none() && self.service_mcp_transport.is_none() {
            return;
        }

        let ctx = ctx.clone();
        let (wake_tx, wake_rx_for_app) = mpsc::sync_channel(1);
        let mut spawned = false;
        if let Some(transport) = &self.debug_mcp_transport {
            let wake_rx = transport.wake_receiver();
            let wake_tx = wake_tx.clone();
            let ctx = ctx.clone();
            spawned |= thread::Builder::new()
                .name("slipway-egui-debug-mcp-wake".to_string())
                .spawn(move || {
                    while wake_rx.recv() {
                        let _ = wake_tx.try_send(());
                        ctx.request_repaint();
                    }
                })
                .is_ok();
        }
        if let Some(transport) = &self.service_mcp_transport {
            let wake_rx = transport.wake_receiver();
            let wake_tx = wake_tx.clone();
            let ctx = ctx.clone();
            spawned |= thread::Builder::new()
                .name("slipway-egui-service-mcp-wake".to_string())
                .spawn(move || {
                    while wake_rx.recv() {
                        let _ = wake_tx.try_send(());
                        ctx.request_repaint();
                    }
                })
                .is_ok();
        }

        if spawned {
            self.egui_mcp_wake_rx = Some(wake_rx_for_app);
        }
    }

    fn drain_egui_mcp_wakes(&mut self) -> usize {
        let Some(wake_rx) = &self.egui_mcp_wake_rx else {
            return 0;
        };

        let mut drained = 0;
        while wake_rx.try_recv().is_ok() {
            drained += 1;
        }
        drained
    }

    fn try_complete_pending_native_physical(&mut self, trace: &BackendInputTrace) {
        let Some(pending_state) = self.pending_native_physical.take() else {
            return;
        };
        let PendingEguiNativePhysicalControl::WaitingForBackendTrace {
            pending,
            lease,
            origin,
        } = pending_state
        else {
            self.pending_native_physical = Some(pending_state);
            return;
        };
        let expected_pass = match &origin {
            PendingEguiTraceOrigin::BackendNativeMutation => EGUI_DEBUG_INPUT_PASS,
            PendingEguiTraceOrigin::DebugInput { token, span } => {
                debug_assert_eq!(*token, span.token);
                EGUI_DEBUG_INPUT_PASS
            }
            PendingEguiTraceOrigin::DebugComposition { .. } => {
                self.pending_native_physical =
                    Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
                        pending,
                        lease,
                        origin,
                    });
                return;
            }
        };
        if trace
            .input
            .dispatch_evidence
            .as_ref()
            .and_then(|evidence| evidence.source.pass_id.as_deref())
            != Some(expected_pass)
        {
            self.pending_native_physical =
                Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
                    pending,
                    lease,
                    origin,
                });
            return;
        }

        let product = self
            .runtime
            .backend_presented_physical_control_product_from_trace_for_backend(
                lease.command().clone(),
                trace,
                EGUI_BACKEND_ID,
            );
        if matches!(product, DebugReplyProduct::Error(_)) {
            self.pending_native_physical =
                Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
                    pending,
                    lease,
                    origin,
                });
            return;
        }

        let _ = lease.complete(product);
        let _ = pending.try_finish_and_respond();
    }

    fn fail_unmatched_pending_native_physical(&mut self) {
        let Some(pending_state) = self.pending_native_physical.take() else {
            return;
        };
        let PendingEguiNativePhysicalControl::WaitingForBackendTrace {
            pending,
            lease,
            origin: _origin,
        } = pending_state
        else {
            self.pending_native_physical = Some(pending_state);
            return;
        };

        // Post-hoc diagnosis (MF-H5): when this frame's routing constructed
        // refusal evidence for the same operation kind, attach it under the
        // distinct `post_hoc_diagnosis` source label so the agent can see WHY
        // the operation produced no trace (dead point / at-limit, with
        // position, candidates, and reason) without ever confusing the
        // diagnosis with real dispatch evidence.
        let diagnosis = self
            .last_frame_dispatch_refusal
            .as_ref()
            .filter(|refusal| {
                let DebugCommandKind::PhysicalControl { operation, .. } = &lease.command().kind
                else {
                    return false;
                };
                physical_operation_matches_refusal_kind(operation, refusal.kind)
            })
            .cloned()
            .map(|mut refusal| {
                refusal.source = EvidenceSource::post_hoc_diagnosis(
                    EGUI_BACKEND_ID,
                    "physical-control-no-match",
                );
                refusal
            });
        let message = match &diagnosis {
            Some(diagnosis) => format!(
                "egui RawInput received the requested physical operation, but the visible backend produced no matching backend-presented trace in this frame; a post-hoc dispatch diagnosis (source label `{}`) is attached: {}; candidates=[{}]",
                slipway_core::EVIDENCE_SOURCE_POST_HOC_DIAGNOSIS,
                diagnosis
                    .refusal_reason
                    .as_deref()
                    .unwrap_or("dispatch refused without a recorded reason"),
                diagnosis
                    .candidate_regions
                    .iter()
                    .map(|region| region.as_str())
                    .collect::<Vec<_>>()
                    .join(", "),
            ),
            None => "egui RawInput received the requested physical operation, but the visible backend produced no matching backend-presented trace in this frame; no routing-level refusal evidence was constructed for it — probe the `diagnostics` kind for retained refusals".to_string(),
        };
        let product = DebugReplyProduct::Error(DebugFailure {
            code: "native-physical-control-no-backend-trace".to_string(),
            message,
            dispatch_evidence: diagnosis,
        });
        let _ = lease.complete(product);
        let _ = pending.try_finish_and_respond();
    }

    fn pending_native_physical_operation(&self) -> Option<&DebugPhysicalControl> {
        let lease = match self.pending_native_physical.as_ref()? {
            PendingEguiNativePhysicalControl::WaitingForCompositionPreflight { lease, .. }
            | PendingEguiNativePhysicalControl::WaitingForBackendTrace { lease, .. } => lease,
            PendingEguiNativePhysicalControl::WaitingForDebugLease(_)
            | PendingEguiNativePhysicalControl::WaitingForIngress { .. } => return None,
        };
        let DebugCommandKind::PhysicalControl { operation, .. } = &lease.command().kind else {
            return None;
        };
        Some(operation)
    }
}

fn egui_screenshot_refusal(
    selector: PresentedScreenshotSelector,
    captured_frame: Option<FrameIdentity>,
    code: impl Into<String>,
    reason: impl Into<String>,
) -> DebugReplyProduct {
    DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Refusal(
        PresentedScreenshotRefusal {
            selector,
            captured_frame,
            backend_id: Some(EGUI_BACKEND_ID.to_string()),
            code: code.into(),
            reason: reason.into(),
            diagnostics: Vec::new(),
        },
    ))
}

/// True when a retained dispatch refusal's kind corresponds to the physical
/// operation that produced no backend trace, so a stale refusal from an
/// unrelated input is never attached as that operation's diagnosis.
fn physical_operation_matches_refusal_kind(
    operation: &DebugPhysicalControl,
    kind: DeclaredEventDispatchKind,
) -> bool {
    matches!(
        (operation, kind),
        (
            DebugPhysicalControl::Pointer { .. },
            DeclaredEventDispatchKind::Pointer
        ) | (
            DebugPhysicalControl::Wheel { .. },
            DeclaredEventDispatchKind::Wheel
        ) | (
            DebugPhysicalControl::Scroll { .. },
            DeclaredEventDispatchKind::Scroll
        ) | (
            DebugPhysicalControl::Focus { .. },
            DeclaredEventDispatchKind::Focus
        ) | (
            DebugPhysicalControl::Text { .. } | DebugPhysicalControl::TextEdit { .. },
            DeclaredEventDispatchKind::Text
        ) | (
            DebugPhysicalControl::TextComposition { .. },
            DeclaredEventDispatchKind::Text
        ) | (
            DebugPhysicalControl::Keyboard { .. },
            DeclaredEventDispatchKind::Keyboard
        ) | (
            DebugPhysicalControl::Command { .. },
            DeclaredEventDispatchKind::Command
        )
    )
}

impl<W, B, F> eframe::App for SlipwayEguiRuntimeApp<W, B, F>
where
    W: SlipwayEguiBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'static,
    B: EguiSlipwayBridge<W> + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let timing_start = Instant::now();
        self.ensure_mcp_wake_forwarder(ctx);
        let woke = self.drain_egui_mcp_wakes();
        self.native_mcp_wake_pending |= woke > 0;
        let (native_drained, native_error) = self.intake_pending_native_command(ctx);
        let (capture_polled, capture_error) = self.drain_pending_presented_capture();
        if woke > 0 || native_error.is_some() || capture_error.is_some() {
            ctx.request_repaint();
        }
        self.frame_timing.record(
            "egui.logic",
            timing_start.elapsed(),
            woke + native_drained + usize::from(capture_polled),
            None,
        );
    }

    fn on_exit(&mut self) {
        self.terminate_pending_presented_capture_for_teardown();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.render_ui(ui);
        self.record_egui_post_presented_frame();
    }
}

fn fill_egui_host_background(ui: &mut egui::Ui) {
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
}

fn egui_root_wheel_delta(ui: &egui::Ui, viewport_size: egui::Vec2) -> egui::Vec2 {
    let input_options = ui.ctx().options(|options| options.input_options);
    ui.input(|input| {
        input
            .events
            .iter()
            .filter_map(|event| match event {
                egui::Event::MouseWheel {
                    unit,
                    delta,
                    phase,
                    modifiers,
                } => Some(egui_convert_wheel_delta(
                    viewport_size,
                    &input_options,
                    *unit,
                    *delta,
                    *phase,
                    *modifiers,
                )),
                _ => None,
            })
            .fold(egui::Vec2::ZERO, |sum, delta| sum + delta)
    })
}

fn egui_convert_wheel_delta(
    viewport_size: egui::Vec2,
    input_options: &egui::InputOptions,
    unit: egui::MouseWheelUnit,
    delta: egui::Vec2,
    phase: egui::TouchPhase,
    modifiers: egui::Modifiers,
) -> egui::Vec2 {
    if phase != egui::TouchPhase::Move {
        return egui::Vec2::ZERO;
    }

    let mut delta = match unit {
        egui::MouseWheelUnit::Point => delta,
        egui::MouseWheelUnit::Line => input_options.line_scroll_speed * delta,
        egui::MouseWheelUnit::Page => viewport_size.y.max(0.0) * delta,
    };

    let is_horizontal = modifiers.matches_any(input_options.horizontal_scroll_modifier);
    let is_vertical = modifiers.matches_any(input_options.vertical_scroll_modifier);
    if is_horizontal && !is_vertical {
        delta = egui::vec2(delta.x + delta.y, 0.0);
    }
    if !is_horizontal && is_vertical {
        delta = egui::vec2(0.0, delta.x + delta.y);
    }
    delta
}

fn backend_traces_handled_wheel(traces: &[BackendInputTrace]) -> bool {
    traces
        .iter()
        .any(|trace| trace.handled && matches!(trace.input.event, InputEvent::Wheel(_)))
}

fn clamp_egui_root_scroll_offset(
    offset: egui::Vec2,
    content_size: egui::Vec2,
    viewport_size: egui::Vec2,
) -> egui::Vec2 {
    egui::vec2(
        offset
            .x
            .clamp(0.0, (content_size.x - viewport_size.x).max(0.0)),
        offset
            .y
            .clamp(0.0, (content_size.y - viewport_size.y).max(0.0)),
    )
}

pub fn run_slipway_egui_runtime_app<W, B, F>(
    title: impl Into<String>,
    runtime: SlipwayRuntime<W>,
    bridge: B,
    on_messages: F,
) -> eframe::Result<()>
where
    W: SlipwayEguiBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'static,
    B: EguiSlipwayBridge<W> + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    let title = title.into();
    let debug_mcp_transport = runtime
        .start_debug_mcp_transport()
        .map_err(|error| eframe::Error::AppCreation(Box::new(error)))?;
    let title = format!("{title} (MCP {})", debug_mcp_transport.local_addr());
    let app = SlipwayEguiRuntimeApp::new(runtime, bridge, on_messages)
        .with_debug_mcp_transport(debug_mcp_transport);

    native_runner::run_slipway_egui_runtime_app_native(&title, app)
}

pub fn run_slipway_egui_runtime_app_with_default_bridge<W, F>(
    title: impl Into<String>,
    runtime: SlipwayRuntime<W>,
    on_messages: F,
) -> eframe::Result<()>
where
    W: SlipwayEguiBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    run_slipway_egui_runtime_app(title, runtime, DefaultEguiBridge::new(), on_messages)
}

fn backend_profile_admission(
    backend_id: &str,
    report: &BackendCapabilityReport,
    required_profiles: &[CapabilityProfileKind],
) -> BackendParityAdmission {
    let mut visible_requirements = Vec::new();
    for profile in required_profiles {
        for capability in profile_visible_requirements(profile) {
            push_view_requirement(
                &mut visible_requirements,
                &format!("profile::{profile:?}::{capability:?}"),
                None,
                capability,
            );
        }
    }

    let unsupported = visible_requirements
        .iter()
        .filter(|requirement| requirement.required)
        .filter(|requirement| {
            !report
                .visible_capabilities
                .iter()
                .any(|capability| capability == &requirement.capability)
        })
        .map(|requirement| UnsupportedCapabilityEvidence {
            backend_id: backend_id.to_string(),
            target: requirement.target.clone(),
            capability: Capability::BackendCapabilityNegotiation,
            visible_capability: Some(requirement.capability.clone()),
            requirement_id: Some(requirement.requirement_id.clone()),
            reason: "required visible backend capability is not declared".to_string(),
            source: EvidenceSource::backend_presented(backend_id, "profile-admission"),
            diagnostics: Vec::new(),
        })
        .collect::<Vec<_>>();

    BackendParityAdmission {
        backend_id: backend_id.to_string(),
        accepted: unsupported.is_empty(),
        required_profiles: required_profiles.to_vec(),
        visible_requirements,
        unsupported,
        source: EvidenceSource::backend_presented(backend_id, "profile-admission"),
        diagnostics: Vec::new(),
    }
}

fn profile_visible_requirements(profile: &CapabilityProfileKind) -> Vec<BackendVisibleCapability> {
    match profile {
        CapabilityProfileKind::TextInput => vec![
            BackendVisibleCapability::FocusRegions,
            BackendVisibleCapability::TextEditRegions,
            BackendVisibleCapability::FontInstallation,
        ],
        CapabilityProfileKind::ScrollableContainer => vec![BackendVisibleCapability::ScrollRegions],
        CapabilityProfileKind::ProviderSurface => vec![egui_provider_surface_visible_capability()],
        CapabilityProfileKind::BackendAdapter => vec![
            BackendVisibleCapability::HitRegions,
            BackendVisibleCapability::Cursor,
            BackendVisibleCapability::FocusRegions,
            BackendVisibleCapability::TextEditRegions,
            BackendVisibleCapability::ScrollRegions,
            BackendVisibleCapability::ShapePathClip,
            BackendVisibleCapability::FontInstallation,
            BackendVisibleCapability::BackendPresentedEvidence,
            egui_provider_surface_visible_capability(),
        ],
        other => vec![BackendVisibleCapability::Custom(format!("{other:?}"))],
    }
}

fn egui_provider_surface_visible_capability() -> BackendVisibleCapability {
    BackendVisibleCapability::Custom(EGUI_PROVIDER_SURFACE_REQUIREMENT.to_string())
}

fn push_view_requirement(
    requirements: &mut Vec<BackendVisibleCapabilityRequirement>,
    id: impl Into<String>,
    target: Option<WidgetId>,
    capability: BackendVisibleCapability,
) {
    let requirement_id = id.into();
    if requirements.iter().any(|requirement| {
        requirement.requirement_id == requirement_id
            && requirement.target == target
            && requirement.capability == capability
    }) {
        return;
    }

    requirements.push(BackendVisibleCapabilityRequirement {
        requirement_id,
        target,
        capability,
        required: true,
    });
}

fn unsupported_egui_visible_paint_diagnostics(
    target: &WidgetId,
    ops: &[PaintOp],
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    collect_unsupported_egui_visible_paint(target, ops, &mut diagnostics);
    diagnostics
}

fn collect_unsupported_egui_visible_paint(
    target: &WidgetId,
    ops: &[PaintOp],
    diagnostics: &mut Vec<Diagnostic>,
) {
    for op in ops {
        match op {
            PaintOp::Fill { shape, .. } | PaintOp::Stroke { shape, .. } => {
                if shape.clip.as_ref().is_some_and(|clip| clip.path.is_some()) {
                    diagnostics.push(Diagnostic::unsupported(
                        Some(target.clone()),
                        "egui.visible_paint.unsupported_shape_clip_path",
                        "egui visible renderer supports rectangular shape clips only in this backend build",
                    ));
                }
            }
            PaintOp::Group { clip, ops, .. } | PaintOp::Layer { clip, ops, .. } => {
                if clip.as_ref().is_some_and(|clip| clip.path.is_some()) {
                    diagnostics.push(Diagnostic::unsupported(
                        Some(target.clone()),
                        "egui.visible_paint.unsupported_group_clip_path",
                        "egui visible renderer supports rectangular group clips only in this backend build",
                    ));
                }
                collect_unsupported_egui_visible_paint(target, ops, diagnostics);
            }
            PaintOp::Text { .. } => {}
        }
    }
}

fn normalize_egui_visible_scroll_regions(
    view: &mut ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
) {
    let mut diagnostics = Vec::new();
    for region in &mut view.scroll_regions {
        normalize_egui_visible_scroll_region(region, geometry_index, &mut diagnostics);
    }
    view.diagnostics.extend(diagnostics);
}

fn egui_view_paint_clip_rect(
    view_origin: egui::Pos2,
    fallback: egui::Rect,
    view: &ViewDefinition,
) -> egui::Rect {
    view.paint_order
        .overflow_bounds
        .map(|bounds| egui_rect(view_origin, bounds.into_rect()))
        .unwrap_or(fallback)
}

fn normalize_egui_visible_scroll_region(
    region: &mut ScrollRegionDeclaration,
    geometry_index: &PresentationGeometryIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let target_bounds = slipway_core::declared_target_local_bounds(
        slipway_core::declared_target_rect_for_region_address_with_geometry_index(
            geometry_index,
            &region.target,
            region.address.as_ref(),
        ),
    )
    .into_rect();

    let viewport = region.viewport.into_rect();
    if !egui_declared_rect_is_valid(viewport) || !egui_declared_rect_is_valid(target_bounds) {
        let safe = safe_zero_rect_inside(target_bounds);
        region.viewport = TargetLocalRect::new(safe);
        region.content_bounds = TargetLocalRect::new(safe);
        region.offset = Point { x: 0.0, y: 0.0 };
        region.enabled = false;
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "egui.visible_scroll.normalized_invalid_geometry",
            "egui visible backend disabled an invalid scroll region instead of allowing it to break the visible surface",
        ));
        return;
    }

    let Some(cropped_viewport) = rect_intersection(viewport, target_bounds) else {
        let safe = safe_zero_rect_inside(target_bounds);
        region.viewport = TargetLocalRect::new(safe);
        region.content_bounds = TargetLocalRect::new(safe);
        region.offset = Point { x: 0.0, y: 0.0 };
        region.enabled = false;
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "egui.visible_scroll.disabled_outside_layout",
            "egui visible backend disabled a scroll region whose viewport is fully outside the target layout bounds",
        ));
        return;
    };

    if cropped_viewport != viewport {
        region.viewport = TargetLocalRect::new(cropped_viewport);
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "egui.visible_scroll.viewport_cropped_to_layout",
            "egui visible backend cropped a scroll viewport to the target layout bounds before visible admission",
        ));
    }

    let content_bounds = region.content_bounds.into_rect();
    let normalized_content = if egui_declared_rect_is_valid(content_bounds) {
        rect_union(content_bounds, cropped_viewport)
    } else {
        cropped_viewport
    };
    if normalized_content != content_bounds {
        region.content_bounds = TargetLocalRect::new(normalized_content);
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "egui.visible_scroll.content_bounds_expanded_to_viewport",
            "egui visible backend expanded invalid or undersized scroll content bounds to contain the visible viewport",
        ));
    }

    let mut offset = region.offset;
    if !offset.x.is_finite() || offset.x < 0.0 || !region.axes.horizontal {
        offset.x = 0.0;
    }
    if !offset.y.is_finite() || offset.y < 0.0 || !region.axes.vertical {
        offset.y = 0.0;
    }

    let max_x = (normalized_content.size.width - cropped_viewport.size.width).max(0.0);
    let max_y = (normalized_content.size.height - cropped_viewport.size.height).max(0.0);
    offset.x = offset.x.clamp(0.0, max_x);
    offset.y = offset.y.clamp(0.0, max_y);
    if offset != region.offset {
        region.offset = offset;
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "egui.visible_scroll.offset_clamped",
            "egui visible backend clamped a scroll offset so the visible surface remains presentable",
        ));
    }
}

fn egui_declared_rect_is_valid(rect: Rect) -> bool {
    rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.size.width >= 0.0
        && rect.size.height >= 0.0
}

fn rect_intersection(a: Rect, b: Rect) -> Option<Rect> {
    let min_x = a.origin.x.max(b.origin.x);
    let min_y = a.origin.y.max(b.origin.y);
    let max_x = (a.origin.x + a.size.width).min(b.origin.x + b.size.width);
    let max_y = (a.origin.y + a.size.height).min(b.origin.y + b.size.height);
    let width = max_x - min_x;
    let height = max_y - min_y;

    (width > 0.0 && height > 0.0).then_some(Rect {
        origin: Point { x: min_x, y: min_y },
        size: Size { width, height },
    })
}

fn rect_union(a: Rect, b: Rect) -> Rect {
    let min_x = a.origin.x.min(b.origin.x);
    let min_y = a.origin.y.min(b.origin.y);
    let max_x = (a.origin.x + a.size.width).max(b.origin.x + b.size.width);
    let max_y = (a.origin.y + a.size.height).max(b.origin.y + b.size.height);
    Rect {
        origin: Point { x: min_x, y: min_y },
        size: Size {
            width: (max_x - min_x).max(0.0),
            height: (max_y - min_y).max(0.0),
        },
    }
}

fn safe_zero_rect_inside(bounds: Rect) -> Rect {
    let max_x = bounds.origin.x + bounds.size.width.max(0.0);
    let max_y = bounds.origin.y + bounds.size.height.max(0.0);
    Rect {
        origin: Point {
            x: bounds.origin.x.clamp(bounds.origin.x, max_x),
            y: bounds.origin.y.clamp(bounds.origin.y, max_y),
        },
        size: Size {
            width: 0.0,
            height: 0.0,
        },
    }
}

fn view_requires_font_installation(view: &ViewDefinition) -> bool {
    view.paint.iter().any(paint_op_requires_font_installation)
        || view.focus_regions.iter().any(|focus| {
            focus.text_edit.as_ref().is_some_and(|text_edit| {
                text_edit.typography.source.is_some()
                    || text_font_installation_required(
                        &text_edit.buffer.text,
                        &text_edit.typography.style,
                    )
            })
        })
}

fn paint_op_requires_font_installation(op: &PaintOp) -> bool {
    match op {
        PaintOp::Text { content, style, .. } => text_font_installation_required(content, style),
        PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
            ops.iter().any(paint_op_requires_font_installation)
        }
        PaintOp::Fill { .. } | PaintOp::Stroke { .. } => false,
    }
}

fn text_font_installation_required(content: &str, style: &TextStyle) -> bool {
    text_style_requires_font_installation(style) || text_requires_cjk_font_evidence(content)
}

fn text_style_requires_font_installation(style: &TextStyle) -> bool {
    !matches!(
        style.font_family.trim().to_ascii_lowercase().as_str(),
        "" | "system-ui" | "sans-serif" | "serif" | "monospace"
    )
}

fn text_requires_cjk_font_evidence(content: &str) -> bool {
    content.chars().any(|ch| {
        matches!(
            ch as u32,
            0x1100..=0x11FF
                | 0x2E80..=0x2EFF
                | 0x2F00..=0x2FDF
                | 0x3040..=0x30FF
                | 0x3130..=0x318F
                | 0x31F0..=0x31FF
                | 0x3400..=0x4DBF
                | 0x4E00..=0x9FFF
                | 0xAC00..=0xD7AF
                | 0xF900..=0xFAFF
        )
    })
}

fn paint_op_uses_shape_path_or_clip(op: &PaintOp) -> bool {
    match op {
        PaintOp::Fill { shape, .. } | PaintOp::Stroke { shape, .. } => {
            shape.path.is_some() || shape.clip.is_some()
        }
        PaintOp::Group { clip, ops, .. } | PaintOp::Layer { clip, ops, .. } => {
            clip.is_some() || ops.iter().any(paint_op_uses_shape_path_or_clip)
        }
        PaintOp::Text { .. } => false,
    }
}

fn egui_position(position: egui::Pos2, origin: egui::Pos2) -> Point {
    Point {
        x: position.x - origin.x,
        y: position.y - origin.y,
    }
}

fn egui_view_root_local_position(
    position: egui::Pos2,
    origin: egui::Pos2,
    frame: &FrameIdentity,
) -> Point {
    let position = egui_position(position, origin);
    Point {
        x: position.x + frame.viewport.origin.x,
        y: position.y + frame.viewport.origin.y,
    }
}

fn egui_region_root_local_position(
    context: &EguiInputContext<'_>,
    region: &EguiPresentedRegion,
    position: egui::Pos2,
) -> Point {
    let target_rect = context
        .geometry_index
        .target_rect_for_region_address(&region.target, region.address.as_ref());
    let target_local_position = egui_position(position, region.target_origin);
    Point {
        x: target_rect.origin.x + target_local_position.x,
        y: target_rect.origin.y + target_local_position.y,
    }
}

fn egui_frame_identity(ui: &egui::Ui, widget_id: &WidgetId, viewport: Rect) -> FrameIdentity {
    FrameIdentity {
        surface_id: "slipway-egui".to_string(),
        surface_instance_id: widget_id.as_str().to_string(),
        revision: ui.ctx().cumulative_pass_nr(),
        frame_index: ui.ctx().cumulative_frame_nr(),
        viewport,
    }
}

#[derive(Clone, Debug)]
struct EguiAuthoredChildSlot {
    child: WidgetId,
    slot: WidgetSlotAddress,
}

#[derive(Clone, Debug, Default)]
struct EguiChildAssembly {
    regions: Vec<EguiPresentedRegion>,
    refused_admissions: Vec<BackendParityAdmission>,
    claimed_slots: Vec<WidgetSlotAddress>,
    presented_slots: Vec<WidgetSlotAddress>,
    paint_jobs: Vec<EguiPaintJob>,
    scroll_indicators: Vec<EguiScrollIndicatorPaint>,
    input_events: Vec<BackendInputEvent>,
    state: Vec<StateObservation>,
    diagnostics: Vec<Diagnostic>,
}

impl EguiChildAssembly {
    fn extend(&mut self, other: EguiChildAssembly) {
        self.regions.extend(other.regions);
        self.refused_admissions.extend(other.refused_admissions);
        self.claimed_slots.extend(other.claimed_slots);
        self.presented_slots.extend(other.presented_slots);
        self.paint_jobs.extend(other.paint_jobs);
        self.scroll_indicators.extend(other.scroll_indicators);
        self.input_events.extend(other.input_events);
        self.state.extend(other.state);
        self.diagnostics.extend(other.diagnostics);
    }
}

#[derive(Clone, Debug)]
struct EguiPaintJob {
    unit: PaintUnit,
    origin: egui::Pos2,
    clip_rect: egui::Rect,
}

#[derive(Clone, Debug)]
struct EguiScrollIndicatorPaint {
    viewport_rect: egui::Rect,
    scroll: ScrollRegionDeclaration,
    offset: Point,
}

#[derive(Clone, Debug)]
#[allow(dead_code)]
struct EguiPaintFlushRecord {
    target: WidgetId,
    sort_key: (i32, usize, usize),
    layer_id: egui::LayerId,
    clip_rect: egui::Rect,
}

#[derive(Clone, Debug, Default)]
struct EguiScrollAllocation {
    region: Option<EguiPresentedRegion>,
    child_assembly: EguiChildAssembly,
}

struct EguiAuthoredChildSlotCollector {
    slots: Vec<EguiAuthoredChildSlot>,
}

impl<ExternalState, AppMessage> SlipwayEguiWidgetListVisitor<ExternalState, AppMessage>
    for EguiAuthoredChildSlotCollector
{
    fn visit_egui_child<C>(
        &mut self,
        widget: &C,
        _external: &ExternalState,
        _local: &C::LocalState,
        slot: WidgetSlotAddress,
    ) where
        C: SlipwayEguiBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        self.slots.push(EguiAuthoredChildSlot {
            child: widget.id(),
            slot,
        });
    }

    fn visit_egui_native_child<N>(
        &mut self,
        widget: &N,
        _external: &ExternalState,
        _local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayEguiNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        self.slots.push(EguiAuthoredChildSlot {
            child: widget.id(),
            slot,
        });
    }
}

fn authored_child_slots<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
) -> Vec<EguiAuthoredChildSlot>
where
    W: SlipwayEguiBackendChildWidget,
{
    let root_slot = WidgetSlotAddress::new(widget.id(), 0);
    authored_child_slots_mounted(widget, external, local, Some(&root_slot))
}

fn authored_child_slots_mounted<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    mounted_parent: Option<&WidgetSlotAddress>,
) -> Vec<EguiAuthoredChildSlot>
where
    W: SlipwayEguiBackendChildWidget,
{
    let mut collector = EguiAuthoredChildSlotCollector { slots: Vec::new() };
    visit_egui_authored_children_mounted(widget, external, local, mounted_parent, &mut collector);
    collector.slots
}

struct EguiAuthoredChildPresenter<'a> {
    ui: &'a mut egui::Ui,
    parent_view: &'a ViewDefinition,
    view_origin: egui::Pos2,
    skipped_slots: &'a [WidgetSlotAddress],
    parent_geometry_index: &'a PresentationGeometryIndex,
    scroll: Option<&'a ScrollRegionDeclaration>,
    native_physical_operation: Option<&'a DebugPhysicalControl>,
    timing_samples: Option<&'a mut Vec<EguiFrameTimingSample>>,
    output: EguiChildAssembly,
}

impl<ExternalState, AppMessage> SlipwayEguiWidgetListVisitor<ExternalState, AppMessage>
    for EguiAuthoredChildPresenter<'_>
{
    fn visit_egui_child<C>(
        &mut self,
        widget: &C,
        external: &ExternalState,
        local: &C::LocalState,
        slot: WidgetSlotAddress,
    ) where
        C: SlipwayEguiBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        if self.skipped_slots.iter().any(|skipped| skipped == &slot) {
            return;
        }

        let Some(placement) = self
            .parent_view
            .layout
            .child_placements()
            .iter()
            .find(|placement| child_placement_matches_slot(placement, &widget.id(), &slot))
        else {
            self.output
                .refused_admissions
                .push(child_without_placement_refusal(widget.id(), &slot, false));
            return;
        };

        if let Some(scroll) = self.scroll {
            if !scroll_owns_placement_with_geometry_index(
                self.parent_geometry_index,
                scroll,
                placement,
            ) {
                return;
            }
            if !scroll_contains_placement_with_geometry_index(
                self.parent_geometry_index,
                scroll,
                placement,
            ) {
                self.output.claimed_slots.push(slot);
                return;
            }
        }

        present_egui_child(
            self.ui,
            widget,
            external,
            local,
            placement.clone(),
            slot,
            self.view_origin,
            &mut self.output,
            self.native_physical_operation,
            self.timing_samples.as_deref_mut(),
        );
    }

    fn visit_egui_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayEguiNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        if self.skipped_slots.iter().any(|skipped| skipped == &slot) {
            return;
        }

        let Some(placement) = self
            .parent_view
            .layout
            .child_placements()
            .iter()
            .find(|placement| child_placement_matches_slot(placement, &widget.id(), &slot))
        else {
            self.output
                .refused_admissions
                .push(child_without_placement_refusal(widget.id(), &slot, true));
            return;
        };

        if let Some(scroll) = self.scroll {
            if !scroll_owns_placement_with_geometry_index(
                self.parent_geometry_index,
                scroll,
                placement,
            ) {
                return;
            }
            if !scroll_contains_placement_with_geometry_index(
                self.parent_geometry_index,
                scroll,
                placement,
            ) {
                self.output.claimed_slots.push(slot);
                return;
            }
        }

        present_egui_native_child(
            self.ui,
            widget,
            external,
            local,
            placement.clone(),
            slot,
            self.view_origin,
            &mut self.output,
            self.timing_samples.as_deref_mut(),
        );
    }
}

#[cfg(test)]
fn collect_authored_children<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    parent_view: &ViewDefinition,
    parent_geometry_index: &PresentationGeometryIndex,
    view_origin: egui::Pos2,
    skipped_slots: &[WidgetSlotAddress],
    scroll: Option<&ScrollRegionDeclaration>,
    native_physical_operation: Option<&DebugPhysicalControl>,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) -> EguiChildAssembly
where
    W: SlipwayEguiBackendChildWidget,
{
    let root_slot = WidgetSlotAddress::new(widget.id(), 0);
    collect_authored_children_mounted(
        ui,
        widget,
        external,
        local,
        parent_view,
        Some(&root_slot),
        parent_geometry_index,
        view_origin,
        skipped_slots,
        scroll,
        native_physical_operation,
        timing_samples,
    )
}

fn collect_authored_children_mounted<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    parent_view: &ViewDefinition,
    mounted_parent: Option<&WidgetSlotAddress>,
    parent_geometry_index: &PresentationGeometryIndex,
    view_origin: egui::Pos2,
    skipped_slots: &[WidgetSlotAddress],
    scroll: Option<&ScrollRegionDeclaration>,
    native_physical_operation: Option<&DebugPhysicalControl>,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) -> EguiChildAssembly
where
    W: SlipwayEguiBackendChildWidget,
{
    let mut presenter = EguiAuthoredChildPresenter {
        ui,
        parent_view,
        parent_geometry_index,
        view_origin,
        skipped_slots,
        scroll,
        native_physical_operation,
        timing_samples,
        output: EguiChildAssembly::default(),
    };
    visit_egui_authored_children_in_paint_order_mounted(
        widget,
        external,
        local,
        parent_view,
        mounted_parent,
        &mut presenter,
    );
    presenter.output
}

#[cfg(test)]
fn present_authored_children<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    parent_view: &ViewDefinition,
    parent_geometry_index: &PresentationGeometryIndex,
    view_origin: egui::Pos2,
    skipped_slots: &[WidgetSlotAddress],
    scroll: Option<&ScrollRegionDeclaration>,
    native_physical_operation: Option<&DebugPhysicalControl>,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) -> EguiChildAssembly
where
    W: SlipwayEguiBackendChildWidget,
{
    let root_slot = WidgetSlotAddress::new(widget.id(), 0);
    present_authored_children_mounted(
        ui,
        widget,
        external,
        local,
        parent_view,
        Some(&root_slot),
        parent_geometry_index,
        view_origin,
        skipped_slots,
        scroll,
        native_physical_operation,
        timing_samples,
    )
}

fn present_authored_children_mounted<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    parent_view: &ViewDefinition,
    mounted_parent: Option<&WidgetSlotAddress>,
    parent_geometry_index: &PresentationGeometryIndex,
    view_origin: egui::Pos2,
    skipped_slots: &[WidgetSlotAddress],
    scroll: Option<&ScrollRegionDeclaration>,
    native_physical_operation: Option<&DebugPhysicalControl>,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) -> EguiChildAssembly
where
    W: SlipwayEguiBackendChildWidget,
{
    let mut timing_samples = timing_samples;
    let mut output = collect_authored_children_mounted(
        ui,
        widget,
        external,
        local,
        parent_view,
        mounted_parent,
        parent_geometry_index,
        view_origin,
        skipped_slots,
        scroll,
        native_physical_operation,
        timing_samples.as_deref_mut(),
    );
    let paint_local_start = Instant::now();
    let paint_records = paint_local_egui_jobs(ui, &mut output.paint_jobs);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.paint_local_jobs",
        paint_local_start.elapsed(),
        paint_records.len(),
    );
    let indicator_start = Instant::now();
    paint_declared_scroll_indicators(ui, &mut output.scroll_indicators);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.paint_scroll_indicators",
        indicator_start.elapsed(),
        output.scroll_indicators.len(),
    );
    output
}

fn paint_local_egui_jobs(ui: &egui::Ui, jobs: &mut Vec<EguiPaintJob>) -> Vec<EguiPaintFlushRecord> {
    let mut local_jobs = Vec::new();
    let mut explicit_jobs = Vec::new();
    for job in jobs.drain(..) {
        if job.unit.order.mode == PaintOrderMode::ExplicitLayered {
            explicit_jobs.push(job);
        } else {
            local_jobs.push(job);
        }
    }
    let records = paint_egui_jobs(ui, &mut local_jobs);
    *jobs = explicit_jobs;
    records
}

#[cfg(test)]
fn push_expanded_egui_paint_jobs(
    jobs: &mut Vec<EguiPaintJob>,
    unit: PaintUnit,
    origin: egui::Pos2,
    clip_rect: egui::Rect,
) {
    jobs.extend(
        expand_paint_unit_layers(unit)
            .into_iter()
            .map(|unit| EguiPaintJob {
                unit,
                origin,
                clip_rect,
            }),
    );
}

fn paint_egui_default_jobs_and_push_explicit_layer_jobs(
    ui: &egui::Ui,
    jobs: &mut Vec<EguiPaintJob>,
    unit: PaintUnit,
    origin: egui::Pos2,
    clip_rect: egui::Rect,
) -> Vec<EguiPaintFlushRecord> {
    let mut local_jobs = Vec::new();
    let unit_contains_extracted_layers = paint_ops_contain_layer(&unit.paint);
    for unit in expand_paint_unit_layers(unit) {
        let job = EguiPaintJob {
            unit,
            origin,
            clip_rect,
        };
        if paint_job_requires_surface_global_flush(&job, unit_contains_extracted_layers) {
            jobs.push(job);
        } else {
            local_jobs.push(job);
        }
    }
    paint_egui_jobs(ui, &mut local_jobs)
}

fn paint_job_requires_surface_global_flush(
    job: &EguiPaintJob,
    unit_contains_extracted_layers: bool,
) -> bool {
    paint_job_contains_expanded_layer(job)
        || (!unit_contains_extracted_layers
            && job.unit.order.mode == PaintOrderMode::ExplicitLayered)
}

fn paint_job_contains_expanded_layer(job: &EguiPaintJob) -> bool {
    paint_ops_contain_layer(&job.unit.paint)
}

fn paint_ops_contain_layer(ops: &[PaintOp]) -> bool {
    ops.iter().any(|op| match op {
        PaintOp::Layer { .. } => true,
        PaintOp::Group { ops, .. } => paint_ops_contain_layer(ops),
        PaintOp::Fill { .. } | PaintOp::Stroke { .. } | PaintOp::Text { .. } => false,
    })
}

fn expanded_paint_unit_sort_key(unit: PaintUnit) -> (i32, usize, usize) {
    expand_paint_unit_layers(unit)
        .iter()
        .map(paint_unit_sort_key)
        .max()
        .unwrap_or((0, 0, 0))
}

fn paint_egui_jobs(ui: &egui::Ui, jobs: &mut Vec<EguiPaintJob>) -> Vec<EguiPaintFlushRecord> {
    jobs.sort_by_key(|job| paint_unit_sort_key(&job.unit));
    let mut records = Vec::with_capacity(jobs.len());
    for job in jobs.drain(..) {
        let layer_id = egui_paint_job_layer_id(ui, &job);
        let painter = egui_paint_job_painter(ui, &job, layer_id);
        records.push(EguiPaintFlushRecord {
            target: job.unit.target.clone(),
            sort_key: paint_unit_sort_key(&job.unit),
            layer_id,
            clip_rect: job.clip_rect,
        });
        for op in &job.unit.paint {
            paint_op(&painter, job.origin, op);
        }
    }
    records
}

fn egui_paint_job_painter(
    ui: &egui::Ui,
    job: &EguiPaintJob,
    layer_id: egui::LayerId,
) -> egui::Painter {
    let has_declared_overflow = job.unit.order.overflow_bounds.is_some();
    if job.unit.order.mode == PaintOrderMode::ExplicitLayered || has_declared_overflow {
        ui.ctx()
            .layer_painter(layer_id)
            .with_clip_rect(job.clip_rect)
    } else {
        ui.painter_at(job.clip_rect)
    }
}

fn egui_paint_job_layer_id(ui: &egui::Ui, job: &EguiPaintJob) -> egui::LayerId {
    if job.unit.order.mode != PaintOrderMode::ExplicitLayered {
        return ui.layer_id();
    }

    egui::LayerId::new(
        egui_order_for_slipway_layer(job.unit.order.z_index),
        egui::Id::new((
            "slipway-explicit-paint-layer",
            egui_order_key(egui_order_for_slipway_layer(job.unit.order.z_index)),
        )),
    )
}

fn egui_order_for_slipway_layer(z_index: i32) -> egui::Order {
    if z_index < 0 {
        egui::Order::Background
    } else if z_index == 0 {
        egui::Order::Middle
    } else {
        egui::Order::Foreground
    }
}

fn egui_order_key(order: egui::Order) -> &'static str {
    match order {
        egui::Order::Background => "background",
        egui::Order::Middle => "middle",
        egui::Order::Foreground => "foreground",
        egui::Order::Tooltip => "tooltip",
        egui::Order::Debug => "debug",
    }
}

fn present_egui_child<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    placement: ChildPlacement,
    slot: WidgetSlotAddress,
    view_origin: egui::Pos2,
    output: &mut EguiChildAssembly,
    native_physical_operation: Option<&DebugPhysicalControl>,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) where
    W: SlipwayEguiBackendChildWidget,
{
    let mut timing_samples = timing_samples;
    let view_definition_start = Instant::now();
    let layout_input = child_layout_input_for_placement(&placement);
    let frame = egui_frame_identity(ui, &widget.id(), layout_input.viewport.into_rect());
    let view = widget.visible_backend_view_definition(
        external,
        local,
        ViewDefinitionInput::new(frame, layout_input),
    );
    let mut view = mount_presented_child_view_addresses(view, &slot);
    let child_rect = egui_rect(view_origin, placement.bounds.into_rect());
    let prepared_geometry = match validate_and_index_view(&view) {
        Ok(prepared) => prepared,
        Err(diagnostics) => {
            view.diagnostics.extend(diagnostics);
            let admission = egui_backend_admission()
                .admit_view_definition_with_capabilities(&widget.capabilities(), &view);
            paint_visible_admission_refusal(ui, child_rect, &admission);
            output.refused_admissions.push(admission);
            return;
        }
    };
    let geometry_index = &prepared_geometry.index;
    normalize_egui_visible_scroll_regions(&mut view, geometry_index);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.view_definition",
        view_definition_start.elapsed(),
        1,
    );
    let response_start = Instant::now();
    let region_id =
        PresentationRegionId::from(format!("egui-child-response:{}", widget_slot_key(&slot)));
    let response = apply_region_cursor(
        ui.interact(
            child_rect,
            egui_region_id(
                EguiPresentedRegionKind::Hit,
                &region_id,
                &widget.id(),
                Some(&slot),
            ),
            egui::Sense::click(),
        ),
        CursorCapability::Default,
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.response_region",
        response_start.elapsed(),
        1,
    );

    let admission_start = Instant::now();
    let capabilities = widget.capabilities();
    let admission =
        egui_backend_admission().admit_view_definition_with_capabilities(&capabilities, &view);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.admission",
        admission_start.elapsed(),
        if admission.accepted { 1 } else { 0 },
    );
    if !admission.accepted {
        paint_visible_admission_refusal(ui, child_rect, &admission);
        output.refused_admissions.push(admission);
        return;
    }

    let font_install_start = Instant::now();
    let font_admissions = install_declared_fonts(ui, widget, external, local, &view);
    let font_refusal_count = font_admissions.len();
    output.refused_admissions.extend(font_admissions);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.font_install",
        font_install_start.elapsed(),
        font_refusal_count,
    );
    output.claimed_slots.push(slot.clone());
    output.presented_slots.push(slot.clone());
    let paint_clip = egui_view_paint_clip_rect(child_rect.min, child_rect, &view);
    let mut unit = PaintUnit::from_view_ref(&view, slot.ordinal);
    unit.address = Some(slot.clone());
    let child_response_sort_key = paint_unit_sort_key(&unit);
    let collect_slots_start = Instant::now();
    let child_slots = authored_child_slots_mounted(widget, external, local, Some(&slot));
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.collect_nested_children",
        collect_slots_start.elapsed(),
        child_slots.len(),
    );
    output.regions.push(child_response_region(
        widget.id(),
        slot.clone(),
        placement.bounds.into_rect(),
        response.clone(),
        child_response_sort_key,
    ));

    let first_job = output.paint_jobs.len();
    let paint_start = Instant::now();
    let paint_records = paint_egui_default_jobs_and_push_explicit_layer_jobs(
        ui,
        &mut output.paint_jobs,
        unit,
        child_rect.min,
        paint_clip,
    );
    let queued_explicit_jobs = output.paint_jobs.len().saturating_sub(first_job);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.paint_local_jobs",
        paint_start.elapsed(),
        paint_records.len() + queued_explicit_jobs,
    );
    let occlusion_start = Instant::now();
    let occlusion_regions = allocate_paint_occlusion_regions(ui, &output.paint_jobs[first_job..]);
    let occlusion_region_count = occlusion_regions.len();
    output.regions.extend(occlusion_regions);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.paint_occlusion_regions",
        occlusion_start.elapsed(),
        occlusion_region_count,
    );

    let mut nested_assembly = EguiChildAssembly::default();
    let nested_regions_start = Instant::now();
    let nested_regions = allocate_presentation_regions_with_timing(
        ui,
        widget,
        external,
        local,
        Some(&slot),
        child_rect.min,
        &view,
        geometry_index,
        &child_slots,
        &mut nested_assembly,
        native_physical_operation,
        timing_samples.as_deref_mut(),
        Some(EGUI_CHILD_PRESENTATION_REGION_TIMING_LABELS),
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.presentation_regions",
        nested_regions_start.elapsed(),
        nested_regions.len(),
    );
    output.regions.extend(nested_regions);
    let nested_collect_start = Instant::now();
    let nested_authored = collect_authored_children_mounted(
        ui,
        widget,
        external,
        local,
        &view,
        Some(&slot),
        geometry_index,
        child_rect.min,
        &nested_assembly.claimed_slots,
        None,
        native_physical_operation,
        timing_samples.as_deref_mut(),
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.collect_nested_children",
        nested_collect_start.elapsed(),
        nested_authored.presented_slots.len(),
    );
    nested_assembly.extend(nested_authored);
    output.extend(nested_assembly);
}

fn present_egui_native_child<N>(
    ui: &mut egui::Ui,
    widget: &N,
    external: &N::ExternalState,
    local: &N::LocalState,
    placement: ChildPlacement,
    slot: WidgetSlotAddress,
    view_origin: egui::Pos2,
    output: &mut EguiChildAssembly,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) where
    N: SlipwayEguiNativeChildWidget,
{
    let mut timing_samples = timing_samples;
    let view_definition_start = Instant::now();
    let layout_input = child_layout_input_for_placement(&placement);
    let frame = egui_frame_identity(ui, &widget.id(), layout_input.viewport.into_rect());
    let view = widget.visible_backend_view_definition(
        external,
        local,
        ViewDefinitionInput::new(frame.clone(), layout_input),
    );
    let mut view = mount_presented_child_view_addresses(view, &slot);
    let child_rect = egui_rect(view_origin, placement.bounds.into_rect());
    let prepared_geometry = match validate_and_index_view(&view) {
        Ok(prepared) => prepared,
        Err(diagnostics) => {
            view.diagnostics.extend(diagnostics);
            let admission = egui_backend_admission()
                .admit_view_definition_with_capabilities(&widget.capabilities(), &view);
            paint_visible_admission_refusal(ui, child_rect, &admission);
            output.refused_admissions.push(admission);
            return;
        }
    };
    let geometry_index = &prepared_geometry.index;
    normalize_egui_visible_scroll_regions(&mut view, geometry_index);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.view_definition",
        view_definition_start.elapsed(),
        1,
    );

    let admission_start = Instant::now();
    let capabilities = widget.capabilities();
    let admission =
        egui_backend_admission().admit_view_definition_with_capabilities(&capabilities, &view);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.admission",
        admission_start.elapsed(),
        if admission.accepted { 1 } else { 0 },
    );
    if !admission.accepted {
        paint_visible_admission_refusal(ui, child_rect, &admission);
        output.refused_admissions.push(admission);
        return;
    }

    output.claimed_slots.push(slot.clone());
    output.presented_slots.push(slot.clone());
    let native_ui_start = Instant::now();
    let native_output = ui
        .scope_builder(
            egui::UiBuilder::new()
                .id_salt(("slipway-native-child", widget_slot_key(&slot)))
                .max_rect(child_rect),
            |ui| {
                widget.egui_native_ui(
                    ui,
                    external,
                    local,
                    EguiNativeWidgetContext {
                        slot: &slot,
                        frame: &frame,
                        placement,
                        rect: child_rect,
                    },
                )
            },
        )
        .inner;
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.native_ui",
        native_ui_start.elapsed(),
        1,
    );

    if native_output.request_repaint {
        ui.ctx().request_repaint();
    }
    output.input_events.extend(native_output.input_events);
    output.state.extend(native_output.state);
    output.diagnostics.extend(native_output.diagnostics);
}

fn allocate_presentation_regions<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    view_origin: egui::Pos2,
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    child_slots: &[EguiAuthoredChildSlot],
    child_assembly: &mut EguiChildAssembly,
    native_physical_operation: Option<&DebugPhysicalControl>,
) -> Vec<EguiPresentedRegion>
where
    W: SlipwayEguiBackendChildWidget,
{
    let root_slot = WidgetSlotAddress::new(widget.id(), 0);
    allocate_presentation_regions_with_timing(
        ui,
        widget,
        external,
        local,
        Some(&root_slot),
        view_origin,
        view,
        geometry_index,
        child_slots,
        child_assembly,
        native_physical_operation,
        None,
        None,
    )
}

fn allocate_presentation_regions_with_timing<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    mounted_parent: Option<&WidgetSlotAddress>,
    view_origin: egui::Pos2,
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    child_slots: &[EguiAuthoredChildSlot],
    child_assembly: &mut EguiChildAssembly,
    native_physical_operation: Option<&DebugPhysicalControl>,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
    timing_labels: Option<EguiPresentationRegionTimingLabels>,
) -> Vec<EguiPresentedRegion>
where
    W: SlipwayEguiBackendChildWidget,
{
    let mut timing_samples = timing_samples;
    let mut regions = Vec::new();
    let scroll_start = Instant::now();
    let mut scroll_region_count = 0;
    for scroll in &view.scroll_regions {
        if region_belongs_to_authored_child(&scroll.target, scroll.address.as_ref(), child_slots) {
            continue;
        }

        let scroll_allocation = allocate_scroll_region_with_skips(
            ui,
            widget,
            external,
            local,
            mounted_parent,
            view_origin,
            view,
            geometry_index,
            scroll,
            &child_assembly.claimed_slots,
            native_physical_operation,
            timing_samples.as_deref_mut(),
        );
        if let Some(region) = scroll_allocation.region {
            regions.push(region);
            scroll_region_count += 1;
        }
        child_assembly.extend(scroll_allocation.child_assembly);
    }
    if let Some(labels) = timing_labels {
        push_egui_frame_timing(
            &mut timing_samples,
            labels.scroll,
            scroll_start.elapsed(),
            scroll_region_count,
        );
    }

    let focus_start = Instant::now();
    let mut focus_region_count = 0;
    for focus in &view.focus_regions {
        if region_belongs_to_authored_child(&focus.target, focus.address.as_ref(), child_slots) {
            continue;
        }

        if let Some(text_edit) = &focus.text_edit {
            let (region, font_admissions) = allocate_text_edit_region(
                ui,
                widget,
                external,
                local,
                view_origin,
                view,
                geometry_index,
                focus,
                text_edit,
                timing_samples.as_deref_mut(),
            );
            child_assembly.refused_admissions.extend(font_admissions);
            regions.push(region);
            focus_region_count += 1;
        } else {
            regions.push(allocate_focus_region(
                ui,
                view_origin,
                view,
                geometry_index,
                focus,
            ));
            focus_region_count += 1;
        }
    }
    if let Some(labels) = timing_labels {
        push_egui_frame_timing(
            &mut timing_samples,
            labels.focus,
            focus_start.elapsed(),
            focus_region_count,
        );
    }

    let hit_start = Instant::now();
    let mut hit_regions = view.hit_regions.iter().collect::<Vec<_>>();
    hit_regions.sort_by_key(|region| {
        (
            region.order.z_index,
            region.order.paint_order,
            region.order.traversal_order,
        )
    });
    for hit in hit_regions {
        if region_belongs_to_authored_child(&hit.target, hit.address.as_ref(), child_slots) {
            continue;
        }

        regions.push(allocate_hit_region(
            ui,
            view_origin,
            view,
            geometry_index,
            hit,
        ));
    }
    if let Some(labels) = timing_labels {
        push_egui_frame_timing(
            &mut timing_samples,
            labels.hit,
            hit_start.elapsed(),
            regions
                .iter()
                .filter(|region| region.kind == EguiPresentedRegionKind::Hit)
                .count(),
        );
    }

    regions
}

fn allocate_hit_region(
    ui: &mut egui::Ui,
    view_origin: egui::Pos2,
    _view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    hit: &HitRegionDeclaration,
) -> EguiPresentedRegion {
    let id = egui_region_id(
        EguiPresentedRegionKind::Hit,
        &hit.id,
        &hit.target,
        hit.address.as_ref(),
    );
    let target_rect = slipway_core::declared_target_rect_for_region_address_with_geometry_index(
        geometry_index,
        &hit.target,
        hit.address.as_ref(),
    );
    let target_origin = egui_point(view_origin, target_rect.origin);
    let target_bounds = target_local_bounds(target_rect);
    let rect = egui_rect(target_origin, hit.bounds.into_rect());
    let sense = if hit.enabled {
        egui_sense_for_hit(hit)
    } else {
        egui::Sense::hover()
    };
    let response = apply_region_cursor(ui.interact(rect, id, sense), hit.cursor.clone());

    EguiPresentedRegion {
        kind: EguiPresentedRegionKind::Hit,
        region_id: hit.id.clone(),
        target: hit.target.clone(),
        address: hit.address.clone(),
        // For a HIT region this is the AUTHOR-DECLARED HitRegionOrder, not
        // a paint-unit allocation key — the occlusion filter
        // (`egui_occlusion_blocks_region`) relies on that (NC-2).
        paint_sort_key: (
            hit.order.z_index,
            hit.order.paint_order,
            hit.order.traversal_order,
        ),
        authored_z_order: None,
        event_target: hit
            .route
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| hit.target.clone()),
        event_target_slot: hit.route.address.clone().or_else(|| hit.address.clone()),
        declared_bounds: hit.bounds.into_rect(),
        target_origin,
        target_bounds,
        event_coordinate_space: hit.event_coordinate_space,
        response,
        cursor: hit.cursor.clone(),
        enabled: hit.enabled,
        text_edit_change: None,
        scroll_state: None,
        blocks_wheel: true,
    }
}

fn allocate_focus_region(
    ui: &mut egui::Ui,
    view_origin: egui::Pos2,
    _view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    focus: &FocusRegionDeclaration,
) -> EguiPresentedRegion {
    let id = egui_region_id(
        EguiPresentedRegionKind::Focus,
        &focus.id,
        &focus.target,
        focus.address.as_ref(),
    );
    let target_rect = slipway_core::declared_target_rect_for_region_address_with_geometry_index(
        geometry_index,
        &focus.target,
        focus.address.as_ref(),
    );
    let target_origin = egui_point(view_origin, target_rect.origin);
    let target_bounds = target_local_bounds(target_rect);
    let rect = egui_rect(target_origin, focus.bounds.into_rect());
    let sense = if focus.enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let response = apply_region_cursor(ui.interact(rect, id, sense), CursorCapability::Default);

    EguiPresentedRegion {
        kind: EguiPresentedRegionKind::Focus,
        region_id: focus.id.clone(),
        target: focus.target.clone(),
        address: focus.address.clone(),
        paint_sort_key: (0, 0, 0),
        authored_z_order: None,
        event_target: focus.target.clone(),
        event_target_slot: focus.address.clone(),
        declared_bounds: focus.bounds.into_rect(),
        target_origin,
        target_bounds,
        event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
        response,
        cursor: CursorCapability::Default,
        enabled: focus.enabled,
        text_edit_change: None,
        scroll_state: None,
        blocks_wheel: true,
    }
}

fn allocate_text_edit_region<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    view_origin: egui::Pos2,
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    focus: &FocusRegionDeclaration,
    text_edit: &TextEditRegionDeclaration,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) -> (EguiPresentedRegion, Vec<BackendParityAdmission>)
where
    W: SlipwayEguiBackendChildWidget,
{
    let mut timing_samples = timing_samples;
    let id = egui_text_edit_id(focus);
    let font_install_start = Instant::now();
    let font_admissions = install_text_edit_font(ui, widget, external, local, focus, text_edit);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.text_edit.font_install",
        font_install_start.elapsed(),
        font_admissions.len(),
    );
    (
        allocate_text_edit_region_without_font_policy(
            ui,
            view_origin,
            view,
            geometry_index,
            focus,
            text_edit,
            id,
            timing_samples.as_deref_mut(),
        ),
        font_admissions,
    )
}

fn egui_text_edit_id(focus: &FocusRegionDeclaration) -> egui::Id {
    egui_region_id(
        EguiPresentedRegionKind::TextEdit,
        &focus.id,
        &focus.target,
        focus.address.as_ref(),
    )
}

fn allocate_text_edit_region_without_font_policy(
    ui: &mut egui::Ui,
    view_origin: egui::Pos2,
    _view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    focus: &FocusRegionDeclaration,
    text_edit: &TextEditRegionDeclaration,
    id: egui::Id,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) -> EguiPresentedRegion {
    let mut timing_samples = timing_samples;
    let geometry_start = Instant::now();
    let target_rect = slipway_core::declared_target_rect_for_region_address_with_geometry_index(
        geometry_index,
        &focus.target,
        focus.address.as_ref(),
    );
    let target_origin = egui_point(view_origin, target_rect.origin);
    let target_bounds = target_local_bounds(target_rect);
    let rect = egui_rect(target_origin, focus.bounds.into_rect());
    let editable = focus.enabled && text_edit.selection.editable;
    let mut text = text_edit.buffer.text.clone();
    let before = text.clone();
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.text_edit.geometry",
        geometry_start.elapsed(),
        1,
    );
    let style_start = Instant::now();
    let value_color = egui_color(text_edit.visual_style.value_color);
    let background_color = egui_color(text_edit.visual_style.background_color);
    let border_color = egui_color(text_edit.visual_style.border_color);
    let border_width = text_edit.visual_style.border_width.max(0.0);
    let corner_radius = egui_corner_radius(text_edit.visual_style.border_radius);
    let frame = egui::Frame::new()
        .fill(background_color)
        .stroke(egui::Stroke::new(border_width, border_color))
        .corner_radius(corner_radius)
        .inner_margin(egui::Margin::symmetric(4, 2));
    let font_id = egui::FontId::new(
        egui_text_font_size(&text_edit.typography.style),
        egui_text_input_font_family(ui.ctx(), text_edit),
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.text_edit.style",
        style_start.elapsed(),
        1,
    );
    let widget_build_start = Instant::now();
    let text_widget = if matches!(text_edit.line_mode, slipway_core::TextLineMode::MultiLine) {
        egui::TextEdit::multiline(&mut text)
            .desired_width(rect.width())
            .font(font_id.clone())
            .background_color(background_color)
            .text_color(value_color)
            .frame(frame)
            .interactive(editable)
    } else {
        egui::TextEdit::singleline(&mut text)
            .desired_width(rect.width())
            .font(font_id)
            .background_color(background_color)
            .text_color(value_color)
            .frame(frame)
            .interactive(editable)
    };
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.text_edit.widget_build",
        widget_build_start.elapsed(),
        1,
    );
    let ui_put_start = Instant::now();
    let response = ui
        .scope(|ui| {
            apply_text_input_visuals_to_egui_scope(ui, &text_edit.visual_style);
            apply_region_cursor(ui.put(rect, text_widget.id(id)), CursorCapability::Text)
        })
        .inner;
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.text_edit.ui_put",
        ui_put_start.elapsed(),
        1,
    );
    let change_start = Instant::now();
    let text_edit_change = if response.changed() && before != text {
        Some(EguiTextEditChange {
            before,
            after: text,
            selection_before: text_edit.selection.selection.clone(),
            selection_after: None,
        })
    } else {
        None
    };
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.child.text_edit.change",
        change_start.elapsed(),
        if text_edit_change.is_some() { 1 } else { 0 },
    );

    EguiPresentedRegion {
        kind: EguiPresentedRegionKind::TextEdit,
        region_id: focus.id.clone(),
        target: focus.target.clone(),
        address: focus.address.clone(),
        paint_sort_key: (0, 0, 0),
        authored_z_order: None,
        event_target: focus.target.clone(),
        event_target_slot: focus.address.clone(),
        declared_bounds: focus.bounds.into_rect(),
        target_origin,
        target_bounds,
        event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
        response,
        cursor: CursorCapability::Text,
        enabled: focus.enabled,
        text_edit_change,
        scroll_state: None,
        blocks_wheel: true,
    }
}

fn allocate_scroll_region_with_skips<W>(
    ui: &mut egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    mounted_parent: Option<&WidgetSlotAddress>,
    view_origin: egui::Pos2,
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    scroll: &ScrollRegionDeclaration,
    skipped_slots: &[WidgetSlotAddress],
    native_physical_operation: Option<&DebugPhysicalControl>,
    timing_samples: Option<&mut Vec<EguiFrameTimingSample>>,
) -> EguiScrollAllocation
where
    W: SlipwayEguiBackendChildWidget,
{
    let mut timing_samples = timing_samples;
    if !scroll.enabled {
        return EguiScrollAllocation::default();
    }

    let id = egui_region_id(
        EguiPresentedRegionKind::Scroll,
        &scroll.id,
        &scroll.target,
        scroll.address.as_ref(),
    );
    let target_rect = slipway_core::declared_target_rect_for_region_address_with_geometry_index(
        geometry_index,
        &scroll.target,
        scroll.address.as_ref(),
    );
    let target_origin = egui_point(view_origin, target_rect.origin);
    let target_bounds = target_local_bounds(target_rect);
    let viewport_rect = egui_rect(target_origin, scroll.viewport.into_rect());
    let interact_start = Instant::now();
    let response = ui.interact(
        viewport_rect,
        id,
        if scroll.consumption.drag {
            egui::Sense::drag()
        } else {
            egui::Sense::hover()
        },
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.widget.presentation_regions.scroll.interact",
        interact_start.elapsed(),
        1,
    );
    let offset_start = Instant::now();
    let native_scroll_offset =
        egui_native_scroll_offset_for_operation(native_physical_operation, geometry_index, scroll);
    let scroll_offset = clamp_declared_scroll_offset(
        native_scroll_offset.unwrap_or(scroll.offset),
        scroll.axes,
        scroll.viewport.into_rect(),
        scroll.content_bounds.into_rect(),
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.widget.presentation_regions.scroll.offset",
        offset_start.elapsed(),
        1,
    );
    let mut child_assembly = EguiChildAssembly::default();
    let content_origin =
        declared_scroll_content_origin(view_origin, target_rect, scroll, scroll_offset);
    let content_rect = declared_scroll_content_rect(content_origin, scroll);
    let mut effective_scroll = scroll.clone();
    effective_scroll.offset = scroll_offset;
    let child_assembly_output = &mut child_assembly;
    let child_scope_start = Instant::now();
    let mut child_present_elapsed = Duration::ZERO;
    ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(("slipway-scroll-scope", scroll.id.as_str()))
            .max_rect(content_rect),
        |content_ui| {
            let child_present_start = Instant::now();
            child_assembly_output.extend(present_authored_children_mounted(
                content_ui,
                widget,
                external,
                local,
                view,
                mounted_parent,
                geometry_index,
                content_origin,
                skipped_slots,
                Some(&effective_scroll),
                native_physical_operation,
                timing_samples.as_deref_mut(),
            ));
            child_present_elapsed = child_present_start.elapsed();
            content_ui.allocate_space(content_rect.size());
        },
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.widget.presentation_regions.scroll.children",
        child_present_elapsed,
        child_assembly.presented_slots.len(),
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.widget.presentation_regions.scroll.claimed_children",
        Duration::ZERO,
        child_assembly.claimed_slots.len(),
    );
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.widget.presentation_regions.scroll.scope",
        child_scope_start.elapsed(),
        child_assembly.presented_slots.len(),
    );
    let clip_start = Instant::now();
    clip_declared_scroll_child_assembly(viewport_rect, &mut child_assembly);
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.widget.presentation_regions.scroll.clip",
        clip_start.elapsed(),
        child_assembly.regions.len(),
    );
    let indicator_start = Instant::now();
    // Declared-indicator honor (Step 210, `ScrollIndicatorMode`): `Hidden`
    // never queues indicator paint; `Auto` and `Visible` queue it and
    // `paint_declared_scroll_indicator` applies the geometric conditions
    // (vertical axis, content taller than viewport) — byte-identical to the
    // pre-control behavior for `Auto`.
    let indicator_queued = scroll.indicator != slipway_core::ScrollIndicatorMode::Hidden;
    if indicator_queued {
        child_assembly
            .scroll_indicators
            .push(EguiScrollIndicatorPaint {
                viewport_rect,
                scroll: scroll.clone(),
                offset: scroll_offset,
            });
    }
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.widget.presentation_regions.scroll.indicator",
        indicator_start.elapsed(),
        usize::from(indicator_queued),
    );

    let state_start = Instant::now();
    let scroll_state = EguiScrollRegionState {
        declared_offset: scroll.offset,
        egui_offset: scroll_offset,
        content_size: scroll.content_bounds.size,
        inner_rect: Rect {
            origin: egui_position(viewport_rect.min, view_origin),
            size: scroll.viewport.size,
        },
        // Interactive-indicator hit geometry: present exactly when the
        // indicator paint was queued above (same gate, same geometry fn).
        indicator: if indicator_queued {
            declared_scroll_indicator_geometry(viewport_rect, scroll, scroll_offset)
        } else {
            None
        },
    };
    push_egui_frame_timing(
        &mut timing_samples,
        "egui.widget.presentation_regions.scroll.state",
        state_start.elapsed(),
        1,
    );
    EguiScrollAllocation {
        region: Some(EguiPresentedRegion {
            kind: EguiPresentedRegionKind::Scroll,
            region_id: scroll.id.clone(),
            target: scroll.target.clone(),
            address: scroll.address.clone(),
            paint_sort_key: (
                scroll.order.z_index,
                scroll.order.paint_order,
                scroll.order.traversal_order,
            ),
            authored_z_order: None,
            event_target: scroll.target.clone(),
            event_target_slot: scroll.address.clone(),
            declared_bounds: scroll.viewport.into_rect(),
            target_origin,
            target_bounds,
            event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
            response,
            cursor: CursorCapability::Default,
            enabled: scroll.enabled,
            text_edit_change: None,
            scroll_state: Some(scroll_state),
            blocks_wheel: true,
        }),
        child_assembly,
    }
}

fn clip_declared_scroll_child_assembly(
    viewport_rect: egui::Rect,
    child_assembly: &mut EguiChildAssembly,
) {
    for job in &mut child_assembly.paint_jobs {
        if job.unit.order.overflow_bounds.is_none() {
            job.clip_rect = job.clip_rect.intersect(viewport_rect);
        }
    }
    for region in &mut child_assembly.regions {
        if region.paint_sort_key.0 <= 0 {
            region.response.interact_rect = region.response.interact_rect.intersect(viewport_rect);
        }
    }
}

fn clamp_declared_scroll_offset(
    offset: Point,
    axes: ScrollAxes,
    viewport: Rect,
    content_bounds: Rect,
) -> Point {
    let max_x = (content_bounds.size.width - viewport.size.width).max(0.0);
    let max_y = (content_bounds.size.height - viewport.size.height).max(0.0);
    Point {
        x: if axes.horizontal {
            offset.x.clamp(0.0, max_x)
        } else {
            0.0
        },
        y: if axes.vertical {
            offset.y.clamp(0.0, max_y)
        } else {
            0.0
        },
    }
}

fn declared_scroll_content_origin(
    view_origin: egui::Pos2,
    target_rect: Rect,
    scroll: &ScrollRegionDeclaration,
    scroll_offset: Point,
) -> egui::Pos2 {
    let viewport = scroll.viewport.into_rect();
    let content_bounds = scroll.content_bounds.into_rect();
    egui::pos2(
        view_origin.x + target_rect.origin.x + viewport.origin.x
            - content_bounds.origin.x
            - scroll_offset.x,
        view_origin.y + target_rect.origin.y + viewport.origin.y
            - content_bounds.origin.y
            - scroll_offset.y,
    )
}

fn declared_scroll_content_rect(
    content_origin: egui::Pos2,
    scroll: &ScrollRegionDeclaration,
) -> egui::Rect {
    egui::Rect::from_min_size(
        content_origin,
        egui::vec2(
            scroll.content_bounds.size.width.max(0.0),
            scroll.content_bounds.size.height.max(0.0),
        ),
    )
}

fn child_layout_input_for_placement(placement: &ChildPlacement) -> LayoutInput {
    slipway_core::child_layout_input_for_placement(placement)
}

#[cfg(test)]
fn child_layout_input(bounds: Rect) -> LayoutInput {
    let viewport = TargetLocalRect::new(Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: bounds.size,
    });
    LayoutInput {
        viewport,
        content: viewport,
        constraints: LayoutConstraints {
            min: Size {
                width: 0.0,
                height: 0.0,
            },
            max: bounds.size,
        },
    }
}

fn child_response_region(
    target: WidgetId,
    slot: WidgetSlotAddress,
    bounds: Rect,
    response: egui::Response,
    paint_sort_key: (i32, usize, usize),
) -> EguiPresentedRegion {
    EguiPresentedRegion {
        kind: EguiPresentedRegionKind::Hit,
        region_id: PresentationRegionId::from(format!(
            "egui-child-response:{}",
            widget_slot_key(&slot)
        )),
        target: target.clone(),
        address: Some(slot.clone()),
        paint_sort_key,
        authored_z_order: None,
        event_target: target,
        event_target_slot: Some(slot),
        declared_bounds: bounds,
        target_origin: response.interact_rect.min,
        target_bounds: Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: bounds.size,
        },
        event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
        response,
        cursor: CursorCapability::Default,
        enabled: true,
        text_edit_change: None,
        scroll_state: None,
        blocks_wheel: true,
    }
}

fn allocate_paint_occlusion_regions(
    ui: &mut egui::Ui,
    jobs: &[EguiPaintJob],
) -> Vec<EguiPresentedRegion> {
    let mut regions = Vec::new();
    for (index, job) in jobs.iter().enumerate() {
        let paint_sort_key = paint_unit_sort_key(&job.unit);
        let authored_z_order = slipway_core::paint_unit_authored_z_order(&job.unit);
        for (bounds, blocks_wheel) in opaque_layer_bounds(&job.unit.paint) {
            let absolute = egui_rect(job.origin, bounds);
            let clipped = absolute.intersect(job.clip_rect);
            if clipped.is_positive() {
                regions.push(paint_occlusion_region(
                    ui,
                    job,
                    index,
                    paint_sort_key,
                    authored_z_order,
                    clipped,
                    blocks_wheel,
                ));
            }
        }
    }
    regions
}

fn paint_occlusion_region(
    ui: &mut egui::Ui,
    job: &EguiPaintJob,
    index: usize,
    paint_sort_key: (i32, usize, usize),
    authored_z_order: Option<usize>,
    clipped: egui::Rect,
    blocks_wheel: bool,
) -> EguiPresentedRegion {
    let region_id = PresentationRegionId::from(format!(
        "egui-paint-occlusion:{}:{}:{}:{}",
        job.unit.target.as_str(),
        paint_sort_key.0,
        paint_sort_key.1,
        index
    ));
    let response = ui.interact(
        clipped,
        egui_region_id(
            EguiPresentedRegionKind::Occlusion,
            &region_id,
            &job.unit.target,
            job.unit.address.as_ref(),
        ),
        egui::Sense::hover(),
    );
    EguiPresentedRegion {
        kind: EguiPresentedRegionKind::Occlusion,
        region_id,
        target: job.unit.target.clone(),
        address: job.unit.address.clone(),
        paint_sort_key,
        authored_z_order,
        event_target: job.unit.target.clone(),
        event_target_slot: job.unit.address.clone(),
        declared_bounds: local_rect_from_egui_rect(job.origin, clipped),
        target_origin: job.origin,
        target_bounds: local_rect_from_egui_rect(job.origin, job.clip_rect),
        event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
        response,
        cursor: CursorCapability::Default,
        enabled: true,
        text_edit_change: None,
        scroll_state: None,
        blocks_wheel,
    }
}

/// Each opaque layer's absorb bounds paired with its resolved wheel-channel
/// opacity (`true` = blocks the wheel, `false` = authored wheel-pass-through).
fn opaque_layer_bounds(ops: &[PaintOp]) -> Vec<(Rect, bool)> {
    let mut bounds = Vec::new();
    collect_opaque_layer_bounds(ops, None, &mut bounds);
    bounds
}

fn collect_opaque_layer_bounds(
    ops: &[PaintOp],
    clip: Option<Rect>,
    bounds: &mut Vec<(Rect, bool)>,
) {
    for op in ops {
        match op {
            PaintOp::Layer {
                input_transparency,
                wheel_transparency,
                clip: layer_clip,
                ops,
                ..
            } => {
                let next_clip =
                    merge_optional_clip(clip, layer_clip.as_ref().map(|clip| clip.bounds));
                if *input_transparency == PaintInputTransparency::Opaque
                    && let Some(bound) = paint_ops_visible_bounds(ops, next_clip)
                {
                    bounds.push((
                        bound,
                        slipway_core::paint_layer_blocks_wheel(
                            *input_transparency,
                            *wheel_transparency,
                        ),
                    ));
                }
                collect_opaque_layer_bounds(ops, next_clip, bounds);
            }
            PaintOp::Group {
                clip: group_clip,
                ops,
                ..
            } => {
                let next_clip =
                    merge_optional_clip(clip, group_clip.as_ref().map(|clip| clip.bounds));
                collect_opaque_layer_bounds(ops, next_clip, bounds);
            }
            PaintOp::Fill { .. } | PaintOp::Stroke { .. } | PaintOp::Text { .. } => {}
        }
    }
}

fn paint_ops_visible_bounds(ops: &[PaintOp], clip: Option<Rect>) -> Option<Rect> {
    let mut bounds = None;
    collect_paint_ops_visible_bounds(ops, clip, &mut bounds);
    bounds
}

fn collect_paint_ops_visible_bounds(
    ops: &[PaintOp],
    clip: Option<Rect>,
    bounds: &mut Option<Rect>,
) {
    for op in ops {
        match op {
            PaintOp::Fill { shape, .. } | PaintOp::Stroke { shape, .. } => {
                push_visible_bound(bounds, shape.bounds, clip);
            }
            PaintOp::Text { bounds: text, .. } => {
                push_visible_bound(bounds, *text, clip);
            }
            PaintOp::Group {
                clip: group_clip,
                ops,
                ..
            }
            | PaintOp::Layer {
                clip: group_clip,
                ops,
                ..
            } => {
                let next_clip =
                    merge_optional_clip(clip, group_clip.as_ref().map(|clip| clip.bounds));
                collect_paint_ops_visible_bounds(ops, next_clip, bounds);
            }
        }
    }
}

fn push_visible_bound(bounds: &mut Option<Rect>, rect: Rect, clip: Option<Rect>) {
    let rect = if let Some(clip) = clip {
        rect_intersection(rect, clip)
    } else {
        Some(rect)
    };
    if let Some(rect) = rect {
        *bounds = Some(bounds.map_or(rect, |current| rect_union(current, rect)));
    }
}

fn merge_optional_clip(current: Option<Rect>, next: Option<Rect>) -> Option<Rect> {
    match (current, next) {
        (Some(current), Some(next)) => rect_intersection(current, next),
        (Some(current), None) => Some(current),
        (None, Some(next)) => Some(next),
        (None, None) => None,
    }
}

fn local_rect_from_egui_rect(origin: egui::Pos2, rect: egui::Rect) -> Rect {
    Rect {
        origin: Point {
            x: rect.min.x - origin.x,
            y: rect.min.y - origin.y,
        },
        size: Size {
            width: rect.width().max(0.0),
            height: rect.height().max(0.0),
        },
    }
}

fn mount_presented_child_view_addresses(
    mut view: ViewDefinition,
    child_slot: &WidgetSlotAddress,
) -> ViewDefinition {
    let scroll_region_count = view.scroll_regions.len();
    let wheel_traversal_boundary = view.wheel_traversal_boundary;
    for region in &mut view.hit_regions {
        region.address = Some(
            region
                .address
                .take()
                .map(|slot| mount_widget_slot_address(slot, child_slot))
                .unwrap_or_else(|| child_slot.clone()),
        );
        region.route.address = Some(
            region
                .route
                .address
                .take()
                .map(|slot| mount_widget_slot_address(slot, child_slot))
                .unwrap_or_else(|| child_slot.clone()),
        );
        region.route.path = region
            .route
            .address
            .as_ref()
            .map(|address| address.path.clone())
            .unwrap_or_default();
    }

    for region in &mut view.focus_regions {
        region.address = Some(
            region
                .address
                .take()
                .map(|slot| mount_widget_slot_address(slot, child_slot))
                .unwrap_or_else(|| child_slot.clone()),
        );
    }

    for region in &mut view.scroll_regions {
        region.address = Some(
            region
                .address
                .take()
                .map(|slot| mount_widget_slot_address(slot, child_slot))
                .unwrap_or_else(|| child_slot.clone()),
        );
    }

    debug_assert_eq!(view.scroll_regions.len(), scroll_region_count);
    debug_assert_eq!(view.wheel_traversal_boundary, wheel_traversal_boundary);

    view
}

fn child_placement_matches_slot(
    placement: &ChildPlacement,
    child: &WidgetId,
    child_slot: &WidgetSlotAddress,
) -> bool {
    if let Some(placement_slot) = &placement.local_state_slot {
        placement_slot == child_slot
            || (placement_slot.widget == child_slot.widget
                && placement_slot.ordinal == child_slot.ordinal
                && child_slot.path.ends_with(&placement_slot.path))
    } else {
        placement.child == *child
    }
}

fn egui_child_paint_sort_key<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    parent_slot: &WidgetSlotAddress,
    source_order: usize,
    parent_view: &ViewDefinition,
) -> (i32, usize, usize)
where
    W: SlipwayWidget + SlipwayViewDefinition,
{
    let child = widget.id();
    let child_slot = parent_slot.child(child.clone(), source_order);
    let Some(placement) = parent_view
        .layout
        .child_placements()
        .iter()
        .find(|placement| child_placement_matches_slot(placement, &child, &child_slot))
    else {
        return (0, source_order, source_order);
    };
    let view = widget.visible_backend_view_definition(
        external,
        local,
        ViewDefinitionInput::new(
            parent_view.frame.clone(),
            child_layout_input_for_placement(placement),
        ),
    );
    expanded_paint_unit_sort_key(PaintUnit::from_view(view, source_order))
}

fn region_belongs_to_authored_child(
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    child_slots: &[EguiAuthoredChildSlot],
) -> bool {
    child_slots.iter().any(|child| {
        address
            .map(|address| slot_contains_address(&child.slot, address))
            .unwrap_or(false)
            || target == &child.child
    })
}

fn slot_contains_address(parent: &WidgetSlotAddress, address: &WidgetSlotAddress) -> bool {
    address.path.len() >= parent.path.len()
        && address
            .path
            .iter()
            .zip(parent.path.iter())
            .all(|(address_part, parent_part)| address_part == parent_part)
}

fn scroll_contains_placement_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    scroll: &ScrollRegionDeclaration,
    placement: &ChildPlacement,
) -> bool {
    let content_bounds = slipway_core::declared_region_root_local_rect_with_geometry_index(
        geometry_index,
        &scroll.target,
        scroll.address.as_ref(),
        scroll.content_bounds.into_rect(),
    );
    let viewport = slipway_core::declared_region_root_local_rect_with_geometry_index(
        geometry_index,
        &scroll.target,
        scroll.address.as_ref(),
        scroll.viewport.into_rect(),
    );
    let offset = clamp_declared_scroll_offset(
        scroll.offset,
        scroll.axes,
        scroll.viewport.into_rect(),
        scroll.content_bounds.into_rect(),
    );
    let visible_content = Rect {
        origin: Point {
            x: content_bounds.origin.x + offset.x,
            y: content_bounds.origin.y + offset.y,
        },
        size: viewport.size,
    };
    rects_intersect(visible_content, placement.bounds.into_rect())
}

fn scroll_owns_placement_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    scroll: &ScrollRegionDeclaration,
    placement: &ChildPlacement,
) -> bool {
    let content_bounds = slipway_core::declared_region_root_local_rect_with_geometry_index(
        geometry_index,
        &scroll.target,
        scroll.address.as_ref(),
        scroll.content_bounds.into_rect(),
    );
    let viewport = slipway_core::declared_region_root_local_rect_with_geometry_index(
        geometry_index,
        &scroll.target,
        scroll.address.as_ref(),
        scroll.viewport.into_rect(),
    );
    rects_intersect(content_bounds, placement.bounds.into_rect())
        || rects_intersect(viewport, placement.bounds.into_rect())
}

fn rects_intersect(a: Rect, b: Rect) -> bool {
    let a_min_x = a.origin.x;
    let a_min_y = a.origin.y;
    let a_max_x = a.origin.x + a.size.width.max(0.0);
    let a_max_y = a.origin.y + a.size.height.max(0.0);
    let b_min_x = b.origin.x;
    let b_min_y = b.origin.y;
    let b_max_x = b.origin.x + b.size.width.max(0.0);
    let b_max_y = b.origin.y + b.size.height.max(0.0);

    a_min_x < b_max_x && a_max_x > b_min_x && a_min_y < b_max_y && a_max_y > b_min_y
}

/// Track and thumb of the declared indicator this backend draws for
/// `scroll`, in ABSOLUTE egui coordinates — `None` when the geometric
/// conditions (vertical axis, content taller than viewport) do not hold.
/// THE single geometry source for the indicator: the painter
/// (`paint_declared_scroll_indicator`) and the interactive
/// thumb-drag/track-click input path (`DefaultEguiBridge::input_events`,
/// Step 212, via the geometry stashed on `EguiScrollRegionState`) both
/// use it, so the pixels and the hit surface cannot diverge.
fn declared_scroll_indicator_geometry(
    viewport_rect: egui::Rect,
    scroll: &ScrollRegionDeclaration,
    offset: Point,
) -> Option<(egui::Rect, egui::Rect)> {
    if !scroll.axes.vertical {
        return None;
    }
    let content_height = scroll.content_bounds.size.height.max(0.0);
    let viewport_height = scroll.viewport.size.height.max(0.0);
    if content_height <= viewport_height || viewport_height <= 0.0 {
        return None;
    }

    let track_width = 4.0;
    let track = egui::Rect::from_min_max(
        egui::pos2(
            viewport_rect.right() - track_width - 4.0,
            viewport_rect.top() + 6.0,
        ),
        egui::pos2(viewport_rect.right() - 4.0, viewport_rect.bottom() - 6.0),
    );
    let max_offset = (content_height - viewport_height).max(0.0);
    let thumb_height =
        (track.height() * viewport_height / content_height).clamp(18.0, track.height().max(18.0));
    let travel = (track.height() - thumb_height).max(0.0);
    let top = track.top() + travel * (offset.y.clamp(0.0, max_offset) / max_offset.max(1.0));
    let thumb = egui::Rect::from_min_size(
        egui::pos2(track.left(), top),
        egui::vec2(track.width(), thumb_height),
    );
    Some((track, thumb))
}

fn paint_declared_scroll_indicator(
    ui: &egui::Ui,
    viewport_rect: egui::Rect,
    scroll: &ScrollRegionDeclaration,
    offset: Point,
) {
    let Some((track, thumb)) = declared_scroll_indicator_geometry(viewport_rect, scroll, offset)
    else {
        return;
    };

    ui.painter().rect_filled(
        track,
        egui::CornerRadius::same(2),
        declared_scroll_indicator_track_color(),
    );
    ui.painter().rect_filled(
        thumb,
        egui::CornerRadius::same(2),
        declared_scroll_indicator_thumb_color(),
    );
}

fn paint_declared_scroll_indicators(ui: &egui::Ui, indicators: &mut Vec<EguiScrollIndicatorPaint>) {
    for indicator in indicators.drain(..) {
        paint_declared_scroll_indicator(
            ui,
            indicator.viewport_rect,
            &indicator.scroll,
            indicator.offset,
        );
    }
}

fn declared_scroll_indicator_track_color() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(148, 163, 184, 40)
}

fn declared_scroll_indicator_thumb_color() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(100, 116, 139, 160)
}

fn child_without_placement_refusal(
    child: WidgetId,
    slot: &WidgetSlotAddress,
    native: bool,
) -> BackendParityAdmission {
    let kind = if native { "native-child" } else { "child" };
    let requirement_id = format!("view.child_placements.{}.{}", widget_slot_key(slot), kind);
    let diagnostic = Diagnostic::unsupported(
        Some(child.clone()),
        "egui.child_placement.missing",
        format!(
            "egui backend visited {kind} `{}` but no matching ChildPlacement was produced",
            child.as_str()
        ),
    );

    BackendParityAdmission {
        backend_id: EGUI_BACKEND_ID.to_string(),
        accepted: false,
        required_profiles: Vec::new(),
        visible_requirements: vec![BackendVisibleCapabilityRequirement {
            requirement_id: requirement_id.clone(),
            target: Some(child.clone()),
            capability: BackendVisibleCapability::Custom(
                "egui.authored_child_placement".to_string(),
            ),
            required: true,
        }],
        unsupported: vec![UnsupportedCapabilityEvidence {
            backend_id: EGUI_BACKEND_ID.to_string(),
            target: Some(child),
            capability: Capability::ChildTraversal,
            visible_capability: Some(BackendVisibleCapability::Custom(
                "egui.authored_child_placement".to_string(),
            )),
            requirement_id: Some(requirement_id),
            reason: "visited authored child cannot be presented without a matching ChildPlacement"
                .to_string(),
            source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "child-assembly"),
            diagnostics: vec![diagnostic.clone()],
        }],
        source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "child-assembly"),
        diagnostics: vec![diagnostic],
    }
}

fn widget_slot_key(slot: &WidgetSlotAddress) -> String {
    let path = slot
        .path
        .iter()
        .map(WidgetId::as_str)
        .collect::<Vec<_>>()
        .join("/");
    format!("{path}:{}", slot.ordinal)
}

fn egui_region_id(
    kind: EguiPresentedRegionKind,
    region_id: &PresentationRegionId,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
) -> egui::Id {
    let address_key = address.map(|address| {
        (
            address.widget.as_str().to_string(),
            address
                .path
                .iter()
                .map(|widget| widget.as_str().to_string())
                .collect::<Vec<_>>(),
            address.ordinal,
        )
    });
    egui::Id::new((
        "slipway-egui-region",
        kind,
        target.as_str().to_string(),
        region_id.as_str().to_string(),
        address_key,
    ))
}

fn egui_sense_for_hit(hit: &HitRegionDeclaration) -> egui::Sense {
    let drag = matches!(
        hit.capture,
        PointerCaptureIntent::DuringDrag | PointerCaptureIntent::Explicit
    ) || matches!(
        hit.cursor,
        CursorCapability::Grab
            | CursorCapability::Grabbing
            | CursorCapability::Move
            | CursorCapability::ResizeHorizontal
            | CursorCapability::ResizeVertical
            | CursorCapability::ResizeBoth
    );
    let click = !matches!(hit.cursor, CursorCapability::Inherited);

    match (click, drag) {
        (true, true) => egui::Sense::click() | egui::Sense::drag(),
        (true, false) => egui::Sense::click(),
        (false, true) => egui::Sense::drag(),
        (false, false) => egui::Sense::hover(),
    }
}

fn apply_region_cursor(response: egui::Response, cursor: CursorCapability) -> egui::Response {
    match egui_cursor_icon(cursor) {
        Some(cursor) => response.on_hover_and_drag_cursor(cursor),
        None => response,
    }
}

fn egui_cursor_icon(cursor: CursorCapability) -> Option<egui::CursorIcon> {
    match cursor {
        CursorCapability::Pointer => Some(egui::CursorIcon::PointingHand),
        CursorCapability::Text => Some(egui::CursorIcon::Text),
        CursorCapability::Grab => Some(egui::CursorIcon::Grab),
        CursorCapability::Grabbing => Some(egui::CursorIcon::Grabbing),
        CursorCapability::Move => Some(egui::CursorIcon::Move),
        CursorCapability::Crosshair => Some(egui::CursorIcon::Crosshair),
        CursorCapability::NotAllowed => Some(egui::CursorIcon::NotAllowed),
        CursorCapability::ResizeHorizontal => Some(egui::CursorIcon::ResizeHorizontal),
        CursorCapability::ResizeVertical => Some(egui::CursorIcon::ResizeVertical),
        CursorCapability::ResizeBoth => Some(egui::CursorIcon::ResizeNwSe),
        CursorCapability::Inherited | CursorCapability::Default | CursorCapability::Custom(_) => {
            None
        }
    }
}

fn egui_region_at_position(
    regions: &[EguiPresentedRegion],
    position: egui::Pos2,
) -> Option<&EguiPresentedRegion> {
    let response_authority = egui_response_authority_region_at_position(regions, position);
    let geometry_authority = egui_geometry_region_at_position(regions, position);
    let selected = egui_declared_region_over_child_response(
        regions,
        position,
        response_authority.or(geometry_authority),
    );
    let occlusion = egui_occlusion_region_at_position(regions, position);

    if let Some(occlusion) = occlusion
        && selected.is_none_or(|region| egui_occlusion_blocks_region(occlusion, region))
    {
        return None;
    }

    selected
}

fn egui_declared_region_over_child_response<'a>(
    regions: &'a [EguiPresentedRegion],
    position: egui::Pos2,
    selected: Option<&'a EguiPresentedRegion>,
) -> Option<&'a EguiPresentedRegion> {
    let selected = selected?;
    if !egui_region_is_child_response(selected) {
        return Some(selected);
    }

    regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.kind != EguiPresentedRegionKind::Occlusion
                && !egui_region_is_child_response(region)
                && ((region.target == selected.target && region.address == selected.address)
                    || selected.address.as_ref().is_some_and(|selected_address| {
                        region.address.as_ref().is_some_and(|region_address| {
                            slot_contains_address(selected_address, region_address)
                        })
                    }))
                && region.response.interact_rect.contains(position)
                && (egui_region_has_response_authority(region)
                    || region.response.sense.interactive()
                    || region.response.hovered()
                    || region.response.contains_pointer())
        })
        .max_by_key(|region| region.paint_sort_key)
        .or(Some(selected))
}

fn egui_region_is_child_response(region: &EguiPresentedRegion) -> bool {
    region
        .region_id
        .as_str()
        .starts_with("egui-child-response:")
}

/// Pointer-channel occlusion filter: routes through the shared core
/// predicate (`paint_occlusion_blocks_declared_hit_region`, NC-2) so both
/// backends and the derived dispatch graph decide occlusion identically. A
/// hit region's `paint_sort_key` carries the AUTHOR-DECLARED
/// `HitRegionOrder` (see `allocate_hit_region`), while an occluder's key is
/// its paint-unit sort key whose tie-break fields default to the mounted
/// slot ordinal — comparing those tie-break fields across that space split
/// is what silently dropped every press over an authored opaque overlay
/// (the naive-consumer modal). Same-owner comparisons at the region's own
/// z therefore consult only the occluding layer's AUTHORED within-z order
/// (`authored_z_order`), which preserves the authored overlay-card stack
/// (explicit `PaintLayerKey::ordered` fronts a lower declared
/// `paint_order`) while a widget's own unordered layer never blocks its
/// own same-z hit region.
fn egui_occlusion_blocks_region(
    occlusion: &EguiPresentedRegion,
    region: &EguiPresentedRegion,
) -> bool {
    slipway_core::paint_occlusion_blocks_declared_hit_region(
        &occlusion.target,
        &egui_hit_region_order_from_key(occlusion.paint_sort_key),
        occlusion.authored_z_order,
        &region.target,
        &egui_hit_region_order_from_key(region.paint_sort_key),
    )
}

fn egui_hit_region_order_from_key(key: (i32, usize, usize)) -> HitRegionOrder {
    HitRegionOrder {
        z_index: key.0,
        paint_order: key.1,
        traversal_order: key.2,
    }
}

fn egui_region_by_id<'a>(
    regions: &'a [EguiPresentedRegion],
    id: &PresentationRegionId,
) -> Option<&'a EguiPresentedRegion> {
    regions
        .iter()
        .find(|region| region.enabled && &region.region_id == id)
}

/// Press resolution for the declared indicators (Step 212): when the
/// press point lands on a painted track, returns the backend-internal
/// drag capture to start — anchored at the current offset for a THUMB
/// press, or at the jumped offset (with `Some(jumped)` to emit) for a
/// TRACK press (jump-to-position). Geometry comes from the
/// `EguiScrollRegionState::indicator` rects stashed at allocation time by
/// the same function that painted the indicator. Points covered by a
/// FRONTING authored region (an opaque overlay layer's occlusion region
/// or a hit region stacked above the indicator's scroll region) are
/// refused, so pointer selection agrees with the visible stacking.
fn egui_indicator_press_interaction(
    regions: &[EguiPresentedRegion],
    pos: egui::Pos2,
) -> Option<(EguiIndicatorDragState, Option<f32>)> {
    let (region, track, thumb) = regions
        .iter()
        .filter(|region| region.enabled && region.kind == EguiPresentedRegionKind::Scroll)
        .filter_map(|region| {
            let (track, thumb) = region.scroll_state.as_ref()?.indicator?;
            track.contains(pos).then_some((region, track, thumb))
        })
        .max_by_key(|(region, _, _)| region.paint_sort_key)?;
    if egui_indicator_press_blocked(regions, pos, region.paint_sort_key) {
        return None;
    }
    let state = region.scroll_state.as_ref()?;
    let track_travel = (track.height() - thumb.height()).max(0.0);
    let max_offset = (state.content_size.height - state.inner_rect.size.height).max(0.0);
    if track_travel <= 0.0 || max_offset <= 0.0 {
        return None;
    }
    if thumb.contains(pos) {
        return Some((
            EguiIndicatorDragState {
                region_id: region.region_id.clone(),
                press_y: pos.y,
                start_offset_y: state.egui_offset.y.clamp(0.0, max_offset),
                track_travel,
                max_offset,
            },
            None,
        ));
    }
    let jumped = ((pos.y - track.top() - thumb.height() / 2.0) / track_travel * max_offset)
        .clamp(0.0, max_offset);
    Some((
        EguiIndicatorDragState {
            region_id: region.region_id.clone(),
            press_y: pos.y,
            start_offset_y: jumped,
            track_travel,
            max_offset,
        },
        Some(jumped),
    ))
}

/// One drag step of an active indicator capture: cumulative pointer
/// travel from the press anchor maps through the track-to-content ratio
/// captured at press time, clamped to the declared travel.
fn egui_indicator_drag_offset(drag: &EguiIndicatorDragState, pointer_y: f32) -> f32 {
    (drag.start_offset_y + (pointer_y - drag.press_y) * drag.max_offset / drag.track_travel)
        .clamp(0.0, drag.max_offset)
}

fn egui_indicator_press_blocked(
    regions: &[EguiPresentedRegion],
    pos: egui::Pos2,
    indicator_key: (i32, usize, usize),
) -> bool {
    regions.iter().any(|region| {
        region.enabled
            && region.paint_sort_key > indicator_key
            && matches!(
                region.kind,
                EguiPresentedRegionKind::Occlusion | EguiPresentedRegionKind::Hit
            )
            && region.response.interact_rect.contains(pos)
    })
}

/// The raw Scroll event an indicator interaction emits for `region` at
/// `offset_y`; `egui_scroll_backend_input_event` then rewrites the
/// declared fields and attaches the dispatch evidence — the same path the
/// offset-sync events take.
fn egui_indicator_scroll_event_for_offset(
    region: &EguiPresentedRegion,
    offset_y: f32,
) -> Option<InputEvent> {
    let state = region.scroll_state.as_ref()?;
    Some(InputEvent::Scroll(ScrollEvent {
        target: region.target.clone(),
        target_slot: region.address.clone(),
        region_id: region.region_id.clone(),
        offset_x: state.egui_offset.x,
        offset_y,
        viewport: TargetLocalRect::new(state.inner_rect),
        content_bounds: TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: state.content_size,
        }),
    }))
}

fn egui_region_requires_stateful_pointer_capture(
    hit_regions: &[HitRegionDeclaration],
    id: &PresentationRegionId,
) -> bool {
    hit_regions
        .iter()
        .find(|region| region.enabled && &region.id == id)
        .is_some_and(|region| {
            matches!(
                region.capture,
                PointerCaptureIntent::DuringDrag | PointerCaptureIntent::Explicit
            )
        })
}

fn egui_region_anchor_position(
    _context: &EguiInputContext<'_>,
    region: &EguiPresentedRegion,
) -> egui::Pos2 {
    let x = region.declared_bounds.origin.x + region.declared_bounds.size.width.min(1.0) * 0.5;
    let y = region.declared_bounds.origin.y + region.declared_bounds.size.height.min(1.0) * 0.5;
    egui::pos2(region.target_origin.x + x, region.target_origin.y + y)
}

fn egui_response_authority_region_at_position(
    regions: &[EguiPresentedRegion],
    position: egui::Pos2,
) -> Option<&EguiPresentedRegion> {
    regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.kind != EguiPresentedRegionKind::Occlusion
                && (egui_region_has_pointer_response_authority(region)
                    || (region.response.interact_rect.contains(position)
                        && egui_region_has_response_authority(region)))
        })
        .max_by_key(|region| region.paint_sort_key)
}

fn egui_region_has_pointer_response_authority(region: &EguiPresentedRegion) -> bool {
    region.response.clicked() || region.response.hovered() || region.response.contains_pointer()
}

fn egui_region_has_response_authority(region: &EguiPresentedRegion) -> bool {
    if region.kind == EguiPresentedRegionKind::Occlusion {
        return false;
    }

    egui_region_has_pointer_response_authority(region)
        || region.response.has_focus()
        || region.response.gained_focus()
        || region.response.lost_focus()
        || region.text_edit_change.is_some()
        || region
            .scroll_state
            .as_ref()
            .is_some_and(egui_scroll_state_changed)
}

fn egui_region_can_request_focus(region: &EguiPresentedRegion) -> bool {
    if region.kind == EguiPresentedRegionKind::Occlusion {
        return false;
    }

    region.response.sense.is_focusable()
        || matches!(
            region.kind,
            EguiPresentedRegionKind::Focus | EguiPresentedRegionKind::TextEdit
        )
}

fn apply_egui_native_physical_region_effect(
    operation: Option<&DebugPhysicalControl>,
    regions: &[EguiPresentedRegion],
) {
    let Some(DebugPhysicalControl::Focus { selector, focused }) = operation else {
        return;
    };
    let Some(region) = egui_focus_region_for_native_selector(regions, selector) else {
        return;
    };

    if *focused {
        region.response.request_focus();
    } else {
        region.response.surrender_focus();
    }
}

fn egui_native_scroll_offset_for_operation(
    operation: Option<&DebugPhysicalControl>,
    geometry_index: &PresentationGeometryIndex,
    scroll: &ScrollRegionDeclaration,
) -> Option<Point> {
    let Some(DebugPhysicalControl::Scroll {
        selector,
        offset_x,
        offset_y,
    }) = operation
    else {
        return None;
    };
    if !egui_scroll_selector_matches(selector, geometry_index, scroll) {
        return None;
    }
    Some(Point {
        x: *offset_x,
        y: *offset_y,
    })
}

fn egui_scroll_selector_matches(
    selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
    geometry_index: &PresentationGeometryIndex,
    scroll: &ScrollRegionDeclaration,
) -> bool {
    match selector {
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Target { target } => {
            &scroll.target == target
        }
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Region { region } => {
            &scroll.id == region
        }
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Position { position } => {
            slipway_core::declared_region_contains_root_local_point_with_geometry_index(
                geometry_index,
                &scroll.target,
                scroll.address.as_ref(),
                scroll.viewport.into_rect(),
                *position,
            )
        }
    }
}

fn egui_focus_region_for_native_selector<'a>(
    regions: &'a [EguiPresentedRegion],
    selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
) -> Option<&'a EguiPresentedRegion> {
    match selector {
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Target { target } => {
            regions.iter().find(|region| {
                region.enabled
                    && egui_region_can_request_focus(region)
                    && &region.event_target == target
            })
        }
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Region { region } => {
            regions.iter().find(|candidate| {
                candidate.enabled
                    && egui_region_can_request_focus(candidate)
                    && &candidate.region_id == region
            })
        }
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Position { position } => {
            let position = egui::pos2(position.x, position.y);
            regions
                .iter()
                .filter(|region| {
                    region.enabled
                        && egui_region_can_request_focus(region)
                        && region.response.interact_rect.contains(position)
                })
                .max_by_key(|region| region.paint_sort_key)
        }
    }
}

fn egui_text_edit_region_for_native_selector<'a>(
    regions: &'a [EguiPresentedRegion],
    selector: &slipway_debug_bridge::DebugPhysicalControlDeclarationSelector,
) -> Option<&'a EguiPresentedRegion> {
    let matches_selector = |region: &&EguiPresentedRegion| match selector {
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Target { target } => {
            &region.event_target == target
        }
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Region { region: id } => {
            &region.region_id == id
        }
        slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Position { position } => {
            region
                .response
                .interact_rect
                .contains(egui::pos2(position.x, position.y))
        }
    };
    regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.kind == EguiPresentedRegionKind::TextEdit
                && matches_selector(region)
        })
        .max_by_key(|region| region.paint_sort_key)
}

fn egui_scroll_state_changed(scroll: &EguiScrollRegionState) -> bool {
    (scroll.egui_offset.x - scroll.declared_offset.x).abs() > f32::EPSILON
        || (scroll.egui_offset.y - scroll.declared_offset.y).abs() > f32::EPSILON
}

fn egui_geometry_region_at_position(
    regions: &[EguiPresentedRegion],
    position: egui::Pos2,
) -> Option<&EguiPresentedRegion> {
    regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.response.interact_rect.contains(position)
                && region.kind != EguiPresentedRegionKind::Occlusion
                && (region.response.sense.interactive()
                    || region.response.hovered()
                    || region.response.contains_pointer())
        })
        .max_by_key(|region| region.paint_sort_key)
}

fn egui_occlusion_region_at_position(
    regions: &[EguiPresentedRegion],
    position: egui::Pos2,
) -> Option<&EguiPresentedRegion> {
    regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.kind == EguiPresentedRegionKind::Occlusion
                && region.response.interact_rect.contains(position)
        })
        .max_by_key(|region| region.paint_sort_key)
}

/// Topmost occlusion that blocks the WHEEL channel at `position`.
///
/// Wheel-specific variant of [`egui_occlusion_region_at_position`]: occluders
/// authored wheel-pass-through (`blocks_wheel == false`) are excluded, so the
/// wheel reaches the scroll region behind them while the pointer path (which
/// uses [`egui_occlusion_region_at_position`]) still treats them as occluders.
fn egui_wheel_occlusion_region_at_position(
    regions: &[EguiPresentedRegion],
    position: egui::Pos2,
) -> Option<&EguiPresentedRegion> {
    regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.kind == EguiPresentedRegionKind::Occlusion
                && region.blocks_wheel
                && region.response.interact_rect.contains(position)
        })
        .max_by_key(|region| region.paint_sort_key)
}

#[cfg(test)]
fn egui_region_position(region: &EguiPresentedRegion, position: egui::Pos2) -> Point {
    match region.event_coordinate_space {
        PointerEventCoordinateSpace::TargetLocal => egui_position(position, region.target_origin),
        PointerEventCoordinateSpace::RegionLocal => {
            egui_position(position, region.response.interact_rect.min)
        }
    }
}

fn egui_backend_input_trace<W>(
    _widget: &W,
    _external: &W::ExternalState,
    _local: &W::LocalState,
    input: BackendInputEvent,
    outcome: &EventOutcome<W::AppMessage>,
) -> BackendInputTrace
where
    W: SlipwayView + SlipwayWidgetTypes,
{
    BackendInputTrace {
        input,
        handled: outcome.handled,
        revision_before: None,
        revision_after: None,
        emitted_messages: outcome
            .emitted_messages
            .iter()
            .map(|message| EmittedMessageEvidence {
                target: message.target.clone(),
                name: message.name.clone(),
            })
            .collect(),
        local_state: Vec::new(),
        changes: compact_egui_backend_trace_changes(&outcome.changes),
        diagnostics: outcome.diagnostics.clone(),
    }
}

fn egui_backend_input_contract_refusal<M>(
    view: &ViewDefinition,
    input: &BackendInputEvent,
) -> Option<EventOutcome<M>> {
    let diagnostics = slipway_core::backend_input_dispatch_evidence_contract_diagnostics(
        view,
        input,
        Some(slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED),
        Some(EGUI_BACKEND_ID),
    );
    if !view_definition_has_blocking_contract_diagnostic(&diagnostics) {
        return None;
    }

    let mut outcome = EventOutcome::ignored();
    outcome.diagnostics = diagnostics;
    Some(outcome)
}

fn compact_egui_backend_trace_changes(changes: &[ChangeEvidence]) -> Vec<ChangeEvidence> {
    changes
        .iter()
        .map(|change| ChangeEvidence {
            target: change.target.clone(),
            slot: change.slot.clone(),
            field: change.field.clone(),
            before: change.before.as_ref().map(|_| "<redacted>".to_string()),
            after: change.after.as_ref().map(|_| "<redacted>".to_string()),
        })
        .collect()
}

fn egui_backend_pointer_input_event(
    context: &EguiInputContext<'_>,
    region: &EguiPresentedRegion,
    position: egui::Pos2,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
    pointer_is_pressed: bool,
) -> Option<BackendInputEvent> {
    let view_root_local_position = egui_region_root_local_position(context, region, position);
    let (dispatch, mut evidence) =
        slipway_core::resolve_declared_pointer_dispatch_with_evidence_and_geometry_index(
            EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input"),
            context.frame.clone(),
            context.geometry_index,
            context.hit_regions,
            view_root_local_position,
            kind,
            button,
            details.clone(),
            pointer_is_pressed,
        );

    if evidence.selected_region.as_ref() != Some(&region.region_id) {
        evidence.diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "egui.backend_dispatch_region_mismatch",
            format!(
                "egui response selected region `{}` but declared resolver selected `{}`",
                region.region_id.as_str(),
                evidence
                    .selected_region
                    .as_ref()
                    .map(|region| region.as_str())
                    .unwrap_or("<none>")
            ),
        ));
        return None;
    }

    dispatch.map(|dispatch| BackendInputEvent::declared(dispatch.input, evidence))
}

fn egui_backend_captured_pointer_input_event(
    context: &EguiInputContext<'_>,
    region: &EguiPresentedRegion,
    position: egui::Pos2,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
    pointer_is_pressed: bool,
) -> Option<BackendInputEvent> {
    let hit = context
        .hit_regions
        .iter()
        .find(|candidate| candidate.enabled && candidate.id == region.region_id)?;
    let view_root_local_position = egui_region_root_local_position(context, region, position);
    let event = slipway_core::declared_pointer_event_for_hit_region_with_geometry_index(
        context.geometry_index,
        hit,
        view_root_local_position,
        kind,
        button,
        details,
    );
    let candidate_regions = context
        .hit_regions
        .iter()
        .filter(|candidate| candidate.enabled)
        .map(|candidate| candidate.id.clone())
        .collect::<Vec<_>>();
    let evidence = slipway_core::DeclaredEventDispatchEvidence {
        source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "captured-input"),
        frame: context.frame.clone(),
        kind: DeclaredEventDispatchKind::Pointer,
        input_position: Some(view_root_local_position),
        input_position_space: Some(slipway_core::DispatchPositionSpace::Content),
        candidate_regions,
        selected_region: Some(hit.id.clone()),
        refusal_reason: None,
        generated_event: Some(event.clone()),
        route: Some(hit.route.clone()),
        capture_event: slipway_core::declared_pointer_capture_for_region(
            hit,
            kind,
            pointer_is_pressed,
        ),
        diagnostics: Vec::new(),
    };

    Some(BackendInputEvent::declared(event, evidence))
}

/// Refusal evidence for a pointer press/release consumed by a
/// pointer-opaque paint layer with no reachable hit region — the egui side
/// of the NC-2 no-silence contract extension (parity with the iced
/// `blocked_pointer_refusal_evidence`): names the front occluder and every
/// containing declared candidate hit region. Retained in the bridge's
/// refusal ring (`dispatch_refusals` -> the runtime's bounded dispatch
/// refusal ring), so the consumed press is diagnosable from the
/// `diagnostics` probe kind and the physical-control reply's
/// `post_hoc_diagnosis` attachment instead of vanishing.
fn egui_blocked_pointer_refusal_evidence(
    context: &EguiInputContext<'_>,
    occlusion: &EguiPresentedRegion,
    position: egui::Pos2,
    kind: PointerEventKind,
    button: Option<PointerButton>,
) -> slipway_core::DeclaredEventDispatchEvidence {
    let view_root_local_position = egui_region_root_local_position(context, occlusion, position);
    let candidate_regions = context
        .hit_regions
        .iter()
        .filter(|region| {
            region.enabled
                && slipway_core::declared_region_contains_root_local_point_with_geometry_index(
                    context.geometry_index,
                    &region.target,
                    region.address.as_ref(),
                    region.bounds.into_rect(),
                    view_root_local_position,
                )
        })
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    let occluder = format!(
        "the pointer-opaque paint layer of target `{}` (occlusion order z={} paint={} traversal={})",
        occlusion.target.as_str(),
        occlusion.paint_sort_key.0,
        occlusion.paint_sort_key.1,
        occlusion.paint_sort_key.2,
    );
    let refusal_reason = if candidate_regions.is_empty() {
        format!(
            "pointer {kind:?} (button {button:?}) was consumed by {occluder}; no enabled hit region contains the point",
        )
    } else {
        format!(
            "pointer {kind:?} (button {button:?}) was consumed by {occluder}, which fronts every candidate hit region at the point; no hit region was reachable",
        )
    };
    slipway_core::DeclaredEventDispatchEvidence {
        source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input"),
        frame: context.frame.clone(),
        kind: DeclaredEventDispatchKind::Pointer,
        input_position: Some(view_root_local_position),
        input_position_space: Some(slipway_core::DispatchPositionSpace::Content),
        candidate_regions,
        selected_region: None,
        refusal_reason: Some(refusal_reason),
        generated_event: None,
        route: None,
        capture_event: false,
        diagnostics: Vec::new(),
    }
}

/// Maps a view-local wheel cursor into root content / dispatch space through
/// the presented region under the cursor, so a wheel and a press at the same
/// visual cursor resolve to the same content-space coordinate. A region's
/// scrolled visual origin (`target_origin`) versus its content-space rect
/// (from the geometry index) encodes every ancestor scroll offset, so after the
/// root scrolls a wheel over a nested region still reaches that region (the
/// Step 190 E1 win). Falls back to the plain view-local mapping when no region
/// covers the cursor.
fn egui_wheel_dispatch_position(context: &EguiInputContext<'_>, position: egui::Pos2) -> Point {
    match egui_region_at_position(context.regions, position) {
        Some(region) => egui_region_root_local_position(context, region, position),
        None => egui_view_root_local_position(position, context.rect.min, context.frame),
    }
}

/// The visible-viewport wheel point: the raw cursor mapped to the top-level view
/// origin plus `frame.viewport.origin`, with NO per-region scroll translation.
/// Core wheel-owner selection tests each scroll region's declared viewport rect
/// at its UN-scrolled geometry position, so a native-scrolled ancestor scroll
/// region (e.g. the root scroll) is only reachable through this point once its
/// own offset has pushed the content-space dispatch point past its viewport
/// band. The `frame.viewport.origin` term keeps this fallback in the same
/// root-local frame family as the primary path (`egui_view_root_local_position`
/// adds it too); omitting it left the fallback point in a third, unpinned space
/// (audit finding MF-M1).
fn egui_wheel_visible_viewport_position(
    context: &EguiInputContext<'_>,
    position: egui::Pos2,
) -> Point {
    egui_view_root_local_position(position, context.rect.min, context.frame)
}

#[cfg(test)]
fn egui_wheel_select_consumer<'a>(
    context: &'a EguiInputContext<'_>,
    point: Point,
    delta_x: f32,
    delta_y: f32,
) -> Option<&'a ScrollRegionDeclaration> {
    slipway_core::select_declared_wheel_consumer_at_root_local_point_with_geometry_index_and_boundary(
        context.geometry_index,
        context.scroll_regions,
        context.wheel_traversal_boundary,
        point,
        delta_x,
        delta_y,
    )
}

/// True when `owner`'s presented viewport sits at its un-scrolled geometry
/// position, i.e. no ancestor scroll displaces it (the root/native scroll). Only
/// such an owner may claim a wheel through the visible-viewport point; a nested
/// content region displaced by an ancestor scroll must not act as a fixed
/// window-band phantom owner through that point.
fn egui_wheel_owner_is_undisplaced_ancestor(
    context: &EguiInputContext<'_>,
    owner: &ScrollRegionDeclaration,
) -> bool {
    let Some(presented) = egui_presented_scroll_region_by_id(context.regions, &owner.id) else {
        return false;
    };
    let target_rect = context
        .geometry_index
        .target_rect_for_region_address(&owner.target, owner.address.as_ref());
    let expected = egui_point(context.rect.min, target_rect.origin);
    const TOLERANCE: f32 = 0.5;
    (presented.target_origin.x - expected.x).abs() <= TOLERANCE
        && (presented.target_origin.y - expected.y).abs() <= TOLERANCE
}

/// Chooses the root-local position to resolve a wheel at, mirroring the iced
/// contract (`iced_wheel_dispatch_root_local_position`). Prefers the
/// content-space dispatch point so nested content owners under an ancestor
/// scroll stay reachable (Step 190 E1). When that point selects NO consumer,
/// falls back to the visible-viewport point ONLY if it selects an un-displaced
/// ancestor scroll region.
///
/// Without the fallback, once an ancestor (root) scroll has advanced far enough
/// that `cursor + ancestor_offset` leaves the ancestor viewport band, the
/// content-space dispatch point exceeds every scroll region's un-scrolled
/// viewport rect, the core resolve returns None, and the wheel goes silently
/// dead ("freezes" mid-scroll) even though the visible cursor still sits inside
/// the scrollable root -- the Step 190 E1 mapping regression that this repairs.
struct EguiResolvedWheel<'a> {
    point: Point,
    position_space: slipway_core::DispatchPositionSpace,
    disposition: slipway_core::DeclaredWheelDisposition<'a>,
}

fn egui_wheel_resolve_position<'a>(
    context: &'a EguiInputContext<'_>,
    position: egui::Pos2,
    delta_x: f32,
    delta_y: f32,
) -> EguiResolvedWheel<'a> {
    let disposition_at = |point| {
        slipway_core::declared_wheel_disposition_at_root_local_point_with_geometry_index(
            context.geometry_index,
            context.scroll_regions,
            context.wheel_traversal_boundary,
            point,
            delta_x,
            delta_y,
        )
    };
    let dispatch_point = egui_wheel_dispatch_position(context, position);
    let dispatch_disposition = disposition_at(dispatch_point);
    if !matches!(
        dispatch_disposition,
        slipway_core::DeclaredWheelDisposition::Bubble
    ) {
        return EguiResolvedWheel {
            point: dispatch_point,
            position_space: slipway_core::DispatchPositionSpace::Content,
            disposition: dispatch_disposition,
        };
    }
    let visual_point = egui_wheel_visible_viewport_position(context, position);
    let visual_disposition = disposition_at(visual_point);
    let visual_owner = match visual_disposition {
        slipway_core::DeclaredWheelDisposition::Moved(owner)
        | slipway_core::DeclaredWheelDisposition::ConsumedNoOp(owner) => Some(owner),
        slipway_core::DeclaredWheelDisposition::Bubble => None,
    };
    if let Some(owner) = visual_owner
        && egui_wheel_owner_is_undisplaced_ancestor(context, owner)
    {
        return EguiResolvedWheel {
            point: visual_point,
            position_space: slipway_core::DispatchPositionSpace::Viewport,
            disposition: visual_disposition,
        };
    }
    EguiResolvedWheel {
        point: dispatch_point,
        position_space: slipway_core::DispatchPositionSpace::Content,
        disposition: dispatch_disposition,
    }
}

struct EguiWheelInputResolution {
    event: Option<BackendInputEvent>,
    terminal_consumed: bool,
}

fn egui_backend_wheel_input_resolution(
    context: &EguiInputContext<'_>,
    position: egui::Pos2,
    delta_x: f32,
    delta_y: f32,
    refusals: &mut Vec<slipway_core::DeclaredEventDispatchEvidence>,
) -> EguiWheelInputResolution {
    let resolved = egui_wheel_resolve_position(context, position, delta_x, delta_y);
    if let slipway_core::DeclaredWheelDisposition::ConsumedNoOp(root) = resolved.disposition {
        let root_key = egui_presented_scroll_region_by_id(context.regions, &root.id)
            .map(|region| region.paint_sort_key)
            .unwrap_or((
                root.order.z_index,
                root.order.paint_order,
                root.order.traversal_order,
            ));
        let blocked = egui_wheel_occlusion_region_at_position(context.regions, position)
            .is_some_and(|occlusion| {
                slipway_core::hit_region_order_is_front_of(
                    &egui_hit_region_order_from_key(occlusion.paint_sort_key),
                    &egui_hit_region_order_from_key(root_key),
                )
            });
        return EguiWheelInputResolution {
            event: None,
            terminal_consumed: !blocked,
        };
    }
    let slipway_core::DeclaredWheelDisposition::Moved(owner) = resolved.disposition else {
        let (_, mut evidence) =
            slipway_core::resolve_bubbled_declared_wheel_dispatch_with_evidence_and_geometry_index(
                EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input"),
                context.frame.clone(),
                context.geometry_index,
                context.scroll_regions,
                resolved.point,
            );
        evidence.input_position_space = Some(resolved.position_space);
        refusals.push(evidence);
        return EguiWheelInputResolution {
            event: None,
            terminal_consumed: false,
        };
    };
    let (dispatch, mut evidence) =
        slipway_core::resolve_selected_declared_wheel_dispatch_with_evidence_and_geometry_index(
            EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input"),
            context.frame.clone(),
            context.geometry_index,
            context.scroll_regions,
            resolved.point,
            delta_x,
            delta_y,
            owner,
        );
    evidence.input_position_space = Some(resolved.position_space);
    let Some(dispatch) = dispatch else {
        // No consumer (dead point / at-limit): retain the already-constructed
        // refusal evidence instead of dropping it (audit finding MF-H3).
        refusals.push(evidence);
        return EguiWheelInputResolution {
            event: None,
            terminal_consumed: false,
        };
    };

    if let Some(occlusion) = egui_wheel_occlusion_region_at_position(context.regions, position) {
        let selected_key =
            egui_presented_scroll_region_by_id(context.regions, &dispatch.selected_region)
                .map(|region| region.paint_sort_key)
                .or_else(|| {
                    context
                        .scroll_regions
                        .iter()
                        .find(|region| region.id == dispatch.selected_region)
                        .map(|region| {
                            (
                                region.order.z_index,
                                region.order.paint_order,
                                region.order.traversal_order,
                            )
                        })
                });
        if selected_key.is_none_or(|key| {
            slipway_core::hit_region_order_is_front_of(
                &egui_hit_region_order_from_key(occlusion.paint_sort_key),
                &egui_hit_region_order_from_key(key),
            )
        }) {
            // Occluded wheel: the dispatch resolved but a front wheel-blocking
            // paint layer swallowed it — retain that as a refusal too (MF-H3).
            evidence.refusal_reason = Some(format!(
                "declared wheel consumer `{}` is occluded by a front wheel-blocking paint layer",
                dispatch.selected_region.as_str()
            ));
            refusals.push(evidence);
            return EguiWheelInputResolution {
                event: None,
                terminal_consumed: false,
            };
        }
    }

    EguiWheelInputResolution {
        event: Some(BackendInputEvent::declared(dispatch.input, evidence)),
        terminal_consumed: false,
    }
}

#[cfg(test)]
fn egui_backend_wheel_input_event(
    context: &EguiInputContext<'_>,
    position: egui::Pos2,
    delta_x: f32,
    delta_y: f32,
    refusals: &mut Vec<slipway_core::DeclaredEventDispatchEvidence>,
) -> Option<BackendInputEvent> {
    egui_backend_wheel_input_resolution(context, position, delta_x, delta_y, refusals).event
}

fn egui_presented_scroll_region_by_id<'a>(
    regions: &'a [EguiPresentedRegion],
    id: &PresentationRegionId,
) -> Option<&'a EguiPresentedRegion> {
    regions
        .iter()
        .find(|region| region.kind == EguiPresentedRegionKind::Scroll && region.region_id == *id)
}

#[cfg(test)]
fn egui_wheel_region_at_position(
    regions: &[EguiPresentedRegion],
    position: egui::Pos2,
    delta_x: f32,
    delta_y: f32,
) -> Option<&EguiPresentedRegion> {
    let mut candidates = regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.response.interact_rect.contains(position)
                && (region.kind == EguiPresentedRegionKind::Scroll
                    || region.kind == EguiPresentedRegionKind::Occlusion)
        })
        .collect::<Vec<_>>();
    candidates.sort_by_key(|region| region.paint_sort_key);

    for region in candidates.into_iter().rev() {
        if region.kind == EguiPresentedRegionKind::Occlusion {
            return None;
        }
        if region
            .scroll_state
            .as_ref()
            .is_some_and(|scroll| egui_scroll_can_move_for_wheel(scroll, delta_x, delta_y))
        {
            return Some(region);
        }
    }

    None
}

#[cfg(test)]
fn egui_scroll_can_move_for_wheel(
    scroll: &EguiScrollRegionState,
    delta_x: f32,
    delta_y: f32,
) -> bool {
    egui_scroll_axis_can_move(
        scroll.egui_offset.x,
        scroll.content_size.width,
        scroll.inner_rect.size.width,
        delta_x,
    ) || egui_scroll_axis_can_move(
        scroll.egui_offset.y,
        scroll.content_size.height,
        scroll.inner_rect.size.height,
        delta_y,
    )
}

#[cfg(test)]
fn egui_scroll_axis_can_move(offset: f32, content: f32, viewport: f32, delta: f32) -> bool {
    if delta.abs() <= f32::EPSILON {
        return false;
    }

    let max_offset = (content - viewport).max(0.0);
    if delta < 0.0 {
        offset < max_offset - f32::EPSILON
    } else {
        offset > f32::EPSILON
    }
}

fn egui_focus_backend_input_event(
    context: &EguiInputContext<'_>,
    region: &EguiPresentedRegion,
    kind: DeclaredEventDispatchKind,
    event: InputEvent,
) -> BackendInputEvent {
    let selected_region = context
        .focus_regions
        .iter()
        .find(|candidate| candidate.id == region.region_id);
    let evidence = slipway_core::declared_focus_text_dispatch_evidence_with_geometry_index(
        EvidenceSource::backend_presented(EGUI_BACKEND_ID, "focused-input"),
        context.frame.clone(),
        context.geometry_index,
        context.focus_regions,
        selected_region,
        kind,
        None,
        event.clone(),
    );
    BackendInputEvent::declared(event, evidence)
}

fn egui_scroll_backend_input_event(
    context: &EguiInputContext<'_>,
    region: &EguiPresentedRegion,
    mut event: InputEvent,
) -> Option<BackendInputEvent> {
    let selected_region = context
        .scroll_regions
        .iter()
        .find(|candidate| candidate.id == region.region_id);
    let selected_region = selected_region?;
    if let InputEvent::Scroll(scroll) = &mut event {
        scroll.target = selected_region.target.clone();
        scroll.target_slot = selected_region.address.clone();
        scroll.region_id = selected_region.id.clone();
        scroll.viewport = selected_region.viewport;
        scroll.content_bounds = selected_region.content_bounds;
    }
    let evidence = slipway_core::declared_scroll_dispatch_evidence_at_position(
        EvidenceSource::backend_presented(EGUI_BACKEND_ID, "native-scroll"),
        context.frame.clone(),
        context.scroll_regions,
        Some(selected_region),
        event.clone(),
        Some(
            slipway_core::declared_region_root_local_rect_with_geometry_index(
                context.geometry_index,
                &selected_region.target,
                selected_region.address.as_ref(),
                selected_region.viewport.into_rect(),
            )
            .origin,
        ),
    );
    Some(BackendInputEvent::declared(event, evidence))
}

fn egui_focused_backend_input_event(
    context: &EguiInputContext<'_>,
    focused_target: Option<&WidgetId>,
    kind: DeclaredEventDispatchKind,
    event: InputEvent,
) -> Option<BackendInputEvent> {
    if let Some(region) = focused_region(context.regions, focused_target) {
        Some(egui_focus_backend_input_event(context, region, kind, event))
    } else {
        None
    }
}

#[cfg(test)]
fn egui_region_target_bounds(region: &EguiPresentedRegion) -> Rect {
    region.target_bounds
}

fn target_local_bounds(target_rect: Rect) -> Rect {
    Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: target_rect.size,
    }
}

fn focused_region<'a>(
    regions: &'a [EguiPresentedRegion],
    focused_target: Option<&WidgetId>,
) -> Option<&'a EguiPresentedRegion> {
    let focused_target = focused_target?;
    regions.iter().find(|region| {
        &region.target == focused_target
            && (region.response.has_focus()
                || matches!(
                    region.kind,
                    EguiPresentedRegionKind::Focus | EguiPresentedRegionKind::TextEdit
                ))
    })
}

fn focused_event_target(
    regions: &[EguiPresentedRegion],
    focused_target: Option<&WidgetId>,
    root_target: &WidgetId,
) -> (WidgetId, Option<WidgetSlotAddress>) {
    focused_event_target_opt(regions, focused_target).unwrap_or_else(|| (root_target.clone(), None))
}

fn focused_event_target_opt(
    regions: &[EguiPresentedRegion],
    focused_target: Option<&WidgetId>,
) -> Option<(WidgetId, Option<WidgetSlotAddress>)> {
    if let Some(region) = focused_region(regions, focused_target) {
        Some((region.target.clone(), region.address.clone()))
    } else {
        focused_target.map(|target| (target.clone(), None))
    }
}

fn egui_composition_event(
    target: Option<WidgetId>,
    target_slot: Option<WidgetSlotAddress>,
    ime: &egui::ImeEvent,
) -> Option<TextCompositionEvent> {
    let target = target?;
    match ime {
        #[allow(deprecated)]
        egui::ImeEvent::Enabled => Some(TextCompositionEvent {
            target,
            target_slot,
            phase: TextCompositionPhase::Start,
            preedit_text: String::new(),
            cursor_range: None,
        }),
        egui::ImeEvent::Preedit {
            text,
            active_range_chars,
        } => Some(TextCompositionEvent {
            target,
            phase: if text.is_empty() {
                TextCompositionPhase::Cancel
            } else {
                TextCompositionPhase::Update
            },
            target_slot,
            preedit_text: text.clone(),
            cursor_range: active_range_chars.as_ref().map(|range| TextSelectionRange {
                anchor: range.start,
                focus: range.end,
            }),
        }),
        egui::ImeEvent::Commit(text) => Some(TextCompositionEvent {
            target,
            target_slot,
            phase: TextCompositionPhase::Commit,
            preedit_text: text.clone(),
            cursor_range: None,
        }),
        #[allow(deprecated)]
        egui::ImeEvent::Disabled => Some(TextCompositionEvent {
            target,
            target_slot,
            phase: TextCompositionPhase::End,
            preedit_text: String::new(),
            cursor_range: None,
        }),
    }
}

fn install_declared_fonts<W>(
    ui: &egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    view: &ViewDefinition,
) -> Vec<BackendParityAdmission>
where
    W: SlipwayEguiBackendChildWidget,
{
    install_declared_fonts_with_metrics(ui, widget, external, local, view).0
}

fn install_declared_fonts_with_metrics<W>(
    ui: &egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    view: &ViewDefinition,
) -> (Vec<BackendParityAdmission>, EguiFontInstallMetrics)
where
    W: SlipwayEguiBackendChildWidget,
{
    let mut diagnostics = Vec::new();
    let mut metrics = EguiFontInstallMetrics::default();

    visit_text_paint_entries(&view.paint, &mut |content, style| {
        collect_font_installation_diagnostics(
            ui.ctx(),
            widget,
            external,
            local,
            &view.target,
            content,
            style,
            None,
            &mut diagnostics,
            &mut metrics,
        );
    });

    (
        font_installation_admissions(view.target.clone(), diagnostics),
        metrics,
    )
}

fn install_text_edit_font<W>(
    ui: &egui::Ui,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    focus: &FocusRegionDeclaration,
    text_edit: &TextEditRegionDeclaration,
) -> Vec<BackendParityAdmission>
where
    W: SlipwayEguiBackendChildWidget,
{
    let mut diagnostics = Vec::new();
    let mut metrics = EguiFontInstallMetrics::default();
    collect_font_installation_diagnostics(
        ui.ctx(),
        widget,
        external,
        local,
        &focus.target,
        &text_edit.buffer.text,
        &text_edit.typography.style,
        text_edit.typography.source.as_ref(),
        &mut diagnostics,
        &mut metrics,
    );
    font_installation_admissions(focus.target.clone(), diagnostics)
}

fn collect_font_installation_diagnostics<W>(
    ctx: &egui::Context,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    target: &WidgetId,
    content: &str,
    style: &TextStyle,
    source: Option<&ResourceSourceDeclaration>,
    diagnostics: &mut Vec<Diagnostic>,
    metrics: &mut EguiFontInstallMetrics,
) where
    W: SlipwayEguiBackendChildWidget,
{
    if source_text_validity(content) != SourceValidityKind::Valid {
        return;
    }
    if source.is_none() && !text_font_installation_required(content, style) {
        return;
    }

    let request = FontResolutionRequest {
        family: style.font_family.clone(),
        fallback_families: egui_font_fallbacks(&style.font_family),
        weight: style.font_weight,
        style: style.font_style,
        source: source.cloned(),
    };
    let evidence = widget.resolve_font(external, local, request);
    let mut results = Vec::new();
    let mut installed_keys = Vec::new();
    if let Some(source) = evidence.request.source.as_ref() {
        installed_keys.push(egui_font_install_key(
            evidence.resolved_ref.as_deref(),
            source,
        ));
    }
    let result = install_font_from_evidence(
        ctx,
        evidence.resolved_ref.as_deref(),
        evidence.request.source.as_ref(),
    );
    metrics.record(result.status);
    results.push(result);
    if let Some(installation) = &evidence.installation {
        let duplicate = installation.source.as_ref().is_some_and(|source| {
            let key = egui_font_install_key(Some(&installation.resource_id), source);
            installed_keys.iter().any(|installed| installed == &key)
        });
        if !duplicate {
            let result = install_font_from_evidence(
                ctx,
                Some(&installation.resource_id),
                installation.source.as_ref(),
            );
            metrics.record(result.status);
            results.push(result);
        }
    }

    if results
        .iter()
        .any(EguiFontInstallResult::satisfies_requirement)
    {
        return;
    }

    diagnostics.push(font_installation_failure_diagnostic(
        target, content, style, &evidence, &results,
    ));
}

fn font_installation_admissions(
    target: WidgetId,
    diagnostics: Vec<Diagnostic>,
) -> Vec<BackendParityAdmission> {
    if diagnostics.is_empty() {
        return Vec::new();
    }

    let requirement_id = "view.font_resource_installation".to_string();
    vec![BackendParityAdmission {
        backend_id: EGUI_BACKEND_ID.to_string(),
        accepted: false,
        required_profiles: Vec::new(),
        visible_requirements: vec![BackendVisibleCapabilityRequirement {
            requirement_id: requirement_id.clone(),
            target: Some(target.clone()),
            capability: BackendVisibleCapability::FontInstallation,
            required: true,
        }],
        unsupported: vec![UnsupportedCapabilityEvidence {
            backend_id: EGUI_BACKEND_ID.to_string(),
            target: Some(target.clone()),
            capability: Capability::FontResourceInstallation,
            visible_capability: Some(BackendVisibleCapability::FontInstallation),
            requirement_id: Some(requirement_id),
            reason: "egui could not prove required font installation for visible text".to_string(),
            source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "font-installation"),
            diagnostics: diagnostics.clone(),
        }],
        source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "font-installation"),
        diagnostics,
    }]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum EguiFontInstallStatus {
    Queued,
    Installed,
    AlreadyInstalled,
    MissingSource,
    MissingAssetRef,
    ReadFailed,
    UnsupportedSource,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct EguiFontInstallMetrics {
    queued: usize,
    installed: usize,
    already_installed: usize,
    missing_source: usize,
    missing_asset_ref: usize,
    read_failed: usize,
    unsupported_source: usize,
}

impl EguiFontInstallMetrics {
    fn record(&mut self, status: EguiFontInstallStatus) {
        match status {
            EguiFontInstallStatus::Queued => self.queued += 1,
            EguiFontInstallStatus::Installed => self.installed += 1,
            EguiFontInstallStatus::AlreadyInstalled => self.already_installed += 1,
            EguiFontInstallStatus::MissingSource => self.missing_source += 1,
            EguiFontInstallStatus::MissingAssetRef => self.missing_asset_ref += 1,
            EguiFontInstallStatus::ReadFailed => self.read_failed += 1,
            EguiFontInstallStatus::UnsupportedSource => self.unsupported_source += 1,
        }
    }

    fn total(&self) -> usize {
        self.queued
            + self.installed
            + self.already_installed
            + self.missing_source
            + self.missing_asset_ref
            + self.read_failed
            + self.unsupported_source
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EguiFontInstallResult {
    status: EguiFontInstallStatus,
}

impl EguiFontInstallResult {
    fn satisfies_requirement(&self) -> bool {
        matches!(
            self.status,
            EguiFontInstallStatus::Queued
                | EguiFontInstallStatus::Installed
                | EguiFontInstallStatus::AlreadyInstalled
        )
    }
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
struct EguiFontInstallCache {
    records: Vec<EguiFontInstallRecord>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct EguiFontInstallRecord {
    key: String,
    status: EguiFontInstallStatus,
    queued_pass: Option<u64>,
}

fn install_font_from_evidence(
    ctx: &egui::Context,
    resolved_ref: Option<&str>,
    source: Option<&ResourceSourceDeclaration>,
) -> EguiFontInstallResult {
    let Some(source) = source else {
        return EguiFontInstallResult {
            status: EguiFontInstallStatus::MissingSource,
        };
    };
    let key = egui_font_install_key(resolved_ref, source);
    if let Some(cached) = cached_font_install_result(ctx, &key) {
        return match cached.status {
            EguiFontInstallStatus::Queued => cached,
            EguiFontInstallStatus::Installed => EguiFontInstallResult {
                status: EguiFontInstallStatus::AlreadyInstalled,
            },
            _ => cached,
        };
    };
    let bytes = match declared_font_bytes(source) {
        Ok(bytes) => bytes,
        Err(status) => {
            let result = EguiFontInstallResult { status };
            store_font_install_result(ctx, key, &result);
            return result;
        }
    };
    let name = resolved_ref
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(source.source_id.as_str());
    let named_family = egui::FontFamily::Name(name.to_owned().into());
    ctx.add_font(egui::epaint::text::FontInsert::new(
        name,
        egui::text::FontData::from_owned(bytes),
        vec![
            egui::epaint::text::InsertFontFamily {
                family: named_family,
                priority: egui::epaint::text::FontPriority::Highest,
            },
            egui::epaint::text::InsertFontFamily {
                family: egui::FontFamily::Proportional,
                priority: egui::epaint::text::FontPriority::Highest,
            },
            egui::epaint::text::InsertFontFamily {
                family: egui::FontFamily::Monospace,
                priority: egui::epaint::text::FontPriority::Lowest,
            },
        ],
    ));
    ctx.request_repaint();
    let result = EguiFontInstallResult {
        status: EguiFontInstallStatus::Queued,
    };
    store_font_install_result(ctx, key, &result);
    result
}

fn cached_font_install_result(ctx: &egui::Context, key: &str) -> Option<EguiFontInstallResult> {
    let current_pass = ctx.cumulative_pass_nr();
    let cache = ctx.data_mut(|data| data.get_persisted::<EguiFontInstallCache>(font_cache_id()))?;
    cache
        .records
        .iter()
        .find(|record| record.key == key)
        .map(|record| EguiFontInstallResult {
            status: if record.status == EguiFontInstallStatus::Queued
                && record
                    .queued_pass
                    .is_some_and(|queued_pass| current_pass > queued_pass)
            {
                EguiFontInstallStatus::Installed
            } else {
                record.status
            },
        })
}

fn store_font_install_result(ctx: &egui::Context, key: String, result: &EguiFontInstallResult) {
    let current_pass = ctx.cumulative_pass_nr();
    ctx.data_mut(|data| {
        let mut cache = data
            .get_persisted::<EguiFontInstallCache>(font_cache_id())
            .unwrap_or_default();
        if let Some(record) = cache.records.iter_mut().find(|record| record.key == key) {
            record.status = result.status;
            record.queued_pass = if result.status == EguiFontInstallStatus::Queued {
                Some(current_pass)
            } else {
                None
            };
        } else {
            cache.records.push(EguiFontInstallRecord {
                key,
                status: result.status,
                queued_pass: if result.status == EguiFontInstallStatus::Queued {
                    Some(current_pass)
                } else {
                    None
                },
            });
        }
        data.insert_persisted(font_cache_id(), cache);
    });
}

fn font_cache_id() -> egui::Id {
    egui::Id::new("slipway-egui-font-install-cache")
}

fn egui_font_install_key(resolved_ref: Option<&str>, source: &ResourceSourceDeclaration) -> String {
    let name = resolved_ref
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(source.source_id.as_str());
    let revision = source
        .revision
        .iter()
        .map(|entry| format!("{}={}", entry.name, entry.value))
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{name}|{}|{:?}|{}|{}|{revision}",
        source.source_id,
        source.kind,
        source.family.as_deref().unwrap_or(""),
        source.asset_ref.as_deref().unwrap_or("")
    )
}

fn declared_font_bytes(
    source: &ResourceSourceDeclaration,
) -> Result<Vec<u8>, EguiFontInstallStatus> {
    match source.kind {
        ResourceSourceKind::Asset | ResourceSourceKind::Embedded => {
            let asset_ref = source
                .asset_ref
                .as_deref()
                .ok_or(EguiFontInstallStatus::MissingAssetRef)?;
            fs::read(Path::new(asset_ref)).map_err(|_| EguiFontInstallStatus::ReadFailed)
        }
        ResourceSourceKind::BackendInstalled
        | ResourceSourceKind::SystemFamily
        | ResourceSourceKind::Custom(_) => Err(EguiFontInstallStatus::UnsupportedSource),
    }
}

fn font_installation_failure_diagnostic(
    target: &WidgetId,
    content: &str,
    style: &TextStyle,
    evidence: &slipway_core::FontResolutionEvidence,
    results: &[EguiFontInstallResult],
) -> Diagnostic {
    let statuses = results
        .iter()
        .map(|result| egui_font_install_status_label(result.status))
        .collect::<Vec<_>>()
        .join(", ");
    let code = if text_requires_cjk_font_evidence(content) {
        "egui.font.cjk_coverage_unproved"
    } else {
        "egui.font.installation_unproved"
    };
    let family = if style.font_family.trim().is_empty() {
        slipway_core::DEFAULT_TEXT_FONT_FAMILY
    } else {
        style.font_family.as_str()
    };
    let resolved = evidence
        .resolved_ref
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("<none>");
    Diagnostic::unsupported(
        Some(target.clone()),
        code,
        format!(
            "egui font installation for family '{family}' was not proven for visible text; resolved_ref={resolved}; install_status={statuses}"
        ),
    )
}

fn egui_font_install_status_label(status: EguiFontInstallStatus) -> &'static str {
    match status {
        EguiFontInstallStatus::Queued => "queued",
        EguiFontInstallStatus::Installed => "installed",
        EguiFontInstallStatus::AlreadyInstalled => "already_installed",
        EguiFontInstallStatus::MissingSource => "missing_source",
        EguiFontInstallStatus::MissingAssetRef => "missing_asset_ref",
        EguiFontInstallStatus::ReadFailed => "read_failed",
        EguiFontInstallStatus::UnsupportedSource => "unsupported_source",
    }
}

fn visit_text_paint_entries<'a, F>(ops: &'a [PaintOp], visitor: &mut F)
where
    F: FnMut(&'a str, &'a TextStyle),
{
    for op in ops {
        match op {
            PaintOp::Text { content, style, .. } => visitor(content.as_str(), style),
            PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                visit_text_paint_entries(ops, visitor)
            }
            PaintOp::Fill { .. } | PaintOp::Stroke { .. } => {}
        }
    }
}

fn egui_font_fallbacks(family: &str) -> Vec<String> {
    let mut fallbacks = vec!["system-ui".to_string(), "sans-serif".to_string()];
    if !family.trim().is_empty() && !fallbacks.iter().any(|fallback| fallback == family) {
        fallbacks.insert(0, family.to_string());
    }
    fallbacks
}

fn source_text_validity(content: &str) -> SourceValidityKind {
    if content.contains('\u{FFFD}') {
        return SourceValidityKind::InvalidUtf8;
    }

    let mojibake_markers = [
        "\u{00c3}",
        "\u{00c2}",
        "\u{00e2}\u{20ac}\u{2122}",
        "\u{00e2}\u{20ac}\u{0153}",
        "\u{00e2}\u{20ac}",
        "\u{00ec}",
        "\u{00ed}",
        "\u{00eb}",
    ];
    if mojibake_markers
        .iter()
        .any(|marker| content.contains(marker))
    {
        return SourceValidityKind::SuspectedMojibake;
    }

    SourceValidityKind::Valid
}

fn egui_pointer_button(button: egui::PointerButton) -> PointerButton {
    match button {
        egui::PointerButton::Primary => PointerButton::Primary,
        egui::PointerButton::Secondary => PointerButton::Secondary,
        egui::PointerButton::Middle | egui::PointerButton::Extra1 | egui::PointerButton::Extra2 => {
            PointerButton::Auxiliary
        }
    }
}

fn egui_pointer_details(
    modifiers: egui::Modifiers,
    button: Option<egui::PointerButton>,
) -> PointerDetails {
    let mut buttons = PointerButtons::default();
    match button {
        Some(egui::PointerButton::Primary) => buttons.primary = true,
        Some(egui::PointerButton::Secondary) => buttons.secondary = true,
        Some(
            egui::PointerButton::Middle | egui::PointerButton::Extra1 | egui::PointerButton::Extra2,
        ) => buttons.auxiliary = true,
        None => {}
    }

    PointerDetails {
        pointer_id: None,
        device: PointerDeviceKind::Mouse,
        buttons,
        modifiers: egui_modifiers(modifiers),
        pressure: None,
        tilt_x: None,
        tilt_y: None,
        twist: None,
    }
}

fn egui_modifiers(modifiers: egui::Modifiers) -> Modifiers {
    Modifiers {
        shift: modifiers.shift,
        control: modifiers.ctrl,
        alt: modifiers.alt,
        meta: modifiers.mac_cmd || modifiers.command,
    }
}

const EGUI_VISIBLE_ADMISSION_REFUSAL_MAX_LINES: usize = 14;
const EGUI_VISIBLE_ADMISSION_REFUSAL_MAX_CHARS: usize = 128;

fn paint_visible_admission_refusal(
    ui: &egui::Ui,
    rect: egui::Rect,
    admission: &BackendParityAdmission,
) {
    if rect.width() <= 1.0 || rect.height() <= 1.0 {
        return;
    }

    let painter = ui.painter_at(rect);
    let panel = rect.shrink(4.0);
    painter.rect_filled(panel, 6.0, egui::Color32::from_rgb(255, 247, 247));
    painter.rect_stroke(
        panel,
        6.0,
        egui::Stroke::new(1.5, egui::Color32::from_rgb(220, 38, 38)),
        egui::StrokeKind::Inside,
    );

    let clipped = painter.with_clip_rect(panel.shrink(8.0));
    let mut cursor = panel.min + egui::vec2(12.0, 10.0);
    for (index, line) in
        visible_admission_refusal_lines(admission, EGUI_VISIBLE_ADMISSION_REFUSAL_MAX_LINES)
            .iter()
            .enumerate()
    {
        let (font, color, line_height) = if index == 0 {
            (
                egui::FontId::proportional(14.0),
                egui::Color32::from_rgb(127, 29, 29),
                18.0,
            )
        } else {
            (
                egui::FontId::monospace(11.0),
                egui::Color32::from_rgb(69, 10, 10),
                15.0,
            )
        };
        clipped.text(cursor, egui::Align2::LEFT_TOP, line, font, color);
        cursor.y += line_height;
        if cursor.y > panel.bottom() - 12.0 {
            break;
        }
    }
}

fn visible_admission_refusal_lines(
    admission: &BackendParityAdmission,
    max_lines: usize,
) -> Vec<String> {
    let mut lines = vec![
        "Slipway visible admission refused".to_string(),
        format!("backend={} accepted=false", admission.backend_id),
    ];

    for unsupported in &admission.unsupported {
        let requirement = unsupported
            .requirement_id
            .as_deref()
            .unwrap_or("unknown-requirement");
        let target = unsupported
            .target
            .as_ref()
            .map(|target| target.as_str())
            .unwrap_or("unknown-target");
        lines.push(truncate_admission_refusal_line(&format!(
            "{target} {requirement}: {}",
            unsupported.reason
        )));
        for diagnostic in &unsupported.diagnostics {
            lines.push(diagnostic_admission_refusal_line(diagnostic));
        }
    }

    for diagnostic in &admission.diagnostics {
        lines.push(diagnostic_admission_refusal_line(diagnostic));
    }

    if lines.len() > max_lines {
        lines.truncate(max_lines.saturating_sub(1));
        lines.push("... more admission diagnostics available through MCP/debug".to_string());
    }

    lines
}

fn diagnostic_admission_refusal_line(diagnostic: &Diagnostic) -> String {
    let target = diagnostic
        .target
        .as_ref()
        .map(|target| target.as_str())
        .unwrap_or("unknown-target");
    truncate_admission_refusal_line(&format!(
        "{target} {:?} {}: {}",
        diagnostic.severity, diagnostic.code, diagnostic.message
    ))
}

fn truncate_admission_refusal_line(line: &str) -> String {
    if line.chars().count() <= EGUI_VISIBLE_ADMISSION_REFUSAL_MAX_CHARS {
        return line.to_string();
    }

    let mut truncated = line
        .chars()
        .take(EGUI_VISIBLE_ADMISSION_REFUSAL_MAX_CHARS.saturating_sub(3))
        .collect::<String>();
    truncated.push_str("...");
    truncated
}

fn paint_op(painter: &egui::Painter, origin: egui::Pos2, op: &PaintOp) {
    match op {
        PaintOp::Fill { shape, color } => paint_fill(painter, origin, shape, *color),
        PaintOp::Stroke {
            shape,
            color,
            width,
        } => paint_stroke(painter, origin, shape, *color, *width),
        PaintOp::Text {
            bounds,
            content,
            color,
            style,
        } => paint_text(painter, origin, *bounds, content, *color, style),
        PaintOp::Group { clip, ops, .. } | PaintOp::Layer { clip, ops, .. } => {
            if let Some(clip) = clip {
                let clipped = painter.with_clip_rect(egui_rect(origin, clip.bounds));
                for op in ops {
                    paint_op(&clipped, origin, op);
                }
            } else {
                for op in ops {
                    paint_op(painter, origin, op);
                }
            }
        }
    }
}

fn paint_fill(
    painter: &egui::Painter,
    origin: egui::Pos2,
    shape: &ShapeDeclaration,
    color: slipway_core::Color,
) {
    match shape.kind {
        ShapeKind::Rectangle => {
            painter.rect_filled(egui_rect(origin, shape.bounds), 0.0, egui_color(color));
        }
        ShapeKind::RoundedRectangle => {
            painter.rect_filled(egui_rect(origin, shape.bounds), 4.0, egui_color(color));
        }
        ShapeKind::Circle => {
            let rect = egui_rect(origin, shape.bounds);
            painter.circle_filled(
                rect.center(),
                rect.width().min(rect.height()) * 0.5,
                egui_color(color),
            );
        }
        ShapeKind::Line => paint_stroke(painter, origin, shape, color, 1.0),
        ShapeKind::Path => {
            if let Some((points, closed)) = egui_path_points(origin, shape.path.as_ref())
                && closed
                && points.len() >= 3
            {
                let clipped = painter.with_clip_rect(egui_rect(origin, shape.bounds));
                clipped.add(egui::epaint::PathShape::convex_polygon(
                    points,
                    egui_color(color),
                    egui::Stroke::NONE,
                ));
            }
        }
        ShapeKind::Text => {}
    }
}

fn paint_stroke(
    painter: &egui::Painter,
    origin: egui::Pos2,
    shape: &ShapeDeclaration,
    color: slipway_core::Color,
    width: f32,
) {
    let stroke = egui::Stroke::new(width.max(0.0), egui_color(color));
    match shape.kind {
        ShapeKind::Rectangle | ShapeKind::RoundedRectangle => {
            painter.rect_stroke(
                egui_rect(origin, shape.bounds),
                if shape.kind == ShapeKind::RoundedRectangle {
                    4.0
                } else {
                    0.0
                },
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        ShapeKind::Circle => {
            let rect = egui_rect(origin, shape.bounds);
            painter.circle_stroke(rect.center(), rect.width().min(rect.height()) * 0.5, stroke);
        }
        ShapeKind::Line => {
            let rect = egui_rect(origin, shape.bounds);
            painter.line_segment([rect.left_top(), rect.right_bottom()], stroke);
        }
        ShapeKind::Path => {
            if let Some((points, closed)) = egui_path_points(origin, shape.path.as_ref()) {
                let path = if closed {
                    egui::epaint::PathShape::closed_line(points, stroke)
                } else {
                    egui::epaint::PathShape::line(points, stroke)
                };
                painter
                    .with_clip_rect(egui_rect(origin, shape.bounds))
                    .add(path);
            }
        }
        ShapeKind::Text => {}
    }
}

fn egui_path_points(
    origin: egui::Pos2,
    path: Option<&PathDeclaration>,
) -> Option<(Vec<egui::Pos2>, bool)> {
    let path = path?;
    let mut points = Vec::new();
    let mut current = None;
    let mut closed = false;

    for command in &path.commands {
        match command {
            PathCommand::MoveTo(point) | PathCommand::LineTo(point) => {
                push_path_point(&mut points, *point)?;
                current = Some(*point);
            }
            PathCommand::QuadraticTo { control, to } => {
                let start = current?;
                if !point_is_finite(*control) || !point_is_finite(*to) {
                    return None;
                }
                for step in 1..=EGUI_PATH_CURVE_SEGMENTS {
                    let t = step as f32 / EGUI_PATH_CURVE_SEGMENTS as f32;
                    push_path_point(&mut points, quadratic_path_point(start, *control, *to, t))?;
                }
                current = Some(*to);
            }
            PathCommand::CubicTo {
                control_1,
                control_2,
                to,
            } => {
                let start = current?;
                if !point_is_finite(*control_1)
                    || !point_is_finite(*control_2)
                    || !point_is_finite(*to)
                {
                    return None;
                }
                for step in 1..=EGUI_PATH_CURVE_SEGMENTS {
                    let t = step as f32 / EGUI_PATH_CURVE_SEGMENTS as f32;
                    push_path_point(
                        &mut points,
                        cubic_path_point(start, *control_1, *control_2, *to, t),
                    )?;
                }
                current = Some(*to);
            }
            PathCommand::Close => {
                closed = true;
            }
        }
    }

    (points.len() >= 2).then(|| {
        (
            points
                .into_iter()
                .map(|point| egui_point(origin, point))
                .collect(),
            closed,
        )
    })
}

const EGUI_PATH_CURVE_SEGMENTS: usize = 16;

fn push_path_point(points: &mut Vec<Point>, point: Point) -> Option<()> {
    point_is_finite(point).then_some(())?;
    points.push(point);
    Some(())
}

fn point_is_finite(point: Point) -> bool {
    point.x.is_finite() && point.y.is_finite()
}

fn quadratic_path_point(start: Point, control: Point, end: Point, t: f32) -> Point {
    let one_minus = 1.0 - t;
    Point {
        x: one_minus * one_minus * start.x + 2.0 * one_minus * t * control.x + t * t * end.x,
        y: one_minus * one_minus * start.y + 2.0 * one_minus * t * control.y + t * t * end.y,
    }
}

fn cubic_path_point(start: Point, control_1: Point, control_2: Point, end: Point, t: f32) -> Point {
    let one_minus = 1.0 - t;
    Point {
        x: one_minus.powi(3) * start.x
            + 3.0 * one_minus.powi(2) * t * control_1.x
            + 3.0 * one_minus * t * t * control_2.x
            + t.powi(3) * end.x,
        y: one_minus.powi(3) * start.y
            + 3.0 * one_minus.powi(2) * t * control_1.y
            + 3.0 * one_minus * t * t * control_2.y
            + t.powi(3) * end.y,
    }
}

fn egui_point(origin: egui::Pos2, point: Point) -> egui::Pos2 {
    egui::pos2(origin.x + point.x, origin.y + point.y)
}

fn egui_rect(origin: egui::Pos2, rect: Rect) -> egui::Rect {
    egui::Rect::from_min_size(
        egui::pos2(origin.x + rect.origin.x, origin.y + rect.origin.y),
        egui::vec2(rect.size.width.max(0.0), rect.size.height.max(0.0)),
    )
}

fn egui_color(color: slipway_core::Color) -> egui::Color32 {
    egui::Rgba::from_rgba_unmultiplied(
        color.red.clamp(0.0, 1.0),
        color.green.clamp(0.0, 1.0),
        color.blue.clamp(0.0, 1.0),
        color.alpha.clamp(0.0, 1.0),
    )
    .into()
}

fn egui_corner_radius(radius: f32) -> egui::CornerRadius {
    egui::CornerRadius::same(radius.clamp(0.0, u8::MAX as f32).round() as u8)
}

fn apply_text_input_visuals_to_egui_scope(
    ui: &mut egui::Ui,
    visual_style: &TextInputVisualStyleDeclaration,
) {
    let value_color = egui_color(visual_style.value_color);
    let placeholder_color = egui_color(visual_style.placeholder_color);
    let background_color = egui_color(visual_style.background_color);
    let border_color = egui_color(visual_style.border_color);
    let selection_color = egui_color(visual_style.selection_color);
    let border = egui::Stroke::new(visual_style.border_width.max(0.0), border_color);
    let corner_radius = egui_corner_radius(visual_style.border_radius);

    let visuals = &mut ui.style_mut().visuals;
    visuals.override_text_color = Some(value_color);
    visuals.weak_text_color = Some(placeholder_color);
    visuals.text_edit_bg_color = Some(background_color);
    visuals.selection.bg_fill = selection_color;
    visuals.selection.stroke = egui::Stroke::new(1.0, value_color);

    for widget_visuals in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget_visuals.bg_fill = background_color;
        widget_visuals.weak_bg_fill = background_color;
        widget_visuals.bg_stroke = border;
        widget_visuals.fg_stroke.color = value_color;
        widget_visuals.corner_radius = corner_radius;
    }
}

fn paint_text(
    painter: &egui::Painter,
    origin: egui::Pos2,
    bounds: Rect,
    content: &str,
    color: slipway_core::Color,
    style: &TextStyle,
) {
    let rect = egui_rect(origin, bounds);
    let clipped = painter.with_clip_rect(rect);
    let text_color = egui_color(color);
    let galley = clipped.layout_job(egui_text_layout_job(
        content,
        text_color,
        style,
        rect.width().max(0.0),
    ));
    // The galley's `halign` (set from the declared `align_x` in
    // `egui_text_layout_job`) makes the position's x an alignment ANCHOR
    // (left edge / center / right edge per row); vertical anchoring is
    // caller-side, from the galley's measured height. `Start`/`Top` keeps
    // the historical `egui_text_position` top-left byte-identically.
    let position = egui_text_anchor(rect, style, galley.size());

    clipped.galley(position, Arc::clone(&galley), text_color);
}

fn egui_text_layout_job(
    content: &str,
    color: egui::Color32,
    style: &TextStyle,
    wrap_width: f32,
) -> egui::text::LayoutJob {
    let mut job =
        egui::text::LayoutJob::simple_format(content.to_string(), egui_text_format(color, style));
    job.halign = egui_text_halign(style.align_x);
    // The declared per-op wrap mode (NC-4): `Word` (the unspecified
    // default) keeps the historical wrap-at-rect-width byte-identically;
    // `None` turns soft wrapping off entirely (`max_width = INFINITY` is
    // egui's documented opt-out — explicit `\n` still breaks rows) and
    // the row clips at the op rect per the existing clip contract.
    // egui's `break_anywhere` glyph-level option is deliberately NOT
    // exposed — iced has no parity-equivalent word-first mode (ADR-0004).
    job.wrap.max_width = match style.wrap {
        slipway_core::TextWrap::Word if wrap_width.is_finite() && wrap_width > 0.0 => wrap_width,
        slipway_core::TextWrap::Word | slipway_core::TextWrap::None => f32::INFINITY,
    };
    job
}

fn egui_text_halign(align_x: slipway_core::TextAlignX) -> egui::Align {
    match align_x {
        // `Align::LEFT` is the `LayoutJob` default — `Start` stays
        // byte-identical to the pre-alignment layout.
        slipway_core::TextAlignX::Start => egui::Align::LEFT,
        slipway_core::TextAlignX::Center => egui::Align::Center,
        slipway_core::TextAlignX::End => egui::Align::RIGHT,
    }
}

/// The galley draw position for the declared alignment: x is the row
/// anchor matching the galley's `halign` (rect left edge / center /
/// right edge), y anchors the measured galley block within the rect
/// (`Top` = the historical `egui_text_position`, byte-identical), plus
/// the baseline-shift y offset that function applies.
fn egui_text_anchor(rect: egui::Rect, style: &TextStyle, galley_size: egui::Vec2) -> egui::Pos2 {
    let base = egui_text_position(rect, style);
    let x = match style.align_x {
        slipway_core::TextAlignX::Start => base.x,
        slipway_core::TextAlignX::Center => rect.center().x,
        slipway_core::TextAlignX::End => rect.right(),
    };
    let y = match style.align_y {
        slipway_core::TextAlignY::Top => base.y,
        slipway_core::TextAlignY::Center => base.y + (rect.height() - galley_size.y) / 2.0,
        slipway_core::TextAlignY::Bottom => base.y + rect.height() - galley_size.y,
    };
    egui::pos2(x, y)
}

/// The egui backend's REAL paint-text metric provider (roadmap Phase 6
/// item 3b slice (iii), audit NC-4): lays the requested content out
/// through the SAME galley pipeline the visible painter draws with —
/// `egui_text_layout_job` (identical font/size/halign/wrap mapping as
/// `paint_text`) laid out by the live `egui::Context`'s fonts — so the
/// measured size equals what a `PaintOp::Text` with that style presents.
/// Injected into `SlipwayLogic::project_text_metrics` via
/// `SlipwayRuntime::project_text_metrics` on the presented-viewport sync
/// cadence. `available_bounds: Some(rect)` measures wrapped layout at
/// that rect's width; `None` measures the intrinsic (unbounded) size.
/// Receipts are always `Valid` with `line_count` from the real galley
/// rows; `baseline` is not reported (`None`), never fabricated.
pub struct EguiTextMetricProvider<'a> {
    ctx: &'a egui::Context,
}

impl<'a> EguiTextMetricProvider<'a> {
    pub fn new(ctx: &'a egui::Context) -> Self {
        Self { ctx }
    }
}

impl slipway_core::SlipwayTextMetricProvider for EguiTextMetricProvider<'_> {
    fn text_metric_source(&self) -> slipway_core::TextMetricSource {
        slipway_core::TextMetricSource {
            provider_id: "slipway-backend-egui.paint-text-metrics".to_string(),
            backend_id: Some(EGUI_BACKEND_ID.to_string()),
            api_name: "egui::Context::fonts_mut/FontsView::layout_job".to_string(),
            kind: slipway_core::TextMetricSourceKind::OfficialBackendApi,
        }
    }

    fn measure_text(
        &mut self,
        request: slipway_core::TextMeasurementRequest,
    ) -> slipway_core::TextMeasurementReceipt {
        let wrap_width = request
            .available_bounds
            .map(|rect| rect.size.width.max(0.0))
            .unwrap_or(f32::INFINITY);
        // Color does not affect galley geometry; the job is otherwise the
        // exact shape `paint_text` submits for the same style.
        let job = egui_text_layout_job(
            &request.content,
            egui::Color32::WHITE,
            &request.style,
            wrap_width,
        );
        let galley = self.ctx.fonts_mut(|fonts| fonts.layout_job(job));
        let size = galley.size();
        let measured = Size {
            width: size.x,
            height: size.y,
        };
        let origin = request
            .available_bounds
            .map(|rect| rect.origin)
            .unwrap_or(Point { x: 0.0, y: 0.0 });
        slipway_core::TextMeasurementReceipt::Valid(slipway_core::ValidTextMeasurement {
            source: slipway_core::SlipwayTextMetricProvider::text_metric_source(self),
            facts: slipway_core::TextMeasurementFacts {
                measured_size: measured,
                content_bounds: Rect {
                    origin,
                    size: measured,
                },
                baseline: None,
                line_count: Some(galley.rows.len()),
                caret_bounds: Vec::new(),
            },
            request,
        })
    }
}

fn egui_text_format(color: egui::Color32, style: &TextStyle) -> egui::text::TextFormat {
    let font_id = egui::FontId::new(egui_text_font_size(style), egui_font_family(style));
    let mut format = egui::text::TextFormat::simple(font_id, color);
    format.italics = style.font_style == FontStyle::Italic;
    format.underline = egui_decoration_stroke(style.decoration.underline, color);
    format.strikethrough = egui_decoration_stroke(style.decoration.strikethrough, color);
    format.valign = egui_text_valign(style.baseline);
    format.line_height = Some(normalized_text_size(style) * 1.2);
    format
        .coords
        .push("wght", egui_font_weight_value(style.font_weight));
    format
}

fn egui_font_family(style: &TextStyle) -> egui::FontFamily {
    let family = style.font_family.trim();
    if family.eq_ignore_ascii_case("monospace")
        || family.eq_ignore_ascii_case("ui-monospace")
        || family.eq_ignore_ascii_case("monospaced")
    {
        egui::FontFamily::Monospace
    } else if family.eq_ignore_ascii_case("system-ui")
        || family.eq_ignore_ascii_case("sans")
        || family.eq_ignore_ascii_case("sans-serif")
        || family.eq_ignore_ascii_case("proportional")
    {
        egui::FontFamily::Proportional
    } else {
        egui::FontFamily::Proportional
    }
}

fn egui_text_input_font_family(
    ctx: &egui::Context,
    text_edit: &TextEditRegionDeclaration,
) -> egui::FontFamily {
    if let Some((family, source)) = text_edit.typography.source.as_ref().and_then(|source| {
        source
            .family
            .as_deref()
            .map(str::trim)
            .filter(|family| !family.is_empty())
            .map(|family| (family, source))
    }) && cached_font_install_result(ctx, &egui_font_install_key(Some(family), source))
        .is_some_and(|result| result.status == EguiFontInstallStatus::Installed)
    {
        egui::FontFamily::Name(family.to_owned().into())
    } else {
        egui_font_family(&text_edit.typography.style)
    }
}

fn egui_text_font_size(style: &TextStyle) -> f32 {
    let size = normalized_text_size(style);
    match style.baseline {
        BaselineShift::Normal => size,
        BaselineShift::Superscript | BaselineShift::Subscript => size * 0.75,
    }
}

fn normalized_text_size(style: &TextStyle) -> f32 {
    if style.font_size.is_finite() {
        style.font_size.max(1.0)
    } else {
        slipway_core::DEFAULT_TEXT_FONT_SIZE
    }
}

fn egui_font_weight_value(weight: FontWeight) -> f32 {
    match weight {
        FontWeight::Normal => 400.0,
        FontWeight::Bold => 700.0,
        FontWeight::Weight(value) => value.clamp(1, 1000) as f32,
    }
}

fn egui_decoration_stroke(enabled: bool, color: egui::Color32) -> egui::Stroke {
    if enabled {
        egui::Stroke::new(1.0, color)
    } else {
        egui::Stroke::NONE
    }
}

fn egui_text_valign(baseline: BaselineShift) -> egui::Align {
    match baseline {
        BaselineShift::Normal => egui::Align::BOTTOM,
        BaselineShift::Superscript => egui::Align::TOP,
        BaselineShift::Subscript => egui::Align::BOTTOM,
    }
}

fn egui_text_position(rect: egui::Rect, style: &TextStyle) -> egui::Pos2 {
    let y_offset = match style.baseline {
        BaselineShift::Normal => 0.0,
        BaselineShift::Superscript => -normalized_text_size(style) * 0.35,
        BaselineShift::Subscript => normalized_text_size(style) * 0.2,
    };
    rect.left_top() + egui::vec2(0.0, y_offset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    #[allow(unused_imports)]
    use std::process::{Command, Stdio};
    use std::rc::Rc;
    use std::thread;
    use std::time::{Duration, Instant};

    use slipway_core::{
        BaselineShift, BoxSpacing, CaretGeometryEvidence, CaretSet, CommandEvent, EventRoute,
        EventRoutePhase, FontResolutionEvidence, FontStyle, FontWeight, FrameIdentity,
        HitRegionOrder, ImeCompositionPolicyDeclaration, PaintOrderDeclaration, Point, Rect,
        ScrollAxes, ScrollConsumptionPolicy, Size, SlipwayLogic, SlipwaySsot, SlipwayView,
        SlipwayWidgetListVisitor, SlipwayWidgetTypes, TextBufferSnapshot, TextDecoration,
        TextSelectionPolicyDeclaration, TextStyle, WheelRouting,
    };
    use slipway_debug_bridge::{
        DebugCommand, DebugPhysicalControl, DebugReplyProduct, MessageDisposition,
    };
    use slipway_runtime::SlipwayRuntime;

    macro_rules! zero_spacing_layout_input {
        (viewport: $viewport:expr, constraints: $constraints:expr $(,)?) => {{
            let viewport = $viewport;
            LayoutInput {
                viewport,
                content: viewport,
                constraints: $constraints,
            }
        }};
    }

    fn test_parent_local(rect: Rect) -> slipway_core::ParentLocalRect {
        let seed = slipway_core::ChildLayoutSeed {
            child: WidgetId::from("test-child"),
            local_state_slot: None,
        };
        let plan = slipway_core::ChildLayoutPlan::explicit_border(
            seed.clone(),
            slipway_core::ContentLocalRect::new(rect),
            BoxSpacing::ZERO,
        );
        let child_bounds = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: rect.size,
        });
        let child_input = LayoutInput {
            viewport: child_bounds,
            content: child_bounds,
            constraints: LayoutConstraints {
                min: rect.size,
                max: rect.size,
            },
        };
        let result = slipway_core::ChildLayoutResult {
            seed,
            layout: slipway_core::prepare_leaf_layout(
                test_layout_builder(&child_input),
                child_bounds,
            ),
            diagnostics: Vec::new(),
        };
        let root_bounds = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: rect.size,
        });
        let root_input = LayoutInput {
            viewport: root_bounds,
            content: root_bounds,
            constraints: LayoutConstraints {
                min: rect.size,
                max: rect.size,
            },
        };
        slipway_core::prepare_resolved_layout(
            test_layout_builder(&root_input),
            root_bounds,
            [(plan, result)],
        )
        .unwrap()
        .child_placements()[0]
            .bounds
    }

    fn test_layout_builder(input: &LayoutInput) -> slipway_core::LayoutOutputBuilder {
        let frame = FrameIdentity {
            surface_id: "test".to_string(),
            surface_instance_id: "test".to_string(),
            revision: 0,
            frame_index: 0,
            viewport: input.viewport.into_rect(),
        };
        let (_, _, output) = ViewDefinitionInput::new(frame, input.clone()).into_layout_parts();
        output
    }

    fn test_leaf_layout(bounds: TargetLocalRect) -> LayoutOutput {
        let input = LayoutInput {
            viewport: bounds,
            content: bounds,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: bounds.into_rect().size,
            },
        };
        slipway_core::prepare_leaf_layout(test_layout_builder(&input), bounds)
    }

    fn test_layout_with_placements(
        bounds: TargetLocalRect,
        placements: impl IntoIterator<Item = ChildPlacement>,
    ) -> LayoutOutput {
        let input = LayoutInput {
            viewport: bounds,
            content: bounds,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: bounds.into_rect().size,
            },
        };
        test_layout_with_placements_from(test_layout_builder(&input), bounds, placements)
    }

    fn test_layout_with_placements_from(
        output: slipway_core::LayoutOutputBuilder,
        bounds: TargetLocalRect,
        placements: impl IntoIterator<Item = ChildPlacement>,
    ) -> LayoutOutput {
        let resolved = placements.into_iter().map(|placement| {
            let seed = slipway_core::ChildLayoutSeed {
                child: placement.child,
                local_state_slot: placement.local_state_slot,
            };
            let border = placement.bounds.into_rect();
            let plan = slipway_core::ChildLayoutPlan::explicit_border(
                seed.clone(),
                slipway_core::ContentLocalRect::new(border),
                placement.spacing,
            );
            let child_bounds = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: border.size,
            });
            let child_input = LayoutInput {
                viewport: child_bounds,
                content: child_bounds,
                constraints: LayoutConstraints {
                    min: border.size,
                    max: border.size,
                },
            };
            let result = slipway_core::ChildLayoutResult {
                seed,
                layout: slipway_core::prepare_leaf_layout(
                    test_layout_builder(&child_input),
                    child_bounds,
                ),
                diagnostics: Vec::new(),
            };
            (plan, result)
        });
        slipway_core::prepare_resolved_layout(output, bounds, resolved).unwrap()
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ProbeWidget {
        id: WidgetId,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct FocusedProbeWidget {
        id: WidgetId,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ScrollProbeWidget {
        id: WidgetId,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TallRootWidget {
        id: WidgetId,
        height: f32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct LayeredPaintChild {
        id: WidgetId,
        z_index: i32,
        order: Option<usize>,
        paint_order_mode: PaintOrderMode,
        paint_bounds: Rect,
        color: slipway_core::Color,
        overflow_bounds: Option<Rect>,
        inner_layer: Option<(i32, Option<usize>, slipway_core::Color)>,
        hit_region: Option<(Rect, HitRegionOrder)>,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct LayeredPaintApp {
        children: (LayeredPaintChild, LayeredPaintChild),
        root_fill: Option<slipway_core::Color>,
        root_layer: Option<(i32, Option<usize>, slipway_core::Color)>,
    }

    #[derive(Clone, Debug, PartialEq)]
    enum ProbeMessage {
        Routed,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct MountedCollectorApp {
        id: WidgetId,
        widgets: (ProbeWidget,),
    }

    impl slipway_core::SlipwayApp for MountedCollectorApp {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = ProbeMessage;
        type Widgets = (ProbeWidget,);

        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {}
    }

    #[derive(Default)]
    struct MountedSlotTrace {
        slots: Vec<WidgetSlotAddress>,
    }

    impl SlipwayEguiWidgetListVisitor<(), ProbeMessage> for MountedSlotTrace {
        fn visit_egui_child<W>(
            &mut self,
            _widget: &W,
            _external: &(),
            _local: &W::LocalState,
            slot: WidgetSlotAddress,
        ) where
            W: SlipwayEguiBackendChildWidget<ExternalState = (), AppMessage = ProbeMessage>,
        {
            self.slots.push(slot);
        }

        fn visit_egui_native_child<N>(
            &mut self,
            _widget: &N,
            _external: &(),
            _local: &N::LocalState,
            slot: WidgetSlotAddress,
        ) where
            N: SlipwayEguiNativeChildWidget<ExternalState = (), AppMessage = ProbeMessage>,
        {
            self.slots.push(slot);
        }
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Nc9Counter {
        NonNested,
        One,
        Two,
        Three,
        Left,
        Right,
    }

    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    struct Nc9Counters {
        non_nested: u32,
        one: u32,
        two: u32,
        three: u32,
        left: u32,
        right: u32,
    }

    impl Nc9Counters {
        fn increment(&mut self, counter: Nc9Counter) {
            *self.value_mut(counter) += 1;
        }

        fn value(&self, counter: Nc9Counter) -> u32 {
            match counter {
                Nc9Counter::NonNested => self.non_nested,
                Nc9Counter::One => self.one,
                Nc9Counter::Two => self.two,
                Nc9Counter::Three => self.three,
                Nc9Counter::Left => self.left,
                Nc9Counter::Right => self.right,
            }
        }

        fn value_mut(&mut self, counter: Nc9Counter) -> &mut u32 {
            match counter {
                Nc9Counter::NonNested => &mut self.non_nested,
                Nc9Counter::One => &mut self.one,
                Nc9Counter::Two => &mut self.two,
                Nc9Counter::Three => &mut self.three,
                Nc9Counter::Left => &mut self.left,
                Nc9Counter::Right => &mut self.right,
            }
        }

        fn total(&self) -> u32 {
            self.non_nested + self.one + self.two + self.three + self.left + self.right
        }
    }

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Nc9Message {
        Increment(Nc9Counter),
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Nc9Leaf {
        counter: Nc9Counter,
    }

    impl Nc9Leaf {
        fn id_value() -> WidgetId {
            WidgetId::from("nc9.leaf")
        }

        fn region_id(&self) -> PresentationRegionId {
            PresentationRegionId::from(format!("nc9.hit.{:?}", self.counter))
        }
    }

    impl SlipwayWidgetTypes for Nc9Leaf {
        type ExternalState = Nc9Counters;
        type LocalState = u32;
        type AppMessage = Nc9Message;
    }

    impl SlipwaySsot for Nc9Leaf {
        fn id(&self) -> WidgetId {
            Self::id_value()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::PointerInput,
                Capability::Paint,
                Capability::StateObservation,
            ]
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

    impl SlipwayLogic for Nc9Leaf {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            let before = *local;
            *local += 1;
            EventOutcome {
                handled: true,
                propagate: false,
                emitted_messages: vec![slipway_core::EmittedMessage {
                    target: self.id(),
                    name: "nc9-increment".to_string(),
                    message: Nc9Message::Increment(self.counter),
                }],
                changes: vec![slipway_core::ChangeEvidence {
                    target: self.id(),
                    slot: Some(WidgetSlotAddress::new(self.id(), 0)),
                    field: "count".to_string(),
                    before: Some(before.to_string()),
                    after: Some(local.to_string()),
                }],
                observations: self.observe_state(_external, local),
                probes: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayView for Nc9Leaf {
        fn initial_local_state(&self) -> Self::LocalState {
            0
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            Vec::new()
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![StateObservation {
                target: self.id(),
                slot: Some(WidgetSlotAddress::new(self.id(), 0)),
                name: "count".to_string(),
                value: local.to_string(),
            }]
        }
    }

    impl slipway_core::SlipwayEventRoutingPolicy for Nc9Leaf {
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
                route: EventRoute {
                    route_id: Some(format!("nc9.route.{:?}", self.counter)),
                    path: address
                        .as_ref()
                        .map(|address| address.path.clone())
                        .unwrap_or_else(|| vec![self.id()]),
                    address,
                    phase: EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for Nc9Leaf {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            route: &EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let handled = event.target() == &self.id();
            let disposition = slipway_core::EventDisposition {
                handled,
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

    impl SlipwayFontResolutionPolicy for Nc9Leaf {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayViewDefinition for Nc9Leaf {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: vec![slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    self.region_id(),
                    None,
                    *layout.bounds(),
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    Some(format!("nc9.route.{:?}", self.counter)),
                    CursorCapability::Pointer,
                    true,
                    PointerCaptureIntent::OnPress,
                )],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayEguiAuthoredChildren for Nc9Leaf {
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

    #[derive(Clone, Debug, PartialEq)]
    struct Nc9App<Widgets> {
        id: WidgetId,
        widgets: Widgets,
    }

    impl<Widgets> slipway_core::SlipwayApp for Nc9App<Widgets>
    where
        Widgets: slipway_core::SlipwayWidgetList<ExternalState = Nc9Counters, AppMessage = Nc9Message>
            + slipway_core::SlipwayWidgetListViewDefinition<
                ExternalState = Nc9Counters,
                AppMessage = Nc9Message,
            >,
    {
        type ExternalState = Nc9Counters;
        type LocalState = ();
        type AppMessage = Nc9Message;
        type Widgets = Widgets;

        fn id(&self) -> WidgetId {
            self.id.clone()
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
            children: Vec<slipway_core::ChildLayoutSeed>,
        ) -> slipway_core::AppLayoutPlan {
            let siblings = self.id.as_str() == "nc9.root.siblings";
            slipway_core::AppLayoutPlan {
                bounds: input.viewport,
                children: children
                    .into_iter()
                    .enumerate()
                    .map(|(index, seed)| {
                        let bounds = if siblings {
                            Rect {
                                origin: Point {
                                    x: index as f32 * 50.0,
                                    y: 0.0,
                                },
                                size: Size {
                                    width: 40.0,
                                    height: 40.0,
                                },
                            }
                        } else {
                            input.viewport.into_rect()
                        };
                        slipway_core::ChildLayoutPlan::explicit_border(
                            seed,
                            slipway_core::ContentLocalRect::new(bounds),
                            BoxSpacing::ZERO,
                        )
                    })
                    .collect(),
                diagnostics: Vec::new(),
            }
        }
    }

    fn nc9_app<Widgets>(
        id: &str,
        widgets: Widgets,
    ) -> slipway_core::SlipwayAppWidget<Nc9App<Widgets>>
    where
        Widgets: slipway_core::SlipwayWidgetList<ExternalState = Nc9Counters, AppMessage = Nc9Message>
            + slipway_core::SlipwayWidgetListViewDefinition<
                ExternalState = Nc9Counters,
                AppMessage = Nc9Message,
            >,
    {
        slipway_core::SlipwayAppWidget::new(Nc9App {
            id: WidgetId::from(id),
            widgets,
        })
    }

    fn reduce_nc9(external: &mut Nc9Counters, messages: Vec<Nc9Message>) {
        for message in messages {
            let Nc9Message::Increment(counter) = message;
            external.increment(counter);
        }
    }

    #[derive(Default)]
    struct Nc9InputCaptureBridge {
        inner: DefaultEguiBridge,
        inputs: Vec<BackendInputEvent>,
    }

    impl<W> EguiSlipwayBridge<W> for Nc9InputCaptureBridge
    where
        W: SlipwayEguiBackendChildWidget,
    {
        fn layout_input(&mut self, context: EguiLayoutContext<'_>) -> LayoutInput {
            <DefaultEguiBridge as EguiSlipwayBridge<W>>::layout_input(&mut self.inner, context)
        }

        fn desired_size(&mut self, layout: &LayoutOutput) -> egui::Vec2 {
            <DefaultEguiBridge as EguiSlipwayBridge<W>>::desired_size(&mut self.inner, layout)
        }

        fn input_events(&mut self, context: EguiInputContext<'_>) -> Vec<BackendInputEvent> {
            let inputs =
                <DefaultEguiBridge as EguiSlipwayBridge<W>>::input_events(&mut self.inner, context);
            self.inputs.extend(inputs.iter().cloned());
            inputs
        }

        fn paint(&mut self, context: EguiPaintContext<'_>, ops: &[PaintOp]) {
            <DefaultEguiBridge as EguiSlipwayBridge<W>>::paint(&mut self.inner, context, ops);
        }

        fn messages(&mut self, outcome: EventOutcome<W::AppMessage>) -> Vec<W::AppMessage> {
            <DefaultEguiBridge as EguiSlipwayBridge<W>>::messages(&mut self.inner, outcome)
        }
    }

    fn test_rgb(red: u8, green: u8, blue: u8) -> slipway_core::Color {
        slipway_core::Color {
            red: f32::from(red) / 255.0,
            green: f32::from(green) / 255.0,
            blue: f32::from(blue) / 255.0,
            alpha: 1.0,
        }
    }

    #[derive(Default)]
    struct TwoCommandBridge {
        emitted: bool,
    }

    #[derive(Default)]
    struct DirectCommandBridge {
        emitted: bool,
    }

    #[derive(Default)]
    struct ForgedDeclaredBridge {
        emitted: bool,
    }

    #[derive(Default)]
    struct HandledWheelBridge {
        emitted: bool,
    }

    impl EguiSlipwayBridge<ProbeWidget> for TwoCommandBridge {
        fn layout_input(&mut self, _context: EguiLayoutContext<'_>) -> LayoutInput {
            zero_spacing_layout_input! {
                viewport: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 40.0,
                    },
                }),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: Size {
                        width: 100.0,
                        height: 40.0,
                    },
                },
            }
        }

        fn desired_size(&mut self, layout: &LayoutOutput) -> egui::Vec2 {
            egui::vec2(
                layout.bounds().as_rect().size.width,
                layout.bounds().as_rect().size.height,
            )
        }

        fn input_events(&mut self, context: EguiInputContext<'_>) -> Vec<BackendInputEvent> {
            if self.emitted {
                return Vec::new();
            }
            self.emitted = true;
            [PointerEventKind::Press, PointerEventKind::Release]
                .into_iter()
                .map(|kind| declared_egui_probe_pointer_input(&context, kind))
                .collect()
        }

        fn paint(&mut self, _context: EguiPaintContext<'_>, _ops: &[PaintOp]) {}

        fn messages(&mut self, outcome: EventOutcome<ProbeMessage>) -> Vec<ProbeMessage> {
            outcome
                .emitted_messages
                .into_iter()
                .map(|message| message.message)
                .collect()
        }
    }

    impl EguiSlipwayBridge<ProbeWidget> for DirectCommandBridge {
        fn layout_input(&mut self, _context: EguiLayoutContext<'_>) -> LayoutInput {
            zero_spacing_layout_input! {
                viewport: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 40.0,
                    },
                }),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: Size {
                        width: 100.0,
                        height: 40.0,
                    },
                },
            }
        }

        fn desired_size(&mut self, layout: &LayoutOutput) -> egui::Vec2 {
            egui::vec2(
                layout.bounds().as_rect().size.width,
                layout.bounds().as_rect().size.height,
            )
        }

        fn input_events(&mut self, context: EguiInputContext<'_>) -> Vec<BackendInputEvent> {
            if self.emitted {
                return Vec::new();
            }
            self.emitted = true;
            vec![BackendInputEvent::direct(InputEvent::Command(
                CommandEvent {
                    target: context.widget_id.clone(),
                    target_slot: None,
                    command: "undeclared".to_string(),
                    payload_ref: None,
                    source: None,
                },
            ))]
        }

        fn paint(&mut self, _context: EguiPaintContext<'_>, _ops: &[PaintOp]) {}

        fn messages(&mut self, outcome: EventOutcome<ProbeMessage>) -> Vec<ProbeMessage> {
            outcome
                .emitted_messages
                .into_iter()
                .map(|message| message.message)
                .collect()
        }
    }

    impl EguiSlipwayBridge<ProbeWidget> for ForgedDeclaredBridge {
        fn layout_input(&mut self, context: EguiLayoutContext<'_>) -> LayoutInput {
            TwoCommandBridge::default().layout_input(context)
        }

        fn desired_size(&mut self, layout: &LayoutOutput) -> egui::Vec2 {
            egui::vec2(
                layout.bounds().as_rect().size.width,
                layout.bounds().as_rect().size.height,
            )
        }

        fn input_events(&mut self, context: EguiInputContext<'_>) -> Vec<BackendInputEvent> {
            if self.emitted {
                return Vec::new();
            }
            self.emitted = true;
            let mut input = declared_egui_probe_pointer_input(&context, PointerEventKind::Press);
            if let Some(evidence) = input.dispatch_evidence.as_mut() {
                evidence.selected_region = Some(PresentationRegionId::from("forged-hit"));
            }
            vec![input]
        }

        fn paint(&mut self, _context: EguiPaintContext<'_>, _ops: &[PaintOp]) {}

        fn messages(&mut self, outcome: EventOutcome<ProbeMessage>) -> Vec<ProbeMessage> {
            outcome
                .emitted_messages
                .into_iter()
                .map(|message| message.message)
                .collect()
        }
    }

    impl EguiSlipwayBridge<TallRootWidget> for HandledWheelBridge {
        fn layout_input(&mut self, context: EguiLayoutContext<'_>) -> LayoutInput {
            let width = context.available_size.x.max(0.0);
            let height = context.available_size.y.max(0.0);
            zero_spacing_layout_input! {
                viewport: TargetLocalRect::new(Rect {
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

        fn desired_size(&mut self, layout: &LayoutOutput) -> egui::Vec2 {
            egui::vec2(
                layout.bounds().as_rect().size.width.max(0.0),
                layout.bounds().as_rect().size.height.max(0.0),
            )
        }

        fn input_events(&mut self, context: EguiInputContext<'_>) -> Vec<BackendInputEvent> {
            if self.emitted {
                return Vec::new();
            }
            self.emitted = true;
            vec![BackendInputEvent::direct(InputEvent::Wheel(
                slipway_core::WheelEvent {
                    target: context.widget_id.clone(),
                    target_slot: None,
                    region_id: None,
                    delta_x: 0.0,
                    delta_y: 7.0,
                },
            ))]
        }

        fn paint(&mut self, _context: EguiPaintContext<'_>, _ops: &[PaintOp]) {}

        fn messages(&mut self, outcome: EventOutcome<ProbeMessage>) -> Vec<ProbeMessage> {
            outcome
                .emitted_messages
                .into_iter()
                .map(|message| message.message)
                .collect()
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct NativeEguiLabel;

    impl SlipwayWidgetTypes for NativeEguiLabel {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = ProbeMessage;
    }

    impl SlipwayEguiNativeWidgetSpec for NativeEguiLabel {
        fn id(&self) -> WidgetId {
            WidgetId::from("egui.native-label")
        }

        fn initial_local_state(&self) -> Self::LocalState {}

        fn egui_native_ui(
            &self,
            ui: &mut egui::Ui,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _context: EguiNativeWidgetContext<'_>,
        ) -> EguiNativeWidgetOutput {
            ui.label("native");
            EguiNativeWidgetOutput::default()
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct NativeEguiProviderSurface {
        kind: ProviderSurfaceKind,
    }

    impl SlipwayWidgetTypes for NativeEguiProviderSurface {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = ProbeMessage;
    }

    impl SlipwayEguiNativeWidgetSpec for NativeEguiProviderSurface {
        fn id(&self) -> WidgetId {
            match self.kind {
                ProviderSurfaceKind::Canvas => WidgetId::from("egui.provider.canvas"),
                ProviderSurfaceKind::Gpu => WidgetId::from("egui.provider.gpu"),
                _ => WidgetId::from("egui.provider.other"),
            }
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::RenderSurface, Capability::ProviderSurfacePolicy]
        }

        fn initial_local_state(&self) -> Self::LocalState {}

        fn egui_native_ui(
            &self,
            ui: &mut egui::Ui,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _context: EguiNativeWidgetContext<'_>,
        ) -> EguiNativeWidgetOutput {
            ui.label("provider");
            EguiNativeWidgetOutput::default()
        }
    }

    impl SlipwayEguiProviderSurfaceSpec for NativeEguiProviderSurface {
        fn provider_surface_request(&self) -> ProviderSurfaceRequest {
            ProviderSurfaceRequest {
                target: self.id(),
                provider_id: match self.kind {
                    ProviderSurfaceKind::Canvas => "egui.canvas.provider".to_string(),
                    ProviderSurfaceKind::Gpu => "egui.gpu.provider".to_string(),
                    _ => "egui.other.provider".to_string(),
                },
                kind: self.kind.clone(),
                bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 64.0,
                        height: 32.0,
                    },
                },
                payload_ref: Some("owned-renderer".to_string()),
                dirty_regions: Vec::new(),
            }
        }
    }

    impl SlipwayEguiSplitGpuProviderSurfaceSpec for NativeEguiProviderSurface {
        type PreparedFrame = ();

        fn prepare_egui_gpu_surface(
            &mut self,
            _context: EguiGpuSurfacePrepareContext<'_>,
        ) -> Result<Self::PreparedFrame, Vec<Diagnostic>> {
            Ok(())
        }

        fn paint_prepared_egui_gpu_surface(
            &self,
            _context: EguiGpuSurfacePaintContext<'_, '_, Self::PreparedFrame>,
        ) -> Result<(), Vec<Diagnostic>> {
            Ok(())
        }
    }

    struct EguiNativeVisitCounter {
        normal: usize,
        native: usize,
    }

    impl SlipwayEguiWidgetListVisitor<(), ProbeMessage> for EguiNativeVisitCounter {
        fn visit_egui_child<W>(
            &mut self,
            _widget: &W,
            _external: &(),
            _local: &W::LocalState,
            _slot: WidgetSlotAddress,
        ) where
            W: SlipwayEguiBackendChildWidget<ExternalState = (), AppMessage = ProbeMessage>,
        {
            self.normal += 1;
        }

        fn visit_egui_native_child<N>(
            &mut self,
            _widget: &N,
            _external: &(),
            _local: &N::LocalState,
            _slot: WidgetSlotAddress,
        ) where
            N: SlipwayEguiNativeChildWidget<ExternalState = (), AppMessage = ProbeMessage>,
        {
            self.native += 1;
        }
    }

    macro_rules! impl_egui_test_leaf_children {
        ($($type:ty),+ $(,)?) => {
            $(
                impl SlipwayEguiAuthoredChildren for $type {
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
            )+
        };
    }

    macro_rules! impl_egui_test_event_policy {
        ($($type:ty),+ $(,)?) => {
            $(
                impl slipway_core::SlipwayEventRoutingPolicy for $type {
                    fn event_routing_policy(
                        &self,
                        _external: &Self::ExternalState,
                        _local: &Self::LocalState,
                        event: &slipway_core::InputEvent,
                    ) -> slipway_core::EventRoutingPolicyDeclaration {
                        let id = self.id();
                        let address = event.target_slot().cloned();
                        let path = address
                            .as_ref()
                            .map(|address| address.path.clone())
                            .unwrap_or_else(|| vec![id.clone()]);
                        slipway_core::EventRoutingPolicyDeclaration {
                            target: id.clone(),
                            event_target: event.target().clone(),
                            route: slipway_core::EventRoute {
                                route_id: None,
                                address,
                                path,
                                phase: slipway_core::EventRoutePhase::Target,
                            },
                            capture: Vec::new(),
                            diagnostics: Vec::new(),
                        }
                    }
                }

                impl slipway_core::SlipwayEventDispositionPolicy for $type {
                    fn event_disposition(
                        &self,
                        _external: &Self::ExternalState,
                        _local: &Self::LocalState,
                        event: &slipway_core::InputEvent,
                        _route: &slipway_core::EventRoute,
                    ) -> slipway_core::EventPropagationEvidence {
                        let id = self.id();
                        let handled = event.target() == &id;
                        let disposition = slipway_core::EventDisposition {
                            handled,
                            propagate: !handled,
                            default_action_allowed: true,
                        };
                        slipway_core::EventPropagationEvidence {
                            target: id.clone(),
                            event: event.clone(),
                            steps: vec![slipway_core::EventPropagationStep {
                                stage: slipway_core::EventPropagationStage::Target,
                                node: Some(id),
                                disposition,
                                emitted_messages: Vec::new(),
                                changes: Vec::new(),
                            }],
                            final_disposition: disposition,
                            diagnostics: Vec::new(),
                        }
                    }
                }
            )+
        };
    }

    impl_egui_test_leaf_children!(
        ProbeWidget,
        FocusedProbeWidget,
        ScrollProbeWidget,
        UnsupportedClipWidget,
        CountingWidget,
    );

    impl_egui_test_event_policy!(
        ProbeWidget,
        FocusedProbeWidget,
        ScrollProbeWidget,
        ParentWithChildWidget,
        ScrollableParentWidget,
        UnsupportedClipWidget,
        CountingWidget,
    );

    impl slipway_core::SlipwayTextBufferPolicy for FocusedProbeWidget {
        fn text_buffer(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TextBufferSnapshot {
            TextBufferSnapshot {
                target: self.id.clone(),
                text: "editable".to_string(),
                revision: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayTextSelectionPolicy for FocusedProbeWidget {
        fn text_selection(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TextSelectionPolicyDeclaration {
            TextSelectionPolicyDeclaration {
                target: self.id.clone(),
                selection: None,
                carets: CaretSet {
                    carets: vec![0],
                    primary: Some(0),
                },
                editable: true,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayImeCompositionPolicy for FocusedProbeWidget {
        fn ime_composition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> ImeCompositionPolicyDeclaration {
            ImeCompositionPolicyDeclaration {
                target: self.id.clone(),
                active: false,
                preedit_text: None,
                cursor_range: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayCaretGeometryPolicy for FocusedProbeWidget {
        fn caret_geometry(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _measurement: Option<&slipway_core::TextMeasurementEvidence>,
        ) -> CaretGeometryEvidence {
            CaretGeometryEvidence {
                target: self.id.clone(),
                caret_bounds: Vec::new(),
                selection_bounds: Vec::new(),
                measurement_request_ids: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayTextEditCommandPolicy for FocusedProbeWidget {
        fn text_edit_commands(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<slipway_core::TextEditCommandDeclaration> {
            vec![
                slipway_core::TextEditCommandDeclaration {
                    command_id: "insert-text".to_string(),
                    kind: TextEditKind::InsertText,
                    enabled: true,
                },
                slipway_core::TextEditCommandDeclaration {
                    command_id: "delete-backward".to_string(),
                    kind: TextEditKind::DeleteBackward,
                    enabled: true,
                },
                slipway_core::TextEditCommandDeclaration {
                    command_id: "replace-buffer".to_string(),
                    kind: TextEditKind::ReplaceBuffer,
                    enabled: true,
                },
            ]
        }
    }

    impl slipway_core::SlipwayTextInputVisualStylePolicy for FocusedProbeWidget {
        fn text_input_visual_style(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TextInputVisualStyleDeclaration {
            TextInputVisualStyleDeclaration::explicit(
                self.id.clone(),
                test_rgb(15, 23, 42),
                test_rgb(100, 116, 139),
                test_rgb(15, 23, 42),
                test_rgb(191, 219, 254),
                test_rgb(248, 250, 252),
                test_rgb(203, 213, 225),
                1.0,
                4.0,
                test_rgb(15, 23, 42),
            )
        }
    }

    impl slipway_core::SlipwayTextInputTypographyPolicy for FocusedProbeWidget {
        fn text_input_typography(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::TextInputTypographyDeclaration {
            slipway_core::TextInputTypographyDeclaration::explicit(
                self.id.clone(),
                TextStyle::plain().with_font_family("system-ui"),
            )
        }
    }

    impl slipway_core::SlipwayTextUndoRedoPolicy for FocusedProbeWidget {
        fn text_undo_redo(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::TextUndoRedoEvidence {
            slipway_core::TextUndoRedoEvidence {
                target: self.id.clone(),
                can_undo: false,
                can_redo: false,
                undo_depth: Some(0),
                redo_depth: Some(0),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayTextFlowPolicy for FocusedProbeWidget {
        fn text_flow_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> slipway_core::TextFlowPolicy {
            slipway_core::TextFlowPolicy {
                target: self.id.clone(),
                line_mode: slipway_core::TextLineMode::SingleLine,
                wrap: slipway_core::TextWrapMode::NoWrap,
                line_clamp: None,
                allow_ellipsis: false,
                baseline: None,
                caret_bounds: Vec::new(),
                viewport: None,
            }
        }
    }

    impl slipway_core::SlipwayTextMeasurementPolicy for FocusedProbeWidget {
        fn text_measurement_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> slipway_core::TextMeasurementPolicyDeclaration {
            slipway_core::TextMeasurementPolicyDeclaration {
                target: self.id.clone(),
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
                target: self.id.clone(),
                policy: slipway_core::SlipwayTextMeasurementPolicy::text_measurement_policy(
                    self, external, local, input,
                ),
                receipts: Vec::new(),
                cache: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayTextMeasurementCachePolicy for FocusedProbeWidget {
        fn text_measurement_cache_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> Vec<slipway_core::TextMeasurementCachePolicyDeclaration> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayCachedTextMeasurementPolicy for FocusedProbeWidget {
        fn cached_text_measurement_evidence<P, C>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: &LayoutInput,
            _provider: &mut P,
            _cache: &mut C,
        ) -> slipway_core::TextMeasurementEvidence
        where
            P: slipway_core::SlipwayTextMetricProvider,
            C: slipway_core::SlipwayTextMeasurementCache,
        {
            slipway_core::TextMeasurementEvidence {
                target: self.id.clone(),
                policy: slipway_core::SlipwayTextMeasurementPolicy::text_measurement_policy(
                    self, external, local, input,
                ),
                receipts: Vec::new(),
                cache: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayFocusTraversal for FocusedProbeWidget {
        fn focus_member(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Option<slipway_core::FocusTraversalMember> {
            None
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

    impl slipway_core::SlipwaySemantics for FocusedProbeWidget {
        fn semantics(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<slipway_core::SemanticNode> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayDebugEventTracePolicy for FocusedProbeWidget {
        fn debug_event_trace_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::DebugEventTracePolicyDeclaration {
            slipway_core::DebugEventTracePolicyDeclaration {
                target: self.id.clone(),
                request_only: true,
                include_route: true,
                include_messages: true,
                include_state_changes: true,
                include_repaint_request: false,
            }
        }
    }

    impl slipway_core::SlipwayContainerLayoutPolicy for ScrollProbeWidget {
        fn container_layout_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> slipway_core::ContainerLayoutPolicyDeclaration {
            slipway_core::ContainerLayoutPolicyDeclaration {
                target: self.id.clone(),
                kind: slipway_core::ContainerLayoutKind::Stack,
                child_order: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayChildConstraintPolicy for ScrollProbeWidget {
        fn child_constraints(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> Vec<slipway_core::ChildConstraintPolicyDeclaration> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayLayoutInvalidationPolicy for ScrollProbeWidget {
        fn layout_invalidation_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::LayoutInvalidationPolicyDeclaration {
            slipway_core::LayoutInvalidationPolicyDeclaration {
                target: self.id.clone(),
                dependencies: Vec::new(),
                revisions: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayLayoutEvidencePolicy for ScrollProbeWidget {
        fn layout_evidence(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            output: &LayoutOutput,
        ) -> slipway_core::LayoutEvidence {
            slipway_core::LayoutEvidence {
                target: self.id.clone(),
                bounds: *output.bounds(),
                child_placements: output.child_placements().to_vec(),
                invalidated: false,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayScrollBehaviorPolicy for ScrollProbeWidget {
        fn scroll_behavior_policy(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
            input: &LayoutInput,
        ) -> slipway_core::ScrollBehaviorPolicyDeclaration {
            let viewport = input.viewport;
            slipway_core::ScrollBehaviorPolicyDeclaration {
                target: self.id.clone(),
                region_id: None,
                address: None,
                axes: ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                extent: Size {
                    width: viewport.size.width,
                    height: viewport.size.height * 4.0,
                },
                viewport,
                content_bounds: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: viewport.size.width,
                        height: viewport.size.height * 4.0,
                    },
                }),
                offset: Point {
                    x: 0.0,
                    y: *local as f32,
                },
                consumption: ScrollConsumptionPolicy {
                    wheel: true,
                    drag: true,
                    keyboard: true,
                    programmatic: true,
                },
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayWheelRoutingPolicy for ScrollProbeWidget {
        fn wheel_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _region: &slipway_core::PresentationRegionId,
        ) -> slipway_core::WheelRoutingPolicyDeclaration {
            slipway_core::WheelRoutingPolicyDeclaration {
                target: self.id.clone(),
                routing: WheelRouting::SelfFirst,
                modifiers: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayViewportObservationPolicy for ScrollProbeWidget {
        fn viewport_observation(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::ViewportObservationEvidence {
            let viewport = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 40.0,
                },
            });
            slipway_core::ViewportObservationEvidence {
                target: self.id.clone(),
                viewport,
                visible_rect: viewport,
                scroll: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayVirtualCollectionPolicy for ScrollProbeWidget {
        fn virtual_collection_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::VirtualCollectionPolicyDeclaration {
            slipway_core::VirtualCollectionPolicyDeclaration {
                target: self.id.clone(),
                item_count: 0,
                visible_range: None,
                realization_hint: slipway_core::VirtualizationHint::None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayHitTesting for ScrollProbeWidget {
        fn hit_test(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: slipway_core::HitTestInput,
        ) -> slipway_core::HitTestOutput {
            slipway_core::HitTestOutput {
                target: Some(input.target.clone()),
                local_point: Some(input.point),
                route: EventRoute {
                    route_id: None,
                    address: None,
                    path: vec![input.target],
                    phase: EventRoutePhase::Target,
                },
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwaySemantics for ScrollProbeWidget {
        fn semantics(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<slipway_core::SemanticNode> {
            Vec::new()
        }
    }

    impl SlipwayEguiAuthoredChildren for ParentWithChildWidget {
        fn visit_egui_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            visitor.visit_egui_child(&self.child, external, &local.child, self.child_slot());
        }
    }

    impl SlipwayEguiAuthoredChildren for ScrollableParentWidget {
        fn visit_egui_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            visitor.visit_egui_child(&self.child, external, &local.child, self.child_slot());
        }
    }

    impl ProbeWidget {
        fn new(id: &str) -> Self {
            Self {
                id: WidgetId::from(id),
            }
        }
    }

    impl FocusedProbeWidget {
        fn new(id: &str) -> Self {
            Self {
                id: WidgetId::from(id),
            }
        }
    }

    impl ScrollProbeWidget {
        fn new(id: &str) -> Self {
            Self {
                id: WidgetId::from(id),
            }
        }
    }

    fn control_message(id: &str, target: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.control","arguments":{{"trace":true,"event":{{"type":"command","target":"{}","command":"routed"}}}}}}}}"#,
            id, target,
        )
    }

    fn frame(index: u64) -> FrameIdentity {
        FrameIdentity {
            surface_id: "egui-test".to_string(),
            surface_instance_id: "egui-test-instance".to_string(),
            revision: index,
            frame_index: index,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 40.0,
                },
            },
        }
    }

    fn frame_json(frame: &FrameIdentity) -> String {
        format!(
            r#"{{"surface_id":"{}","surface_instance_id":"{}","revision":{},"frame_index":{},"viewport":{{"origin":{{"x":{},"y":{}}},"size":{{"width":{},"height":{}}}}}}}"#,
            frame.surface_id,
            frame.surface_instance_id,
            frame.revision,
            frame.frame_index,
            frame.viewport.origin.x,
            frame.viewport.origin.y,
            frame.viewport.size.width,
            frame.viewport.size.height,
        )
    }

    fn physical_pointer_message(id: &str, frame: &FrameIdentity, x: f32, y: f32) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"pointer","phase":"press","position":{{"x":{},"y":{}}},"button":"primary","device":"mouse"}}}}}}}}"#,
            id,
            frame_json(frame),
            x,
            y,
        )
    }

    fn physical_text_message(id: &str, frame: &FrameIdentity, target: &str, text: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"text","target":"{}","text":"{}"}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
            text,
        )
    }

    fn physical_focus_message(
        id: &str,
        frame: &FrameIdentity,
        target: &str,
        focused: bool,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"focus","target":"{}","focused":{}}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
            focused,
        )
    }

    fn physical_keyboard_message(
        id: &str,
        frame: &FrameIdentity,
        target: &str,
        key: &str,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"keyboard","target":"{}","key":"{}","phase":"press"}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
            key,
        )
    }

    fn physical_command_no_payload_message(
        id: &str,
        frame: &FrameIdentity,
        target: &str,
        command: &str,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"command","target":"{}","command":"{}"}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
            command,
        )
    }

    fn physical_scroll_message(
        id: &str,
        frame: &FrameIdentity,
        target: &str,
        offset_x: f32,
        offset_y: f32,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"scroll","target":"{}","offset_x":{},"offset_y":{}}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
            offset_x,
            offset_y,
        )
    }

    fn physical_wheel_message(
        id: &str,
        frame: &FrameIdentity,
        x: f32,
        y: f32,
        delta_x: f32,
        delta_y: f32,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"wheel","position":{{"x":{},"y":{}}},"delta_x":{},"delta_y":{}}}}}}}}}"#,
            id,
            frame_json(frame),
            x,
            y,
            delta_x,
            delta_y,
        )
    }

    fn screenshot_message(id: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.screenshot","arguments":{{"frame":{}}}}}}}"#,
            id,
            frame_json(frame),
        )
    }

    #[allow(dead_code)]
    fn current_screenshot_message(id: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.screenshot","arguments":{{"frame":"current"}}}}}}"#,
            id,
        )
    }

    #[allow(dead_code)]
    fn live_screenshot_object_message(id: &str, frame: &str, forged_current: bool) -> String {
        let forged = if forged_current {
            r#", "_slipway_frame_admission":"current""#
        } else {
            ""
        };
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.screenshot","arguments":{{"frame":{} {}}}}}}}"#,
            id, frame, forged,
        )
    }

    #[allow(dead_code)]
    fn json_object_after<'a>(payload: &'a str, field: &str) -> &'a str {
        let field_start = payload.find(field).expect("JSON field is present");
        let object_start = payload[field_start..]
            .find('{')
            .map(|offset| field_start + offset)
            .expect("JSON field contains an object");
        let mut depth = 0usize;
        let mut in_string = false;
        let mut escaped = false;
        for (offset, byte) in payload.as_bytes()[object_start..].iter().enumerate() {
            if in_string {
                if escaped {
                    escaped = false;
                } else if *byte == b'\\' {
                    escaped = true;
                } else if *byte == b'"' {
                    in_string = false;
                }
                continue;
            }
            match *byte {
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &payload[object_start..=object_start + offset];
                    }
                }
                _ => {}
            }
        }
        panic!("JSON object is terminated");
    }

    #[allow(dead_code)]
    fn json_u64_after(payload: &str, field: &str) -> u64 {
        let start = payload.find(field).expect("numeric JSON field is present") + field.len();
        let digits = payload[start..]
            .trim_start()
            .chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>();
        digits.parse().expect("JSON field is an unsigned integer")
    }

    fn text_composition_message(
        id: &str,
        frame: &FrameIdentity,
        target: &str,
        preedit: &str,
        commit: &str,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"text_composition","target":"{}","updates":[{{"preedit_text":"{}","cursor_range":{{"anchor":0,"focus":1}}}}],"commit":"{}"}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
            preedit,
            commit,
        )
    }

    fn raw_wheel_input(delta_y: f32) -> egui::RawInput {
        raw_wheel_input_with_unit(egui::MouseWheelUnit::Point, egui::vec2(0.0, delta_y))
    }

    fn raw_wheel_input_with_unit(unit: egui::MouseWheelUnit, delta: egui::Vec2) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(100.0, 100.0),
            )),
            events: vec![egui::Event::MouseWheel {
                unit,
                delta,
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            }],
            ..egui::RawInput::default()
        }
    }

    fn rect_fill_shape_index(output: &egui::FullOutput, color: egui::Color32) -> Option<usize> {
        output.shapes.iter().position(|shape| {
            matches!(
                &shape.shape,
                egui::Shape::Rect(rect) if rect.fill == color
            )
        })
    }

    fn rect_fill_shape_clip(output: &egui::FullOutput, color: egui::Color32) -> Option<egui::Rect> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Rect(rect) if rect.fill == color => Some(shape.clip_rect),
            _ => None,
        })
    }

    fn test_hit_region(
        id: &str,
        target: WidgetId,
        bounds: impl Into<Rect>,
        traversal_order: usize,
    ) -> HitRegionDeclaration {
        let address = Some(WidgetSlotAddress::new(target.clone(), traversal_order));
        slipway_core::hit_region_from_pointer_capability(
            &ProbeWidget { id: target.clone() },
            &(),
            &7,
            PresentationRegionId::from(id),
            address.clone(),
            TargetLocalRect::new(bounds.into()),
            slipway_core::PointerEventCoordinateSpace::TargetLocal,
            HitRegionOrder {
                z_index: 0,
                paint_order: traversal_order,
                traversal_order,
            },
            Some(id.to_string()),
            CursorCapability::Pointer,
            true,
            PointerCaptureIntent::None,
        )
    }

    fn declared_egui_probe_pointer_input(
        context: &EguiInputContext<'_>,
        kind: PointerEventKind,
    ) -> BackendInputEvent {
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input"),
            context.frame.clone(),
            context.layout,
            context.hit_regions,
            Point { x: 1.0, y: 1.0 },
            kind,
            Some(PointerButton::Primary),
            egui_pointer_details(
                egui::Modifiers::default(),
                Some(egui::PointerButton::Primary),
            ),
            matches!(kind, PointerEventKind::Press),
        );
        BackendInputEvent::declared(
            dispatch
                .expect("egui probe pointer resolves declared hit region")
                .input,
            evidence,
        )
    }

    fn declared_egui_probe_pointer_input_for_runtime(
        runtime: &SlipwayRuntime<ProbeWidget>,
        frame: FrameIdentity,
        kind: PointerEventKind,
    ) -> BackendInputEvent {
        let layout_input = zero_spacing_layout_input! {
            viewport: TargetLocalRect::new(frame.viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame.viewport.size,
            },
        };
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput::new(frame.clone(), layout_input),
        );
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input"),
            frame,
            &view.layout,
            &view.hit_regions,
            Point { x: 1.0, y: 1.0 },
            kind,
            Some(PointerButton::Primary),
            egui_pointer_details(
                egui::Modifiers::default(),
                Some(egui::PointerButton::Primary),
            ),
            matches!(kind, PointerEventKind::Press),
        );
        BackendInputEvent::declared(
            dispatch
                .expect("egui runtime probe pointer resolves declared hit region")
                .input,
            evidence,
        )
    }

    fn test_text_edit_region(target: WidgetId, bounds: impl Into<Rect>) -> FocusRegionDeclaration {
        let bounds = TargetLocalRect::new(bounds.into());
        slipway_core::text_edit_focus_region_from_capability(
            &FocusedProbeWidget { id: target.clone() },
            &(),
            &0,
            PresentationRegionId::from("text-focus"),
            Some(WidgetSlotAddress::new(target.clone(), 0)),
            bounds,
            None,
            true,
            &zero_spacing_layout_input! {
                viewport: bounds,
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: bounds.size,
                },
            },
            None,
        )
    }

    fn test_scroll_region(target: WidgetId, viewport: impl Into<Rect>) -> ScrollRegionDeclaration {
        let viewport = viewport.into();
        let layout = test_leaf_layout(TargetLocalRect::new(viewport));
        slipway_core::scroll_region_from_scrollable_capability(
            &ScrollProbeWidget { id: target.clone() },
            &(),
            &12,
            &layout,
            Some(PresentationRegionId::from("scroll-region")),
            Some(WidgetSlotAddress::new(target, 0)),
            true,
        )
    }

    fn egui_test_rect(x: f32, y: f32, width: f32, height: f32) -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height))
    }

    fn slipway_test_rect(rect: egui::Rect) -> Rect {
        Rect {
            origin: Point {
                x: rect.min.x,
                y: rect.min.y,
            },
            size: Size {
                width: rect.width().max(0.0),
                height: rect.height().max(0.0),
            },
        }
    }

    fn test_presented_region(
        ui: &mut egui::Ui,
        id: &str,
        kind: EguiPresentedRegionKind,
        rect: egui::Rect,
        sense: egui::Sense,
        cursor: CursorCapability,
    ) -> EguiPresentedRegion {
        let response =
            apply_region_cursor(ui.interact(rect, egui::Id::new(id), sense), cursor.clone());
        EguiPresentedRegion {
            kind,
            region_id: PresentationRegionId::from(id),
            target: WidgetId::from(id),
            address: None,
            paint_sort_key: (0, 0, 0),
            authored_z_order: None,
            event_target: WidgetId::from(id),
            event_target_slot: None,
            declared_bounds: slipway_test_rect(rect),
            target_origin: rect.min,
            target_bounds: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: rect.width().max(0.0),
                    height: rect.height().max(0.0),
                },
            },
            event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
            response,
            cursor,
            enabled: true,
            text_edit_change: None,
            scroll_state: None,
            blocks_wheel: true,
        }
    }

    fn test_text_edit_change() -> EguiTextEditChange {
        EguiTextEditChange {
            before: "before".to_string(),
            after: "after".to_string(),
            selection_before: None,
            selection_after: None,
        }
    }

    #[test]
    fn egui_mounted_collectors_keep_rooted_slots_through_three_app_hops() {
        let app_ids = ["app-1", "app-2", "app-3"];
        for depth in 1..=app_ids.len() {
            let app_id = app_ids[depth - 1];
            let widget = slipway_core::SlipwayAppWidget::new(MountedCollectorApp {
                id: WidgetId::from(app_id),
                widgets: (ProbeWidget {
                    id: WidgetId::from("leaf"),
                },),
            });
            let local = widget.initial_local_state();
            let mut mounted_parent = WidgetSlotAddress::new(WidgetId::from("root"), 0);
            for id in &app_ids[..depth] {
                mounted_parent = mounted_parent.child(WidgetId::from(*id), 0);
            }
            let expected = mounted_parent.child(WidgetId::from("leaf"), 0);

            let mut normal = MountedSlotTrace::default();
            visit_egui_authored_children_mounted(
                &widget,
                &(),
                &local,
                Some(&mounted_parent),
                &mut normal,
            );
            assert_eq!(normal.slots, vec![expected.clone()]);

            let parent_view = widget.visible_backend_view_definition(
                &(),
                &local,
                ViewDefinitionInput::new(
                    frame(222),
                    child_layout_input(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 40.0,
                        },
                    }),
                ),
            );
            let parent_view = mount_presented_child_view_addresses(parent_view, &mounted_parent);
            assert_eq!(parent_view.hit_regions[0].address, Some(expected.clone()));
            assert_eq!(
                parent_view.hit_regions[0].route.address,
                Some(expected.clone())
            );
            assert_eq!(parent_view.hit_regions[0].route.path, expected.path.clone());
            let local_child =
                WidgetSlotAddress::new(WidgetId::from(app_id), 0).child(WidgetId::from("leaf"), 0);
            assert!(
                parent_view
                    .layout
                    .child_placements()
                    .iter()
                    .any(|placement| {
                        child_placement_matches_slot(
                            placement,
                            &WidgetId::from("leaf"),
                            &local_child,
                        )
                    })
            );
            let mut paint = MountedSlotTrace::default();
            visit_egui_authored_children_in_paint_order_mounted(
                &widget,
                &(),
                &local,
                &parent_view,
                Some(&mounted_parent),
                &mut paint,
            );
            assert_eq!(paint.slots, vec![expected]);
        }
    }

    fn set_nc9_target_slot(event: &mut InputEvent, slot: WidgetSlotAddress) {
        let InputEvent::Pointer(pointer) = event else {
            panic!("NC-9 fixture must generate pointer input");
        };
        pointer.target_slot = Some(slot);
    }

    fn nc9_raw_input(events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(100.0, 40.0),
            )),
            events,
            ..Default::default()
        }
    }

    fn run_egui_nc9_row<W>(
        widget: W,
        pointer_x: f32,
        expected_slot: WidgetSlotAddress,
        expected_counter: Nc9Counter,
        assert_local: impl FnOnce(&W::LocalState),
        wrong_branch: Option<WidgetSlotAddress>,
    ) where
        W: SlipwayEguiBackendWidget<ExternalState = Nc9Counters, AppMessage = Nc9Message> + Clone,
        W::LocalState: Clone,
    {
        let viewport = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 100.0,
                height: 40.0,
            },
        };
        let mut selection_app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(widget.clone(), Nc9Counters::default()),
            Nc9InputCaptureBridge::default(),
            reduce_nc9 as fn(&mut Nc9Counters, Vec<Nc9Message>),
        );
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(nc9_raw_input(Vec::new()), |ui| selection_app.render_ui(ui));
        let position = egui::pos2(pointer_x, 5.0);
        let _ = ctx.run_ui(
            nc9_raw_input(vec![egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }]),
            |ui| selection_app.render_ui(ui),
        );

        let input = selection_app
            .bridge
            .inputs
            .iter()
            .find(|input| {
                input.event.target_slot() == Some(&expected_slot)
                    && matches!(
                        input.event,
                        InputEvent::Pointer(ref pointer)
                            if pointer.kind == PointerEventKind::Press
                    )
            })
            .cloned()
            .expect("NC-9 egui press produces rooted backend input");
        assert_eq!(
            selection_app.runtime.external().value(expected_counter),
            1,
            "the egui frame reducer must mutate the intended counter"
        );
        assert_eq!(
            selection_app.runtime.external().total(),
            1,
            "the egui frame reducer must leave sibling counters unchanged"
        );
        let frame_trace = selection_app
            .runtime
            .backend_input_traces()
            .find(|trace| {
                trace.input.event.target_slot() == Some(&expected_slot)
                    && matches!(
                        trace.input.event,
                        InputEvent::Pointer(ref pointer)
                            if pointer.kind == PointerEventKind::Press
                    )
            })
            .expect("the egui frame records the rooted press trace");
        assert!(frame_trace.handled, "egui frame trace: {frame_trace:#?}");
        assert!(frame_trace.diagnostics.iter().all(|diagnostic| {
            diagnostic.code != slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_FRAME_MISMATCH
        }));
        assert!(frame_trace.changes.iter().any(|change| {
            change.target == Nc9Leaf::id_value() && change.slot.as_ref() == Some(&expected_slot)
        }));
        assert_eq!(input.event.target_slot(), Some(&expected_slot));
        let evidence = input
            .dispatch_evidence
            .as_ref()
            .expect("NC-9 egui input retains backend evidence");
        assert_eq!(
            evidence.selected_region,
            Some(
                Nc9Leaf {
                    counter: expected_counter
                }
                .region_id()
            )
        );
        assert_eq!(evidence.generated_event.as_ref(), Some(&input.event));
        let route = evidence
            .route
            .as_ref()
            .expect("NC-9 egui evidence carries route");
        assert_eq!(route.address.as_ref(), Some(&expected_slot));
        assert_eq!(route.path, expected_slot.path);
        let selected_region = evidence.selected_region.clone();

        let mut runtime = SlipwayRuntime::new(widget, Nc9Counters::default());
        runtime.record_presented_viewport(viewport);

        if let Some(wrong_branch) = wrong_branch {
            for wrong_slot in [
                wrong_branch,
                WidgetSlotAddress {
                    ordinal: 99,
                    ..expected_slot.clone()
                },
            ] {
                let mut wrong_event = input.event.clone();
                set_nc9_target_slot(&mut wrong_event, wrong_slot);
                let mut wrong_local = runtime.widget().initial_local_state();
                let outcome = runtime.widget().handle_event(
                    runtime.external(),
                    &mut wrong_local,
                    wrong_event,
                );
                assert!(!outcome.handled);
                assert!(outcome.emitted_messages.is_empty());
                assert!(outcome.changes.is_empty());
            }
        }

        let mut reducer = reduce_nc9;
        let report = runtime.apply_backend_input_event_for_backend_with_app_reducer(
            input,
            EGUI_BACKEND_ID,
            &mut reducer,
        );
        assert!(report.handled, "NC-9 input must be handled: {report:?}");
        assert_eq!(report.emitted_messages, 1);
        assert_eq!(report.applied_messages, 1);
        assert_local(runtime.local_state());
        assert_eq!(runtime.external().value(expected_counter), 1);
        assert_eq!(
            runtime.external().total(),
            1,
            "sibling reducer counters stay unchanged"
        );

        let trace = runtime
            .last_backend_input_trace()
            .expect("NC-9 runtime records backend input trace");
        assert!(trace.handled);
        assert_eq!(trace.input.event.target_slot(), Some(&expected_slot));
        let trace_evidence = trace
            .input
            .dispatch_evidence
            .as_ref()
            .expect("NC-9 trace retains evidence");
        assert_eq!(trace_evidence.selected_region, selected_region);
        assert_eq!(
            trace_evidence
                .route
                .as_ref()
                .and_then(|route| route.address.as_ref()),
            Some(&expected_slot)
        );
        assert!(trace.revision_after > trace.revision_before);
        assert!(trace.changes.iter().any(|change| {
            change.target == Nc9Leaf::id_value() && change.slot.as_ref() == Some(&expected_slot)
        }));
    }

    #[test]
    fn egui_nc9_recursive_backend_pointer_runtime_matrix() {
        let leaf = Nc9Leaf::id_value();

        let non_nested_root = WidgetId::from("nc9.root.non-nested");
        run_egui_nc9_row(
            nc9_app(
                non_nested_root.as_str(),
                (Nc9Leaf {
                    counter: Nc9Counter::NonNested,
                },),
            ),
            5.0,
            WidgetSlotAddress::new(non_nested_root, 0).child(leaf.clone(), 0),
            Nc9Counter::NonNested,
            |local| assert_eq!(local.widgets.0, 1),
            None,
        );

        let one_root = WidgetId::from("nc9.root.one");
        let app_1 = WidgetId::from("nc9.app-1");
        run_egui_nc9_row(
            nc9_app(
                one_root.as_str(),
                (nc9_app(
                    app_1.as_str(),
                    (Nc9Leaf {
                        counter: Nc9Counter::One,
                    },),
                ),),
            ),
            5.0,
            WidgetSlotAddress::new(one_root, 0)
                .child(app_1.clone(), 0)
                .child(leaf.clone(), 0),
            Nc9Counter::One,
            |local| assert_eq!(local.widgets.0.widgets.0, 1),
            None,
        );

        let two_root = WidgetId::from("nc9.root.two");
        let app_2 = WidgetId::from("nc9.app-2");
        run_egui_nc9_row(
            nc9_app(
                two_root.as_str(),
                (nc9_app(
                    app_1.as_str(),
                    (nc9_app(
                        app_2.as_str(),
                        (Nc9Leaf {
                            counter: Nc9Counter::Two,
                        },),
                    ),),
                ),),
            ),
            5.0,
            WidgetSlotAddress::new(two_root, 0)
                .child(app_1.clone(), 0)
                .child(app_2.clone(), 0)
                .child(leaf.clone(), 0),
            Nc9Counter::Two,
            |local| assert_eq!(local.widgets.0.widgets.0.widgets.0, 1),
            None,
        );

        let three_root = WidgetId::from("nc9.root.three");
        let app_3 = WidgetId::from("nc9.app-3");
        let three_slot = WidgetSlotAddress::new(three_root.clone(), 0)
            .child(app_1.clone(), 0)
            .child(app_2.clone(), 0)
            .child(app_3.clone(), 0)
            .child(leaf.clone(), 0);
        let wrong_branch = WidgetSlotAddress::new(three_root.clone(), 0)
            .child(app_1.clone(), 0)
            .child(WidgetId::from("nc9.wrong-app"), 0)
            .child(app_3.clone(), 0)
            .child(leaf.clone(), 0);
        run_egui_nc9_row(
            nc9_app(
                three_root.as_str(),
                (nc9_app(
                    app_1.as_str(),
                    (nc9_app(
                        app_2.as_str(),
                        (nc9_app(
                            app_3.as_str(),
                            (Nc9Leaf {
                                counter: Nc9Counter::Three,
                            },),
                        ),),
                    ),),
                ),),
            ),
            5.0,
            three_slot,
            Nc9Counter::Three,
            |local| assert_eq!(local.widgets.0.widgets.0.widgets.0.widgets.0, 1),
            Some(wrong_branch),
        );

        let sibling_root = WidgetId::from("nc9.root.siblings");
        let left_app = WidgetId::from("nc9.left-app");
        let right_app = WidgetId::from("nc9.right-app");
        let sibling_fixture = || {
            nc9_app(
                sibling_root.as_str(),
                (
                    nc9_app(
                        left_app.as_str(),
                        (Nc9Leaf {
                            counter: Nc9Counter::Left,
                        },),
                    ),
                    nc9_app(
                        right_app.as_str(),
                        (Nc9Leaf {
                            counter: Nc9Counter::Right,
                        },),
                    ),
                ),
            )
        };
        run_egui_nc9_row(
            sibling_fixture(),
            5.0,
            WidgetSlotAddress::new(sibling_root.clone(), 0)
                .child(left_app.clone(), 0)
                .child(leaf.clone(), 0),
            Nc9Counter::Left,
            |local| {
                assert_eq!(local.widgets.0.widgets.0, 1);
                assert_eq!(local.widgets.1.widgets.0, 0);
            },
            None,
        );
        run_egui_nc9_row(
            sibling_fixture(),
            55.0,
            WidgetSlotAddress::new(sibling_root, 0)
                .child(right_app.clone(), 1)
                .child(leaf, 0),
            Nc9Counter::Right,
            |local| {
                assert_eq!(local.widgets.0.widgets.0, 0);
                assert_eq!(local.widgets.1.widgets.0, 1);
            },
            None,
        );
    }

    #[test]
    fn egui_region_anchor_position_includes_target_origin() {
        egui::__run_test_ui(|ui| {
            let frame = FrameIdentity {
                surface_id: "egui-test".to_string(),
                surface_instance_id: "anchor".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 240.0,
                        height: 120.0,
                    },
                },
            };
            let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let response = ui.interact(
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(1.0, 1.0)),
                egui::Id::new("anchor-response"),
                egui::Sense::hover(),
            );
            let region = EguiPresentedRegion {
                kind: EguiPresentedRegionKind::Hit,
                region_id: PresentationRegionId::from("child-hit"),
                target: WidgetId::from("child"),
                address: None,
                paint_sort_key: (0, 0, 0),
                authored_z_order: None,
                event_target: WidgetId::from("child"),
                event_target_slot: None,
                declared_bounds: Rect {
                    origin: Point { x: 10.0, y: 6.0 },
                    size: Size {
                        width: 20.0,
                        height: 12.0,
                    },
                },
                target_origin: egui::pos2(80.0, 40.0),
                target_bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 60.0,
                    },
                },
                event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
                response,
                cursor: CursorCapability::Default,
                enabled: true,
                text_edit_change: None,
                scroll_state: None,
                blocks_wheel: true,
            };
            let regions = vec![region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &[],
                scroll_regions: &[],
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };

            let anchor = egui_region_anchor_position(&context, &regions[0]);

            assert_eq!(anchor, egui::pos2(90.5, 46.5));
        });
    }

    #[test]
    fn captured_pointer_event_keeps_declared_region_after_pointer_leaves_bounds() {
        egui::__run_test_ui(|ui| {
            let frame = FrameIdentity {
                surface_id: "egui-test".to_string(),
                surface_instance_id: "capture".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 160.0,
                        height: 120.0,
                    },
                },
            };
            let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let mut hit = test_hit_region(
                "drag-hit",
                WidgetId::from("drag"),
                Rect {
                    origin: Point { x: 8.0, y: 8.0 },
                    size: Size {
                        width: 40.0,
                        height: 24.0,
                    },
                },
                0,
            );
            hit.capture = PointerCaptureIntent::DuringDrag;
            let hit_regions = vec![hit];
            let region = test_presented_region(
                ui,
                "drag-hit",
                EguiPresentedRegionKind::Hit,
                egui_test_rect(8.0, 8.0, 40.0, 24.0),
                egui::Sense::click_and_drag(),
                CursorCapability::Grab,
            );
            let regions = vec![region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &hit_regions,
                focus_regions: &[],
                scroll_regions: &[],
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };

            let event = egui_backend_captured_pointer_input_event(
                &context,
                &regions[0],
                egui::pos2(140.0, 96.0),
                PointerEventKind::Move,
                None,
                PointerDetails::default(),
                true,
            )
            .expect("captured pointer event is generated from the captured region");

            let evidence = event
                .dispatch_evidence
                .as_ref()
                .expect("captured event keeps dispatch evidence");
            assert_eq!(
                evidence.selected_region,
                Some(PresentationRegionId::from("drag-hit"))
            );
            assert_eq!(evidence.source.label, "backend_presented");
            assert_eq!(evidence.source.pass_id.as_deref(), Some("captured-input"));
            assert!(evidence.capture_event);
            let InputEvent::Pointer(pointer) = event.event else {
                panic!("expected pointer event");
            };
            assert_eq!(pointer.target, WidgetId::from("drag"));
            assert_eq!(pointer.kind, PointerEventKind::Move);
            assert!(
                pointer.position.x > 48.0 && pointer.position.y > 32.0,
                "captured coordinates must preserve the outside-drag position"
            );
        });
    }

    fn test_scroll_state() -> EguiScrollRegionState {
        EguiScrollRegionState {
            declared_offset: Point { x: 0.0, y: 0.0 },
            egui_offset: Point { x: 0.0, y: 8.0 },
            content_size: Size {
                width: 100.0,
                height: 200.0,
            },
            inner_rect: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 50.0,
                },
            },
            indicator: None,
        }
    }

    #[test]
    fn egui_native_wrapper_participates_in_child_list_entry_traversal() {
        let children = (SlipwayEguiNativeWidget::new(NativeEguiLabel),);
        let local = ((),);
        let root_slot = WidgetSlotAddress::new(WidgetId::from("egui.native-root"), 0);
        let mut counter = EguiNativeVisitCounter {
            normal: 0,
            native: 0,
        };

        children.visit_egui_children(&(), &local, &root_slot, &mut counter);

        assert_eq!(counter.normal, 0);
        assert_eq!(counter.native, 1);
    }

    #[test]
    fn egui_provider_surface_wrapper_exposes_canvas_and_gpu_slots() {
        fn assert_provider_surface<W: slipway_core::SlipwayProviderSurfaceCapability>(_widget: &W) {
        }

        let canvas = SlipwayEguiNativeWidget::new(NativeEguiProviderSurface {
            kind: ProviderSurfaceKind::Canvas,
        });
        assert_provider_surface(&canvas);
        assert_eq!(canvas.canvas_surfaces().len(), 1);
        assert!(canvas.gpu_surfaces().is_empty());
        assert!(canvas.media_surfaces().is_empty());
        assert!(canvas.plot_surfaces().is_empty());
        assert_eq!(
            canvas.render_surfaces(&(), &())[0].capabilities,
            vec!["canvas".to_string()]
        );

        let mut gpu = SlipwayEguiNativeWidget::new(NativeEguiProviderSurface {
            kind: ProviderSurfaceKind::Gpu,
        });
        assert_provider_surface(&gpu);
        assert!(gpu.canvas_surfaces().is_empty());
        assert_eq!(gpu.gpu_surfaces().len(), 1);
        assert_eq!(gpu.gpu_surfaces()[0].provider_id, "egui.gpu.provider");
        assert_eq!(
            gpu.render_surfaces(&(), &())[0].capabilities,
            vec!["gpu".to_string()]
        );

        let hit = gpu.provider_hit_test(HitTestInput {
            target: WidgetId::from("egui.provider.gpu"),
            point: Point { x: 2.0, y: 3.0 },
            pointer: slipway_core::PointerDetails::default(),
        });
        assert_eq!(hit.provider_id, "egui.gpu.provider");
        assert_eq!(hit.hit, None);
        assert!(hit.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "egui.provider_surface.hit_test_unsupported"
                && diagnostic.severity == slipway_core::DiagnosticSeverity::Unsupported
        }));

        let snapshot = gpu.provider_snapshot(ProviderSnapshotRequest {
            target: WidgetId::from("egui.provider.gpu"),
            provider_id: "egui.gpu.provider".to_string(),
            bounds: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 64.0,
                    height: 32.0,
                },
            },
            frame: frame(212),
        });
        assert_eq!(snapshot.provider_id, "egui.gpu.provider");
        assert!(snapshot.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "egui.provider_surface.snapshot_unsupported"
                && diagnostic.severity == slipway_core::DiagnosticSeverity::Unsupported
        }));
    }

    #[test]
    fn egui_split_gpu_provider_contract_is_backend_specific_and_mut_prepare_only() {
        fn assert_split_gpu<W: SlipwayEguiSplitGpuSurfaceProvider<PreparedFrame = ()>>(
            _widget: &W,
        ) {
        }

        let mut gpu = SlipwayEguiNativeWidget::new(NativeEguiProviderSurface {
            kind: ProviderSurfaceKind::Gpu,
        });

        assert_split_gpu(&gpu);
        assert_eq!(gpu.gpu_surfaces().len(), 1);
        assert_eq!(
            gpu.native().provider_surface_request().kind,
            ProviderSurfaceKind::Gpu
        );

        gpu.native_mut().kind = ProviderSurfaceKind::Canvas;
        assert!(gpu.gpu_surfaces().is_empty());
        assert_eq!(gpu.canvas_surfaces().len(), 1);
    }

    #[test]
    fn egui_provider_surface_profile_is_admitted() {
        let admission = egui_backend_admission()
            .backend_parity_admission(&[CapabilityProfileKind::ProviderSurface]);

        assert!(admission.accepted);
        assert!(admission.visible_requirements.iter().any(|requirement| {
            requirement.capability == egui_provider_surface_visible_capability()
        }));
    }

    #[test]
    fn egui_input_lock_collects_raw_input_only() {
        let source = include_str!("lib.rs");
        let snapshot_fn = source
            .find("fn egui_raw_input_snapshot")
            .expect("raw input snapshot helper is present");
        let snapshot_end = source[snapshot_fn..]
            .find("#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]")
            .map(|offset| snapshot_fn + offset)
            .expect("next item after raw input helper is present");
        let snapshot_body = &source[snapshot_fn..snapshot_end];

        assert!(snapshot_body.contains("ui.input(|input| EguiRawInputSnapshot"));
        assert!(snapshot_body.contains("events: input.events.clone()"));
        assert!(snapshot_body.contains("modifiers: input.modifiers"));
        assert!(snapshot_body.contains("hover_pos: input.pointer.hover_pos()"));
        for forbidden in [
            "response.",
            "egui_region_at_position",
            "focused_target",
            "request_focus",
            "focused_event_target",
            "focused_region_kind",
        ] {
            assert!(
                !snapshot_body.contains(forbidden),
                "Context::input closure must collect raw input only; found {forbidden}"
            );
        }

        let input_events = source
            .find(
                "fn input_events(&mut self, context: EguiInputContext<'_>) -> Vec<BackendInputEvent> {",
            )
            .expect("input_events method is present");
        let input_events_end = source[input_events..]
            .find("fn paint(&mut self, context: EguiPaintContext<'_>, ops: &[PaintOp])")
            .map(|offset| input_events + offset)
            .expect("paint method follows input_events");
        let input_events_body = &source[input_events..input_events_end];
        let raw_snapshot = input_events_body
            .find("let raw_input = egui_raw_input_snapshot(context.ui);")
            .expect("input_events snapshots raw input");
        let response_routing = input_events_body
            .find("egui_region_at_position")
            .expect("input_events routes with region responses after snapshot");

        assert!(
            raw_snapshot < response_routing,
            "response routing must happen after raw input snapshot"
        );
        assert!(!input_events_body.contains("context.ui.input(|input|"));
    }

    #[test]
    fn runtime_app_drain_uses_budgeted_live_debug_turn_api() {
        let source = include_str!("lib.rs");
        let drain_method = source
            .find("pub fn drain_debug_pending(&mut self)")
            .expect("runtime app drain method is present");
        let drain_end = source[drain_method..]
            .find("pub fn sense")
            .map(|offset| drain_method + offset)
            .expect("next method after drain is present");
        let drain_body = &source[drain_method..drain_end];

        assert!(drain_body.contains("drain_live_debug_turn_with_app_reducer"));
        assert!(!drain_body.contains("drain_debug_pending_with_app_reducer"));
        assert!(!drain_body.contains("drain_runtime_mcp_pending_with_app_reducer"));
    }

    #[test]
    fn response_authoritative_region_wins_over_overlapping_geometric_region() {
        egui::__run_test_ui(|ui| {
            let rect = egui_test_rect(0.0, 0.0, 60.0, 40.0);
            let mut response_region = test_presented_region(
                ui,
                "scroll-response",
                EguiPresentedRegionKind::Scroll,
                rect,
                egui::Sense::click_and_drag(),
                CursorCapability::Default,
            );
            response_region.scroll_state = Some(test_scroll_state());

            let geometric_region = test_presented_region(
                ui,
                "geometry-hit",
                EguiPresentedRegionKind::Hit,
                rect,
                egui::Sense::click(),
                CursorCapability::Pointer,
            );
            let regions = vec![response_region, geometric_region];

            let selected = egui_region_at_position(&regions, egui::pos2(20.0, 20.0))
                .expect("overlapping point should select a region");

            assert_eq!(selected.region_id.as_str(), "scroll-response");
        });
    }

    #[test]
    fn unchanged_scroll_background_is_not_response_authority() {
        egui::__run_test_ui(|ui| {
            let mut scroll_region = test_presented_region(
                ui,
                "scroll-background",
                EguiPresentedRegionKind::Scroll,
                egui_test_rect(0.0, 0.0, 60.0, 40.0),
                egui::Sense::drag(),
                CursorCapability::Default,
            );
            scroll_region.scroll_state = Some(EguiScrollRegionState {
                declared_offset: Point { x: 4.0, y: 8.0 },
                egui_offset: Point { x: 4.0, y: 8.0 },
                ..test_scroll_state()
            });

            assert!(
                !egui_region_has_response_authority(&scroll_region),
                "unchanged scroll metadata must not outrank a child response"
            );
        });
    }

    #[test]
    fn wheel_region_at_boundary_bubbles_to_next_scroll_owner() {
        egui::__run_test_ui(|ui| {
            let rect = egui_test_rect(0.0, 0.0, 100.0, 100.0);
            let mut outer = test_presented_region(
                ui,
                "outer-scroll",
                EguiPresentedRegionKind::Scroll,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            outer.paint_sort_key = (0, 0, 0);
            outer.scroll_state = Some(EguiScrollRegionState {
                declared_offset: Point { x: 0.0, y: 0.0 },
                egui_offset: Point { x: 0.0, y: 0.0 },
                content_size: Size {
                    width: 100.0,
                    height: 300.0,
                },
                inner_rect: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
                indicator: None,
            });

            let mut inner = test_presented_region(
                ui,
                "inner-scroll",
                EguiPresentedRegionKind::Scroll,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            inner.paint_sort_key = (1, 0, 0);
            inner.scroll_state = Some(EguiScrollRegionState {
                declared_offset: Point { x: 0.0, y: 200.0 },
                egui_offset: Point { x: 0.0, y: 200.0 },
                content_size: Size {
                    width: 100.0,
                    height: 300.0,
                },
                inner_rect: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
                indicator: None,
            });

            let regions = vec![outer, inner];

            let selected_down =
                egui_wheel_region_at_position(&regions, egui::pos2(20.0, 20.0), 0.0, -4.0)
                    .expect("outer scroll owner should receive boundary wheel");
            assert_eq!(selected_down.region_id.as_str(), "outer-scroll");

            let selected_up =
                egui_wheel_region_at_position(&regions, egui::pos2(20.0, 20.0), 0.0, 4.0)
                    .expect("inner scroll owner can still move in the opposite direction");
            assert_eq!(selected_up.region_id.as_str(), "inner-scroll");
        });
    }

    // --- Step 212: interactive declared indicators (thumb drag + track jump) ---
    // Parity fixture with the iced `declared_indicator_routed_view` tests:
    // viewport (0,0,200,100), content 300, offset 40.

    fn indicator_test_declaration() -> ScrollRegionDeclaration {
        let target = WidgetId::from("declared-indicator-target");
        ScrollRegionDeclaration::explicit(
            PresentationRegionId::from("declared-indicator-region"),
            target.clone(),
            Some(WidgetSlotAddress::new(target, 0)),
            TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 100.0,
                },
            }),
            TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 300.0,
                },
            }),
            Point { x: 0.0, y: 40.0 },
            ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            WheelRouting::NearestScrollable,
            HitRegionOrder::default(),
            ScrollConsumptionPolicy::exclusive_wheel(),
            true,
        )
    }

    fn indicator_test_region(ui: &mut egui::Ui) -> (EguiPresentedRegion, egui::Rect, egui::Rect) {
        let rect = egui_test_rect(0.0, 0.0, 200.0, 100.0);
        let scroll = indicator_test_declaration();
        let offset = Point { x: 0.0, y: 40.0 };
        let (track, thumb) = declared_scroll_indicator_geometry(rect, &scroll, offset)
            .expect("the fixture region paints an indicator");
        let mut region = test_presented_region(
            ui,
            "declared-indicator-region",
            EguiPresentedRegionKind::Scroll,
            rect,
            egui::Sense::hover(),
            CursorCapability::Default,
        );
        region.scroll_state = Some(EguiScrollRegionState {
            declared_offset: offset,
            egui_offset: offset,
            content_size: Size {
                width: 200.0,
                height: 300.0,
            },
            inner_rect: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 100.0,
                },
            },
            indicator: Some((track, thumb)),
        });
        (region, track, thumb)
    }

    // REVERT-AND-FAIL for the interactive-indicator seam: reverting the
    // press handling (`egui_indicator_press_interaction`) makes the thumb
    // press resolve to nothing and this test fails.
    #[test]
    fn declared_indicator_thumb_press_starts_drag_and_track_click_jumps() {
        egui::__run_test_ui(|ui| {
            let (region, track, thumb) = indicator_test_region(ui);
            let regions = vec![region];

            // Thumb press: backend-internal drag anchored at the current
            // offset, nothing jumped.
            let thumb_center = thumb.center();
            let (drag, jump) = egui_indicator_press_interaction(&regions, thumb_center)
                .expect("a thumb press must be claimed by the indicator");
            assert_eq!(jump, None);
            assert_eq!(drag.start_offset_y, 40.0);
            assert_eq!(
                drag.region_id,
                PresentationRegionId::from("declared-indicator-region")
            );

            // Drag down 30px: the offset advances by the track-to-content
            // ratio, clamped to the declared travel.
            let expected = 40.0 + 30.0 * drag.max_offset / drag.track_travel;
            let offset = egui_indicator_drag_offset(&drag, thumb_center.y + 30.0);
            assert!(
                (offset - expected.min(drag.max_offset)).abs() < 0.5,
                "drag must map track travel to the declared offset: got {offset}, expected {expected}"
            );

            // Track click at the bottom: JUMP-TO-POSITION clamps to the max
            // offset (the documented semantics; the press keeps dragging
            // from the jumped anchor).
            let (drag, jump) = egui_indicator_press_interaction(
                &regions,
                egui::pos2(track.center().x, track.bottom() - 2.0),
            )
            .expect("a track press must be claimed by the indicator");
            assert_eq!(jump, Some(200.0));
            assert_eq!(drag.start_offset_y, 200.0);
        });
    }

    // The synthesized hit surface must not collide with authored content
    // stacked ABOVE the indicator's region: a fronting opaque layer
    // (occlusion region) owns its pixels — the overlay-panel case.
    #[test]
    fn declared_indicator_press_defers_to_fronting_occlusion_region() {
        egui::__run_test_ui(|ui| {
            let (region, _track, thumb) = indicator_test_region(ui);
            let mut occluder = test_presented_region(
                ui,
                "covering-panel",
                EguiPresentedRegionKind::Occlusion,
                egui_test_rect(180.0, 0.0, 20.0, 100.0),
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            occluder.paint_sort_key = (10, 0, 0);
            let thumb_center = thumb.center();
            let regions = vec![region, occluder];
            assert!(
                egui_indicator_press_interaction(&regions, thumb_center).is_none(),
                "a press under an opaque fronting layer must not start an indicator drag"
            );
        });
    }

    // The drag emits a declared Scroll event through the same evidence
    // path as the offset sync: the declared region is selected and the
    // synthesized offset is carried.
    #[test]
    fn declared_indicator_drag_synthesizes_declared_scroll_event() {
        egui::__run_test_ui(|ui| {
            let frame = FrameIdentity {
                surface_id: "egui-test".to_string(),
                surface_instance_id: "indicator-drag".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 200.0,
                        height: 100.0,
                    },
                },
            };
            let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let scroll_regions = vec![indicator_test_declaration()];
            let (region, _track, thumb) = indicator_test_region(ui);
            let regions = vec![region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &[],
                scroll_regions: &scroll_regions,
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };

            let (drag, _) = egui_indicator_press_interaction(&regions, thumb.center())
                .expect("thumb press claimed");
            let offset_y = egui_indicator_drag_offset(&drag, thumb.center().y + 30.0);
            let event = egui_indicator_scroll_event_for_offset(&regions[0], offset_y)
                .expect("scroll event built");
            let input = egui_scroll_backend_input_event(&context, &regions[0], event)
                .expect("declared scroll input synthesized");
            let evidence = input
                .dispatch_evidence
                .as_ref()
                .expect("indicator drag carries dispatch evidence");
            assert_eq!(
                evidence.selected_region,
                Some(PresentationRegionId::from("declared-indicator-region"))
            );
            let InputEvent::Scroll(scroll) = input.event else {
                panic!("expected a Scroll event");
            };
            assert_eq!(
                scroll.region_id,
                PresentationRegionId::from("declared-indicator-region")
            );
            assert!(
                (scroll.offset_y - offset_y).abs() < f32::EPSILON,
                "the synthesized event must carry the drag-computed offset"
            );
        });
    }

    #[test]
    fn mounted_live_root_bottom_wheel_is_consumed_no_op() {
        egui::__run_test_ui(|ui| {
            let target = WidgetId::from("scroll");
            let frame = FrameIdentity {
                surface_id: "egui-test".to_string(),
                surface_instance_id: "wheel".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 800.0,
                        height: 600.0,
                    },
                },
            };
            let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let mut outer_scroll = test_scroll_region(target.clone(), frame.viewport);
            outer_scroll.id = PresentationRegionId::from("outer-scroll");
            let mounted_slot = WidgetSlotAddress::new(target.clone(), 0);
            outer_scroll.address = Some(mounted_slot.clone());
            outer_scroll.content_bounds = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 800.0,
                    height: 1484.0,
                },
            });
            outer_scroll.offset = Point { x: 0.0, y: 0.0 };
            outer_scroll.axes = ScrollAxes {
                horizontal: false,
                vertical: true,
            };
            outer_scroll.wheel_routing = WheelRouting::NearestScrollable;
            outer_scroll.order = HitRegionOrder::default();
            outer_scroll.consumption = ScrollConsumptionPolicy::exclusive_wheel();
            let mut inner_scroll = outer_scroll.clone();
            inner_scroll.id = PresentationRegionId::from("inner-scroll");
            inner_scroll.offset.y = 884.0;
            inner_scroll.order = HitRegionOrder {
                z_index: 1,
                paint_order: 0,
                traversal_order: 0,
            };
            let mounted_view = mount_presented_child_view_addresses(
                ViewDefinition {
                    target: target.clone(),
                    frame: frame.clone(),
                    layout: layout.clone(),
                    paint: Vec::new(),
                    paint_order: PaintOrderDeclaration::source_order(target.clone()),
                    hit_regions: Vec::new(),
                    focus_regions: Vec::new(),
                    scroll_regions: vec![outer_scroll.clone(), inner_scroll],
                    wheel_traversal_boundary: slipway_core::DeclaredWheelTraversalBoundary {
                        terminal_region_index: Some(0),
                    },
                    semantic_slots: Vec::new(),
                    probe_metadata: Vec::new(),
                    diagnostics: Vec::new(),
                },
                &mounted_slot,
            );
            assert!(mounted_view.scroll_regions[0].address.is_some());
            assert_eq!(
                mounted_view.wheel_traversal_boundary.terminal_region_index,
                Some(0)
            );
            let scroll_regions = mounted_view.scroll_regions;

            let rect = egui_test_rect(0.0, 0.0, 800.0, 600.0);
            let mut outer_region = test_presented_region(
                ui,
                "outer-scroll",
                EguiPresentedRegionKind::Scroll,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            outer_region.paint_sort_key = (0, 0, 0);
            let mut inner_region = test_presented_region(
                ui,
                "inner-scroll",
                EguiPresentedRegionKind::Scroll,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            inner_region.paint_sort_key = (1, 0, 0);
            let regions = vec![outer_region, inner_region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &[],
                scroll_regions: &scroll_regions,
                wheel_traversal_boundary: slipway_core::DeclaredWheelTraversalBoundary {
                    terminal_region_index: Some(0),
                },
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };

            let event = egui_backend_wheel_input_event(
                &context,
                egui::pos2(20.0, 580.0),
                0.0,
                -4.0,
                &mut Vec::new(),
            )
            .expect("outer receives wheel when inner is at bottom");
            let evidence = event
                .dispatch_evidence
                .as_ref()
                .expect("declared wheel evidence");
            assert_eq!(
                evidence.selected_region,
                Some(PresentationRegionId::from("outer-scroll"))
            );
            assert_eq!(
                evidence.input_position_space,
                Some(slipway_core::DispatchPositionSpace::Content),
                "the content dispatch branch must annotate its space"
            );
            let InputEvent::Wheel(wheel) = event.event else {
                panic!("expected wheel event");
            };
            assert_eq!(
                wheel.region_id,
                Some(PresentationRegionId::from("outer-scroll"))
            );
            assert_eq!(wheel.target, target);

            outer_scroll.offset.y = 884.0;
            let scroll_regions = vec![outer_scroll, scroll_regions[1].clone()];
            let context = EguiInputContext {
                scroll_regions: &scroll_regions,
                ..context
            };
            let mut refusals = Vec::new();
            let resolution = egui_backend_wheel_input_resolution(
                &context,
                egui::pos2(20.0, 580.0),
                0.0,
                -4.0,
                &mut refusals,
            );
            assert!(resolution.event.is_none());
            assert!(resolution.terminal_consumed);
            assert!(
                refusals.is_empty(),
                "terminal root absorption is not a refusal"
            );
        });
    }

    #[test]
    fn egui_wheel_resolution_computes_disposition_once() {
        let source = include_str!("lib.rs");
        let resolver = source
            .split("fn egui_wheel_resolve_position")
            .nth(1)
            .and_then(|tail| tail.split("struct EguiWheelInputResolution").next())
            .expect("wheel resolver source");
        assert_eq!(
            resolver
                .matches("declared_wheel_disposition_at_root_local_point_with_geometry_index")
                .count(),
            1,
            "the resolver has one disposition authority reused for visual fallback"
        );
        let input_resolution = source
            .split("fn egui_backend_wheel_input_resolution")
            .nth(1)
            .and_then(|tail| tail.split("fn egui_presented_scroll_region_by_id").next())
            .expect("wheel input resolution source");
        assert_eq!(
            input_resolution
                .matches("declared_wheel_disposition_at_root_local_point_with_geometry_index")
                .count(),
            0,
            "the selected disposition is passed forward, never recomputed"
        );
    }

    #[test]
    fn backend_wheel_event_honors_declared_self_first_outer_scroll() {
        // Mirror of `backend_wheel_event_uses_declared_boundary_bubbling`,
        // but the back outer region declares SelfFirst while the fronter
        // inner region is consumable: the backend wheel path must select the
        // SelfFirst outer region even though the inner would win by order
        // (ADR-0002 B2 routed through the shared core selector).
        egui::__run_test_ui(|ui| {
            let target = WidgetId::from("scroll");
            let frame = FrameIdentity {
                surface_id: "egui-test".to_string(),
                surface_instance_id: "wheel-self-first".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
            };
            let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let mut outer_scroll = test_scroll_region(target.clone(), frame.viewport);
            outer_scroll.id = PresentationRegionId::from("outer-scroll");
            outer_scroll.address = None;
            outer_scroll.content_bounds = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 300.0,
                },
            });
            outer_scroll.offset = Point { x: 0.0, y: 0.0 };
            outer_scroll.axes = ScrollAxes {
                horizontal: false,
                vertical: true,
            };
            outer_scroll.order = HitRegionOrder::default();
            outer_scroll.consumption = ScrollConsumptionPolicy::exclusive_wheel();
            let mut inner_scroll = outer_scroll.clone();
            inner_scroll.id = PresentationRegionId::from("inner-scroll");
            inner_scroll.wheel_routing = WheelRouting::NearestScrollable;
            inner_scroll.order = HitRegionOrder {
                z_index: 1,
                paint_order: 0,
                traversal_order: 0,
            };
            outer_scroll.wheel_routing = WheelRouting::SelfFirst;
            let scroll_regions = vec![outer_scroll, inner_scroll];

            let rect = egui_test_rect(0.0, 0.0, 100.0, 100.0);
            let mut outer_region = test_presented_region(
                ui,
                "outer-scroll",
                EguiPresentedRegionKind::Scroll,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            outer_region.paint_sort_key = (0, 0, 0);
            let mut inner_region = test_presented_region(
                ui,
                "inner-scroll",
                EguiPresentedRegionKind::Scroll,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            inner_region.paint_sort_key = (1, 0, 0);
            let regions = vec![outer_region, inner_region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &[],
                scroll_regions: &scroll_regions,
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };

            // Both regions contain the cursor and can consume delta_y = -4
            // (offset 0 of 200 max). The default would pick the fronter
            // inner region; SelfFirst overrides the order key.
            let event = egui_backend_wheel_input_event(
                &context,
                egui::pos2(20.0, 20.0),
                0.0,
                -4.0,
                &mut Vec::new(),
            )
            .expect("SelfFirst outer region consumes the wheel");
            let evidence = event
                .dispatch_evidence
                .as_ref()
                .expect("declared wheel evidence");
            assert_eq!(
                evidence.selected_region,
                Some(PresentationRegionId::from("outer-scroll"))
            );
            let InputEvent::Wheel(wheel) = event.event else {
                panic!("expected wheel event");
            };
            assert_eq!(
                wheel.region_id,
                Some(PresentationRegionId::from("outer-scroll"))
            );
            assert_eq!(wheel.target, target);
        });
    }

    #[test]
    fn wheel_transparent_occlusion_reaches_scroll_region_while_opaque_absorbs() {
        egui::__run_test_ui(|ui| {
            let target = WidgetId::from("scroll");
            let frame = FrameIdentity {
                surface_id: "egui-test".to_string(),
                surface_instance_id: "wheel-transparency".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
            };
            let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let mut scroll = test_scroll_region(target.clone(), frame.viewport);
            scroll.id = PresentationRegionId::from("root-scroll");
            scroll.address = None;
            scroll.content_bounds = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 300.0,
                },
            });
            scroll.offset = Point { x: 0.0, y: 0.0 };
            scroll.axes = ScrollAxes {
                horizontal: false,
                vertical: true,
            };
            scroll.wheel_routing = WheelRouting::NearestScrollable;
            scroll.order = HitRegionOrder::default();
            scroll.consumption = ScrollConsumptionPolicy::exclusive_wheel();
            let scroll_regions = vec![scroll];

            let rect = egui_test_rect(0.0, 0.0, 100.0, 100.0);
            let mut scroll_region = test_presented_region(
                ui,
                "root-scroll",
                EguiPresentedRegionKind::Scroll,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            scroll_region.paint_sort_key = (0, 0, 0);
            // A front (higher-key) opaque overlay occlusion over the scroll.
            let mut opaque_occlusion = test_presented_region(
                ui,
                "overlay-occlusion",
                EguiPresentedRegionKind::Occlusion,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            opaque_occlusion.paint_sort_key = (10, 0, 0);
            assert!(
                opaque_occlusion.blocks_wheel,
                "an occlusion derived from a default-opaque layer blocks the wheel"
            );
            // Same overlay authored wheel-pass-through: still a pointer occluder
            // (kind stays Occlusion) but the wheel channel is transparent.
            let mut wheel_transparent_occlusion = opaque_occlusion.clone();
            wheel_transparent_occlusion.blocks_wheel = false;

            // Default-opaque overlay ABSORBS the wheel (no dispatch produced).
            let opaque_regions = vec![scroll_region.clone(), opaque_occlusion];
            let opaque_context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &[],
                scroll_regions: &scroll_regions,
                wheel_traversal_boundary: Default::default(),
                response: &opaque_regions[0].response,
                regions: &opaque_regions,
                native_physical_operation: None,
            };
            let mut occlusion_refusals = Vec::new();
            assert!(
                egui_backend_wheel_input_event(
                    &opaque_context,
                    egui::pos2(20.0, 20.0),
                    0.0,
                    -4.0,
                    &mut occlusion_refusals
                )
                .is_none(),
                "a front default-opaque overlay must still absorb the wheel"
            );
            // MF-H3 (occlusion arm): the absorbed wheel must leave refusal
            // evidence naming the would-be consumer and the occluder cause.
            assert_eq!(occlusion_refusals.len(), 1);
            let occlusion_refusal = &occlusion_refusals[0];
            assert_eq!(
                occlusion_refusal.selected_region,
                Some(PresentationRegionId::from("root-scroll"))
            );
            assert!(
                occlusion_refusal.refusal_reason.as_deref().is_some_and(
                    |reason| reason.contains("occluded by a front wheel-blocking paint layer")
                ),
                "the refusal reason names the occluder: {:?}",
                occlusion_refusal.refusal_reason
            );

            // Wheel-transparent overlay lets the wheel REACH the scroll region.
            let through_regions = vec![scroll_region, wheel_transparent_occlusion];
            let through_context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &[],
                scroll_regions: &scroll_regions,
                wheel_traversal_boundary: Default::default(),
                response: &through_regions[0].response,
                regions: &through_regions,
                native_physical_operation: None,
            };
            let event = egui_backend_wheel_input_event(
                &through_context,
                egui::pos2(20.0, 20.0),
                0.0,
                -4.0,
                &mut Vec::new(),
            )
            .expect("a wheel-transparent overlay must let the scroll region behind consume it");
            let evidence = event
                .dispatch_evidence
                .as_ref()
                .expect("declared wheel evidence");
            assert_eq!(
                evidence.selected_region,
                Some(PresentationRegionId::from("root-scroll"))
            );
        });
    }

    #[test]
    fn backend_wheel_dispatch_position_follows_scrolled_region_like_pointer() {
        egui::__run_test_ui(|ui| {
            // Content/dispatch space is 100x400. The root has scrolled down by
            // 200, so the nested list (content y[200,300]) is shown at view-local
            // y[0,100]. Its presented region carries the scrolled visual origin,
            // exactly the offset source the pointer path already uses.
            let frame = FrameIdentity {
                surface_id: "egui-test".to_string(),
                surface_instance_id: "wheel-scrolled-region".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 400.0,
                    },
                },
            };
            // The list target sits at content-space y[200,300] (the geometry the
            // scrolled layout produced), while it is painted at view-local
            // y[0,100].
            let layout = test_layout_with_placements(
                TargetLocalRect::new(frame.viewport),
                [ChildPlacement {
                    child: WidgetId::from("list-scroll"),
                    bounds: test_parent_local(Rect {
                        origin: Point { x: 0.0, y: 200.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    local_state_slot: None,
                    spacing: BoxSpacing::ZERO,
                }],
            );
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);

            let mut root_scroll = test_scroll_region(WidgetId::from("root"), frame.viewport);
            root_scroll.id = PresentationRegionId::from("root-scroll");
            root_scroll.address = None;
            root_scroll.viewport = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 400.0,
                },
            });
            root_scroll.content_bounds = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 800.0,
                },
            });
            root_scroll.offset = Point { x: 0.0, y: 0.0 };
            root_scroll.axes = ScrollAxes {
                horizontal: false,
                vertical: true,
            };
            root_scroll.wheel_routing = WheelRouting::NearestScrollable;
            root_scroll.order = HitRegionOrder::default();
            root_scroll.consumption = ScrollConsumptionPolicy::exclusive_wheel();
            root_scroll.enabled = true;

            let mut list_scroll = root_scroll.clone();
            list_scroll.id = PresentationRegionId::from("list-scroll");
            list_scroll.target = WidgetId::from("list-scroll");
            list_scroll.viewport = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            });
            list_scroll.content_bounds = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 400.0,
                },
            });
            list_scroll.order = HitRegionOrder {
                z_index: 1,
                paint_order: 0,
                traversal_order: 0,
            };

            let scroll_regions = vec![root_scroll, list_scroll];
            // The list's presented region is painted at view-local y[0,100]
            // (`target_origin`), while its content rect (from the geometry index)
            // is at y[200,300]. That difference encodes the root scroll offset.
            let list_region = test_presented_region(
                ui,
                "list-scroll",
                EguiPresentedRegionKind::Scroll,
                egui_test_rect(0.0, 0.0, 100.0, 100.0),
                egui::Sense::click_and_drag(),
                CursorCapability::Default,
            );
            let regions = vec![list_region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &[],
                scroll_regions: &scroll_regions,
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };

            // The wheel maps its view-local cursor through the region under it
            // (like the pointer path), so (20, 50) view-local becomes (20, 250)
            // in content/dispatch space.
            assert_eq!(
                egui_wheel_dispatch_position(&context, egui::pos2(20.0, 50.0)),
                Point { x: 20.0, y: 250.0 },
                "wheel dispatch must map through the scrolled region, not stay view-local"
            );

            let event = egui_backend_wheel_input_event(
                &context,
                egui::pos2(20.0, 50.0),
                0.0,
                -4.0,
                &mut Vec::new(),
            )
            .expect("scrolled wheel resolves to the visually shown nested list");
            let evidence = event
                .dispatch_evidence
                .as_ref()
                .expect("declared wheel evidence");
            assert_eq!(
                evidence.selected_region,
                Some(PresentationRegionId::from("list-scroll")),
                "wheel over the nested list's visual position must route to the list, not the root"
            );
            assert_eq!(
                evidence.input_position,
                Some(Point { x: 20.0, y: 250.0 }),
                "wheel dispatch position must include the scroll offset the region encodes"
            );
            assert_eq!(
                evidence.input_position_space,
                Some(slipway_core::DispatchPositionSpace::Content),
                "the content dispatch branch must annotate its space"
            );
            let InputEvent::Wheel(wheel) = event.event else {
                panic!("expected wheel event");
            };
            assert_eq!(
                wheel.region_id,
                Some(PresentationRegionId::from("list-scroll"))
            );

            // Contrast: with no region under the cursor the wheel falls back to
            // the plain view-local mapping (20, 50), which only the root scroll
            // region contains -- the pre-fix behaviour that misrouted the wheel.
            let no_region: Vec<EguiPresentedRegion> = Vec::new();
            let fallback_context = EguiInputContext {
                regions: &no_region,
                ..context
            };
            assert_eq!(
                egui_wheel_dispatch_position(&fallback_context, egui::pos2(20.0, 50.0)),
                Point { x: 20.0, y: 50.0 }
            );
            let event = egui_backend_wheel_input_event(
                &fallback_context,
                egui::pos2(20.0, 50.0),
                0.0,
                -4.0,
                &mut Vec::new(),
            )
            .expect("fallback wheel resolves to the root scroll region");
            assert_eq!(
                event
                    .dispatch_evidence
                    .as_ref()
                    .expect("declared wheel evidence")
                    .selected_region,
                Some(PresentationRegionId::from("root-scroll"))
            );
        });
    }

    #[test]
    fn backend_wheel_root_chaining_survives_ancestor_scroll_past_viewport_band() {
        // Regression for the sustained-wheel FREEZE exposed by the Step 190 E1
        // content-space wheel mapping. The visible root scroll owns a 400-tall
        // viewport over 1000-tall content and has already scrolled down by 200.
        // The cursor sits low in the window (view-local y 380) over a nested
        // panel that the root scroll has displaced upward, so the content-space
        // dispatch point maps to y 580 -- past the root's un-scrolled viewport
        // band [0,400]. Core wheel-owner selection tests declared viewport rects
        // at their un-scrolled geometry positions, so no scroll region contains
        // y 580 and the raw content point yields no consumer: that is the dead
        // wheel the user hit ("works a few notches, then freezes"). The repair
        // falls back to the visible-viewport point (y 380), which the
        // un-displaced root scroll still contains, so root chaining continues.
        egui::__run_test_ui(|ui| {
            let frame = FrameIdentity {
                surface_id: "egui-test".to_string(),
                surface_instance_id: "wheel-freeze".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 400.0,
                    },
                },
            };
            // The nested panel target sits at content-space y[500,650] but is
            // painted (target_origin) at view-local y[300,450] -- the 200 gap is
            // the ancestor root scroll offset.
            let layout = test_layout_with_placements(
                TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 1000.0,
                    },
                }),
                [ChildPlacement {
                    child: WidgetId::from("panel"),
                    bounds: test_parent_local(Rect {
                        origin: Point { x: 0.0, y: 500.0 },
                        size: Size {
                            width: 100.0,
                            height: 150.0,
                        },
                    }),
                    local_state_slot: None,
                    spacing: BoxSpacing::ZERO,
                }],
            );
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);

            let mut root_scroll = test_scroll_region(
                WidgetId::from("root"),
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 400.0,
                    },
                },
            );
            root_scroll.id = PresentationRegionId::from("root-scroll");
            root_scroll.address = None;
            root_scroll.viewport = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 400.0,
                },
            });
            root_scroll.content_bounds = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 1000.0,
                },
            });
            root_scroll.offset = Point { x: 0.0, y: 200.0 };
            root_scroll.axes = ScrollAxes {
                horizontal: false,
                vertical: true,
            };
            let scroll_regions = vec![root_scroll];

            // The root scroll is presented at its un-scrolled viewport position
            // (target_origin == view origin), so it is an undisplaced ancestor.
            let root_region = test_presented_region(
                ui,
                "root-scroll",
                EguiPresentedRegionKind::Scroll,
                egui_test_rect(0.0, 0.0, 100.0, 400.0),
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            // The nested panel is presented displaced (view-local y[300,450]).
            let mut panel_region = test_presented_region(
                ui,
                "panel",
                EguiPresentedRegionKind::Hit,
                egui_test_rect(0.0, 300.0, 100.0, 150.0),
                egui::Sense::click_and_drag(),
                CursorCapability::Default,
            );
            panel_region.paint_sort_key = (1, 0, 0);
            let regions = vec![root_region, panel_region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &[],
                scroll_regions: &scroll_regions,
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };

            // The content-space dispatch point drifts past the root viewport band
            // and no scroll owner accepts it -- the freeze (pre-fix dead wheel).
            assert_eq!(
                egui_wheel_dispatch_position(&context, egui::pos2(20.0, 380.0)),
                Point { x: 20.0, y: 580.0 },
                "cursor over the ancestor-displaced panel maps to content y 580"
            );
            assert!(
                egui_wheel_select_consumer(&context, Point { x: 20.0, y: 580.0 }, 0.0, -4.0)
                    .is_none(),
                "content point past the root viewport band has no wheel owner (the freeze)"
            );

            // The repaired resolve position falls back to the visible-viewport
            // point, which the undisplaced root scroll still owns.
            let event = egui_backend_wheel_input_event(
                &context,
                egui::pos2(20.0, 380.0),
                0.0,
                -4.0,
                &mut Vec::new(),
            )
            .expect(
                "sustained wheel must keep chaining to the root after the ancestor scroll \
                     pushes the content point past the viewport band (no freeze)",
            );
            let evidence = event
                .dispatch_evidence
                .as_ref()
                .expect("declared wheel evidence");
            assert_eq!(
                evidence.selected_region,
                Some(PresentationRegionId::from("root-scroll")),
                "the wheel resolves to the root scroll owner via the visible-viewport point"
            );
            assert_eq!(
                evidence.input_position,
                Some(Point { x: 20.0, y: 380.0 }),
                "the repaired resolve position is the visible-viewport point, not the drifted \
                 content point"
            );
            assert_eq!(
                evidence.input_position_space,
                Some(slipway_core::DispatchPositionSpace::Viewport),
                "the visible-viewport fallback branch must annotate its space"
            );
            let InputEvent::Wheel(wheel) = event.event else {
                panic!("expected wheel event");
            };
            assert_eq!(
                wheel.region_id,
                Some(PresentationRegionId::from("root-scroll"))
            );

            // MF-M1 fallback-origin fix: the visible-viewport point must add
            // `frame.viewport.origin` exactly like the primary path
            // (`egui_view_root_local_position`) so both wheel branches live in
            // the same root-local frame family.
            let mut origin_frame = frame.clone();
            origin_frame.viewport.origin = Point { x: 8.0, y: 40.0 };
            let origin_context = EguiInputContext {
                frame: &origin_frame,
                ..context
            };
            assert_eq!(
                egui_wheel_visible_viewport_position(&origin_context, egui::pos2(20.0, 380.0)),
                Point { x: 28.0, y: 420.0 },
                "the fallback point must include frame.viewport.origin"
            );
        });
    }

    #[test]
    fn captured_drag_move_preserves_pressed_primary_button() {
        let ctx = egui::Context::default();
        let mut bridge = DefaultEguiBridge::new();

        let frame = FrameIdentity {
            surface_id: "egui-test".to_string(),
            surface_instance_id: "captured-drag".to_string(),
            revision: 1,
            frame_index: 1,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 160.0,
                    height: 120.0,
                },
            },
        };
        let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
        let drag_bounds = Rect {
            origin: Point { x: 8.0, y: 8.0 },
            size: Size {
                width: 40.0,
                height: 24.0,
            },
        };

        // Pass 1: a primary press inside the drag handle begins the capture.
        let press_input = egui::RawInput {
            events: vec![egui::Event::PointerButton {
                pos: egui::pos2(20.0, 20.0),
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::default(),
            }],
            ..Default::default()
        };
        let _ = ctx.run_ui(press_input, |ui| {
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let mut hit = test_hit_region("drag-hit", WidgetId::from("drag"), drag_bounds, 0);
            hit.capture = PointerCaptureIntent::DuringDrag;
            let hit_regions = vec![hit];
            let region = test_presented_region(
                ui,
                "drag-hit",
                EguiPresentedRegionKind::Hit,
                egui_test_rect(8.0, 8.0, 40.0, 24.0),
                egui::Sense::click_and_drag(),
                CursorCapability::Grab,
            );
            let regions = vec![region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &hit_regions,
                focus_regions: &[],
                scroll_regions: &[],
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };
            let _ = EguiSlipwayBridge::<ProbeWidget>::input_events(&mut bridge, context);
        });

        assert!(
            bridge.pointer_capture_region.is_some(),
            "primary press on a drag handle must begin the stateful pointer capture"
        );
        assert_eq!(
            bridge.pointer_capture_button,
            Some(egui::PointerButton::Primary),
            "the capture must remember the button that started the drag"
        );

        // Pass 2: a captured move (pointer leaves the handle bounds) must still
        // carry the held primary button so shared drag handlers do not cancel.
        let move_input = egui::RawInput {
            events: vec![egui::Event::PointerMoved(egui::pos2(140.0, 96.0))],
            ..Default::default()
        };
        let mut produced: Vec<BackendInputEvent> = Vec::new();
        let _ = ctx.run_ui(move_input, |ui| {
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let mut hit = test_hit_region("drag-hit", WidgetId::from("drag"), drag_bounds, 0);
            hit.capture = PointerCaptureIntent::DuringDrag;
            let hit_regions = vec![hit];
            let region = test_presented_region(
                ui,
                "drag-hit",
                EguiPresentedRegionKind::Hit,
                egui_test_rect(8.0, 8.0, 40.0, 24.0),
                egui::Sense::click_and_drag(),
                CursorCapability::Grab,
            );
            let regions = vec![region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &hit_regions,
                focus_regions: &[],
                scroll_regions: &[],
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };
            produced = EguiSlipwayBridge::<ProbeWidget>::input_events(&mut bridge, context);
        });

        let move_pointer = produced
            .iter()
            .find_map(|event| match &event.event {
                InputEvent::Pointer(pointer) if pointer.kind == PointerEventKind::Move => {
                    Some(pointer)
                }
                _ => None,
            })
            .expect("captured drag move must produce a pointer Move input");
        assert!(
            move_pointer.details.buttons.primary,
            "captured drag move must preserve the pressed primary button state"
        );
    }

    #[test]
    fn text_edit_response_wins_inside_broader_region() {
        egui::__run_test_ui(|ui| {
            let text_rect = egui_test_rect(10.0, 10.0, 40.0, 20.0);
            let mut text_region = test_presented_region(
                ui,
                "text-edit",
                EguiPresentedRegionKind::TextEdit,
                text_rect,
                egui::Sense::click(),
                CursorCapability::Text,
            );
            text_region.text_edit_change = Some(test_text_edit_change());

            let broad_region = test_presented_region(
                ui,
                "broad-hit",
                EguiPresentedRegionKind::Hit,
                egui_test_rect(0.0, 0.0, 100.0, 80.0),
                egui::Sense::click(),
                CursorCapability::Pointer,
            );
            let regions = vec![text_region, broad_region];

            let selected = egui_region_at_position(&regions, egui::pos2(20.0, 15.0))
                .expect("text edit point should select a region");

            assert_eq!(selected.region_id.as_str(), "text-edit");
            assert_eq!(selected.kind, EguiPresentedRegionKind::TextEdit);
        });
    }

    #[test]
    fn geometry_fallback_still_selects_region_without_response_authority() {
        egui::__run_test_ui(|ui| {
            let first_region = test_presented_region(
                ui,
                "first-hit",
                EguiPresentedRegionKind::Hit,
                egui_test_rect(0.0, 0.0, 40.0, 40.0),
                egui::Sense::click(),
                CursorCapability::Pointer,
            );
            let fallback_region = test_presented_region(
                ui,
                "fallback-hit",
                EguiPresentedRegionKind::Hit,
                egui_test_rect(10.0, 10.0, 40.0, 40.0),
                egui::Sense::click(),
                CursorCapability::Crosshair,
            );
            let regions = vec![first_region, fallback_region];

            let selected = egui_region_at_position(&regions, egui::pos2(20.0, 20.0))
                .expect("geometry fallback should select an overlapping region");

            assert_eq!(selected.region_id.as_str(), "fallback-hit");
        });
    }

    #[test]
    fn opaque_layer_occlusion_does_not_block_same_key_hit_region() {
        egui::__run_test_ui(|ui| {
            let rect = egui_test_rect(0.0, 0.0, 80.0, 48.0);
            let mut hit_region = test_presented_region(
                ui,
                "overlay-titlebar-hit",
                EguiPresentedRegionKind::Hit,
                rect,
                egui::Sense::click_and_drag(),
                CursorCapability::Grab,
            );
            hit_region.paint_sort_key = (10, 3, 3);
            let mut occlusion = test_presented_region(
                ui,
                "overlay-paint-occlusion",
                EguiPresentedRegionKind::Occlusion,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            occlusion.paint_sort_key = (10, 3, 3);
            let regions = vec![occlusion, hit_region];

            let selected = egui_region_at_position(&regions, egui::pos2(20.0, 12.0))
                .expect("same-key overlay titlebar hit must remain interactive");

            assert_eq!(selected.region_id.as_str(), "overlay-titlebar-hit");
        });
    }

    #[test]
    fn higher_opaque_layer_occlusion_blocks_lower_hit_region() {
        egui::__run_test_ui(|ui| {
            let rect = egui_test_rect(0.0, 0.0, 80.0, 48.0);
            let mut lower_hit = test_presented_region(
                ui,
                "lower-button-hit",
                EguiPresentedRegionKind::Hit,
                rect,
                egui::Sense::click(),
                CursorCapability::Pointer,
            );
            lower_hit.paint_sort_key = (10, 2, 2);
            let mut higher_occlusion = test_presented_region(
                ui,
                "higher-overlay-occlusion",
                EguiPresentedRegionKind::Occlusion,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            higher_occlusion.paint_sort_key = (10, 3, 3);
            let regions = vec![lower_hit, higher_occlusion];

            assert!(
                egui_region_at_position(&regions, egui::pos2(20.0, 12.0)).is_none(),
                "higher opaque layer must absorb pointer input over lower controls"
            );
        });
    }

    #[test]
    fn higher_same_owner_opaque_layer_blocks_lower_hit_region() {
        egui::__run_test_ui(|ui| {
            let rect = egui_test_rect(0.0, 0.0, 80.0, 48.0);
            let owner = WidgetId::from("same-owner-overlay");
            let address = Some(WidgetSlotAddress::new(owner.clone(), 0));
            let mut lower_hit = test_presented_region(
                ui,
                "same-owner-lower-hit",
                EguiPresentedRegionKind::Hit,
                rect,
                egui::Sense::click(),
                CursorCapability::Pointer,
            );
            lower_hit.target = owner.clone();
            lower_hit.address = address.clone();
            lower_hit.paint_sort_key = (10, 2, 9);
            let mut higher_occlusion = test_presented_region(
                ui,
                "same-owner-higher-occlusion",
                EguiPresentedRegionKind::Occlusion,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            higher_occlusion.target = owner;
            higher_occlusion.address = address;
            higher_occlusion.paint_sort_key = (10, 3, 0);
            // The higher paint_order is EXPLICIT (an authored
            // `PaintLayerKey::ordered` within-z order) — that is what this
            // test pins; an unordered same-owner layer no longer blocks
            // (NC-2, next test).
            higher_occlusion.authored_z_order = Some(3);
            let regions = vec![lower_hit, higher_occlusion];

            assert!(
                egui_region_at_position(&regions, egui::pos2(20.0, 12.0)).is_none(),
                "same owner/address must not bypass a higher explicit paint_order"
            );
        });
    }

    /// NC-2 regression (roadmap Phase 6 item 1) on the egui side: the
    /// consumer modal shape. The widget's own opaque layer occluder carries
    /// the paint-unit sort key whose tie-break fields defaulted to the
    /// MOUNTED SLOT ORDINAL (here 7) with NO authored within-z order, while
    /// the widget's own hit region carries its author-declared
    /// `HitRegionOrder` (z 100, 0, 0) — exactly what `allocate_hit_region`
    /// and `paint_occlusion_region` produce for an authored opaque overlay
    /// mounted as the 8th child. The press must route to the hit region.
    /// Revert-and-fail: the pre-repair same-owner exemption required exact
    /// key.1 equality (7 != 0), so the overlay was occluded by itself.
    #[test]
    fn own_unordered_opaque_layer_does_not_block_same_z_declared_hit_region() {
        egui::__run_test_ui(|ui| {
            let rect = egui_test_rect(0.0, 0.0, 120.0, 80.0);
            let owner = WidgetId::from("nc2-egui-modal-overlay");
            let address = Some(WidgetSlotAddress::new(owner.clone(), 7));
            let mut hit_region = test_presented_region(
                ui,
                "nc2-egui-modal-overlay-hit",
                EguiPresentedRegionKind::Hit,
                rect,
                egui::Sense::click(),
                CursorCapability::Pointer,
            );
            hit_region.target = owner.clone();
            hit_region.address = address.clone();
            // The author-declared order (allocate_hit_region stores the
            // declared HitRegionOrder for hit regions).
            hit_region.paint_sort_key = (100, 0, 0);
            let mut occlusion = test_presented_region(
                ui,
                "nc2-egui-modal-overlay-occlusion",
                EguiPresentedRegionKind::Occlusion,
                rect,
                egui::Sense::hover(),
                CursorCapability::Default,
            );
            occlusion.target = owner;
            occlusion.address = address;
            // The paint-unit key of `PaintLayerKey::new(100)` mounted at
            // slot ordinal 7: defaulted tie-break fields, no authored
            // within-z order.
            occlusion.paint_sort_key = (100, 7, 7);
            occlusion.authored_z_order = None;
            let regions = vec![occlusion, hit_region];

            let selected = egui_region_at_position(&regions, egui::pos2(20.0, 12.0))
                .expect("the overlay's own same-z hit region receives the press (NC-2)");
            assert_eq!(selected.region_id.as_str(), "nc2-egui-modal-overlay-hit");
        });
    }

    /// NC-2 no-silence contract, egui parity arm (roadmap Phase 6 item 1):
    /// a press — and its release — consumed by a pointer-opaque paint layer
    /// with NO reachable hit region must leave refusal evidence in the
    /// bridge's refusal ring, never silence. Mirrors the iced press/release
    /// occlusion arms (`blocked_pointer_refusal_evidence`). Revert-and-fail:
    /// before the repair the blocked pointer event hit the bare
    /// `else { continue; }` in `input_events` and vanished without a trace.
    #[test]
    fn blocked_pointer_press_over_opaque_layer_records_refusal_evidence() {
        let ctx = egui::Context::default();
        let mut bridge = DefaultEguiBridge::new();

        let frame = FrameIdentity {
            surface_id: "egui-test".to_string(),
            surface_instance_id: "nc2-blocked-press".to_string(),
            revision: 1,
            frame_index: 1,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 160.0,
                    height: 120.0,
                },
            },
        };
        let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
        let covered_bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 160.0,
                height: 120.0,
            },
        };

        for (pressed, phase) in [(true, "Press"), (false, "Release")] {
            let input = egui::RawInput {
                events: vec![egui::Event::PointerButton {
                    pos: egui::pos2(20.0, 20.0),
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                }],
                ..Default::default()
            };
            let mut produced: Vec<BackendInputEvent> = Vec::new();
            let _ = ctx.run_ui(input, |ui| {
                let geometry_index = PresentationGeometryIndex::from_layout(&layout);
                // A declared hit region genuinely covers the point — the
                // refusal must name it as a candidate.
                let hit_regions = vec![test_hit_region(
                    "covered-hit",
                    WidgetId::from("covered"),
                    covered_bounds,
                    0,
                )];
                // The only PRESENTED region at the point is a foreign
                // opaque layer's occlusion region fronting everything.
                let mut occlusion = test_presented_region(
                    ui,
                    "nc2-blocking-overlay-occlusion",
                    EguiPresentedRegionKind::Occlusion,
                    egui_test_rect(0.0, 0.0, 160.0, 120.0),
                    egui::Sense::hover(),
                    CursorCapability::Default,
                );
                occlusion.target = WidgetId::from("blocking-overlay");
                occlusion.paint_sort_key = (10, 0, 0);
                occlusion.authored_z_order = Some(0);
                let regions = vec![occlusion];
                let context = EguiInputContext {
                    ui,
                    widget_id: WidgetId::from("root"),
                    frame: &frame,
                    rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                    layout: &layout,
                    geometry_index: &geometry_index,
                    hit_regions: &hit_regions,
                    focus_regions: &[],
                    scroll_regions: &[],
                    wheel_traversal_boundary: Default::default(),
                    response: &regions[0].response,
                    regions: &regions,
                    native_physical_operation: None,
                };
                produced = EguiSlipwayBridge::<ProbeWidget>::input_events(&mut bridge, context);
            });
            assert!(
                produced.is_empty(),
                "{phase}: the blocked pointer event produces no backend input: {produced:?}"
            );
            let refusals = bridge.take_dispatch_refusals();
            assert_eq!(
                refusals.len(),
                1,
                "{phase}: exactly one refusal is retained (NC-2 no-silence contract): {refusals:?}"
            );
            let refusal = &refusals[0];
            assert_eq!(refusal.kind, DeclaredEventDispatchKind::Pointer);
            assert!(refusal.selected_region.is_none());
            assert!(
                refusal
                    .candidate_regions
                    .contains(&PresentationRegionId::from("covered-hit")),
                "{phase}: refusal names the covered candidate region: {:?}",
                refusal.candidate_regions
            );
            let reason = refusal
                .refusal_reason
                .as_deref()
                .expect("blocked-pointer refusal carries a reason");
            assert!(
                reason.contains("blocking-overlay") && reason.contains(phase),
                "{phase}: refusal names the occluding layer's owner and the pointer phase: {reason}"
            );
        }
    }

    #[test]
    fn cursor_selection_follows_chosen_response_region() {
        egui::__run_test_ui(|ui| {
            let mut text_region = test_presented_region(
                ui,
                "cursor-text",
                EguiPresentedRegionKind::TextEdit,
                egui_test_rect(5.0, 5.0, 50.0, 24.0),
                egui::Sense::click(),
                CursorCapability::Text,
            );
            text_region.text_edit_change = Some(test_text_edit_change());

            let geometric_region = test_presented_region(
                ui,
                "cursor-geometry",
                EguiPresentedRegionKind::Hit,
                egui_test_rect(0.0, 0.0, 80.0, 60.0),
                egui::Sense::click(),
                CursorCapability::Pointer,
            );
            let regions = vec![text_region, geometric_region];

            let selected = egui_region_at_position(&regions, egui::pos2(10.0, 10.0))
                .expect("cursor point should select a region");

            assert_eq!(selected.region_id.as_str(), "cursor-text");
            assert_eq!(selected.cursor, CursorCapability::Text);
        });
    }

    impl SlipwayWidgetTypes for ProbeWidget {
        type ExternalState = ();
        type LocalState = usize;
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for ProbeWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::PointerInput, Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id.clone())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for ProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            *local += 1;
            EventOutcome::message(self.id.clone(), "routed", ProbeMessage::Routed)
        }
    }

    impl SlipwayView for ProbeWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            7
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some("probe-fill".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds().into_rect(),
                    path: None,
                    clip: None,
                },
                color: slipway_core::Color {
                    red: 0.1,
                    green: 0.2,
                    blue: 0.3,
                    alpha: 1.0,
                },
            }]
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![StateObservation {
                target: self.id.clone(),
                slot: None,
                name: "local".to_string(),
                value: local.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for ProbeWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id.clone(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id.clone()),
                hit_regions: vec![test_hit_region(
                    "probe-hit",
                    self.id.clone(),
                    *layout.bounds(),
                    0,
                )],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl TallRootWidget {
        fn new(height: f32) -> Self {
            Self {
                id: WidgetId::from("egui.tall-root"),
                height,
            }
        }
    }

    impl SlipwayWidgetTypes for TallRootWidget {
        type ExternalState = ();
        type LocalState = usize;
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for TallRootWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id.clone())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for TallRootWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            if matches!(event, InputEvent::Wheel(_)) {
                *local += 1;
                EventOutcome::message(self.id.clone(), "wheeled", ProbeMessage::Routed)
            } else {
                EventOutcome::ignored()
            }
        }
    }

    impl SlipwayView for TallRootWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            0
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: input.viewport.into_rect().size.width,
                    height: self.height,
                },
            }))
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some("tall-root-fill".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds().into_rect(),
                    path: None,
                    clip: None,
                },
                color: test_rgb(24, 36, 48),
            }]
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            Vec::new()
        }
    }

    impl SlipwayViewDefinition for TallRootWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayFontResolutionPolicy for TallRootWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayEguiAuthoredChildren for TallRootWidget {
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

    impl slipway_core::SlipwayEventRoutingPolicy for TallRootWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            slipway_core::EventRoutingPolicyDeclaration {
                target: self.id(),
                event_target: event.target().clone(),
                route: slipway_core::EventRoute {
                    route_id: Some("egui.tall-root.route".to_string()),
                    address: event.target_slot().cloned(),
                    path: vec![self.id()],
                    phase: slipway_core::EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for TallRootWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            _route: &slipway_core::EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let handled = event.target() == &self.id();
            let disposition = slipway_core::EventDisposition {
                handled,
                propagate: !handled,
                default_action_allowed: true,
            };
            slipway_core::EventPropagationEvidence {
                target: self.id(),
                event: event.clone(),
                steps: Vec::new(),
                final_disposition: disposition,
                diagnostics: Vec::new(),
            }
        }
    }

    impl LayeredPaintChild {
        fn new(id: &str, z_index: i32, order: Option<usize>) -> Self {
            Self {
                id: WidgetId::from(id),
                z_index,
                order,
                paint_order_mode: PaintOrderMode::ExplicitLayered,
                paint_bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 50.0,
                        height: 50.0,
                    },
                },
                color: test_rgb(80, 120, 160),
                overflow_bounds: None,
                inner_layer: None,
                hit_region: None,
            }
        }

        fn with_source_order(mut self) -> Self {
            self.paint_order_mode = PaintOrderMode::SourceOrder;
            self
        }

        fn with_paint_bounds(mut self, paint_bounds: Rect) -> Self {
            self.paint_bounds = paint_bounds;
            self
        }

        fn with_color(mut self, color: slipway_core::Color) -> Self {
            self.color = color;
            self
        }

        fn with_overflow_bounds(mut self, overflow_bounds: Rect) -> Self {
            self.overflow_bounds = Some(overflow_bounds);
            self
        }

        fn with_inner_layer(
            mut self,
            z_index: i32,
            order: Option<usize>,
            color: slipway_core::Color,
        ) -> Self {
            self.inner_layer = Some((z_index, order, color));
            self
        }

        fn with_hit_region(mut self, bounds: Rect, order: HitRegionOrder) -> Self {
            self.hit_region = Some((bounds, order));
            self
        }
    }

    impl SlipwayWidgetTypes for LayeredPaintChild {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for LayeredPaintChild {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id.clone())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for LayeredPaintChild {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for LayeredPaintChild {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            let mut paint = vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some(format!("{}-fill", self.id.as_str())),
                    kind: ShapeKind::Rectangle,
                    bounds: self.paint_bounds,
                    path: None,
                    clip: None,
                },
                color: self.color,
            }];
            if let Some((z_index, order, color)) = self.inner_layer {
                let key = order.map_or_else(
                    || slipway_core::PaintLayerKey::new(z_index),
                    |order| slipway_core::PaintLayerKey::ordered(z_index, order),
                );
                paint.push(
                    PaintOp::keyed_layer(
                        key,
                        vec![PaintOp::Fill {
                            shape: ShapeDeclaration {
                                id: Some(format!("{}-inner-layer-fill", self.id.as_str())),
                                kind: ShapeKind::Rectangle,
                                bounds: Rect {
                                    origin: Point { x: 8.0, y: 8.0 },
                                    size: Size {
                                        width: 34.0,
                                        height: 34.0,
                                    },
                                },
                                path: None,
                                clip: None,
                            },
                            color,
                        }],
                    )
                    .with_layer_id(format!("{}-inner-layer", self.id.as_str())),
                );
            }
            paint
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            Vec::new()
        }
    }

    impl SlipwayViewDefinition for LayeredPaintChild {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            let mut paint_order = match &self.paint_order_mode {
                PaintOrderMode::SourceOrder => {
                    slipway_core::PaintOrderDeclaration::source_order(self.id())
                }
                PaintOrderMode::ExplicitLayered => {
                    let mut paint_order =
                        slipway_core::PaintOrderDeclaration::layer(self.id(), self.z_index);
                    paint_order.order = self.order;
                    paint_order
                }
            };
            if let Some(overflow_bounds) = self.overflow_bounds {
                paint_order.allow_overlap = true;
                paint_order =
                    paint_order.with_overflow_bounds(TargetLocalRect::new(overflow_bounds));
            }
            let hit_regions = self
                .hit_region
                .as_ref()
                .map(|(bounds, order)| {
                    slipway_core::hit_region_from_pointer_capability(
                        self,
                        external,
                        local,
                        PresentationRegionId::from(format!("{}:hit", self.id.as_str())),
                        None,
                        TargetLocalRect::new(*bounds),
                        PointerEventCoordinateSpace::TargetLocal,
                        order.clone(),
                        Some(format!("{}:hit", self.id.as_str())),
                        CursorCapability::Pointer,
                        true,
                        PointerCaptureIntent::DuringDrag,
                    )
                })
                .into_iter()
                .collect();

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order,
                hit_regions,
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayFontResolutionPolicy for LayeredPaintChild {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayEguiAuthoredChildren for LayeredPaintChild {
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

    impl slipway_core::SlipwayEventRoutingPolicy for LayeredPaintChild {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            slipway_core::EventRoutingPolicyDeclaration {
                target: self.id(),
                event_target: event.target().clone(),
                route: slipway_core::EventRoute {
                    route_id: Some(format!("{}.route", self.id.as_str())),
                    address: event.target_slot().cloned(),
                    path: vec![self.id()],
                    phase: slipway_core::EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for LayeredPaintChild {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            _route: &slipway_core::EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let handled = event.target() == &self.id();
            let disposition = slipway_core::EventDisposition {
                handled,
                propagate: !handled,
                default_action_allowed: true,
            };
            slipway_core::EventPropagationEvidence {
                target: self.id(),
                event: event.clone(),
                steps: Vec::new(),
                final_disposition: disposition,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayWidgetTypes for LayeredPaintApp {
        type ExternalState = ();
        type LocalState = ((), ());
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for LayeredPaintApp {
        fn id(&self) -> WidgetId {
            WidgetId::from("egui.layered-app")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::ChildTraversal,
                Capability::Layout,
                Capability::Paint,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode {
                id: self.id(),
                children: vec![
                    TopologyNode::leaf(self.children.0.id()),
                    TopologyNode::leaf(self.children.1.id()),
                ],
                local_state_slot: Some(WidgetSlotAddress::new(self.id(), 0)),
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }

        fn visit_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            visitor.visit_child(&self.children.0, external, &local.0, self.child_slot(0));
            visitor.visit_child(&self.children.1, external, &local.1, self.child_slot(1));
        }
    }

    impl SlipwayLogic for LayeredPaintApp {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for LayeredPaintApp {
        fn initial_local_state(&self) -> Self::LocalState {
            ((), ())
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            test_layout_with_placements_from(
                output,
                input.viewport,
                [
                    ChildPlacement {
                        child: self.children.0.id(),
                        bounds: test_parent_local(Rect {
                            origin: Point { x: 0.0, y: 0.0 },
                            size: Size {
                                width: 50.0,
                                height: 50.0,
                            },
                        }),
                        local_state_slot: Some(self.child_slot(0)),
                        spacing: BoxSpacing::ZERO,
                    },
                    ChildPlacement {
                        child: self.children.1.id(),
                        bounds: test_parent_local(Rect {
                            origin: Point { x: 10.0, y: 10.0 },
                            size: Size {
                                width: 50.0,
                                height: 50.0,
                            },
                        }),
                        local_state_slot: Some(self.child_slot(1)),
                        spacing: BoxSpacing::ZERO,
                    },
                ],
            )
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            let mut paint = Vec::new();
            if let Some(color) = self.root_fill {
                paint.push(PaintOp::Fill {
                    shape: ShapeDeclaration {
                        id: Some("root-source-order-fill".to_string()),
                        kind: ShapeKind::Rectangle,
                        bounds: Rect {
                            origin: Point { x: 0.0, y: 0.0 },
                            size: Size {
                                width: 100.0,
                                height: 100.0,
                            },
                        },
                        path: None,
                        clip: None,
                    },
                    color,
                });
            }
            paint.extend(self.root_layer.map(|(z_index, order, color)| {
                let key = order.map_or_else(
                    || slipway_core::PaintLayerKey::new(z_index),
                    |order| slipway_core::PaintLayerKey::ordered(z_index, order),
                );
                PaintOp::keyed_layer(
                    key,
                    vec![PaintOp::Fill {
                        shape: ShapeDeclaration {
                            id: Some("root-keyed-layer-fill".to_string()),
                            kind: ShapeKind::Rectangle,
                            bounds: Rect {
                                origin: Point { x: 5.0, y: 5.0 },
                                size: Size {
                                    width: 60.0,
                                    height: 60.0,
                                },
                            },
                            path: None,
                            clip: None,
                        },
                        color,
                    }],
                )
                .with_layer_id("root-keyed-layer")
            }));
            paint
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            Vec::new()
        }
    }

    impl SlipwayViewDefinition for LayeredPaintApp {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayFontResolutionPolicy for LayeredPaintApp {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventRoutingPolicy for LayeredPaintApp {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            slipway_core::EventRoutingPolicyDeclaration {
                target: self.id(),
                event_target: event.target().clone(),
                route: slipway_core::EventRoute {
                    route_id: Some("egui.layered-app.route".to_string()),
                    address: event.target_slot().cloned(),
                    path: vec![self.id(), event.target().clone()],
                    phase: slipway_core::EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for LayeredPaintApp {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            _route: &slipway_core::EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let handled = event.target() == &self.id();
            let disposition = slipway_core::EventDisposition {
                handled,
                propagate: !handled,
                default_action_allowed: true,
            };
            slipway_core::EventPropagationEvidence {
                target: self.id(),
                event: event.clone(),
                steps: Vec::new(),
                final_disposition: disposition,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayEguiAuthoredChildren for LayeredPaintApp {
        fn visit_egui_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayEguiWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            visitor.visit_egui_child(&self.children.0, external, &local.0, self.child_slot(0));
            visitor.visit_egui_child(&self.children.1, external, &local.1, self.child_slot(1));
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
            let root_slot = WidgetSlotAddress::new(self.id(), 0);
            let mut order = vec![
                (
                    egui_child_paint_sort_key(
                        &self.children.0,
                        external,
                        &local.0,
                        &root_slot,
                        0,
                        parent_view,
                    ),
                    0,
                ),
                (
                    egui_child_paint_sort_key(
                        &self.children.1,
                        external,
                        &local.1,
                        &root_slot,
                        1,
                        parent_view,
                    ),
                    1,
                ),
            ];
            order.sort_by_key(|(key, _)| *key);
            for (_, index) in order {
                match index {
                    0 => visitor.visit_egui_child(
                        &self.children.0,
                        external,
                        &local.0,
                        self.child_slot(0),
                    ),
                    1 => visitor.visit_egui_child(
                        &self.children.1,
                        external,
                        &local.1,
                        self.child_slot(1),
                    ),
                    _ => {}
                }
            }
        }
    }

    impl LayeredPaintApp {
        fn child_slot(&self, index: usize) -> WidgetSlotAddress {
            let child = if index == 0 {
                self.children.0.id()
            } else {
                self.children.1.id()
            };
            WidgetSlotAddress::new(self.id(), 0).child(child, index)
        }
    }

    impl SlipwayWidgetTypes for ScrollProbeWidget {
        type ExternalState = ();
        type LocalState = usize;
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for ScrollProbeWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::WheelInput,
                Capability::ScrollRegionPresentation,
                Capability::Paint,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id.clone())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for ScrollProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Scroll(scroll)
                    if scroll.target == self.id.clone()
                        && scroll.region_id == PresentationRegionId::from("scroll-region") =>
                {
                    *local += 1;
                    EventOutcome::message(self.id.clone(), "scrolled", ProbeMessage::Routed)
                }
                InputEvent::Wheel(wheel)
                    if wheel.target == self.id.clone()
                        && wheel.target_slot
                            == Some(WidgetSlotAddress::new(self.id.clone(), 0))
                        && wheel.delta_x == 0.0
                        && wheel.delta_y == 7.0 =>
                {
                    *local += 1;
                    EventOutcome::message(self.id.clone(), "wheeled", ProbeMessage::Routed)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for ScrollProbeWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            7
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some("scroll-probe-fill".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds().into_rect(),
                    path: None,
                    clip: None,
                },
                color: slipway_core::Color {
                    red: 0.1,
                    green: 0.3,
                    blue: 0.4,
                    alpha: 1.0,
                },
            }]
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![StateObservation {
                target: self.id.clone(),
                slot: None,
                name: "scrolls".to_string(),
                value: local.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for ScrollProbeWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id.clone(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id.clone()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: vec![test_scroll_region(
                    self.id.clone(),
                    layout.bounds().into_rect(),
                )],
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayFontResolutionPolicy for ScrollProbeWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayLayoutIntent for ScrollProbeWidget {
        fn layout_intent(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
        ) -> LayoutIntentProbe {
            LayoutIntentProbe {
                target: self.id.clone(),
                intrinsic_size: None,
                size_policy: Some(slipway_core::SizePolicyDeclaration {
                    target: self.id.clone(),
                    width: slipway_core::SizePolicy::Fill { weight: 1.0 },
                    height: slipway_core::SizePolicy::FitContent,
                }),
                resize_policy: None,
                overflow_policy: Some(slipway_core::OverflowPolicyDeclaration {
                    target: self.id.clone(),
                    x: slipway_core::OverflowBehavior::Clip,
                    y: slipway_core::OverflowBehavior::Scroll,
                }),
                auto_layout: None,
                responsive_variant: Some(slipway_core::ResponsiveVariant {
                    target: self.id.clone(),
                    key: if input.viewport.size.width < 400.0 {
                        "compact".to_string()
                    } else {
                        "wide".to_string()
                    },
                    active_breakpoints: Vec::new(),
                    reason: None,
                }),
                text_flow: None,
                text_measurement_cache: Vec::new(),
                text_measurement: None,
                fit_overflow: Vec::new(),
                layer: None,
                scroll: None,
                collection: None,
                interaction_styles: Vec::new(),
            }
        }
    }

    impl SlipwayFontResolutionPolicy for ProbeWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayLayoutIntent for ProbeWidget {
        fn layout_intent(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
        ) -> LayoutIntentProbe {
            LayoutIntentProbe {
                target: self.id.clone(),
                intrinsic_size: None,
                size_policy: Some(slipway_core::SizePolicyDeclaration {
                    target: self.id.clone(),
                    width: slipway_core::SizePolicy::Fill { weight: 1.0 },
                    height: slipway_core::SizePolicy::FitContent,
                }),
                resize_policy: None,
                overflow_policy: Some(slipway_core::OverflowPolicyDeclaration {
                    target: self.id.clone(),
                    x: slipway_core::OverflowBehavior::Clip,
                    y: slipway_core::OverflowBehavior::Scroll,
                }),
                auto_layout: None,
                responsive_variant: Some(slipway_core::ResponsiveVariant {
                    target: self.id.clone(),
                    key: if input.viewport.size.width < 400.0 {
                        "compact".to_string()
                    } else {
                        "wide".to_string()
                    },
                    active_breakpoints: Vec::new(),
                    reason: None,
                }),
                text_flow: None,
                text_measurement_cache: Vec::new(),
                text_measurement: None,
                fit_overflow: Vec::new(),
                layer: None,
                scroll: None,
                collection: None,
                interaction_styles: Vec::new(),
            }
        }
    }

    impl SlipwayWidgetTypes for FocusedProbeWidget {
        type ExternalState = ();
        type LocalState = usize;
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for FocusedProbeWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::KeyboardInput,
                Capability::TextInput,
                Capability::FocusInput,
                Capability::FocusRegionPresentation,
                Capability::TextEditRegionPresentation,
                Capability::Paint,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id.clone())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for FocusedProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            if event.target() != &self.id {
                return EventOutcome::ignored();
            }
            *local += 1;
            EventOutcome::message(self.id.clone(), "focused", ProbeMessage::Routed)
        }
    }

    impl SlipwayView for FocusedProbeWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            7
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some("focused-probe-fill".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds().into_rect(),
                    path: None,
                    clip: None,
                },
                color: slipway_core::Color {
                    red: 0.1,
                    green: 0.2,
                    blue: 0.8,
                    alpha: 1.0,
                },
            }]
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![StateObservation {
                target: self.id.clone(),
                slot: None,
                name: "local".to_string(),
                value: local.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for FocusedProbeWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id.clone(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id.clone()),
                hit_regions: Vec::new(),
                focus_regions: vec![test_text_edit_region(
                    self.id.clone(),
                    layout.bounds().into_rect(),
                )],
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayFontResolutionPolicy for FocusedProbeWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ParentWithChildWidget {
        id: WidgetId,
        child: ProbeWidget,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ScrollableParentWidget {
        id: WidgetId,
        child: ProbeWidget,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ParentWithChildLocal {
        child: usize,
    }

    impl ParentWithChildWidget {
        fn new() -> Self {
            Self {
                id: WidgetId::from("parent"),
                child: ProbeWidget::new("child"),
            }
        }

        fn child_slot(&self) -> WidgetSlotAddress {
            WidgetSlotAddress::new(self.id.clone(), 0).child(self.child.id(), 0)
        }

        fn child_bounds(&self) -> Rect {
            Rect {
                origin: Point { x: 12.0, y: 10.0 },
                size: Size {
                    width: 48.0,
                    height: 28.0,
                },
            }
        }
    }

    impl ScrollableParentWidget {
        fn new() -> Self {
            Self {
                id: WidgetId::from("egui.scroll.host"),
                child: ProbeWidget::new("egui.scroll.child"),
            }
        }

        fn child_slot(&self) -> WidgetSlotAddress {
            WidgetSlotAddress::new(self.id.clone(), 0).child(self.child.id(), 0)
        }

        fn child_bounds(&self) -> Rect {
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 180.0,
                },
            }
        }

        fn scroll_region(&self) -> ScrollRegionDeclaration {
            let viewport = TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 40.0,
                },
            });
            let layout = test_layout_with_placements(
                viewport,
                [ChildPlacement {
                    child: self.child.id(),
                    bounds: test_parent_local(self.child_bounds()),
                    local_state_slot: Some(self.child_slot()),
                    spacing: BoxSpacing::ZERO,
                }],
            );
            scroll_region_from_scrollable_capability(
                &ScrollProbeWidget {
                    id: self.id.clone(),
                },
                &(),
                &0,
                &layout,
                Some(PresentationRegionId::from("scroll-region")),
                Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                true,
            )
        }
    }

    impl SlipwayWidgetTypes for ParentWithChildWidget {
        type ExternalState = ();
        type LocalState = ParentWithChildLocal;
        type AppMessage = ProbeMessage;
    }

    impl SlipwayWidgetTypes for ScrollableParentWidget {
        type ExternalState = ();
        type LocalState = ParentWithChildLocal;
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for ParentWithChildWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::PointerInput, Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode {
                id: self.id.clone(),
                children: vec![TopologyNode::leaf(self.child.id())],
                local_state_slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }

        fn visit_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            visitor.visit_child(&self.child, external, &local.child, self.child_slot());
        }
    }

    impl SlipwaySsot for ScrollableParentWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::WheelInput,
                Capability::ScrollRegionPresentation,
                Capability::Paint,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode {
                id: self.id.clone(),
                children: vec![TopologyNode::leaf(self.child.id())],
                local_state_slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }

        fn visit_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            visitor.visit_child(&self.child, external, &local.child, self.child_slot());
        }
    }

    impl SlipwayLogic for ParentWithChildWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayLogic for ScrollableParentWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Scroll(scroll)
                    if scroll.target == self.id
                        && scroll.region_id == PresentationRegionId::from("scroll-region") =>
                {
                    local.child += 1;
                    EventOutcome::message(self.id.clone(), "scrolled", ProbeMessage::Routed)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for ParentWithChildWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            ParentWithChildLocal {
                child: self.child.initial_local_state(),
            }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            test_layout_with_placements_from(
                output,
                input.viewport,
                [ChildPlacement {
                    child: self.child.id(),
                    bounds: test_parent_local(self.child_bounds()),
                    local_state_slot: Some(self.child_slot()),
                    spacing: BoxSpacing::ZERO,
                }],
            )
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            Vec::new()
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            Vec::new()
        }
    }

    impl SlipwayView for ScrollableParentWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            ParentWithChildLocal {
                child: self.child.initial_local_state(),
            }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            test_layout_with_placements_from(
                output,
                input.viewport,
                [ChildPlacement {
                    child: self.child.id(),
                    bounds: test_parent_local(self.child_bounds()),
                    local_state_slot: Some(self.child_slot()),
                    spacing: BoxSpacing::ZERO,
                }],
            )
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            Vec::new()
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![StateObservation {
                target: self.id.clone(),
                slot: None,
                name: "scrolls".to_string(),
                value: local.child.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for ParentWithChildWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout: layout.clone(),
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: vec![test_hit_region(
                    "parent-root",
                    self.id(),
                    *layout.bounds(),
                    0,
                )],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayViewDefinition for ScrollableParentWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout: layout.clone(),
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: vec![self.scroll_region()],
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayFontResolutionPolicy for ParentWithChildWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayFontResolutionPolicy for ScrollableParentWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct UnsupportedClipWidget;

    impl SlipwayWidgetTypes for UnsupportedClipWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for UnsupportedClipWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("egui.unsupported")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for UnsupportedClipWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for UnsupportedClipWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Group {
                id: Some("path-clip-group".to_string()),
                clip: Some(slipway_core::ClipDeclaration {
                    id: Some("path-clip".to_string()),
                    bounds: layout.bounds().into_rect(),
                    path: Some(PathDeclaration {
                        commands: vec![
                            PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                            PathCommand::LineTo(Point { x: 20.0, y: 0.0 }),
                            PathCommand::LineTo(Point { x: 20.0, y: 20.0 }),
                            PathCommand::Close,
                        ],
                    }),
                }),
                ops: Vec::new(),
            }]
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            Vec::new()
        }
    }

    impl SlipwayViewDefinition for UnsupportedClipWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            let paint = self.paint(external, local, &layout);

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint,
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayFontResolutionPolicy for UnsupportedClipWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    #[test]
    fn egui_text_edit_scope_visuals_are_declared_not_backend_defaults() {
        let style = TextInputVisualStyleDeclaration::explicit(
            WidgetId::from("text"),
            test_rgb(15, 23, 42),
            test_rgb(100, 116, 139),
            test_rgb(15, 23, 42),
            test_rgb(191, 219, 254),
            test_rgb(248, 250, 252),
            test_rgb(203, 213, 225),
            1.0,
            4.0,
            test_rgb(15, 23, 42),
        );

        egui::__run_test_ui(|ui| {
            apply_text_input_visuals_to_egui_scope(ui, &style);

            assert_eq!(
                ui.style().visuals.override_text_color,
                Some(egui_color(style.value_color))
            );
            assert_eq!(
                ui.style().visuals.weak_text_color,
                Some(egui_color(style.placeholder_color))
            );
            assert_eq!(
                ui.style().visuals.text_edit_bg_color,
                Some(egui_color(style.background_color))
            );
            assert_eq!(
                ui.style().visuals.widgets.inactive.bg_fill,
                egui_color(style.background_color)
            );
            assert_eq!(
                ui.style().visuals.widgets.inactive.bg_stroke.color,
                egui_color(style.border_color)
            );
            assert_eq!(
                ui.style().visuals.selection.bg_fill,
                egui_color(style.selection_color)
            );
        });
    }

    #[test]
    fn app_preserves_multiple_authored_widget_slots() {
        let app = SlipwayEguiApp::new(
            (),
            vec![ProbeWidget::new("one"), ProbeWidget::new("two")],
            DefaultEguiBridge::new(),
            |_, _| {},
        );

        assert_eq!(app.widget_count(), 2);
        assert_eq!(app.slots[0].widget.id(), WidgetId::from("one"));
        assert_eq!(app.slots[1].widget.id(), WidgetId::from("two"));
        assert_eq!(app.slots[0].local_state, 7);
        assert_eq!(app.slots[1].local_state, 7);
    }

    #[test]
    fn default_bridge_translates_available_size_to_layout_input() {
        let mut bridge = DefaultEguiBridge::new();

        egui::__run_test_ui(|ui| {
            let input = <DefaultEguiBridge as EguiSlipwayBridge<ProbeWidget>>::layout_input(
                &mut bridge,
                EguiLayoutContext {
                    ui,
                    available_size: egui::vec2(80.0, 40.0),
                    pixels_per_point: 1.0,
                },
            );

            assert_eq!(input.viewport.size.width, 80.0);
            assert_eq!(input.viewport.size.height, 40.0);
            assert_eq!(input.constraints.max.width, 80.0);
            assert_eq!(input.constraints.max.height, 40.0);
        });
    }

    #[test]
    fn default_bridge_extracts_messages_without_widget_semantics() {
        let mut bridge = DefaultEguiBridge::new();
        let outcome = EventOutcome::message(WidgetId::from("one"), "routed", ProbeMessage::Routed);

        let messages =
            <DefaultEguiBridge as EguiSlipwayBridge<ProbeWidget>>::messages(&mut bridge, outcome);

        assert_eq!(messages, vec![ProbeMessage::Routed]);
        assert!(bridge.take_probe_products().is_empty());
    }

    #[test]
    fn declared_hit_regions_allocate_distinct_egui_responses() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let layout = test_leaf_layout(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 120.0,
                height: 80.0,
            },
        }));
        let view = ViewDefinition {
            target: widget.id(),
            frame: FrameIdentity {
                surface_id: "test".to_string(),
                surface_instance_id: "test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: layout.bounds().into_rect(),
            },
            layout: layout.clone(),
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id()),
            hit_regions: vec![
                test_hit_region(
                    "left",
                    WidgetId::from("left"),
                    Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 40.0,
                            height: 40.0,
                        },
                    },
                    0,
                ),
                test_hit_region(
                    "right",
                    WidgetId::from("right"),
                    Rect {
                        origin: Point { x: 50.0, y: 0.0 },
                        size: Size {
                            width: 40.0,
                            height: 40.0,
                        },
                    },
                    1,
                ),
            ],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        egui::__run_test_ui(|ui| {
            let (surface_rect, root_response) =
                ui.allocate_exact_size(egui::vec2(120.0, 80.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );

            assert!(!root_response.sense.interactive());
            assert_eq!(regions.len(), 2);
            assert!(child_assembly.regions.is_empty());
            assert_ne!(regions[0].response.id, regions[1].response.id);
            assert_ne!(
                regions[0].response.interact_rect,
                regions[1].response.interact_rect
            );
            assert!(
                regions
                    .iter()
                    .all(|region| region.response.sense.senses_click())
            );
        });
    }

    #[test]
    fn declared_hit_region_allocation_preserves_route_event_target() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let layout = test_leaf_layout(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 120.0,
                height: 80.0,
            },
        }));
        let route_target = WidgetId::from("route-target");
        let route_slot = WidgetSlotAddress::new(route_target.clone(), 9);
        let mut hit = test_hit_region(
            "routed-hit",
            WidgetId::from("visual-target"),
            layout.bounds().into_rect(),
            0,
        );
        hit.route.path = vec![WidgetId::from("root"), route_target.clone()];
        hit.route.address = Some(route_slot.clone());

        let view = ViewDefinition {
            target: widget.id(),
            frame: FrameIdentity {
                surface_id: "test".to_string(),
                surface_instance_id: "test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: layout.bounds().into_rect(),
            },
            layout: layout.clone(),
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id()),
            hit_regions: vec![hit],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(120.0, 80.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );

            assert_eq!(regions.len(), 1);
            assert_eq!(regions[0].target, WidgetId::from("visual-target"));
            assert_eq!(regions[0].event_target, route_target);
            assert_eq!(regions[0].event_target_slot, Some(route_slot.clone()));

            let position = surface_rect.min + egui::vec2(4.0, 4.0);
            let target_local_position = Point { x: 4.0, y: 4.0 };
            let (dispatch, evidence) =
                slipway_core::resolve_declared_pointer_dispatch_with_evidence(
                    EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input"),
                    view.frame.clone(),
                    &view.layout,
                    &view.hit_regions,
                    target_local_position,
                    PointerEventKind::Press,
                    Some(slipway_core::PointerButton::Primary),
                    slipway_core::PointerDetails::default(),
                    true,
                );
            let dispatch = dispatch.expect("core resolver selects egui hit region");
            let InputEvent::Pointer(pointer) = dispatch.input else {
                panic!("expected pointer dispatch");
            };

            assert_eq!(evidence.source.backend_id.as_deref(), Some(EGUI_BACKEND_ID));
            assert_eq!(
                evidence.selected_region,
                Some(slipway_core::PresentationRegionId::from("routed-hit"))
            );
            assert_eq!(pointer.target, regions[0].event_target);
            assert_eq!(pointer.target_slot, regions[0].event_target_slot);
            assert_eq!(
                pointer.position,
                egui_region_position(&regions[0], position)
            );
            assert_eq!(
                pointer.target_bounds.map(TargetLocalRect::into_rect),
                Some(egui_region_target_bounds(&regions[0]))
            );
        });
    }

    #[test]
    fn empty_root_space_is_not_a_region_target() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let layout = test_leaf_layout(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 100.0,
                height: 80.0,
            },
        }));
        let view = ViewDefinition {
            target: widget.id(),
            frame: FrameIdentity {
                surface_id: "test".to_string(),
                surface_instance_id: "test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: layout.bounds().into_rect(),
            },
            layout: layout.clone(),
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id()),
            hit_regions: vec![test_hit_region(
                "small",
                WidgetId::from("small"),
                Rect {
                    origin: Point { x: 10.0, y: 10.0 },
                    size: Size {
                        width: 20.0,
                        height: 20.0,
                    },
                },
                0,
            )],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(100.0, 80.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );
            let empty_space = egui::pos2(surface_rect.min.x + 70.0, surface_rect.min.y + 70.0);

            assert!(egui_region_at_position(&regions, empty_space).is_none());
        });
    }

    #[test]
    fn text_edit_allocates_input_region_and_self_painted_scroll_metadata() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let layout = test_leaf_layout(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 200.0,
                height: 120.0,
            },
        }));
        let view = ViewDefinition {
            target: widget.id(),
            frame: FrameIdentity {
                surface_id: "test".to_string(),
                surface_instance_id: "test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: layout.bounds().into_rect(),
            },
            layout: layout.clone(),
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id()),
            hit_regions: Vec::new(),
            focus_regions: vec![test_text_edit_region(
                WidgetId::from("text"),
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 24.0,
                    },
                },
            )],
            scroll_regions: vec![test_scroll_region(
                WidgetId::from("scroll"),
                Rect {
                    origin: Point { x: 0.0, y: 32.0 },
                    size: Size {
                        width: 160.0,
                        height: 50.0,
                    },
                },
            )],
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(200.0, 120.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );

            let text = regions
                .iter()
                .find(|region| region.kind == EguiPresentedRegionKind::TextEdit)
                .expect("text edit region allocated");
            assert_eq!(text.cursor, CursorCapability::Text);
            assert!(text.response.sense.is_focusable());

            let scroll = regions
                .iter()
                .find(|region| region.kind == EguiPresentedRegionKind::Scroll)
                .expect("scroll region allocated");
            assert!(scroll.scroll_state.is_some());
            assert!(scroll.response.sense.senses_drag());
            assert!(child_assembly.refused_admissions.is_empty());
        });
    }

    #[test]
    fn focused_input_without_presented_region_returns_refusal_dispatch_evidence() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let layout = test_leaf_layout(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 120.0,
                height: 48.0,
            },
        }));
        let view = ViewDefinition {
            target: widget.id(),
            frame: FrameIdentity {
                surface_id: "test".to_string(),
                surface_instance_id: "test".to_string(),
                revision: 0,
                frame_index: 7,
                viewport: layout.bounds().into_rect(),
            },
            layout: layout.clone(),
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id()),
            hit_regions: Vec::new(),
            focus_regions: vec![test_text_edit_region(
                WidgetId::from("candidate"),
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 24.0,
                    },
                },
            )],
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        egui::__run_test_ui(|ui| {
            let (surface_rect, root_response) =
                ui.allocate_exact_size(egui::vec2(120.0, 48.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );
            let focused_target = WidgetId::from("missing-focused-target");
            let event = InputEvent::Text(TextInputEvent {
                target: focused_target.clone(),
                target_slot: None,
                text: "x".to_string(),
            });
            let context = EguiInputContext {
                ui,
                widget_id: widget.id(),
                frame: &view.frame,
                rect: surface_rect,
                layout: &view.layout,
                geometry_index: &geometry_index,
                hit_regions: &view.hit_regions,
                focus_regions: &view.focus_regions,
                scroll_regions: &view.scroll_regions,
                wheel_traversal_boundary: Default::default(),
                response: &root_response,
                regions: &regions,
                native_physical_operation: None,
            };

            let backend_input = egui_focused_backend_input_event(
                &context,
                Some(&focused_target),
                DeclaredEventDispatchKind::Text,
                event.clone(),
            );

            assert!(
                backend_input.is_none(),
                "stale focused backend input must not create a mutating declared event"
            );
        });
    }

    #[test]
    fn authored_child_response_wins_over_root_synthetic_hit_region() {
        let widget = ParentWithChildWidget::new();
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
            ),
        );

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(100.0, 80.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let mut regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );
            let skipped_slots = child_assembly.claimed_slots.clone();
            child_assembly.extend(present_authored_children(
                ui,
                &widget,
                &(),
                &local,
                &view,
                &geometry_index,
                surface_rect.min,
                &skipped_slots,
                None,
                None,
                None,
            ));
            regions.extend(child_assembly.regions);

            let child_point = egui::pos2(surface_rect.min.x + 20.0, surface_rect.min.y + 20.0);
            let target_region = egui_region_at_position(&regions, child_point)
                .expect("child point should target a region");

            assert_eq!(target_region.target, widget.child.id());
            assert_eq!(target_region.address, Some(widget.child_slot()));
            assert!(
                target_region
                    .region_id
                    .as_str()
                    .starts_with("egui-child-response:")
                    || target_region.region_id.as_str() == "probe-hit"
            );
            assert_eq!(
                target_region.response.interact_rect.min,
                egui::pos2(surface_rect.min.x + 12.0, surface_rect.min.y + 10.0)
            );
        });
    }

    #[test]
    fn visited_child_without_matching_child_placement_emits_refusal() {
        let widget = ParentWithChildWidget::new();
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
            ),
        );
        let bounds = *view.layout.bounds();
        let input = LayoutInput {
            viewport: bounds,
            content: bounds,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: bounds.into_rect().size,
            },
        };
        view.layout = slipway_core::prepare_leaf_layout(test_layout_builder(&input), bounds);

        egui::__run_test_ui(|ui| {
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let output = present_authored_children(
                ui,
                &widget,
                &(),
                &local,
                &view,
                &geometry_index,
                egui::pos2(0.0, 0.0),
                &[],
                None,
                None,
                None,
            );

            assert!(output.regions.is_empty());
            assert_eq!(output.refused_admissions.len(), 1);
            let admission = &output.refused_admissions[0];
            assert!(!admission.accepted);
            assert_eq!(admission.source.pass_id.as_deref(), Some("child-assembly"));
            assert!(admission.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "egui.child_placement.missing"
                    && diagnostic.severity == slipway_core::DiagnosticSeverity::Unsupported
            }));
            assert!(admission.unsupported.iter().any(|unsupported| {
                unsupported.capability == Capability::ChildTraversal
                    && unsupported.target == Some(widget.child.id())
            }));
        });
    }

    #[test]
    fn visited_native_child_without_matching_child_placement_emits_refusal() {
        let root = ProbeWidget::new("native-parent");
        let local = root.initial_local_state();
        let view = root.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
            ),
        );
        let native = SlipwayEguiNativeWidget::new(NativeEguiLabel);
        let slot = WidgetSlotAddress::new(root.id(), 0).child(native.id(), 0);

        egui::__run_test_ui(|ui| {
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let mut presenter = EguiAuthoredChildPresenter {
                ui,
                parent_view: &view,
                view_origin: egui::pos2(0.0, 0.0),
                skipped_slots: &[],
                parent_geometry_index: &geometry_index,
                scroll: None,
                native_physical_operation: None,
                timing_samples: None,
                output: EguiChildAssembly::default(),
            };

            presenter.visit_egui_native_child(&native, &(), &(), slot.clone());
            let output = presenter.output;

            assert!(output.regions.is_empty());
            assert_eq!(output.refused_admissions.len(), 1);
            let admission = &output.refused_admissions[0];
            assert!(!admission.accepted);
            assert_eq!(admission.source.pass_id.as_deref(), Some("child-assembly"));
            assert!(admission.diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "egui.child_placement.missing"
                    && diagnostic.severity == slipway_core::DiagnosticSeverity::Unsupported
            }));
            assert!(admission.unsupported.iter().any(|unsupported| {
                unsupported.capability == Capability::ChildTraversal
                    && unsupported.target == Some(native.id())
            }));
        });
    }

    #[test]
    fn scrollbar_extent_matches_declared_content_bounds_with_presented_children() {
        let widget = ParentWithChildWidget::new();
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
            ),
        );
        view.scroll_regions = vec![test_scroll_region(
            widget.id(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 42.0,
                },
            },
        )];

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(100.0, 80.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );

            assert!(
                regions
                    .iter()
                    .any(|region| region.kind == EguiPresentedRegionKind::Scroll)
            );
            let scroll_region = regions
                .iter()
                .find(|region| region.kind == EguiPresentedRegionKind::Scroll)
                .expect("scroll region allocated");
            let scroll_state = scroll_region
                .scroll_state
                .as_ref()
                .expect("native scroll state recorded");
            assert_eq!(
                scroll_state.content_size,
                view.scroll_regions[0].content_bounds.size
            );
            assert!(
                child_assembly
                    .presented_slots
                    .iter()
                    .any(|slot| slot == &widget.child_slot())
            );
            assert!(child_assembly.refused_admissions.is_empty());
            assert!(
                child_assembly
                    .regions
                    .iter()
                    .any(|region| region.target == widget.child.id())
            );
            let forbidden_fake_content_call = ["set", "_min", "_size"].concat();
            assert!(!include_str!("lib.rs").contains(&forbidden_fake_content_call));
        });
    }

    #[test]
    fn declared_scrollarea_uses_content_bounds_without_presented_children() {
        let widget = ScrollProbeWidget::new("self-painted-scroll");
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 120.0,
                            height: 60.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 120.0,
                            height: 60.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 120.0,
                            height: 60.0,
                        },
                    },
                },
            ),
        );

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(120.0, 60.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );

            assert!(child_slots.is_empty());
            assert!(child_assembly.refused_admissions.is_empty());
            assert!(child_assembly.presented_slots.is_empty());
            let region = regions
                .iter()
                .find(|region| {
                    region.kind == EguiPresentedRegionKind::Scroll
                        && region.target == WidgetId::from("self-painted-scroll")
                })
                .expect("self-painted scroll region allocated");
            let scroll_state = region
                .scroll_state
                .as_ref()
                .expect("self-painted scroll still records native scroll state");
            assert_eq!(
                scroll_state.content_size,
                view.scroll_regions[0].content_bounds.size
            );
            assert!(
                scroll_state.content_size.height > view.scroll_regions[0].viewport.size.height,
                "declared extent should require a native scrollbar"
            );
        });
    }

    #[test]
    fn declared_scroll_indicator_paints_after_scroll_content() {
        let widget = ScrollableParentWidget::new();
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
            ),
        );
        let child_fill_color = egui_color(slipway_core::Color {
            red: 0.1,
            green: 0.2,
            blue: 0.3,
            alpha: 1.0,
        });
        let mut allocated_scroll_region = false;
        let ctx = egui::Context::default();

        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 80.0),
                )),
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(100.0, 80.0), egui::Sense::hover());
                let child_slots = authored_child_slots(&widget, &(), &local);
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut child_assembly = EguiChildAssembly::default();
                let regions = allocate_presentation_regions(
                    ui,
                    &widget,
                    &(),
                    &local,
                    surface_rect.min,
                    &view,
                    &geometry_index,
                    &child_slots,
                    &mut child_assembly,
                    None,
                );
                allocated_scroll_region = regions
                    .iter()
                    .any(|region| region.kind == EguiPresentedRegionKind::Scroll);
                let skipped_slots = child_assembly.claimed_slots.clone();
                child_assembly.extend(present_authored_children(
                    ui,
                    &widget,
                    &(),
                    &local,
                    &view,
                    &geometry_index,
                    surface_rect.min,
                    &skipped_slots,
                    None,
                    None,
                    None,
                ));
                paint_declared_scroll_indicators(ui, &mut child_assembly.scroll_indicators);
            },
        );

        assert!(allocated_scroll_region);
        let child_shape = rect_fill_shape_index(&output, child_fill_color)
            .expect("scroll content fill is visible in egui output");
        let track_shape = rect_fill_shape_index(&output, declared_scroll_indicator_track_color())
            .expect("declared scroll indicator track is visible in egui output");
        let thumb_shape = rect_fill_shape_index(&output, declared_scroll_indicator_thumb_color())
            .expect("declared scroll indicator thumb is visible in egui output");

        assert!(
            track_shape > child_shape,
            "declared scroll indicator track must paint after scroll content"
        );
        assert!(
            thumb_shape > child_shape,
            "declared scroll indicator thumb must paint after scroll content"
        );
        assert!(
            thumb_shape > track_shape,
            "declared scroll indicator thumb must paint above its track"
        );
    }

    // --- Step 210: declared scroll-indicator control (ScrollIndicatorMode) ---
    // Fixture: a CANVAS-PAINTED (routed, no child placements) declared
    // scroll region — ScrollProbeWidget's own view. Egui's presentation
    // path allocates every declared region through
    // `allocate_scroll_region_with_skips`, so the declared mode gates the
    // indicator queue there.
    fn declared_indicator_canvas_output(
        indicator: slipway_core::ScrollIndicatorMode,
    ) -> (bool, egui::FullOutput) {
        let widget = ScrollProbeWidget {
            id: WidgetId::from("declared-indicator-probe"),
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
            ),
        );
        for region in &mut view.scroll_regions {
            *region = region.clone().with_scroll_indicator(indicator);
        }
        assert!(
            view.layout.child_placements().is_empty(),
            "fixture must stay canvas-painted (no child placements)"
        );

        let mut allocated_scroll_region = false;
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 80.0),
                )),
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(100.0, 80.0), egui::Sense::hover());
                let child_slots = authored_child_slots(&widget, &(), &local);
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut child_assembly = EguiChildAssembly::default();
                let regions = allocate_presentation_regions(
                    ui,
                    &widget,
                    &(),
                    &local,
                    surface_rect.min,
                    &view,
                    &geometry_index,
                    &child_slots,
                    &mut child_assembly,
                    None,
                );
                allocated_scroll_region = regions
                    .iter()
                    .any(|region| region.kind == EguiPresentedRegionKind::Scroll);
                paint_declared_scroll_indicators(ui, &mut child_assembly.scroll_indicators);
            },
        );
        (allocated_scroll_region, output)
    }

    // Auto = byte-identical pre-control behavior: the canvas-painted
    // overflowing region gets an indicator on egui's presentation path
    // (the control case the Hidden test contrasts against).
    #[test]
    fn declared_indicator_auto_paints_for_canvas_painted_region() {
        let (allocated, output) =
            declared_indicator_canvas_output(slipway_core::ScrollIndicatorMode::Auto);
        assert!(allocated);
        assert!(
            rect_fill_shape_index(&output, declared_scroll_indicator_track_color()).is_some(),
            "Auto must keep the pre-control indicator for a canvas-painted region"
        );
        assert!(rect_fill_shape_index(&output, declared_scroll_indicator_thumb_color()).is_some());
    }

    // Revert-and-fail guard: if the Hidden honor in
    // `allocate_scroll_region_with_skips` is reverted, the indicator
    // reappears and this test fails. Scrolling/wheel routing must stay
    // intact (the scroll region is still allocated).
    #[test]
    fn declared_indicator_hidden_suppresses_indicator_but_keeps_scroll_region() {
        let (allocated, output) =
            declared_indicator_canvas_output(slipway_core::ScrollIndicatorMode::Hidden);
        assert!(
            allocated,
            "Hidden is a visual control only; the scroll region must still allocate"
        );
        assert!(
            rect_fill_shape_index(&output, declared_scroll_indicator_track_color()).is_none(),
            "Hidden must never paint an indicator track"
        );
        assert!(
            rect_fill_shape_index(&output, declared_scroll_indicator_thumb_color()).is_none(),
            "Hidden must never paint an indicator thumb"
        );
    }

    #[test]
    fn declared_indicator_visible_paints_for_canvas_painted_region() {
        let (allocated, output) =
            declared_indicator_canvas_output(slipway_core::ScrollIndicatorMode::Visible);
        assert!(allocated);
        assert!(
            rect_fill_shape_index(&output, declared_scroll_indicator_track_color()).is_some(),
            "Visible must paint the indicator for a geometrically sensible region"
        );
        assert!(rect_fill_shape_index(&output, declared_scroll_indicator_thumb_color()).is_some());
    }

    // Step 211 upgrade (composed-level, position-aware): the Step 210 egui
    // fixture pinned ONE region per view, while the composed reference
    // example declares OVERLAPPING regions with distinct per-region modes on
    // one child (outer Auto + inner-0 Visible + inner-1 Hidden — the nested
    // card). This drives the live allocation path with that exact shape and
    // asserts indicator PLACEMENT in the real egui output shapes: the
    // Visible inner paints its indicator inside ITS viewport, the Hidden
    // inner paints none inside its viewport, and the Auto outer keeps its
    // own at the band's right edge.
    #[test]
    fn declared_indicator_modes_gate_per_region_in_nested_card_view() {
        let widget = ScrollProbeWidget {
            id: WidgetId::from("authored.nested"),
        };
        let local = widget.initial_local_state();
        let card = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 560.0,
                height: 176.0,
            },
        };
        let outer_band = Rect {
            origin: Point { x: 0.0, y: 28.0 },
            size: Size {
                width: 560.0,
                height: 136.0,
            },
        };
        let inner_viewport = |panel: usize| Rect {
            origin: Point {
                x: 12.0,
                y: 46.0 + panel as f32 * 76.0,
            },
            size: Size {
                width: 536.0,
                height: 42.0,
            },
        };
        let region = |id: &str, viewport: Rect, content_height: f32, index: usize| {
            ScrollRegionDeclaration::explicit(
                PresentationRegionId::from(id),
                widget.id.clone(),
                Some(WidgetSlotAddress::new(widget.id.clone(), 0)),
                TargetLocalRect::new(viewport),
                TargetLocalRect::new(Rect {
                    origin: viewport.origin,
                    size: Size {
                        width: viewport.size.width,
                        height: content_height,
                    },
                }),
                Point { x: 0.0, y: 0.0 },
                slipway_core::ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                slipway_core::WheelRouting::NearestScrollable,
                HitRegionOrder {
                    z_index: if index == 0 { 0 } else { 1 },
                    paint_order: index,
                    traversal_order: index,
                },
                slipway_core::ScrollConsumptionPolicy::exclusive_wheel(),
                true,
            )
        };
        let view = ViewDefinition {
            target: widget.id.clone(),
            frame: FrameIdentity {
                surface_id: "test".to_string(),
                surface_instance_id: "test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: card,
            },
            layout: test_leaf_layout(TargetLocalRect::new(card)),
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id.clone()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: vec![
                region("authored.nested:outer", outer_band, 152.0, 0),
                region("authored.nested:inner-0", inner_viewport(0), 112.0, 1)
                    .with_scroll_indicator(slipway_core::ScrollIndicatorMode::Visible),
                region("authored.nested:inner-1", inner_viewport(1), 112.0, 2)
                    .with_scroll_indicator(slipway_core::ScrollIndicatorMode::Hidden),
            ],
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let mut surface_min = egui::Pos2::ZERO;
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 220.0),
                )),
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(560.0, 176.0), egui::Sense::hover());
                surface_min = surface_rect.min;
                let child_slots = authored_child_slots(&widget, &(), &local);
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut child_assembly = EguiChildAssembly::default();
                let _ = allocate_presentation_regions(
                    ui,
                    &widget,
                    &(),
                    &local,
                    surface_rect.min,
                    &view,
                    &geometry_index,
                    &child_slots,
                    &mut child_assembly,
                    None,
                );
                paint_declared_scroll_indicators(ui, &mut child_assembly.scroll_indicators);
            },
        );

        let to_screen = |rect: Rect| {
            egui::Rect::from_min_size(
                egui::pos2(surface_min.x + rect.origin.x, surface_min.y + rect.origin.y),
                egui::vec2(rect.size.width, rect.size.height),
            )
        };
        let thumb_rects: Vec<egui::Rect> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Rect(rect) if rect.fill == declared_scroll_indicator_thumb_color() => {
                    Some(rect.rect)
                }
                _ => None,
            })
            .collect();
        let inner_0 = to_screen(inner_viewport(0));
        let inner_1 = to_screen(inner_viewport(1));
        let outer = to_screen(outer_band);
        assert!(
            thumb_rects.iter().any(|rect| inner_0.contains_rect(*rect)),
            "the Visible inner region must paint its indicator thumb inside its own viewport: {thumb_rects:?} vs {inner_0:?}"
        );
        assert!(
            !thumb_rects.iter().any(|rect| rect.intersects(inner_1)),
            "the Hidden inner region must paint no indicator inside its viewport: {thumb_rects:?}"
        );
        assert!(
            thumb_rects.iter().any(|rect| outer.contains_rect(*rect)
                && !inner_0.contains_rect(*rect)
                && !inner_1.intersects(*rect)),
            "the Auto outer region must keep its own indicator at the band edge: {thumb_rects:?}"
        );
    }

    // Step 211 (composed-level pointer routing): egui resolves the pointer
    // press by the COMBINED presented-region order (hit AND scroll regions
    // compete by paint_sort_key), so a clickable row declared UNDER its own
    // scroll region's z is unreachable — the press resolves to the scroll
    // region and produces no pointer dispatch (the live
    // `native-physical-control-no-backend-trace` finding on the reference
    // example's nested rows). This pins the working declaration shape: a
    // row hit region that FRONTS the scroll region it sits on wins the
    // press, and documents the combined-order rule with the losing shape.
    #[test]
    fn pointer_press_over_scrolled_row_resolves_to_the_fronting_row_hit_region() {
        let widget = ScrollProbeWidget {
            id: WidgetId::from("authored.nested"),
        };
        let local = widget.initial_local_state();
        let card = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 560.0,
                height: 176.0,
            },
        };
        let inner_viewport = Rect {
            origin: Point { x: 12.0, y: 46.0 },
            size: Size {
                width: 536.0,
                height: 42.0,
            },
        };
        let row_rect = Rect {
            origin: Point { x: 20.0, y: 60.0 },
            size: Size {
                width: 520.0,
                height: 12.0,
            },
        };
        let address = Some(WidgetSlotAddress::new(widget.id.clone(), 0));
        let scroll_region = ScrollRegionDeclaration::explicit(
            PresentationRegionId::from("authored.nested:inner-0"),
            widget.id.clone(),
            address.clone(),
            TargetLocalRect::new(inner_viewport),
            TargetLocalRect::new(Rect {
                origin: inner_viewport.origin,
                size: Size {
                    width: inner_viewport.size.width,
                    height: 112.0,
                },
            }),
            Point { x: 0.0, y: 0.0 },
            slipway_core::ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            slipway_core::WheelRouting::NearestScrollable,
            HitRegionOrder {
                z_index: 1,
                paint_order: 1,
                traversal_order: 1,
            },
            slipway_core::ScrollConsumptionPolicy::exclusive_wheel(),
            true,
        );
        let row_region = |z_index: i32| {
            slipway_core::hit_region_from_pointer_capability(
                &widget,
                &(),
                &local,
                PresentationRegionId::from("authored.nested:panel0-row-1"),
                address.clone(),
                TargetLocalRect::new(row_rect),
                slipway_core::PointerEventCoordinateSpace::TargetLocal,
                HitRegionOrder {
                    z_index,
                    paint_order: 1,
                    traversal_order: 1,
                },
                Some("authored.nested:panel0-row-1".to_string()),
                CursorCapability::Pointer,
                true,
                PointerCaptureIntent::OnPress,
            )
        };
        let view_for = |row_z: i32| ViewDefinition {
            target: widget.id.clone(),
            frame: FrameIdentity {
                surface_id: "test".to_string(),
                surface_instance_id: "test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: card,
            },
            layout: test_leaf_layout(TargetLocalRect::new(card)),
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id.clone()),
            hit_regions: vec![row_region(row_z)],
            focus_regions: Vec::new(),
            scroll_regions: vec![scroll_region.clone()],
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let selected_region_id_for = |row_z: i32| {
            let view = view_for(row_z);
            let mut selected: Option<String> = None;
            let ctx = egui::Context::default();
            // Two frames with the REAL pointer hovering the row point: the
            // live selection runs with pointer response authority
            // (`hovered`/`contains_pointer`), which needs the widgets of a
            // previous frame — a hoverless single frame falls back to the
            // geometry branch and hides the live behavior.
            let mut surface_min = egui::Pos2::ZERO;
            for evaluate in [false, true] {
                let events = if evaluate {
                    vec![egui::Event::PointerMoved(egui::pos2(
                        surface_min.x + 60.0,
                        surface_min.y + 66.0,
                    ))]
                } else {
                    Vec::new()
                };
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(600.0, 220.0),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ui| {
                        let (surface_rect, _root_response) =
                            ui.allocate_exact_size(egui::vec2(560.0, 176.0), egui::Sense::hover());
                        surface_min = surface_rect.min;
                        let child_slots = authored_child_slots(&widget, &(), &local);
                        let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                        let mut child_assembly = EguiChildAssembly::default();
                        let regions = allocate_presentation_regions(
                            ui,
                            &widget,
                            &(),
                            &local,
                            surface_rect.min,
                            &view,
                            &geometry_index,
                            &child_slots,
                            &mut child_assembly,
                            None,
                        );
                        if evaluate {
                            // The same point, inside the row (and so inside
                            // the scroll region's viewport).
                            let point =
                                egui::pos2(surface_rect.min.x + 60.0, surface_rect.min.y + 66.0);
                            selected = egui_region_at_position(&regions, point)
                                .map(|region| region.region_id.as_str().to_string());
                        }
                    },
                );
            }
            selected.expect("a presented region resolves at the row point")
        };

        // The working declaration: the row FRONTS its scroll region (z 2 >
        // z 1) and wins the pointer press.
        let fronting = selected_region_id_for(2);
        assert!(
            fronting.contains("panel0-row-1"),
            "a row hit region fronting its scroll region must win the pointer press; got {fronting}"
        );
        // The combined-order rule, documented: a row UNDER the scroll
        // region's z loses the press to the scroll region (which emits no
        // pointer dispatch — the unreachable-row failure shape).
        let under = selected_region_id_for(0);
        assert!(
            under.contains("inner-0"),
            "a row under its scroll region's z loses the press to the scroll region; got {under}"
        );
    }

    #[test]
    fn egui_explicit_layer_paints_above_later_normal_sibling_in_output_shapes() {
        let layered_color = test_rgb(220, 38, 38);
        let normal_color = test_rgb(37, 99, 235);
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.earlier-top", 10, Some(0)).with_color(layered_color),
                LayeredPaintChild::new("egui.later-normal", 0, None)
                    .with_source_order()
                    .with_color(normal_color),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );
        let earlier_view = widget.children.0.visible_backend_view_definition(
            &(),
            &local.0,
            ViewDefinitionInput::new(
                view.frame.clone(),
                child_layout_input(view.layout.child_placements()[0].bounds.into_rect()),
            ),
        );
        let later_view = widget.children.1.visible_backend_view_definition(
            &(),
            &local.1,
            ViewDefinitionInput::new(
                view.frame.clone(),
                child_layout_input(view.layout.child_placements()[1].bounds.into_rect()),
            ),
        );
        assert_eq!(
            earlier_view.paint_order.mode,
            PaintOrderMode::ExplicitLayered,
            "earlier child must be an explicit Slipway layer"
        );
        assert_eq!(
            later_view.paint_order.mode,
            PaintOrderMode::SourceOrder,
            "later sibling must be a true source-order normal child"
        );

        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 100.0),
                )),
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut assembly = collect_authored_children(
                    ui,
                    &widget,
                    &(),
                    &local,
                    &view,
                    &geometry_index,
                    surface_rect.min,
                    &[],
                    None,
                    None,
                    None,
                );

                paint_egui_jobs(ui, &mut assembly.paint_jobs);
            },
        );

        let normal_shape = rect_fill_shape_index(&output, egui_color(normal_color))
            .expect("normal child fill shape is in egui output");
        let layered_shape = rect_fill_shape_index(&output, egui_color(layered_color))
            .expect("layered child fill shape is in egui output");
        assert!(
            layered_shape > normal_shape,
            "explicit Slipway layer must paint after a later normal sibling"
        );
    }

    #[test]
    fn egui_higher_positive_explicit_layer_paints_last_in_output_shapes() {
        let low_color = test_rgb(251, 146, 60);
        let high_color = test_rgb(20, 184, 166);
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.low-explicit", 2, Some(0)).with_color(low_color),
                LayeredPaintChild::new("egui.high-explicit", 12, Some(0)).with_color(high_color),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );

        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 100.0),
                )),
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut assembly = collect_authored_children(
                    ui,
                    &widget,
                    &(),
                    &local,
                    &view,
                    &geometry_index,
                    surface_rect.min,
                    &[],
                    None,
                    None,
                    None,
                );

                paint_egui_jobs(ui, &mut assembly.paint_jobs);
            },
        );

        let low_shape = rect_fill_shape_index(&output, egui_color(low_color))
            .expect("low explicit fill shape is in egui output");
        let high_shape = rect_fill_shape_index(&output, egui_color(high_color))
            .expect("high explicit fill shape is in egui output");
        assert!(
            high_shape > low_shape,
            "higher explicit Slipway layer must paint after lower explicit layer"
        );
    }

    #[test]
    fn child_default_paint_stays_below_its_extracted_keyed_layers() {
        let child_default_color = test_rgb(226, 232, 240);
        let child_layer_color = test_rgb(124, 58, 237);
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.child-with-inner-layer", 10, Some(10))
                    .with_color(child_default_color)
                    .with_inner_layer(10, Some(3), child_layer_color),
                LayeredPaintChild::new("egui.child-without-layer", 0, None).with_source_order(),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );

        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 100.0),
                )),
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut assembly = collect_authored_children(
                    ui,
                    &widget,
                    &(),
                    &local,
                    &view,
                    &geometry_index,
                    surface_rect.min,
                    &[],
                    None,
                    None,
                    None,
                );

                paint_egui_jobs(ui, &mut assembly.paint_jobs);
            },
        );

        let default_shape = rect_fill_shape_index(&output, egui_color(child_default_color))
            .expect("child default fill is visible");
        let layer_shape = rect_fill_shape_index(&output, egui_color(child_layer_color))
            .expect("child inner keyed layer fill is visible");

        assert!(
            layer_shape > default_shape,
            "a child's default paint must not cover its extracted keyed PaintOp::Layer"
        );
    }

    #[test]
    fn child_response_fallback_does_not_steal_keyed_layer_hit_region() {
        let hit_order = HitRegionOrder {
            z_index: 10,
            paint_order: 3,
            traversal_order: 0,
        };
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.background-child", 0, None).with_source_order(),
                LayeredPaintChild::new("egui.keyed-hit-child", 0, None)
                    .with_source_order()
                    .with_inner_layer(10, Some(3), test_rgb(124, 58, 237))
                    .with_hit_region(
                        Rect {
                            origin: Point { x: 0.0, y: 0.0 },
                            size: Size {
                                width: 50.0,
                                height: 28.0,
                            },
                        },
                        hit_order,
                    ),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );

        let ctx = egui::Context::default();
        let mut selected_region = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 100.0),
                )),
                events: vec![egui::Event::PointerMoved(egui::pos2(24.0, 24.0))],
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut assembly = collect_authored_children(
                    ui,
                    &widget,
                    &(),
                    &local,
                    &view,
                    &geometry_index,
                    surface_rect.min,
                    &[],
                    None,
                    None,
                    None,
                );
                paint_egui_jobs(ui, &mut assembly.paint_jobs);
                selected_region =
                    egui_region_at_position(&assembly.regions, egui::pos2(24.0, 24.0))
                        .map(|region| region.region_id.clone());
            },
        );

        assert_eq!(
            selected_region,
            Some(PresentationRegionId::from("egui.keyed-hit-child:hit")),
            "synthetic child fallback response must not steal a declared keyed-layer hit region"
        );
    }

    #[test]
    fn child_response_fallback_does_not_steal_same_owner_local_hit_region() {
        let hit_order = HitRegionOrder {
            z_index: 0,
            paint_order: 0,
            traversal_order: 0,
        };
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.background-child", 0, None).with_source_order(),
                LayeredPaintChild::new("egui.local-hit-child", 0, None)
                    .with_source_order()
                    .with_hit_region(
                        Rect {
                            origin: Point { x: 0.0, y: 0.0 },
                            size: Size {
                                width: 50.0,
                                height: 28.0,
                            },
                        },
                        hit_order,
                    ),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );

        let ctx = egui::Context::default();
        let mut selected_region = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 100.0),
                )),
                events: vec![egui::Event::PointerMoved(egui::pos2(24.0, 24.0))],
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut assembly = collect_authored_children(
                    ui,
                    &widget,
                    &(),
                    &local,
                    &view,
                    &geometry_index,
                    surface_rect.min,
                    &[],
                    None,
                    None,
                    None,
                );
                paint_egui_jobs(ui, &mut assembly.paint_jobs);
                selected_region =
                    egui_region_at_position(&assembly.regions, egui::pos2(24.0, 24.0))
                        .map(|region| region.region_id.clone());
            },
        );

        assert_eq!(
            selected_region,
            Some(PresentationRegionId::from("egui.local-hit-child:hit")),
            "synthetic child fallback response must not steal a same-owner declared local hit region"
        );
    }

    #[test]
    fn root_keyed_paint_layer_participates_in_egui_widget_global_order() {
        let child_color = test_rgb(37, 99, 235);
        let root_layer_color = test_rgb(190, 24, 93);
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.root-layer-child", 12, Some(0))
                    .with_color(child_color),
                LayeredPaintChild::new("egui.root-layer-normal", 0, None).with_source_order(),
            ),
            root_fill: None,
            root_layer: Some((20, Some(0), root_layer_color)),
        };
        let external = ();
        let mut local = widget.initial_local_state();
        let mut bridge = DefaultEguiBridge::new();
        let mut messages = Vec::new();
        let layout_input = zero_spacing_layout_input! {
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            }),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: Size {
                    width: 100.0,
                    height: 100.0,
                },
            },
        };
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 100.0),
                )),
                ..Default::default()
            },
            |ui| {
                ui.add(
                    SlipwayEguiWidget::new(
                        &widget,
                        &external,
                        &mut local,
                        &mut bridge,
                        &mut messages,
                    )
                    .layout_input_override(layout_input.clone()),
                );
            },
        );

        let child_shape =
            rect_fill_shape_index(&output, egui_color(child_color)).expect("child fill is visible");
        let root_layer_shape = rect_fill_shape_index(&output, egui_color(root_layer_color))
            .expect("root keyed layer fill is visible");

        assert!(
            root_layer_shape > child_shape,
            "root PaintOp::Layer key must participate in the same global output order as child paint"
        );
    }

    #[test]
    fn root_default_paint_stays_below_children_while_keyed_layer_goes_global() {
        let root_fill_color = test_rgb(226, 232, 240);
        let child_color = test_rgb(37, 99, 235);
        let root_layer_color = test_rgb(190, 24, 93);
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.root-default-child", 0, None)
                    .with_source_order()
                    .with_color(child_color),
                LayeredPaintChild::new("egui.root-default-other", 0, None).with_source_order(),
            ),
            root_fill: Some(root_fill_color),
            root_layer: Some((20, Some(0), root_layer_color)),
        };
        let external = ();
        let mut local = widget.initial_local_state();
        let mut bridge = DefaultEguiBridge::new();
        let mut messages = Vec::new();
        let layout_input = zero_spacing_layout_input! {
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            }),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: Size {
                    width: 100.0,
                    height: 100.0,
                },
            },
        };
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 100.0),
                )),
                ..Default::default()
            },
            |ui| {
                ui.add(
                    SlipwayEguiWidget::new(
                        &widget,
                        &external,
                        &mut local,
                        &mut bridge,
                        &mut messages,
                    )
                    .layout_input_override(layout_input.clone()),
                );
            },
        );

        let root_fill_shape = rect_fill_shape_index(&output, egui_color(root_fill_color))
            .expect("root source-order fill is visible");
        let child_shape =
            rect_fill_shape_index(&output, egui_color(child_color)).expect("child fill is visible");
        let root_layer_shape = rect_fill_shape_index(&output, egui_color(root_layer_color))
            .expect("root keyed layer fill is visible");

        assert!(
            root_fill_shape < child_shape,
            "root default/source-order paint must not be deferred until after authored children"
        );
        assert!(
            root_layer_shape > child_shape,
            "root keyed PaintOp::Layer still participates in the global overlay order"
        );
    }

    #[test]
    fn root_keyed_paint_layer_creates_default_opaque_occlusion_region() {
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.occlusion-child", 0, None).with_source_order(),
                LayeredPaintChild::new("egui.occlusion-other", 0, None).with_source_order(),
            ),
            root_fill: None,
            root_layer: Some((20, Some(0), test_rgb(190, 24, 93))),
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );

        egui::__run_test_ui(|ui| {
            let mut jobs = Vec::new();
            push_expanded_egui_paint_jobs(
                &mut jobs,
                PaintUnit::from_view_ref(&view, 0),
                egui::Pos2::ZERO,
                egui_test_rect(0.0, 0.0, 100.0, 100.0),
            );
            let regions = allocate_paint_occlusion_regions(ui, &jobs);
            let occlusion = regions
                .iter()
                .find(|region| region.kind == EguiPresentedRegionKind::Occlusion)
                .expect("opaque root keyed paint layer creates an occlusion region");

            assert_eq!(occlusion.paint_sort_key, (20, 0, 0));
            assert!(
                occlusion
                    .response
                    .interact_rect
                    .contains(egui::pos2(20.0, 20.0)),
                "occlusion region must cover the visible root keyed layer bounds"
            );
        });
    }

    #[test]
    fn earlier_scroll_explicit_overlay_flushes_after_later_lower_phase_and_routes_hit() {
        let high_color = test_rgb(14, 165, 233);
        let low_color = test_rgb(244, 114, 182);
        let high_bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 50.0,
                height: 50.0,
            },
        };
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.scroll-high-explicit", 12, Some(0))
                    .with_color(high_color)
                    .with_overflow_bounds(high_bounds)
                    .with_hit_region(
                        high_bounds,
                        HitRegionOrder {
                            z_index: 12,
                            paint_order: 0,
                            traversal_order: 0,
                        },
                    ),
                LayeredPaintChild::new("egui.later-low-explicit", 2, Some(0)).with_color(low_color),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );
        let mut scroll = test_scroll_region(
            widget.id(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 5.0,
                    height: 5.0,
                },
            },
        );
        scroll.content_bounds = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 5.0,
                height: 5.0,
            },
        });
        view.scroll_regions = vec![scroll];

        let ctx = egui::Context::default();
        let mut selected_target = None;
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(100.0, 100.0),
                )),
                ..Default::default()
            },
            |ui| {
                let (surface_rect, _root_response) =
                    ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
                let child_slots = authored_child_slots(&widget, &(), &local);
                let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                let mut child_assembly = EguiChildAssembly::default();
                let mut regions = allocate_presentation_regions(
                    ui,
                    &widget,
                    &(),
                    &local,
                    surface_rect.min,
                    &view,
                    &geometry_index,
                    &child_slots,
                    &mut child_assembly,
                    None,
                );
                let skipped_slots = child_assembly.claimed_slots.clone();
                child_assembly.extend(present_authored_children(
                    ui,
                    &widget,
                    &(),
                    &local,
                    &view,
                    &geometry_index,
                    surface_rect.min,
                    &skipped_slots,
                    None,
                    None,
                    None,
                ));
                regions.extend(child_assembly.regions);
                paint_egui_jobs(ui, &mut child_assembly.paint_jobs);

                let hit_point = egui::pos2(surface_rect.min.x + 24.0, surface_rect.min.y + 24.0);
                selected_target = egui_region_at_position(&regions, hit_point)
                    .map(|region| region.target.clone());
            },
        );
        assert_eq!(
            selected_target,
            Some(WidgetId::from("egui.scroll-high-explicit")),
            "hit routing must choose the high explicit overlay from the earlier scroll phase"
        );

        let low_shape = rect_fill_shape_index(&output, egui_color(low_color))
            .expect("later lower explicit fill shape is in egui output");
        let high_shape = rect_fill_shape_index(&output, egui_color(high_color))
            .expect("earlier scroll high explicit fill shape is in egui output");
        assert!(
            high_shape > low_shape,
            "surface-global explicit flush must paint the earlier scroll high rank after the later lower rank"
        );
    }

    #[test]
    fn egui_declared_overflow_clip_is_not_parent_ui_clip() {
        let overflow = Rect {
            origin: Point { x: -20.0, y: -12.0 },
            size: Size {
                width: 90.0,
                height: 82.0,
            },
        };
        let overflow_color = test_rgb(234, 88, 12);
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.overflow-child", 10, Some(0))
                    .with_paint_bounds(overflow)
                    .with_color(overflow_color)
                    .with_overflow_bounds(overflow),
                LayeredPaintChild::new("egui.normal-child", 0, None),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );

        let ctx = egui::Context::default();
        let parent_clip = egui::Rect::from_min_size(egui::pos2(30.0, 30.0), egui::vec2(50.0, 50.0));
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(120.0, 120.0),
                )),
                ..Default::default()
            },
            |ui| {
                ui.scope_builder(egui::UiBuilder::new().max_rect(parent_clip), |ui| {
                    let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
                    let mut assembly = collect_authored_children(
                        ui,
                        &widget,
                        &(),
                        &local,
                        &view,
                        &geometry_index,
                        parent_clip.min,
                        &[],
                        None,
                        None,
                        None,
                    );
                    paint_egui_jobs(ui, &mut assembly.paint_jobs);
                });
            },
        );
        let expected_clip = egui_rect(parent_clip.min, overflow);
        let output_clip = rect_fill_shape_clip(&output, egui_color(overflow_color))
            .expect("overflow fill shape is in egui output");

        assert_ne!(output_clip, parent_clip);
        assert_eq!(output_clip, expected_clip);
    }

    #[test]
    fn overlapping_hit_regions_route_to_visual_top_explicit_layer() {
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.hit-top", 10, Some(0)),
                LayeredPaintChild::new("egui.hit-normal", 0, None).with_source_order(),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );
        let top_view = widget.children.0.visible_backend_view_definition(
            &(),
            &local.0,
            ViewDefinitionInput::new(
                view.frame.clone(),
                child_layout_input(view.layout.child_placements()[0].bounds.into_rect()),
            ),
        );
        let normal_view = widget.children.1.visible_backend_view_definition(
            &(),
            &local.1,
            ViewDefinitionInput::new(
                view.frame.clone(),
                child_layout_input(view.layout.child_placements()[1].bounds.into_rect()),
            ),
        );
        assert_eq!(
            top_view.paint_order.mode,
            PaintOrderMode::ExplicitLayered,
            "visual top child must be an explicit Slipway layer"
        );
        assert_eq!(
            normal_view.paint_order.mode,
            PaintOrderMode::SourceOrder,
            "overlapped later sibling must be true source-order normal paint"
        );

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let mut assembly = collect_authored_children(
                ui,
                &widget,
                &(),
                &local,
                &view,
                &geometry_index,
                surface_rect.min,
                &[],
                None,
                None,
                None,
            );
            paint_egui_jobs(ui, &mut assembly.paint_jobs);
            let hit_point = egui::pos2(surface_rect.min.x + 24.0, surface_rect.min.y + 24.0);
            let selected = egui_region_at_position(&assembly.regions, hit_point)
                .expect("overlapping child response should route to a region");

            assert_eq!(
                selected.target,
                WidgetId::from("egui.hit-top"),
                "hit routing must select the same explicit layer that paints topmost"
            );
        });
    }

    #[test]
    fn overlapping_hit_regions_route_to_highest_positive_explicit_layer() {
        let widget = LayeredPaintApp {
            children: (
                LayeredPaintChild::new("egui.hit-low-explicit", 2, Some(0)),
                LayeredPaintChild::new("egui.hit-high-explicit", 12, Some(0)),
            ),
            root_fill: None,
            root_layer: None,
        };
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 100.0,
                        },
                    },
                },
            ),
        );

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(100.0, 100.0), egui::Sense::hover());
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let mut assembly = collect_authored_children(
                ui,
                &widget,
                &(),
                &local,
                &view,
                &geometry_index,
                surface_rect.min,
                &[],
                None,
                None,
                None,
            );
            paint_egui_jobs(ui, &mut assembly.paint_jobs);
            let hit_point = egui::pos2(surface_rect.min.x + 24.0, surface_rect.min.y + 24.0);
            let selected = egui_region_at_position(&assembly.regions, hit_point)
                .expect("overlapping explicit child responses should route to a region");

            assert_eq!(
                selected.target,
                WidgetId::from("egui.hit-high-explicit"),
                "hit routing must select the same highest explicit rank that paints topmost"
            );
        });
    }

    #[test]
    fn scroll_background_response_does_not_cover_hosted_child_response() {
        let widget = ParentWithChildWidget::new();
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &(),
            &local,
            ViewDefinitionInput::new(
                FrameIdentity {
                    surface_id: "test".to_string(),
                    surface_instance_id: "test".to_string(),
                    revision: 0,
                    frame_index: 0,
                    viewport: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
                zero_spacing_layout_input! {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 100.0,
                            height: 80.0,
                        },
                    },
                },
            ),
        );
        view.scroll_regions = vec![test_scroll_region(
            widget.id(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 42.0,
                },
            },
        )];

        let build_regions = |ui: &mut egui::Ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(100.0, 80.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let mut regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );
            let skipped_slots = child_assembly.claimed_slots.clone();
            child_assembly.extend(present_authored_children(
                ui,
                &widget,
                &(),
                &local,
                &view,
                &geometry_index,
                surface_rect.min,
                &skipped_slots,
                None,
                None,
                None,
            ));
            regions.extend(child_assembly.regions);

            let child_point = egui::pos2(surface_rect.min.x + 20.0, surface_rect.min.y + 20.0);
            (regions, child_point)
        };

        let ctx = egui::Context::default();
        ctx.set_fonts(egui::FontDefinitions::empty());

        let mut child_point = None;
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let (_regions, point) = build_regions(ui);
            child_point = Some(point);
        });
        let child_point = child_point.expect("first pass should record child point");

        let mut selected = None;
        let _ = ctx.run_ui(
            egui::RawInput {
                events: vec![egui::Event::PointerMoved(child_point)],
                ..Default::default()
            },
            |ui| {
                let (regions, _point) = build_regions(ui);
                let selected_region = egui_region_at_position(&regions, child_point)
                    .expect("pointer over child should select a region");
                let child_has_pointer_authority = regions.iter().any(|region| {
                    region.target == widget.child.id()
                        && (region.response.hovered() || region.response.contains_pointer())
                });
                selected = Some((
                    selected_region.target.clone(),
                    selected_region.region_id.clone(),
                    child_has_pointer_authority,
                ));
            },
        );

        let (target, region_id, child_has_pointer_authority) =
            selected.expect("second pass should select region");
        assert_eq!(
            target,
            widget.child.id(),
            "scroll background response must not cover hosted child response; selected {region_id:?}"
        );
        assert!(child_has_pointer_authority);
    }

    #[test]
    fn scroll_background_response_is_registered_before_declared_scroll_content() {
        let source = include_str!("lib.rs");
        let scroll_fn = source
            .find("fn allocate_scroll_region_with_skips")
            .expect("scroll allocation function is present");
        let scroll_end = source[scroll_fn..]
            .find("\nfn clip_declared_scroll_child_assembly")
            .expect("next scroll helper is present")
            + scroll_fn;
        let scroll_body = &source[scroll_fn..scroll_end];
        let response_interact = scroll_body
            .find("let response = ui.interact(")
            .expect("scroll response interact is present");
        let content_origin = scroll_body
            .find("declared_scroll_content_origin(")
            .expect("declared scroll content origin is present");
        let child_presentation = scroll_body
            .find("present_authored_children_mounted(")
            .expect("declared scroll content child presentation is present");

        assert!(
            response_interact < content_origin && content_origin < child_presentation,
            "egui scroll response must be registered before declared content origin and child presentation"
        );
        assert!(
            !scroll_body.contains(".show_viewport(ui,"),
            "declared scroll presenter must not delegate authority to egui ScrollArea::show_viewport"
        );
    }

    #[test]
    fn egui_backend_widget_trait_captures_visible_runtime_requirements() {
        fn assert_backend_widget<W: SlipwayEguiBackendWidget>() {}
        fn assert_layout_intent_widget<W: SlipwayEguiLayoutIntentBackendWidget>() {}
        fn assert_child_widget<W: SlipwayEguiBackendChildWidget>() {}

        assert_backend_widget::<ProbeWidget>();
        assert_layout_intent_widget::<ProbeWidget>();
        assert_child_widget::<ProbeWidget>();
    }

    #[test]
    fn egui_backend_admission_accepts_supported_path_and_regions() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput::new(
            FrameIdentity {
                surface_id: "egui-admission".to_string(),
                surface_instance_id: "test-instance".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                },
            },
            zero_spacing_layout_input! {
                viewport: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                }),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                },
            },
        );
        let mut view = widget.view_definition(&(), &local, input);
        view.paint = vec![PaintOp::Stroke {
            shape: ShapeDeclaration {
                id: Some("supported-path".to_string()),
                kind: ShapeKind::Path,
                bounds: view.layout.bounds().into_rect(),
                path: Some(PathDeclaration {
                    commands: vec![
                        PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                        PathCommand::LineTo(Point { x: 20.0, y: 20.0 }),
                    ],
                }),
                clip: None,
            },
            color: slipway_core::Color {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 1.0,
            },
            width: 1.0,
        }];
        view.focus_regions = vec![test_text_edit_region(
            WidgetId::from("text"),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 24.0,
                },
            },
        )];
        view.scroll_regions = vec![test_scroll_region(
            WidgetId::from("scroll"),
            Rect {
                origin: Point { x: 0.0, y: 30.0 },
                size: Size {
                    width: 80.0,
                    height: 40.0,
                },
            },
        )];

        let admission = egui_backend_admission().admit_view_definition(&view);

        assert!(admission.accepted);
        assert!(admission.unsupported.is_empty());
        assert_eq!(admission.source.label(), "backend_presented");
        assert_eq!(
            admission.source.backend_id.as_deref(),
            Some(EGUI_BACKEND_ID)
        );
        assert!(admission.visible_requirements.iter().any(|requirement| {
            requirement.capability == BackendVisibleCapability::TextEditRegions
        }));
        assert!(
            admission.visible_requirements.iter().any(
                |requirement| requirement.capability == BackendVisibleCapability::ScrollRegions
            )
        );
        assert!(admission.visible_requirements.iter().any(|requirement| {
            requirement.capability == BackendVisibleCapability::ShapePathClip
        }));
    }

    #[test]
    fn egui_path_points_flatten_cubic_and_reject_non_finite_points() {
        let path = PathDeclaration {
            commands: vec![
                PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                PathCommand::CubicTo {
                    control_1: Point { x: 12.0, y: 40.0 },
                    control_2: Point { x: 36.0, y: -20.0 },
                    to: Point { x: 48.0, y: 12.0 },
                },
            ],
        };

        let (points, closed) = egui_path_points(egui::pos2(10.0, 20.0), Some(&path))
            .expect("finite cubic path should be presentable");

        assert!(!closed);
        assert_eq!(points.len(), EGUI_PATH_CURVE_SEGMENTS + 1);
        assert_eq!(points[0], egui::pos2(10.0, 20.0));
        assert_eq!(points[points.len() - 1], egui::pos2(58.0, 32.0));
        assert!(
            points
                .iter()
                .any(|point| point.y > 32.0 && point.x > 10.0 && point.x < 58.0),
            "cubic path should include sampled curve points, not only control/end points"
        );

        let invalid = PathDeclaration {
            commands: vec![
                PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                PathCommand::LineTo(Point {
                    x: f32::NAN,
                    y: 1.0,
                }),
            ],
        };

        assert!(egui_path_points(egui::pos2(0.0, 0.0), Some(&invalid)).is_none());
    }

    #[test]
    fn egui_backend_admission_refuses_blocking_view_contract_errors() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput::new(
            FrameIdentity {
                surface_id: "egui-admission".to_string(),
                surface_instance_id: "contract-test".to_string(),
                revision: 1,
                frame_index: 2,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                },
            },
            zero_spacing_layout_input! {
                viewport: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                }),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                },
            },
        );
        let mut view = widget.view_definition(&(), &local, input);
        view.hit_regions[0].route.path.clear();

        let admission = egui_backend_admission().admit_view_definition(&view);

        assert!(!admission.accepted);
        let unsupported = admission
            .unsupported
            .iter()
            .find(|entry| entry.requirement_id.as_deref() == Some("view.contract"))
            .expect("blocking contract diagnostics must refuse visible launch");
        assert!(
            unsupported
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.hit_route_empty")
        );
        assert!(
            admission
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.hit_route_empty")
        );
    }

    #[test]
    fn visible_admission_refusal_lines_include_blocking_diagnostics() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput::new(
            FrameIdentity {
                surface_id: "egui-admission-lines".to_string(),
                surface_instance_id: "contract-test".to_string(),
                revision: 1,
                frame_index: 2,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                },
            },
            zero_spacing_layout_input! {
                viewport: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                }),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                },
            },
        );
        let mut view = widget.view_definition(&(), &local, input);
        view.hit_regions[0].route.path.clear();
        let admission = egui_backend_admission().admit_view_definition(&view);

        let lines = visible_admission_refusal_lines(&admission, 10);
        let joined = lines.join("\n");

        assert!(joined.contains("Slipway visible admission refused"));
        assert!(joined.contains("view.contract"));
        assert!(joined.contains("view_contract.hit_route_empty"));
    }

    #[test]
    fn egui_visible_scroll_normalization_crops_bad_viewport_before_admission() {
        let target = WidgetId::from("root");
        let layout_bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 100.0,
                height: 100.0,
            },
        };
        let layout = test_leaf_layout(TargetLocalRect::new(layout_bounds));
        let mut cropped_scroll = test_scroll_region(
            target.clone(),
            Rect {
                origin: Point { x: -4.0, y: 92.0 },
                size: Size {
                    width: 120.0,
                    height: 16.0,
                },
            },
        );
        cropped_scroll.address = None;
        cropped_scroll.content_bounds = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 10.0,
                height: 10.0,
            },
        });
        cropped_scroll.offset = Point { x: 7.0, y: 999.0 };
        let mut disabled_scroll = test_scroll_region(
            target.clone(),
            Rect {
                origin: Point { x: 0.0, y: 140.0 },
                size: Size {
                    width: 40.0,
                    height: 20.0,
                },
            },
        );
        disabled_scroll.address = None;
        disabled_scroll.offset = Point { x: 0.0, y: 8.0 };

        let mut view = ViewDefinition {
            target: target.clone(),
            frame: FrameIdentity {
                surface_id: "egui-scroll-normalization".to_string(),
                surface_instance_id: "contract-test".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: layout_bounds,
            },
            layout,
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(target),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: vec![cropped_scroll, disabled_scroll],
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let original = egui_backend_admission().admit_view_definition(&view);
        assert!(
            !original.accepted,
            "bad scroll geometry must fail before visible backend normalization"
        );

        let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
        normalize_egui_visible_scroll_regions(&mut view, &geometry_index);

        let normalized = egui_backend_admission().admit_view_definition(&view);
        assert!(
            normalized.accepted,
            "visible backend normalization must keep the surface presentable: {:?}",
            normalized.unsupported
        );
        assert_eq!(
            view.scroll_regions[0].viewport.into_rect(),
            Rect {
                origin: Point { x: 0.0, y: 92.0 },
                size: Size {
                    width: 100.0,
                    height: 8.0,
                },
            }
        );
        assert_eq!(view.scroll_regions[0].offset, Point { x: 0.0, y: 92.0 });
        assert!(!view.scroll_regions[1].enabled);
        assert_eq!(view.scroll_regions[1].offset, Point { x: 0.0, y: 0.0 });
        assert!(view.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "egui.visible_scroll.viewport_cropped_to_layout"
        }));
        assert!(view.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "egui.visible_scroll.disabled_outside_layout"
        }));
        assert!(view.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "egui.visible_scroll.content_bounds_expanded_to_viewport"
        }));
        assert!(
            view.diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "egui.visible_scroll.offset_clamped")
        );
    }

    #[test]
    fn egui_backend_admission_refuses_text_input_without_text_edit_focus_region() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput::new(
            FrameIdentity {
                surface_id: "egui-admission".to_string(),
                surface_instance_id: "text-contract-test".to_string(),
                revision: 1,
                frame_index: 3,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                },
            },
            zero_spacing_layout_input! {
                viewport: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                }),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: Size {
                        width: 120.0,
                        height: 80.0,
                    },
                },
            },
        );
        let mut view = widget.view_definition(&(), &local, input);
        view.focus_regions.clear();

        let admission = egui_backend_admission()
            .admit_view_definition_with_capabilities(&[Capability::TextInput], &view);

        assert!(!admission.accepted);
        let unsupported = admission
            .unsupported
            .iter()
            .find(|entry| entry.requirement_id.as_deref() == Some("view.contract"))
            .expect("missing text edit focus region must refuse visible launch");
        assert!(unsupported.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "view_contract.text_input_missing_text_edit_focus_region"
        }));
    }

    #[test]
    fn egui_backend_admission_refuses_unsupported_path_clip() {
        let widget = ProbeWidget::new("root");
        let layout = test_leaf_layout(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 120.0,
                height: 80.0,
            },
        }));
        let view = ViewDefinition {
            target: widget.id(),
            frame: FrameIdentity {
                surface_id: "egui-admission".to_string(),
                surface_instance_id: "test-instance".to_string(),
                revision: 1,
                frame_index: 2,
                viewport: layout.bounds().into_rect(),
            },
            layout: layout.clone(),
            paint: vec![PaintOp::Group {
                id: Some("unsupported-clip".to_string()),
                clip: Some(slipway_core::ClipDeclaration {
                    id: Some("clip-path".to_string()),
                    bounds: layout.bounds().into_rect(),
                    path: Some(PathDeclaration {
                        commands: vec![
                            PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                            PathCommand::LineTo(Point { x: 20.0, y: 0.0 }),
                            PathCommand::LineTo(Point { x: 20.0, y: 20.0 }),
                            PathCommand::Close,
                        ],
                    }),
                }),
                ops: Vec::new(),
            }],
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id()),
            hit_regions: vec![test_hit_region("hit", widget.id(), *layout.bounds(), 0)],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let admission = egui_backend_admission().admit_view_definition(&view);

        assert!(!admission.accepted);
        let unsupported = admission
            .unsupported
            .iter()
            .find(|entry| entry.visible_capability == Some(BackendVisibleCapability::ShapePathClip))
            .expect("path clip must be refused before visible launch");
        assert_eq!(unsupported.source.label(), "backend_presented");
        assert_eq!(
            unsupported.source.backend_id.as_deref(),
            Some(EGUI_BACKEND_ID)
        );
        assert!(
            unsupported
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code
                    == "egui.visible_paint.unsupported_group_clip_path")
        );
    }

    #[test]
    fn egui_visible_widget_refuses_unsupported_clip_before_paint_path() {
        let widget = UnsupportedClipWidget;
        let mut local = widget.initial_local_state();
        let mut bridge = DefaultEguiBridge::new();
        let mut messages = Vec::new();

        egui::__run_test_ui(|ui| {
            ui.add(SlipwayEguiWidget::new(
                &widget,
                &(),
                &mut local,
                &mut bridge,
                &mut messages,
            ));
        });

        assert!(messages.is_empty());
        let refusals = bridge.take_refused_admissions();
        assert_eq!(refusals.len(), 1);
        let refusal = &refusals[0];
        assert!(!refusal.accepted);
        assert_eq!(refusal.source.label(), "backend_presented");
        assert_eq!(refusal.source.backend_id.as_deref(), Some(EGUI_BACKEND_ID));
        assert!(refusal.unsupported.iter().any(|entry| {
            entry.visible_capability == Some(BackendVisibleCapability::ShapePathClip)
        }));
    }

    #[test]
    fn egui_default_bridge_deduplicates_repeated_admission_observations() {
        let mut bridge = DefaultEguiBridge::new();
        let admission = BackendParityAdmission {
            backend_id: EGUI_BACKEND_ID.to_string(),
            accepted: false,
            required_profiles: Vec::new(),
            visible_requirements: Vec::new(),
            unsupported: Vec::new(),
            source: EvidenceSource::backend_presented(EGUI_BACKEND_ID, "test-admission"),
            diagnostics: vec![Diagnostic::warning(
                None,
                "egui.test.repeated_admission",
                "same admission should not accumulate every visible frame",
            )],
        };

        <DefaultEguiBridge as EguiSlipwayBridge<ProbeWidget>>::visible_admission_refused(
            &mut bridge,
            admission.clone(),
        );
        <DefaultEguiBridge as EguiSlipwayBridge<ProbeWidget>>::visible_admission_refused(
            &mut bridge,
            admission,
        );

        let refusals = bridge.take_refused_admissions();
        assert_eq!(refusals.len(), 1);
    }

    #[test]
    fn egui_physical_input_position_includes_visible_viewport_origin() {
        let frame = FrameIdentity {
            surface_id: "egui-scroll".to_string(),
            surface_instance_id: "root".to_string(),
            revision: 1,
            frame_index: 2,
            viewport: Rect {
                origin: Point { x: 12.0, y: 240.0 },
                size: Size {
                    width: 640.0,
                    height: 480.0,
                },
            },
        };

        let position =
            egui_view_root_local_position(egui::pos2(360.0, 398.0), egui::pos2(20.0, 30.0), &frame);

        assert_eq!(position, Point { x: 352.0, y: 608.0 });
    }

    #[test]
    fn egui_pointer_dispatch_uses_presented_region_geometry_after_root_scroll() {
        egui::__run_test_ui(|ui| {
            let frame = FrameIdentity {
                surface_id: "egui-scroll".to_string(),
                surface_instance_id: "root".to_string(),
                revision: 1,
                frame_index: 2,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 640.0,
                        height: 480.0,
                    },
                },
            };
            let overlay = WidgetId::from("overlay");
            let overlay_slot = WidgetSlotAddress::new(overlay.clone(), 0);
            let layout = test_layout_with_placements(
                TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 640.0,
                        height: 900.0,
                    },
                }),
                [ChildPlacement {
                    child: overlay.clone(),
                    bounds: test_parent_local(Rect {
                        origin: Point { x: 220.0, y: 520.0 },
                        size: Size {
                            width: 200.0,
                            height: 120.0,
                        },
                    }),
                    local_state_slot: Some(overlay_slot.clone()),
                    spacing: BoxSpacing::ZERO,
                }],
            );
            let geometry_index = PresentationGeometryIndex::from_layout(&layout);
            let mut hit = test_hit_region(
                "overlay-hit",
                overlay.clone(),
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 200.0,
                        height: 32.0,
                    },
                },
                0,
            );
            hit.address = Some(overlay_slot);
            hit.capture = PointerCaptureIntent::DuringDrag;
            let hit_regions = vec![hit];
            let visual_titlebar =
                egui::Rect::from_min_size(egui::pos2(220.0, 280.0), egui::vec2(200.0, 32.0));
            let response = ui.interact(
                visual_titlebar,
                egui::Id::new("overlay-hit"),
                egui::Sense::click_and_drag(),
            );
            let region = EguiPresentedRegion {
                kind: EguiPresentedRegionKind::Hit,
                region_id: PresentationRegionId::from("overlay-hit"),
                target: overlay.clone(),
                address: hit_regions[0].address.clone(),
                paint_sort_key: (12, 12, 12),
                authored_z_order: None,
                event_target: overlay.clone(),
                event_target_slot: hit_regions[0].address.clone(),
                declared_bounds: hit_regions[0].bounds.into_rect(),
                target_origin: egui::pos2(220.0, 280.0),
                target_bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 200.0,
                        height: 120.0,
                    },
                },
                event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
                response,
                cursor: CursorCapability::Grab,
                enabled: true,
                text_edit_change: None,
                scroll_state: None,
                blocks_wheel: true,
            };
            let regions = vec![region];
            let context = EguiInputContext {
                ui,
                widget_id: WidgetId::from("root"),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &hit_regions,
                focus_regions: &[],
                scroll_regions: &[],
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };

            let event = egui_backend_pointer_input_event(
                &context,
                &regions[0],
                egui::pos2(250.0, 292.0),
                PointerEventKind::Press,
                Some(PointerButton::Primary),
                PointerDetails::default(),
                true,
            )
            .expect("scrolled visible overlay titlebar resolves to declared overlay hit");

            let InputEvent::Pointer(pointer) = &event.event else {
                panic!("expected pointer event");
            };
            assert_eq!(pointer.target, overlay);
            assert_eq!(pointer.position, Point { x: 30.0, y: 12.0 });
            assert_eq!(
                event
                    .dispatch_evidence
                    .as_ref()
                    .and_then(|evidence| evidence.selected_region.as_ref()),
                Some(&PresentationRegionId::from("overlay-hit"))
            );
        });
    }

    #[test]
    fn egui_text_format_maps_declared_style() {
        let style = TextStyle {
            font_family: "monospace".to_string(),
            font_size: 24.0,
            font_weight: FontWeight::Bold,
            font_style: FontStyle::Italic,
            decoration: TextDecoration {
                underline: true,
                strikethrough: true,
            },
            baseline: BaselineShift::Superscript,
            // Unspecified alignment: the historical top-left anchoring.
            ..TextStyle::plain()
        };
        let format = egui_text_format(egui::Color32::WHITE, &style);

        assert_eq!(format.font_id.family, egui::FontFamily::Monospace);
        assert_eq!(format.font_id.size, 18.0);
        assert_eq!(format.coords.as_ref().len(), 1);
        assert_eq!(format.coords.as_ref()[0].1, 700.0);
        assert!(format.italics);
        assert_eq!(format.underline.color, egui::Color32::WHITE);
        assert_eq!(format.underline.width, 1.0);
        assert_eq!(format.strikethrough.color, egui::Color32::WHITE);
        assert_eq!(format.strikethrough.width, 1.0);
        assert_eq!(format.valign, egui::Align::TOP);

        let custom = TextStyle {
            font_family: "Inter".to_string(),
            ..TextStyle::plain()
        };
        assert_eq!(egui_font_family(&custom), egui::FontFamily::Proportional);
    }

    #[test]
    fn egui_default_text_style_stays_plain() {
        let style = TextStyle::plain();
        let format = egui_text_format(egui::Color32::BLACK, &style);
        let job = egui_text_layout_job("plain", egui::Color32::BLACK, &style, 42.0);

        assert_eq!(format.font_id.family, egui::FontFamily::Proportional);
        assert_eq!(format.font_id.size, slipway_core::DEFAULT_TEXT_FONT_SIZE);
        assert_eq!(format.coords.as_ref()[0].1, 400.0);
        assert!(!format.italics);
        assert_eq!(format.underline, egui::Stroke::NONE);
        assert_eq!(format.strikethrough, egui::Stroke::NONE);
        assert_eq!(format.valign, egui::Align::BOTTOM);
        assert_eq!(job.text, "plain");
        assert_eq!(job.wrap.max_width, 42.0);
        // Default equivalence (NC-14): unspecified alignment keeps the
        // LayoutJob default halign and the historical top-left position.
        assert_eq!(job.halign, egui::Align::LEFT);
        let rect = egui::Rect::from_min_size(egui::pos2(15.0, 18.0), egui::vec2(80.0, 24.0));
        assert_eq!(
            egui_text_anchor(rect, &style, egui::vec2(40.0, 12.0)),
            egui_text_position(rect, &style)
        );
    }

    // NC-4 honor path on egui: the declared per-op wrap mode must reach
    // the layout job — `TextWrap::None` turns the wrap width off
    // (`max_width = INFINITY`, egui's documented opt-out) while the
    // unspecified default keeps the historical wrap-at-rect-width
    // byte-identically. Reverting `egui_text_layout_job` to the
    // unconditional rect-width wrap fails the no-wrap assertion.
    #[test]
    fn egui_declared_wrap_optout_disables_wrap_width() {
        let default_job =
            egui_text_layout_job("plain", egui::Color32::BLACK, &TextStyle::plain(), 42.0);
        assert_eq!(default_job.wrap.max_width, 42.0);

        let no_wrap_job = egui_text_layout_job(
            "single line",
            egui::Color32::BLACK,
            &TextStyle::plain().no_wrap(),
            42.0,
        );
        assert_eq!(no_wrap_job.wrap.max_width, f32::INFINITY);
    }

    // NC-4 measurement path: the backend's paint-text metric provider
    // measures through the REAL egui galley pipeline (the same layout
    // job `paint_text` submits), so measured sizes track content and
    // honor the declared wrap mode. Reverting the provider to any
    // estimate (or the wrap mapping to unconditional word wrap) fails
    // the relational assertions.
    #[test]
    fn egui_text_metric_provider_measures_real_galley() {
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            let inner_ctx = ui.ctx().clone();
            let mut provider = EguiTextMetricProvider::new(&inner_ctx);
            let request = |content: &str, style: TextStyle, available: Option<Rect>| {
                slipway_core::TextMeasurementRequest {
                    target: WidgetId::from("measure-widget"),
                    request_id: "measure".to_string(),
                    content: content.to_string(),
                    style,
                    available_bounds: available,
                    flow: None,
                    purposes: vec![slipway_core::TextMeasurementPurpose::IntrinsicSize],
                }
            };
            let facts = |receipt: slipway_core::TextMeasurementReceipt| match receipt {
                slipway_core::TextMeasurementReceipt::Valid(valid) => {
                    assert_eq!(
                        valid.source.kind,
                        slipway_core::TextMetricSourceKind::OfficialBackendApi
                    );
                    assert_eq!(valid.source.backend_id.as_deref(), Some(EGUI_BACKEND_ID));
                    valid.facts
                }
                other => panic!("expected a valid receipt from the real galley, got {other:?}"),
            };
            use slipway_core::SlipwayTextMetricProvider as _;

            // Intrinsic (unbounded) size: real galley, single row, wider
            // content measures wider.
            let short = facts(provider.measure_text(request("short", TextStyle::plain(), None)));
            assert!(short.measured_size.width > 0.0);
            assert!(short.measured_size.height > 0.0);
            assert_eq!(short.line_count, Some(1));
            let longer = facts(provider.measure_text(request(
                "a much longer measured label",
                TextStyle::plain(),
                None,
            )));
            assert!(longer.measured_size.width > short.measured_size.width);

            // Bounded word wrap: a narrow available rect wraps the galley
            // into multiple rows and grows the measured height.
            let narrow = Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 60.0,
                    height: 200.0,
                },
            };
            let wrapped = facts(provider.measure_text(request(
                "wrap wrap wrap wrap wrap",
                TextStyle::plain(),
                Some(narrow),
            )));
            assert!(wrapped.line_count.expect("line count") > 1);
            assert!(wrapped.measured_size.height > short.measured_size.height);

            // The declared opt-out reaches measurement too: the same
            // content in the same narrow rect stays on one row.
            let unwrapped = facts(provider.measure_text(request(
                "wrap wrap wrap wrap wrap",
                TextStyle::plain().no_wrap(),
                Some(narrow),
            )));
            assert_eq!(unwrapped.line_count, Some(1));
            assert!(unwrapped.measured_size.width > narrow.size.width);
        });
    }

    // NC-14 honor path on egui: a declared alignment must reach the real
    // egui output shape — the galley is laid out with the mapped halign
    // and positioned at the alignment anchor within the declared rect.
    // Reverting `paint_text` to the historical hardcoded top-left fails
    // both the halign and the position assertions.
    #[test]
    fn egui_declared_center_alignment_anchors_galley_in_bounds() {
        let bounds = Rect {
            origin: Point { x: 10.0, y: 12.0 },
            size: Size {
                width: 80.0,
                height: 24.0,
            },
        };
        let style = TextStyle::plain().centered();
        let ctx = egui::Context::default();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(200.0, 100.0),
                )),
                ..Default::default()
            },
            |ui| {
                paint_text(
                    ui.painter(),
                    egui::pos2(5.0, 6.0),
                    bounds,
                    "mid",
                    test_rgb(20, 30, 40),
                    &style,
                );
            },
        );

        let (pos, galley) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((text.pos, Arc::clone(&text.galley))),
                _ => None,
            })
            .expect("painted text reaches the egui output shapes");
        assert_eq!(galley.job.halign, egui::Align::Center);
        // Horizontal anchor = the translated rect's center x; with halign
        // Center each galley row is centered around it.
        let rect = egui::Rect::from_min_size(egui::pos2(15.0, 18.0), egui::vec2(80.0, 24.0));
        assert_eq!(pos.x, rect.center().x);
        // Vertical anchor = the measured galley block centered in the rect.
        let expected_y = rect.top() + (rect.height() - galley.size().y) / 2.0;
        assert!(
            (pos.y - expected_y).abs() < 0.01,
            "galley y {} must center the block (expected {expected_y})",
            pos.y
        );
        assert!(galley.size().y > 0.0, "galley must have laid-out rows");

        // End/Bottom: anchor x = right edge; block bottom = rect bottom.
        let style = TextStyle::plain()
            .with_align_x(slipway_core::TextAlignX::End)
            .with_align_y(slipway_core::TextAlignY::Bottom);
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(200.0, 100.0),
                )),
                ..Default::default()
            },
            |ui| {
                paint_text(
                    ui.painter(),
                    egui::pos2(5.0, 6.0),
                    bounds,
                    "end",
                    test_rgb(20, 30, 40),
                    &style,
                );
            },
        );
        let (pos, galley) = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((text.pos, Arc::clone(&text.galley))),
                _ => None,
            })
            .expect("painted text reaches the egui output shapes");
        assert_eq!(galley.job.halign, egui::Align::RIGHT);
        assert_eq!(pos.x, rect.right());
        let expected_y = rect.bottom() - galley.size().y;
        assert!(
            (pos.y - expected_y).abs() < 0.01,
            "galley y {} must bottom-anchor the block (expected {expected_y})",
            pos.y
        );
    }

    #[test]
    fn egui_text_input_uses_declared_source_family_when_present() {
        let ctx = egui::Context::default();
        let mut focus = test_text_edit_region(
            WidgetId::from("text"),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 120.0,
                    height: 24.0,
                },
            },
        );
        let text_edit = focus
            .text_edit
            .as_mut()
            .expect("test focus has a text edit");
        text_edit.typography.style = TextStyle::plain().with_font_family("system-ui");
        text_edit.typography.source = Some(ResourceSourceDeclaration {
            source_id: "authored-cjk".to_string(),
            kind: ResourceSourceKind::Asset,
            family: Some("Authored CJK".to_string()),
            asset_ref: Some("unused.ttf".to_string()),
            revision: Vec::new(),
        });
        let source = text_edit.typography.source.as_ref().expect("source is set");
        store_font_install_result(
            &ctx,
            egui_font_install_key(source.family.as_deref(), source),
            &EguiFontInstallResult {
                status: EguiFontInstallStatus::Installed,
            },
        );

        assert_eq!(
            egui_text_input_font_family(&ctx, text_edit),
            egui::FontFamily::Name("Authored CJK".into())
        );
    }

    #[test]
    fn cjk_paint_without_installable_font_reports_backend_evidence() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let layout = test_leaf_layout(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 120.0,
                height: 40.0,
            },
        }));
        let view = ViewDefinition {
            target: widget.id(),
            frame: FrameIdentity {
                surface_id: "egui-font".to_string(),
                surface_instance_id: "font-test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: layout.bounds().into_rect(),
            },
            layout: layout.clone(),
            paint: vec![PaintOp::Text {
                bounds: layout.bounds().into_rect(),
                content: "\u{d55c}\u{ae00}".to_string(),
                color: slipway_core::Color {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 1.0,
                },
                style: TextStyle::plain(),
            }],
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let admission = egui_backend_admission().admit_view_definition(&view);
        assert!(admission.visible_requirements.iter().any(|requirement| {
            requirement.capability == BackendVisibleCapability::FontInstallation
        }));

        egui::__run_test_ui(|ui| {
            let refusals = install_declared_fonts(ui, &widget, &(), &local, &view);

            assert_eq!(refusals.len(), 1);
            assert!(!refusals[0].accepted);
            assert_eq!(refusals[0].source.label(), "backend_presented");
            assert_eq!(
                refusals[0].source.pass_id.as_deref(),
                Some("font-installation")
            );
            assert!(refusals[0].diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "egui.font.cjk_coverage_unproved"
                    && diagnostic.message.contains("missing_source")
            }));
        });
    }

    #[test]
    fn cjk_text_edit_stays_native_and_reports_font_evidence() {
        let widget = ProbeWidget::new("root");
        let local = widget.initial_local_state();
        let layout = test_leaf_layout(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 160.0,
                height: 48.0,
            },
        }));
        let mut focus = test_text_edit_region(
            WidgetId::from("text"),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 120.0,
                    height: 24.0,
                },
            },
        );
        focus
            .text_edit
            .as_mut()
            .expect("test focus has a text edit")
            .buffer
            .text = "\u{d55c}\u{ae00}".to_string();
        {
            let text_edit = focus
                .text_edit
                .as_mut()
                .expect("test focus has a text edit");
            text_edit.typography.style = TextStyle::plain().with_font_family("AuthoredInputCjk");
            text_edit.typography.source = Some(ResourceSourceDeclaration {
                source_id: "authored-input-cjk".to_string(),
                kind: ResourceSourceKind::Asset,
                family: Some("AuthoredInputCjk".to_string()),
                asset_ref: Some(
                    std::env::temp_dir()
                        .join("slipway-missing-authored-input-cjk-font.ttf")
                        .to_string_lossy()
                        .into_owned(),
                ),
                revision: Vec::new(),
            });
        }
        let view = ViewDefinition {
            target: widget.id(),
            frame: FrameIdentity {
                surface_id: "egui-font".to_string(),
                surface_instance_id: "text-edit-font-test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: layout.bounds().into_rect(),
            },
            layout: layout.clone(),
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(widget.id()),
            hit_regions: Vec::new(),
            focus_regions: vec![focus],
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        egui::__run_test_ui(|ui| {
            let (surface_rect, _root_response) =
                ui.allocate_exact_size(egui::vec2(160.0, 48.0), egui::Sense::hover());
            let child_slots = authored_child_slots(&widget, &(), &local);
            let mut child_assembly = EguiChildAssembly::default();
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let regions = allocate_presentation_regions(
                ui,
                &widget,
                &(),
                &local,
                surface_rect.min,
                &view,
                &geometry_index,
                &child_slots,
                &mut child_assembly,
                None,
            );

            assert!(
                regions
                    .iter()
                    .any(|region| region.kind == EguiPresentedRegionKind::TextEdit)
            );
            assert!(child_assembly.refused_admissions.iter().any(|admission| {
                admission.source.pass_id.as_deref() == Some("font-installation")
                    && admission.diagnostics.iter().any(|diagnostic| {
                        diagnostic.code == "egui.font.cjk_coverage_unproved"
                            && diagnostic.message.contains("AuthoredInputCjk")
                            && diagnostic.message.contains("read_failed")
                    })
            }));
        });
    }

    #[test]
    fn font_install_cache_prevents_repeated_asset_reads() {
        let ctx = egui::Context::default();
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "slipway-egui-font-cache-{}-{unique}.font",
            std::process::id()
        ));
        std::fs::write(&path, b"cached-font-bytes").expect("write temp font bytes");
        let source = ResourceSourceDeclaration {
            source_id: "cache-font".to_string(),
            kind: ResourceSourceKind::Asset,
            family: None,
            asset_ref: Some(path.to_string_lossy().into_owned()),
            revision: Vec::new(),
        };

        let first = install_font_from_evidence(&ctx, Some("cache-font"), Some(&source));
        std::fs::remove_file(&path).expect("remove temp font bytes after first install");
        let second = install_font_from_evidence(&ctx, Some("cache-font"), Some(&source));

        assert_eq!(first.status, EguiFontInstallStatus::Queued);
        assert_eq!(second.status, EguiFontInstallStatus::Queued);
    }

    #[test]
    fn runtime_app_debug_drain_uses_message_reducer_for_traced_control() {
        let applied = Rc::new(Cell::new(0usize));
        let applied_for_reducer = Rc::clone(&applied);
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            DefaultEguiBridge::new(),
            move |_, messages: Vec<ProbeMessage>| {
                applied_for_reducer.set(applied_for_reducer.get() + messages.len());
            },
        );
        let frame = FrameIdentity {
            surface_id: "egui-runtime-shell".to_string(),
            surface_instance_id: "test-instance".to_string(),
            revision: 1,
            frame_index: 7,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 40.0,
                },
            },
        };
        let handle = app
            .runtime()
            .bridge_client()
            .submit(DebugCommand::control_with_trace(
                "egui-trace",
                frame,
                InputEvent::Command(CommandEvent {
                    target: WidgetId::from("one"),
                    target_slot: None,
                    command: "routed".to_string(),
                    payload_ref: None,
                    source: None,
                }),
            ))
            .expect("submit traced control");

        let (drained, error) = app.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert_eq!(applied.get(), 1);

        let reply = handle
            .try_recv()
            .expect("reply channel ok")
            .expect("reply produced");
        let DebugReplyProduct::ControlTrace(trace) = reply.product else {
            panic!("expected control trace product");
        };
        assert_eq!(trace.messages.len(), 1);
        assert_eq!(trace.messages[0].disposition, MessageDisposition::Consumed);
    }

    #[test]
    fn runtime_app_accepts_backend_presented_physical_control_from_native_runner() {
        let applied = Rc::new(Cell::new(0usize));
        let applied_for_reducer = Rc::clone(&applied);
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            DefaultEguiBridge::new(),
            move |_, messages: Vec<ProbeMessage>| {
                applied_for_reducer.set(applied_for_reducer.get() + messages.len());
            },
        );
        let frame = app.runtime().last_frame_identity();
        let backend_input = declared_egui_probe_pointer_input_for_runtime(
            app.runtime(),
            frame.clone(),
            PointerEventKind::Press,
        );

        let product = app.handle_backend_presented_physical_control(
            DebugCommand::physical_control_with_trace(
                "egui-native-runner-press",
                frame,
                DebugPhysicalControl::Pointer {
                    position: Point { x: 1.0, y: 1.0 },
                    kind: PointerEventKind::Press,
                    button: Some(PointerButton::Primary),
                    details: egui_pointer_details(
                        egui::Modifiers::default(),
                        Some(egui::PointerButton::Primary),
                    ),
                    pointer_is_pressed: true,
                },
            ),
            backend_input,
        );

        assert_eq!(*app.runtime().local_state(), 8);
        assert_eq!(applied.get(), 1);
        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("native runner ingress must return physical control trace");
        };
        assert_eq!(
            trace.mode,
            slipway_debug_bridge::DebugControlMode::PhysicalEquivalent
        );
        assert!(trace.handled);
        let evidence = trace
            .dispatch_evidence
            .as_ref()
            .expect("trace carries backend dispatch evidence");
        assert_eq!(
            evidence.source,
            EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input")
        );
    }

    #[test]
    fn runtime_app_raw_input_hook_injects_physical_mcp_before_ui() {
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            DefaultEguiBridge::new(),
            move |_, _messages: Vec<ProbeMessage>| {},
        );
        let frame = app.runtime().last_frame_identity();
        let handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_pointer_message(
                "egui-native-pending",
                &frame,
                1.0,
                1.0,
            ))
            .expect("runtime MCP physical request queued");
        let mut raw_input = egui::RawInput::default();

        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);

        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert!(matches!(
            raw_input.events.as_slice(),
            [
                egui::Event::PointerMoved(_),
                egui::Event::PointerButton { pressed: true, .. }
            ]
        ));
        assert!(
            handle.try_recv().expect("response channel ok").is_none(),
            "native-aware drain must not complete MCP before backend-presented input is observed"
        );
    }

    #[test]
    fn egui_native_physical_converter_supports_text_keyboard_and_native_commands() {
        let selector = slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Target {
            target: WidgetId::from("text-probe"),
        };
        let raw_input = egui::RawInput::default();

        let text_events = native_runner::egui_test_events_for_native_physical_operation(
            &DebugPhysicalControl::Text {
                selector: selector.clone(),
                text: "abc".to_string(),
            },
            &raw_input,
        )
        .expect("text input maps to egui text event");
        assert_eq!(text_events, vec![egui::Event::Text("abc".to_string())]);

        let keyboard_events = native_runner::egui_test_events_for_native_physical_operation(
            &DebugPhysicalControl::Keyboard {
                selector: selector.clone(),
                key: "Enter".to_string(),
                kind: KeyEventKind::Press,
                modifiers: Modifiers {
                    shift: true,
                    control: true,
                    alt: false,
                    meta: false,
                },
                details: KeyboardDetails {
                    repeat: true,
                    ..KeyboardDetails::default()
                },
            },
            &raw_input,
        )
        .expect("supported key maps to egui key event");
        assert!(matches!(
            keyboard_events.as_slice(),
            [egui::Event::Key {
                key: egui::Key::Enter,
                pressed: true,
                repeat: true,
                modifiers,
                ..
            }] if modifiers.shift && modifiers.ctrl && modifiers.command
        ));

        let copy_events = native_runner::egui_test_events_for_native_physical_operation(
            &DebugPhysicalControl::Command {
                selector: selector.clone(),
                command: "copy".to_string(),
                payload_ref: None,
            },
            &raw_input,
        )
        .expect("copy maps to egui copy event");
        assert_eq!(copy_events, vec![egui::Event::Copy]);

        let cut_events = native_runner::egui_test_events_for_native_physical_operation(
            &DebugPhysicalControl::Command {
                selector,
                command: "cut".to_string(),
                payload_ref: None,
            },
            &raw_input,
        )
        .expect("cut maps to egui cut event");
        assert_eq!(cut_events, vec![egui::Event::Cut]);
    }

    #[test]
    fn egui_native_physical_converter_refuses_non_raw_input_variants() {
        let selector = slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Target {
            target: WidgetId::from("text-probe"),
        };
        let raw_input = egui::RawInput::default();

        let text_edit_error = native_runner::egui_test_events_for_native_physical_operation(
            &DebugPhysicalControl::TextEdit {
                selector: selector.clone(),
                kind: TextEditKind::ReplaceBuffer,
                text: Some("abc".to_string()),
                selection_before: None,
                selection_after: None,
            },
            &raw_input,
        )
        .expect_err("text-edit is not a raw input event");
        assert_eq!(
            text_edit_error.code,
            "native-physical-control-text-edit-unsupported"
        );

        let focus_plan = native_runner::egui_test_events_for_native_physical_operation(
            &DebugPhysicalControl::Focus {
                selector: selector.clone(),
                focused: true,
            },
            &raw_input,
        )
        .expect("focus maps to backend-native mutation plan");
        assert!(
            focus_plan.is_empty(),
            "focus must not pretend to be RawInput"
        );

        let command_error = native_runner::egui_test_events_for_native_physical_operation(
            &DebugPhysicalControl::Command {
                selector: selector.clone(),
                command: "submit".to_string(),
                payload_ref: Some("payload".to_string()),
            },
            &raw_input,
        )
        .expect_err("arbitrary command payloads are not egui raw input");
        assert_eq!(
            command_error.code,
            "native-physical-control-command-unsupported"
        );

        let scroll_plan = native_runner::egui_test_events_for_native_physical_operation(
            &DebugPhysicalControl::Scroll {
                selector,
                offset_x: 0.0,
                offset_y: 24.0,
            },
            &raw_input,
        )
        .expect("scroll maps to backend-native mutation plan");
        assert!(
            scroll_plan.is_empty(),
            "scroll must not pretend to be RawInput"
        );
    }

    #[test]
    fn runtime_app_returns_error_when_injected_physical_has_no_backend_trace() {
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            DefaultEguiBridge::new(),
            move |_, _messages: Vec<ProbeMessage>| {},
        );
        let frame = app.runtime().last_frame_identity();
        let handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_text_message(
                "egui-native-no-trace",
                &frame,
                "text-probe",
                "abc",
            ))
            .expect("runtime MCP physical text request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert_eq!(raw_input.events, vec![egui::Event::Text("abc".to_string())]);

        let ctx = egui::Context::default();
        let _ = ctx.run_ui(raw_input, |ui| {
            app.render_ui(ui);
        });

        let response = handle
            .recv()
            .expect("response channel ok")
            .expect("native no-trace response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(payload.contains(r#""product_kind":"error""#), "{payload}");
        assert!(
            payload.contains("native-physical-control-no-backend-trace"),
            "{payload}"
        );
    }

    #[test]
    fn runtime_app_native_focus_completes_from_backend_presented_trace() {
        let applied = Rc::new(Cell::new(0usize));
        let applied_for_reducer = Rc::clone(&applied);
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(FocusedProbeWidget::new("egui.focused"), ()),
            DefaultEguiBridge::new(),
            move |_, messages: Vec<ProbeMessage>| {
                applied_for_reducer.set(applied_for_reducer.get() + messages.len());
            },
        );
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_ui(ui);
        });
        let frame = app.runtime().last_frame_identity();
        let handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_focus_message(
                "egui-native-focus",
                &frame,
                "egui.focused",
                true,
            ))
            .expect("runtime MCP physical focus request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert!(
            raw_input.events.is_empty(),
            "focus must use egui backend-native focus request, not fabricated RawInput"
        );

        let _ = ctx.run_ui(raw_input, |ui| {
            app.render_ui(ui);
        });

        let response = handle
            .recv()
            .expect("response channel ok")
            .expect("native focus response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(
            payload.contains(r#""product_kind":"control_trace""#),
            "{payload}"
        );
        assert!(
            payload.contains(r#""physical_equivalent":true"#),
            "{payload}"
        );
        assert!(
            payload.contains(r#""label":"backend_presented""#),
            "{payload}"
        );
        assert!(payload.contains(r#""kind":"focus""#), "{payload}");
        assert_eq!(*app.runtime().local_state(), 8);
        assert_eq!(applied.get(), 1);
    }

    #[test]
    fn runtime_app_native_text_and_keyboard_complete_after_focus() {
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(FocusedProbeWidget::new("egui.focused"), ()),
            DefaultEguiBridge::new(),
            move |_, _messages: Vec<ProbeMessage>| {},
        );
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_ui(ui);
        });

        let focus_handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_focus_message(
                "egui-native-focus-for-text",
                &app.runtime().last_frame_identity(),
                "egui.focused",
                true,
            ))
            .expect("runtime MCP physical focus request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let _ = ctx.run_ui(raw_input, |ui| app.render_ui(ui));
        focus_handle
            .recv()
            .expect("focus response channel ok")
            .expect("focus response sent");

        let text_handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_text_message(
                "egui-native-text-after-focus",
                &app.runtime().last_frame_identity(),
                "egui.focused",
                "abc",
            ))
            .expect("runtime MCP physical text request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert!(matches!(
            raw_input.events.as_slice(),
            [egui::Event::Text(text)] if text == "abc"
        ));
        let _ = ctx.run_ui(raw_input, |ui| app.render_ui(ui));
        let response = text_handle
            .recv()
            .expect("text response channel ok")
            .expect("text response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(
            payload.contains(r#""product_kind":"control_trace""#),
            "{payload}"
        );
        assert!(payload.contains(r#""kind":"text""#), "{payload}");
        assert!(
            payload.contains(r#""label":"backend_presented""#),
            "{payload}"
        );

        let focus_handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_focus_message(
                "egui-native-refocus-for-keyboard",
                &app.runtime().last_frame_identity(),
                "egui.focused",
                true,
            ))
            .expect("runtime MCP physical refocus request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let _ = ctx.run_ui(raw_input, |ui| app.render_ui(ui));
        focus_handle
            .recv()
            .expect("refocus response channel ok")
            .expect("refocus response sent");

        let keyboard_handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_keyboard_message(
                "egui-native-keyboard-after-focus",
                &app.runtime().last_frame_identity(),
                "egui.focused",
                "A",
            ))
            .expect("runtime MCP physical keyboard request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert!(matches!(
            raw_input.events.as_slice(),
            [egui::Event::Key {
                key: egui::Key::A,
                pressed: true,
                ..
            }]
        ));
        let _ = ctx.run_ui(raw_input, |ui| app.render_ui(ui));
        let response = keyboard_handle
            .recv()
            .expect("keyboard response channel ok")
            .expect("keyboard response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(
            payload.contains(r#""product_kind":"control_trace""#),
            "{payload}"
        );
        assert!(payload.contains(r#""kind":"keyboard""#), "{payload}");
        assert!(
            payload.contains(r#""label":"backend_presented""#),
            "{payload}"
        );

        let command_handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_command_no_payload_message(
                "egui-native-command-after-focus",
                &app.runtime().last_frame_identity(),
                "egui.focused",
                "copy",
            ))
            .expect("runtime MCP physical command request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert_eq!(raw_input.events, vec![egui::Event::Copy]);
        let _ = ctx.run_ui(raw_input, |ui| app.render_ui(ui));
        let response = command_handle
            .recv()
            .expect("command response channel ok")
            .expect("command response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(
            payload.contains(r#""product_kind":"control_trace""#),
            "{payload}"
        );
        assert!(payload.contains(r#""kind":"command""#), "{payload}");
        assert!(payload.contains(r#""command":"copy""#), "{payload}");
        assert!(
            payload.contains(r#""label":"backend_presented""#),
            "{payload}"
        );
    }

    #[test]
    fn runtime_app_native_scroll_completes_from_backend_scrollarea_trace() {
        let applied = Rc::new(Cell::new(0usize));
        let applied_for_reducer = Rc::clone(&applied);
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ScrollableParentWidget::new(), ()),
            DefaultEguiBridge::new(),
            move |_, messages: Vec<ProbeMessage>| {
                applied_for_reducer.set(applied_for_reducer.get() + messages.len());
            },
        );
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.render_ui(ui);
        });

        let handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_scroll_message(
                "egui-native-scroll",
                &app.runtime().last_frame_identity(),
                "egui.scroll.host",
                0.0,
                24.0,
            ))
            .expect("runtime MCP physical scroll request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert!(
            raw_input.events.is_empty(),
            "absolute scroll must use egui ScrollArea state, not fabricated RawInput"
        );

        let _ = ctx.run_ui(raw_input, |ui| app.render_ui(ui));

        let response = handle
            .recv()
            .expect("scroll response channel ok")
            .expect("scroll response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(
            payload.contains(r#""product_kind":"control_trace""#),
            "{payload}"
        );
        assert!(payload.contains(r#""kind":"scroll""#), "{payload}");
        assert!(
            payload.contains(r#""label":"backend_presented""#),
            "{payload}"
        );
        assert!(
            payload.contains(r#""physical-input/debug-injected""#),
            "{payload}"
        );
        assert_eq!(app.runtime().local_state().child, 8);
        assert_eq!(applied.get(), 1);
    }

    #[test]
    fn runtime_app_records_every_backend_trace_from_frame() {
        let applied = Rc::new(Cell::new(0usize));
        let applied_for_reducer = Rc::clone(&applied);
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            TwoCommandBridge::default(),
            move |_, messages: Vec<ProbeMessage>| {
                applied_for_reducer.set(applied_for_reducer.get() + messages.len());
            },
        );

        egui::__run_test_ui(|ui| {
            app.render_ui(ui);
        });

        let traces = app.runtime().backend_input_traces().collect::<Vec<_>>();
        assert_eq!(traces.len(), 2);
        let pointer_kinds = traces
            .iter()
            .map(|trace| match &trace.input.event {
                InputEvent::Pointer(pointer) => pointer.kind,
                _ => panic!("expected pointer event"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            pointer_kinds,
            vec![PointerEventKind::Press, PointerEventKind::Release]
        );
        for trace in traces {
            let evidence = trace
                .input
                .dispatch_evidence
                .as_ref()
                .expect("egui backend shell input carries declaration evidence");
            assert_eq!(
                evidence.source,
                EvidenceSource::backend_presented(EGUI_BACKEND_ID, "physical-input")
            );
            assert_eq!(
                evidence.selected_region,
                Some(PresentationRegionId::from("probe-hit"))
            );
        }
        assert_eq!(*app.runtime().local_state(), 9);
        assert_eq!(applied.get(), 2);
    }

    #[test]
    fn runtime_app_refuses_direct_bridge_input_before_runtime_mutation() {
        let applied = Rc::new(Cell::new(0usize));
        let applied_for_reducer = Rc::clone(&applied);
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            DirectCommandBridge::default(),
            move |_, messages: Vec<ProbeMessage>| {
                applied_for_reducer.set(applied_for_reducer.get() + messages.len());
            },
        );

        egui::__run_test_ui(|ui| {
            app.render_ui(ui);
        });

        assert_eq!(
            *app.runtime().local_state(),
            7,
            "direct backend bridge input must not mutate without dispatch evidence"
        );
        assert_eq!(applied.get(), 0);
        let traces = app.runtime().backend_input_traces().collect::<Vec<_>>();
        assert_eq!(traces.len(), 1);
        assert!(!traces[0].handled);
        assert!(traces[0].input.dispatch_evidence.is_none());
        assert!(traces[0].diagnostics.iter().any(|diagnostic| {
            diagnostic.code == slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_MISSING
        }));
    }

    #[test]
    fn runtime_app_refuses_forged_declared_backend_input_before_runtime_mutation() {
        let applied = Rc::new(Cell::new(0usize));
        let applied_for_reducer = Rc::clone(&applied);
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            ForgedDeclaredBridge::default(),
            move |_, messages: Vec<ProbeMessage>| {
                applied_for_reducer.set(applied_for_reducer.get() + messages.len());
            },
        );

        egui::__run_test_ui(|ui| {
            app.render_ui(ui);
        });

        assert_eq!(
            *app.runtime().local_state(),
            7,
            "forged declared backend input must not mutate"
        );
        assert_eq!(applied.get(), 0);
        let traces = app.runtime().backend_input_traces().collect::<Vec<_>>();
        assert_eq!(traces.len(), 1);
        assert!(!traces[0].handled);
        assert_eq!(
            traces[0]
                .input
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("forged-hit"))
        );
        assert!(traces[0].diagnostics.iter().any(|diagnostic| {
            diagnostic.code == slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_REGION_MISMATCH
        }));
    }

    #[test]
    fn root_wheel_uses_egui_unit_conversion_for_line_point_page() {
        let run_wheel = |raw_input: egui::RawInput| {
            let mut app = SlipwayEguiRuntimeApp::new(
                SlipwayRuntime::new(TallRootWidget::new(2_000.0), ()),
                DefaultEguiBridge::new(),
                move |_, _messages: Vec<ProbeMessage>| {},
            );
            let ctx = egui::Context::default();

            let _ = ctx.run_ui(raw_input, |ui| app.render_ui(ui));
            app.root_scroll_offset
        };
        let line_scroll_speed =
            egui::Context::default().options(|options| options.input_options.line_scroll_speed);

        assert_eq!(
            run_wheel(raw_wheel_input_with_unit(
                egui::MouseWheelUnit::Point,
                egui::vec2(0.0, -10.0),
            )),
            egui::vec2(0.0, 10.0)
        );
        assert_eq!(
            run_wheel(raw_wheel_input_with_unit(
                egui::MouseWheelUnit::Line,
                egui::vec2(0.0, -2.0),
            )),
            egui::vec2(0.0, 2.0 * line_scroll_speed)
        );
        assert_eq!(
            run_wheel(raw_wheel_input_with_unit(
                egui::MouseWheelUnit::Page,
                egui::vec2(0.0, -1.0),
            )),
            egui::vec2(0.0, 100.0)
        );
        assert_eq!(
            egui_convert_wheel_delta(
                egui::vec2(100.0, 100.0),
                &egui::InputOptions::default(),
                egui::MouseWheelUnit::Point,
                egui::vec2(0.0, -7.0),
                egui::TouchPhase::Move,
                egui::Modifiers {
                    shift: true,
                    ..Default::default()
                },
            ),
            egui::vec2(-7.0, 0.0),
            "egui shift-wheel horizontal remapping must be preserved"
        );
    }

    #[test]
    fn direct_slipway_wheel_does_not_suppress_root_fallback() {
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(TallRootWidget::new(2_000.0), ()),
            HandledWheelBridge::default(),
            move |_, _messages: Vec<ProbeMessage>| {},
        );
        let ctx = egui::Context::default();

        let _ = ctx.run_ui(raw_wheel_input(-10.0), |ui| app.render_ui(ui));

        assert_eq!(*app.runtime().local_state(), 0);
        assert_eq!(
            app.root_scroll_offset.y, 10.0,
            "direct Slipway wheel without dispatch evidence must not suppress root fallback scrolling"
        );
    }

    #[test]
    fn runtime_app_root_wheel_fallback_is_not_doubled_by_native_scrollarea_state() {
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(TallRootWidget::new(2_000.0), ()),
            DefaultEguiBridge::new(),
            move |_, _messages: Vec<ProbeMessage>| {},
        );
        let ctx = egui::Context::default();

        let _ = ctx.run_ui(raw_wheel_input(-4.0), |ui| app.render_ui(ui));

        assert_eq!(app.root_scroll_offset.y, 4.0);
    }

    #[test]
    fn egui_backend_focus_input_event_evidence_matches_runtime_contract() {
        egui::__run_test_ui(|ui| {
            let target = WidgetId::from("egui.focused");
            let other_target = WidgetId::from("egui.other");
            let frame = FrameIdentity {
                surface_id: "egui-focused-input".to_string(),
                surface_instance_id: "test-instance".to_string(),
                revision: 1,
                frame_index: 81,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 40.0,
                    },
                },
            };
            let layout = test_leaf_layout(TargetLocalRect::new(frame.viewport));
            let mut selected_focus = test_text_edit_region(target.clone(), frame.viewport);
            selected_focus.id = PresentationRegionId::from("egui-selected-focus");
            selected_focus.address = None;
            let mut other_focus = test_text_edit_region(
                other_target,
                Rect {
                    origin: Point { x: 0.0, y: 44.0 },
                    size: Size {
                        width: 100.0,
                        height: 40.0,
                    },
                },
            );
            other_focus.id = PresentationRegionId::from("egui-other-focus");
            let focus_regions = vec![other_focus, selected_focus.clone()];
            let view = ViewDefinition {
                target: target.clone(),
                frame: frame.clone(),
                layout: layout.clone(),
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
                hit_regions: Vec::new(),
                focus_regions: focus_regions.clone(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            };
            let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
            let presented = allocate_focus_region(
                ui,
                egui::pos2(0.0, 0.0),
                &view,
                &geometry_index,
                &selected_focus,
            );
            let regions = vec![presented];
            let context = EguiInputContext {
                ui,
                widget_id: target.clone(),
                frame: &frame,
                rect: egui_rect(egui::pos2(0.0, 0.0), frame.viewport),
                layout: &layout,
                geometry_index: &geometry_index,
                hit_regions: &[],
                focus_regions: &focus_regions,
                scroll_regions: &[],
                wheel_traversal_boundary: Default::default(),
                response: &regions[0].response,
                regions: &regions,
                native_physical_operation: None,
            };
            let event = InputEvent::Keyboard(KeyboardEvent {
                target: target.clone(),
                target_slot: None,
                key: "Enter".to_string(),
                kind: KeyEventKind::Press,
                modifiers: slipway_core::Modifiers::default(),
                details: KeyboardDetails::default(),
            });

            let input = egui_focus_backend_input_event(
                &context,
                &regions[0],
                DeclaredEventDispatchKind::Keyboard,
                event,
            );
            let diagnostics = slipway_core::backend_input_dispatch_evidence_contract_diagnostics(
                &view,
                &input,
                Some(slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED),
                Some(EGUI_BACKEND_ID),
            );

            assert!(diagnostics.is_empty(), "{diagnostics:?}");
            let evidence = input
                .dispatch_evidence
                .as_ref()
                .expect("focused backend input carries evidence");
            assert_eq!(
                evidence.candidate_regions,
                vec![PresentationRegionId::from("egui-selected-focus")]
            );
        });
    }

    #[test]
    fn seam_wheel_and_real_ingress_wheel_share_dispatch_and_result_identity() {
        // MF-M9 port to the current seam, headless: the seam side drives the
        // REAL egui physical-control path (runtime MCP submit ->
        // raw_input_hook injection -> full frame -> backend-presented trace
        // completion); the real side feeds the same hand-built egui events a
        // real OS wheel delivers through egui_winit. Both sides run one
        // dispatch frame from identical fresh apps, and the runtime
        // event_equivalence comparator must report dispatch AND result
        // identity match. The seam trace clone is relabeled `debug_mcp` only
        // to select its role in the comparator's pair contract; declared
        // dispatch identity ignores the evidence source by contract.
        let real_events = vec![
            egui::Event::PointerMoved(egui::pos2(1.0, 1.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, 7.0),
                phase: egui::TouchPhase::Move,
                modifiers: egui::Modifiers::default(),
            },
        ];

        // Real-ingress side.
        let native_applied = Rc::new(Cell::new(0usize));
        let native_applied_for_reducer = Rc::clone(&native_applied);
        let mut native_app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ScrollProbeWidget::new("egui.scroll"), ()),
            DefaultEguiBridge::new(),
            move |_, messages: Vec<ProbeMessage>| {
                native_applied_for_reducer.set(native_applied_for_reducer.get() + messages.len());
            },
        );
        let native_ctx = egui::Context::default();
        // Warm-up frame: records the presented viewport so both apps enter
        // their dispatch frame with the same frame identity.
        let _ = native_ctx.run_ui(egui::RawInput::default(), |ui| native_app.render_ui(ui));
        let _ = native_ctx.run_ui(
            egui::RawInput {
                events: real_events.clone(),
                ..egui::RawInput::default()
            },
            |ui| native_app.render_ui(ui),
        );
        let native_trace = native_app
            .runtime()
            .backend_input_traces()
            .find(|trace| matches!(trace.input.event, InputEvent::Wheel(_)))
            .cloned()
            .expect("real ingress wheel records a backend input trace");

        // Seam side: the full current physical-control path, headless.
        let seam_applied = Rc::new(Cell::new(0usize));
        let seam_applied_for_reducer = Rc::clone(&seam_applied);
        let mut seam_app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ScrollProbeWidget::new("egui.scroll"), ()),
            DefaultEguiBridge::new(),
            move |_, messages: Vec<ProbeMessage>| {
                seam_applied_for_reducer.set(seam_applied_for_reducer.get() + messages.len());
            },
        );
        let seam_ctx = egui::Context::default();
        let _ = seam_ctx.run_ui(egui::RawInput::default(), |ui| seam_app.render_ui(ui));
        let frame = seam_app.runtime().last_frame_identity();
        let physical_handle = seam_app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_wheel_message(
                "egui-seam-wheel-equivalence",
                &frame,
                1.0,
                1.0,
                0.0,
                7.0,
            ))
            .expect("runtime MCP physical wheel request queued");
        let mut raw_input = egui::RawInput::default();
        let (drained, error) =
            seam_app.inject_pending_native_physical_into_raw_input(&mut raw_input);
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert_eq!(
            raw_input.events, real_events,
            "the seam must inject exactly the egui events real OS ingress delivers"
        );
        let _ = seam_ctx.run_ui(raw_input, |ui| seam_app.render_ui(ui));
        let response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical wheel response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(
            payload.contains(r#""mode":"physical_equivalent""#),
            "seam reply must be a physical-equivalent control trace: {payload}"
        );
        assert!(
            payload.contains(r#""handled":true"#),
            "seam wheel must be handled: {payload}"
        );
        let seam_trace = seam_app
            .runtime()
            .backend_input_traces()
            .find(|trace| matches!(trace.input.event, InputEvent::Wheel(_)))
            .cloned()
            .expect("seam wheel records a backend input trace");

        // The equivalence bar: same state effect and comparator-proven
        // dispatch/result identity.
        assert_eq!(
            native_app.runtime().local_state(),
            seam_app.runtime().local_state(),
            "seam and real wheel must leave identical widget state"
        );
        assert_eq!(native_applied.get(), seam_applied.get());
        let mut mcp_side = seam_trace.clone();
        if let Some(evidence) = mcp_side.input.dispatch_evidence.as_mut() {
            evidence.source = EvidenceSource::debug_mcp("physical-control");
        }
        let diagnostics = slipway_runtime::backend_input_trace_equivalence_diagnostics(&[
            &mcp_side,
            &native_trace,
        ]);
        assert_eq!(
            diagnostics.len(),
            1,
            "comparator must pair the seam and real wheel traces: {diagnostics:?}"
        );
        assert_eq!(
            diagnostics[0].code, "event_equivalence.identity_match",
            "seam and real wheel must share dispatch AND result identity: {diagnostics:?}"
        );
    }

    #[test]
    fn runtime_app_transport_wake_drains_same_runtime_without_window() {
        let applied = Rc::new(Cell::new(0usize));
        let applied_for_reducer = Rc::clone(&applied);
        let runtime = SlipwayRuntime::new(ProbeWidget::new("one"), ());
        let transport = runtime
            .start_debug_mcp_transport()
            .expect("runtime MCP transport starts");
        let addr = transport.local_addr();
        let mut app = SlipwayEguiRuntimeApp::new(
            runtime,
            DefaultEguiBridge::new(),
            move |_, messages: Vec<ProbeMessage>| {
                applied_for_reducer.set(applied_for_reducer.get() + messages.len());
            },
        )
        .with_debug_mcp_transport(transport);
        assert_eq!(app.debug_mcp_transport_addr(), Some(addr));
        let ctx = egui::Context::default();
        app.ensure_mcp_wake_forwarder(&ctx);

        let mut stream = TcpStream::connect(addr).expect("connect to runtime MCP transport");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        writeln!(stream, "{}", control_message("egui-tcp-control", "one"))
            .expect("write JSON-RPC line");
        stream.flush().expect("flush JSON-RPC line");

        let deadline = Instant::now() + Duration::from_secs(2);
        while app.drain_egui_mcp_wakes() == 0 {
            assert!(
                Instant::now() < deadline,
                "transport wake should reach egui app"
            );
            thread::sleep(Duration::from_millis(10));
        }

        let (drained, error) = app.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert_eq!(*app.runtime().local_state(), 8);
        assert_eq!(applied.get(), 1);

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .expect("read JSON-RPC response line");
        assert!(response_line.contains(r#""id":"egui-tcp-control""#));
        assert!(response_line.contains("control_trace"));
        assert!(response_line.contains("Consumed"));
    }

    #[test]
    fn observations_are_explicit_and_moved_out_with_take() {
        let mut bridge = DefaultEguiBridge::new();

        assert!(
            !<DefaultEguiBridge as EguiSlipwayBridge<ProbeWidget>>::wants_observation(&mut bridge)
        );

        bridge.request_observation();
        assert!(
            <DefaultEguiBridge as EguiSlipwayBridge<ProbeWidget>>::wants_observation(&mut bridge)
        );
        assert!(
            !<DefaultEguiBridge as EguiSlipwayBridge<ProbeWidget>>::wants_observation(&mut bridge)
        );

        <DefaultEguiBridge as EguiSlipwayBridge<ProbeWidget>>::observe(
            &mut bridge,
            EguiObservationContext {
                widget_id: WidgetId::from("one"),
                capabilities: Vec::new(),
                topology: TopologyNode::leaf(WidgetId::from("one")),
                unsupported: Vec::new(),
                state: vec![StateObservation {
                    target: WidgetId::from("one"),
                    slot: None,
                    name: "local".to_string(),
                    value: "7".to_string(),
                }],
                layout_intent: None,
            },
        );

        assert_eq!(bridge.take_probe_products().len(), 2);
        assert!(bridge.take_probe_products().is_empty());
    }

    #[test]
    fn ordinary_widget_path_does_not_emit_layout_intent_even_if_available() {
        let widget = ProbeWidget::new("one");
        let mut local = widget.initial_local_state();
        let mut bridge = DefaultEguiBridge::new();
        let mut messages = Vec::new();

        bridge.request_observation();
        egui::__run_test_ui(|ui| {
            ui.add(SlipwayEguiWidget::new(
                &widget,
                &(),
                &mut local,
                &mut bridge,
                &mut messages,
            ));
        });

        let products = bridge.take_probe_products();
        assert_eq!(products.len(), 2);
        assert!(
            !products
                .iter()
                .any(|product| matches!(product, ProbeProduct::LayoutIntent(_)))
        );
    }

    #[test]
    fn layout_intent_widget_path_emits_layout_intent_only_when_requested() {
        let widget = ProbeWidget::new("one");
        let mut local = widget.initial_local_state();
        let mut bridge = DefaultEguiBridge::new();
        let mut messages = Vec::new();

        egui::__run_test_ui(|ui| {
            ui.add(SlipwayEguiLayoutIntentWidget::new(
                &widget,
                &(),
                &mut local,
                &mut bridge,
                &mut messages,
            ));
        });

        assert!(bridge.take_probe_products().is_empty());

        bridge.request_observation();
        egui::__run_test_ui(|ui| {
            ui.add(SlipwayEguiLayoutIntentWidget::new(
                &widget,
                &(),
                &mut local,
                &mut bridge,
                &mut messages,
            ));
        });

        let products = bridge.take_probe_products();
        assert_eq!(products.len(), 3);
        let layout_intent = products
            .iter()
            .find_map(|product| match product {
                ProbeProduct::LayoutIntent(layout_intent) => Some(layout_intent),
                _ => None,
            })
            .expect("layout intent probe");

        assert_eq!(layout_intent.target, WidgetId::from("one"));
        assert!(layout_intent.responsive_variant.is_some());
        assert!(layout_intent.size_policy.is_some());
        assert!(layout_intent.overflow_policy.is_some());
    }

    #[derive(Debug, Default)]
    struct CallCounts {
        capabilities: Cell<u32>,
        topology: Cell<u32>,
        unsupported: Cell<u32>,
        observe_state: Cell<u32>,
    }

    #[derive(Clone, Debug)]
    struct CountingWidget {
        calls: Rc<CallCounts>,
    }

    impl PartialEq for CountingWidget {
        fn eq(&self, other: &Self) -> bool {
            Rc::ptr_eq(&self.calls, &other.calls)
        }
    }

    impl SlipwayWidgetTypes for CountingWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = ProbeMessage;
    }

    impl SlipwaySsot for CountingWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("counting")
        }

        fn capabilities(&self) -> Vec<Capability> {
            self.calls
                .capabilities
                .set(self.calls.capabilities.get() + 1);
            vec![Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            self.calls.topology.set(self.calls.topology.get() + 1);
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            self.calls.unsupported.set(self.calls.unsupported.get() + 1);
            Vec::new()
        }
    }

    impl SlipwayLogic for CountingWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for CountingWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: LayoutInput,
            output: slipway_core::LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 10.0,
                    height: 10.0,
                },
            }))
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            Vec::new()
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            self.calls
                .observe_state
                .set(self.calls.observe_state.get() + 1);
            Vec::new()
        }
    }

    impl SlipwayViewDefinition for CountingWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = slipway_core::layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout: layout.clone(),
                paint: self.paint(external, local, &layout),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayFontResolutionPolicy for CountingWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            FontResolutionEvidence {
                request,
                resolved_ref: None,
                fallback_chain: Vec::new(),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    #[test]
    fn observation_only_computes_non_admission_authority_products_when_requested() {
        let calls = Rc::new(CallCounts::default());
        let widget = CountingWidget {
            calls: Rc::clone(&calls),
        };
        let mut bridge = DefaultEguiBridge::new();
        let mut local = widget.initial_local_state();
        let mut messages = Vec::new();

        egui::__run_test_ui(|ui| {
            ui.add(SlipwayEguiWidget::new(
                &widget,
                &(),
                &mut local,
                &mut bridge,
                &mut messages,
            ));
        });

        assert_eq!(calls.capabilities.get(), 1);
        assert_eq!(calls.topology.get(), 0);
        assert_eq!(calls.unsupported.get(), 0);
        assert_eq!(calls.observe_state.get(), 0);

        bridge.request_observation();
        egui::__run_test_ui(|ui| {
            ui.add(SlipwayEguiWidget::new(
                &widget,
                &(),
                &mut local,
                &mut bridge,
                &mut messages,
            ));
        });

        assert_eq!(calls.capabilities.get(), 3);
        assert_eq!(calls.topology.get(), 1);
        assert_eq!(calls.unsupported.get(), 1);
        assert_eq!(calls.observe_state.get(), 1);
        assert_eq!(bridge.take_probe_products().len(), 2);
        assert!(bridge.take_probe_products().is_empty());
    }

    #[test]
    fn step223_mixed_native_and_debug_events_keep_exact_origin_span() {
        let span = EguiDebugInputSpan {
            token: 41,
            start_event_index: 1,
            end_event_index: 2,
        };
        let event = egui::Event::Copy;
        assert_eq!(
            egui_raw_event_source(Some(span), false, 0, &event)
                .pass_id
                .as_deref(),
            Some(EGUI_NATIVE_OS_INPUT_PASS)
        );
        assert_eq!(
            egui_raw_event_source(Some(span), false, 1, &event)
                .pass_id
                .as_deref(),
            Some(EGUI_DEBUG_INPUT_PASS)
        );
        assert_eq!(
            egui_raw_event_source(Some(span), false, 2, &event)
                .pass_id
                .as_deref(),
            Some(EGUI_NATIVE_OS_INPUT_PASS)
        );
        assert_eq!(
            egui_raw_event_source(
                Some(span),
                true,
                1,
                &egui::Event::Ime(egui::ImeEvent::Commit("x".to_string())),
            )
            .pass_id
            .as_deref(),
            Some(EGUI_DEBUG_COMPOSITION_PASS)
        );
    }

    fn step223_probe_reducer(_: &mut (), _: Vec<ProbeMessage>) {}

    fn step223_probe_app()
    -> SlipwayEguiRuntimeApp<ProbeWidget, DefaultEguiBridge, fn(&mut (), Vec<ProbeMessage>)> {
        SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            DefaultEguiBridge::new(),
            step223_probe_reducer,
        )
    }

    #[test]
    fn step223_post_present_starts_absent_and_ui_work_only_records_candidate() {
        let mut app = step223_probe_app();
        assert!(app.last_successfully_presented.is_none());
        assert!(app.rendered_frame_candidate.is_none());
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.render_ui(ui));
        let first_candidate = app
            .rendered_frame_candidate
            .clone()
            .expect("UI work records a paint candidate");
        assert!(app.last_successfully_presented.is_none());
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.render_ui(ui));
        assert_eq!(app.rendered_frame_candidate, Some(first_candidate));
        assert!(app.last_successfully_presented.is_none());
    }

    #[cfg(any())]
    #[test]
    fn step223_post_present_ordinary_is_stable_and_advances_once_per_actual_present() {
        let mut app = step223_probe_app();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.render_ui(ui));
        let first_candidate = app.rendered_frame_candidate.clone().unwrap();
        app.record_slipway_debug_post_present(eframe::SlipwayDebugPostPresent {
            viewport_id: egui::ViewportId::ROOT,
            capture_token: None,
        });
        assert_eq!(
            app.last_successfully_presented,
            Some(first_candidate.clone())
        );

        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.render_ui(ui));
        assert_eq!(
            app.last_successfully_presented,
            Some(first_candidate.clone())
        );
        let mut second_expected = app.rendered_frame_candidate.clone().unwrap();
        second_expected.frame_index = first_candidate.frame_index + 1;
        app.record_slipway_debug_post_present(eframe::SlipwayDebugPostPresent {
            viewport_id: egui::ViewportId::ROOT,
            capture_token: None,
        });
        assert_eq!(
            app.last_successfully_presented,
            Some(second_expected.clone())
        );
        app.record_slipway_debug_post_present(eframe::SlipwayDebugPostPresent {
            viewport_id: egui::ViewportId::ROOT,
            capture_token: None,
        });
        assert_eq!(app.last_successfully_presented, Some(second_expected));
    }

    #[cfg(any())]
    #[test]
    fn step223_post_present_wrong_nonroot_and_late_tokens_are_isolated() {
        let mut app = step223_probe_app();
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.render_ui(ui));
        let candidate = app.rendered_frame_candidate.clone();
        app.record_slipway_debug_post_present(eframe::SlipwayDebugPostPresent {
            viewport_id: egui::ViewportId::from_hash_of("secondary"),
            capture_token: None,
        });
        assert_eq!(app.rendered_frame_candidate, candidate);
        assert!(app.last_successfully_presented.is_none());
        app.record_slipway_debug_post_present(eframe::SlipwayDebugPostPresent {
            viewport_id: egui::ViewportId::ROOT,
            capture_token: Some(999),
        });
        assert_eq!(app.rendered_frame_candidate, candidate);
        assert!(app.last_successfully_presented.is_none());
    }

    #[test]
    fn step223_composition_provenance_commit_key_rejects_shift_and_ambiguous_mutation() {
        let operation = DebugPhysicalControl::TextComposition {
            selector: slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Target {
                target: WidgetId::from("egui.focused"),
            },
            updates: vec![slipway_debug_bridge::DebugTextCompositionUpdate {
                preedit_text: "preedit".to_string(),
                cursor_range: Some(TextSelectionRange {
                    anchor: 0,
                    focus: 1,
                }),
            }],
            commit: "commit".to_string(),
        };
        let span = EguiDebugInputSpan {
            token: 71,
            start_event_index: 1,
            end_event_index: 3,
        };
        let debug_events = [
            egui::Event::Copy,
            egui::Event::Ime(egui::ImeEvent::Preedit {
                text: "preedit".to_string(),
                active_range_chars: Some(0..1),
            }),
            egui::Event::Ime(egui::ImeEvent::Commit("commit".to_string())),
            egui::Event::Copy,
        ];
        let (commit, count) =
            egui_accepted_composition_commit_key(&debug_events, Some(span), true, Some(&operation))
                .expect("the exact nonzero accepted span establishes commit custody");
        assert_eq!(count, 2);
        assert_eq!(commit.event_index, 2);
        let shifted = EguiDebugInputSpan {
            start_event_index: 0,
            end_event_index: 2,
            ..span
        };
        assert!(
            egui_accepted_composition_commit_key(
                &debug_events,
                Some(shifted),
                true,
                Some(&operation),
            )
            .is_none()
        );
        let mut ambiguous = debug_events.to_vec();
        ambiguous[0] = egui::Event::Text("native".to_string());
        assert!(
            egui_accepted_composition_commit_key(&ambiguous, Some(span), true, Some(&operation),)
                .is_none()
        );
        assert_eq!(
            egui_raw_event_source(Some(span), true, 0, &ambiguous[0])
                .pass_id
                .as_deref(),
            Some(EGUI_NATIVE_OS_INPUT_PASS)
        );
    }

    #[cfg(any())]
    #[test]
    fn step223_composition_plan_uses_logical_ime_and_multibyte_ranges() {
        let operation = DebugPhysicalControl::TextComposition {
            selector: slipway_debug_bridge::DebugPhysicalControlDeclarationSelector::Target {
                target: WidgetId::from("egui.focused"),
            },
            updates: vec![slipway_debug_bridge::DebugTextCompositionUpdate {
                preedit_text: "한글".to_string(),
                cursor_range: Some(TextSelectionRange {
                    anchor: 1,
                    focus: 2,
                }),
            }],
            commit: "界".to_string(),
        };
        let native_runner::NativePhysicalControlPlan::Input(events) =
            native_runner::egui_events_for_native_physical_operation(&operation, 1.0)
                .expect("composition maps to dependency-owned native IME ingress")
        else {
            panic!("composition must not use a backend mutation shortcut");
        };
        assert!(matches!(
            events.as_slice(),
            [
                eframe::egui_winit::SlipwayDebugInputEvent::Ime(winit::event::Ime::Preedit(
                    text,
                    Some((3, 6))
                )),
                eframe::egui_winit::SlipwayDebugInputEvent::Ime(winit::event::Ime::Commit(commit)),
            ] if text == "한글" && commit == "界"
        ));
    }

    #[cfg(any())]
    #[test]
    fn step223_capture_state_completes_after_presented_and_mapped_in_either_order() {
        let run = |mapped_first: bool, post_present_first: bool| {
            let mut app = SlipwayEguiRuntimeApp::new(
                SlipwayRuntime::new(ProbeWidget::new("one"), ()),
                DefaultEguiBridge::new(),
                move |_, _messages: Vec<ProbeMessage>| {},
            );
            let expected = app.runtime().last_frame_identity();
            app.last_successfully_presented = Some(expected.clone());
            app.rendered_frame_candidate = Some(expected.clone());
            let handle = app
                .runtime()
                .runtime_mcp_client_clone()
                .submit(screenshot_message("egui-direct-capture", &expected))
                .expect("screenshot MCP request queued");
            let pending = app
                .runtime_mut()
                .take_pending_native_mcp_call()
                .expect("runtime MCP receive")
                .expect("pending screenshot call");
            let lease = app
                .runtime()
                .take_debug_command_lease()
                .expect("debug bridge receive")
                .expect("screenshot lease");
            let (event_tx, event_rx) = mpsc::sync_channel(3);
            app.pending_presented_capture = Some(PendingEguiPresentedCapture {
                pending,
                lease,
                token: 77,
                selector: PresentedScreenshotSelector::Exact {
                    expected_frame: expected.clone(),
                },
                event_rx,
                deadline: Instant::now() + Duration::from_secs(1),
                post_presented_candidate: None,
                post_presented_frame: None,
                presented: None,
                mapped: None,
            });
            if post_present_first {
                app.record_slipway_debug_post_present(eframe::SlipwayDebugPostPresent {
                    viewport_id: egui::ViewportId::ROOT,
                    capture_token: Some(77),
                });
            }
            let presented = egui_wgpu::winit::DirectCaptureEvent::Presented {
                token: 77,
                format: egui_wgpu::winit::DirectCaptureFormat::Bgra8UnormSrgb,
                alpha: egui_wgpu::winit::DirectCaptureAlphaMode::Opaque,
                width: 2,
                height: 1,
            };
            let mapped = egui_wgpu::winit::DirectCaptureEvent::Mapped {
                token: 77,
                result: Ok(Arc::<[u8]>::from(vec![1, 2, 3, 255, 4, 5, 6, 255])),
            };
            if mapped_first {
                event_tx.send(mapped).expect("send mapped");
                event_tx.send(presented).expect("send presented");
                event_tx
                    .send(egui_wgpu::winit::DirectCaptureEvent::Mapped {
                        token: 77,
                        result: Ok(Arc::<[u8]>::from(vec![9; 8])),
                    })
                    .expect("send duplicate mapped");
            } else {
                event_tx.send(presented).expect("send presented");
                event_tx
                    .send(egui_wgpu::winit::DirectCaptureEvent::Presented {
                        token: 77,
                        format: egui_wgpu::winit::DirectCaptureFormat::Bgra8UnormSrgb,
                        alpha: egui_wgpu::winit::DirectCaptureAlphaMode::Opaque,
                        width: 2,
                        height: 1,
                    })
                    .expect("send duplicate presented");
                event_tx.send(mapped).expect("send mapped");
            }
            drop(event_tx);
            if !post_present_first {
                let (polled, error) = app.drain_pending_presented_capture();
                assert!(polled);
                assert_eq!(error, None);
                assert!(app.pending_presented_capture.is_some());
                app.record_slipway_debug_post_present(eframe::SlipwayDebugPostPresent {
                    viewport_id: egui::ViewportId::ROOT,
                    capture_token: Some(77),
                });
            }
            let (polled, error) = app.drain_pending_presented_capture();
            assert!(polled);
            assert_eq!(error, None);
            assert!(app.pending_presented_capture.is_none());
            let response = handle
                .recv()
                .expect("screenshot response channel")
                .expect("screenshot response");
            let payload = response["result"]["content"][0]["text"]
                .as_str()
                .expect("screenshot payload");
            assert!(
                payload.contains(r#""product_kind":"presented_screenshot""#),
                "{payload}"
            );
            assert!(
                payload.contains("direct_acquired_surface_texture_copy"),
                "{payload}"
            );
            assert!(
                payload.contains("presented-pixels/direct-surface-copy"),
                "{payload}"
            );
            assert_eq!(
                app.last_successfully_presented
                    .as_ref()
                    .map(|frame| frame.frame_index),
                Some(expected.frame_index + 1)
            );
        };
        run(false, true);
        run(true, true);
        run(false, false);
        run(true, false);
    }

    #[cfg(any())]
    #[test]
    fn step223_current_capture_ignores_context_and_uses_post_present_candidate() {
        let mut app = step223_probe_app();
        let request_context = app.runtime().last_frame_identity();
        let mut factual_presented = request_context.clone();
        factual_presented.frame_index = 20;
        app.last_successfully_presented = Some(factual_presented);
        app.rendered_frame_candidate = Some(request_context.clone());

        let handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(current_screenshot_message("egui-current-capture"))
            .expect("current screenshot MCP request queued");
        let pending = app
            .runtime_mut()
            .take_pending_native_mcp_call()
            .expect("runtime MCP receive")
            .expect("pending current screenshot call");
        let lease = app
            .runtime()
            .take_debug_command_lease()
            .expect("debug bridge receive")
            .expect("current screenshot lease");
        let selector = match &lease.command().kind {
            DebugCommandKind::Screenshot { request } => request.selector.clone(),
            other => panic!("expected screenshot command, got {other:?}"),
        };
        assert!(matches!(
            &selector,
            PresentedScreenshotSelector::Current {
                request_context: context
            } if context == &request_context
        ));
        assert!(egui_screenshot_selector_matches_intake(
            &selector,
            app.last_successfully_presented.as_ref(),
        ));

        let (event_tx, event_rx) = mpsc::sync_channel(3);
        app.pending_presented_capture = Some(PendingEguiPresentedCapture {
            pending,
            lease,
            token: 78,
            selector,
            event_rx,
            deadline: Instant::now() + Duration::from_secs(1),
            post_presented_candidate: None,
            post_presented_frame: None,
            presented: None,
            mapped: None,
        });
        app.record_slipway_debug_post_present(eframe::SlipwayDebugPostPresent {
            viewport_id: egui::ViewportId::ROOT,
            capture_token: Some(78),
        });
        event_tx
            .send(egui_wgpu::winit::DirectCaptureEvent::Presented {
                token: 78,
                format: egui_wgpu::winit::DirectCaptureFormat::Bgra8UnormSrgb,
                alpha: egui_wgpu::winit::DirectCaptureAlphaMode::Opaque,
                width: 1,
                height: 1,
            })
            .unwrap();
        event_tx
            .send(egui_wgpu::winit::DirectCaptureEvent::Mapped {
                token: 78,
                result: Ok(Arc::<[u8]>::from([1, 2, 3, 255])),
            })
            .unwrap();
        assert_eq!(app.drain_pending_presented_capture().1, None);

        let response = handle.recv().unwrap().unwrap();
        let payload = response["result"]["content"][0]["text"].as_str().unwrap();
        assert!(payload.contains(r#""admission":"current""#), "{payload}");
        assert!(payload.contains(r#""kind":"current""#), "{payload}");
        assert!(payload.contains(r#""frame_index":21"#), "{payload}");
        assert!(!payload.contains("screenshot-frame-mismatch"), "{payload}");
    }

    #[cfg(any())]
    #[test]
    fn step223_selector_admission_and_current_candidate_provenance_are_exhaustive() {
        let expected = frame(4);
        let exact = PresentedScreenshotSelector::Exact {
            expected_frame: expected.clone(),
        };
        let current = PresentedScreenshotSelector::Current {
            request_context: frame(1),
        };
        assert!(egui_screenshot_selector_matches_intake(
            &exact,
            Some(&expected)
        ));
        assert!(!egui_screenshot_selector_matches_intake(
            &exact,
            Some(&frame(5))
        ));
        assert!(egui_screenshot_selector_matches_intake(
            &current,
            Some(&frame(99))
        ));
        assert!(egui_screenshot_selector_matches_intake(&current, None));

        let mut captured = expected.clone();
        captured.frame_index = 88;
        assert!(egui_capture_frame_has_candidate_provenance(
            &expected, &captured
        ));
        captured.surface_instance_id.push_str("-other");
        assert!(!egui_capture_frame_has_candidate_provenance(
            &expected, &captured
        ));
    }

    #[cfg(any())]
    #[test]
    fn step223_capture_state_maps_map_poll_and_surface_acquire_terminals() {
        let cases = [
            (
                egui_wgpu::winit::DirectCaptureEvent::Mapped {
                    token: 81,
                    result: Err(egui_wgpu::winit::DirectCaptureMapError::MapFailed),
                },
                "screenshot-map-failed",
            ),
            (
                egui_wgpu::winit::DirectCaptureEvent::PollFailed {
                    token: 81,
                    error: egui_wgpu::winit::DirectCapturePollError::Device,
                },
                "screenshot-poll-failed",
            ),
            (
                egui_wgpu::winit::DirectCaptureEvent::Refused {
                    token: 81,
                    reason: egui_wgpu::winit::DirectCaptureRefusal::SurfaceAcquireFailed,
                },
                "screenshot-surface-acquire-failed",
            ),
        ];
        for (event, expected_code) in cases {
            let mut app = step223_probe_app();
            let expected = app.runtime().last_frame_identity();
            let handle = app
                .runtime()
                .runtime_mcp_client_clone()
                .submit(screenshot_message("egui-terminal", &expected))
                .unwrap();
            let pending = app
                .runtime_mut()
                .take_pending_native_mcp_call()
                .unwrap()
                .unwrap();
            let lease = app.runtime().take_debug_command_lease().unwrap().unwrap();
            let (event_tx, event_rx) = mpsc::sync_channel(3);
            app.pending_presented_capture = Some(PendingEguiPresentedCapture {
                pending,
                lease,
                token: 81,
                selector: PresentedScreenshotSelector::Exact {
                    expected_frame: expected,
                },
                event_rx,
                deadline: Instant::now() + Duration::from_secs(1),
                post_presented_candidate: None,
                post_presented_frame: None,
                presented: None,
                mapped: None,
            });
            event_tx.send(event).unwrap();
            assert_eq!(app.drain_pending_presented_capture().1, None);
            let response = handle.recv().unwrap().unwrap();
            let payload = response["result"]["content"][0]["text"].as_str().unwrap();
            assert!(payload.contains(expected_code), "{payload}");
            assert!(app.pending_presented_capture.is_none());
        }
    }

    #[cfg(any())]
    #[test]
    fn step223_capture_state_deadline_and_teardown_complete_exactly_once() {
        let run = |teardown: bool, expected_code: &str| {
            let mut app = step223_probe_app();
            let expected = app.runtime().last_frame_identity();
            let handle = app
                .runtime()
                .runtime_mcp_client_clone()
                .submit(screenshot_message("egui-terminal", &expected))
                .unwrap();
            let pending = app
                .runtime_mut()
                .take_pending_native_mcp_call()
                .unwrap()
                .unwrap();
            let lease = app.runtime().take_debug_command_lease().unwrap().unwrap();
            let (_event_tx, event_rx) = mpsc::sync_channel(3);
            app.pending_presented_capture = Some(PendingEguiPresentedCapture {
                pending,
                lease,
                token: 82,
                selector: PresentedScreenshotSelector::Exact {
                    expected_frame: expected,
                },
                event_rx,
                deadline: if teardown {
                    Instant::now() + Duration::from_secs(1)
                } else {
                    Instant::now() - Duration::from_millis(1)
                },
                post_presented_candidate: None,
                post_presented_frame: None,
                presented: None,
                mapped: None,
            });
            if teardown {
                app.terminate_pending_presented_capture_for_teardown();
                app.terminate_pending_presented_capture_for_teardown();
            } else {
                assert_eq!(app.drain_pending_presented_capture().1, None);
            }
            let response = handle.recv().unwrap().unwrap();
            let payload = response["result"]["content"][0]["text"].as_str().unwrap();
            assert!(payload.contains(expected_code), "{payload}");
            assert!(app.pending_presented_capture.is_none());
        };
        run(false, "screenshot-deadline");
        run(true, "screenshot-teardown");
    }

    #[test]
    fn step223_composition_provenance_mixed_span_carries_exact_indices_and_mutation() {
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(FocusedProbeWidget::new("egui.focused"), ()),
            DefaultEguiBridge::new(),
            move |_, _messages: Vec<ProbeMessage>| {},
        );
        let ctx = egui::Context::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.render_ui(ui));

        let focus_handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(physical_focus_message(
                "egui-composition-focus",
                &app.runtime().last_frame_identity(),
                "egui.focused",
                true,
            ))
            .expect("focus request queued");
        let mut focus_input = egui::RawInput::default();
        let (drained, error) = app.inject_pending_native_physical_into_raw_input(&mut focus_input);
        assert_eq!((drained, error), (1, None));
        let _ = ctx.run_ui(focus_input, |ui| app.render_ui(ui));
        focus_handle
            .recv()
            .expect("focus response channel")
            .expect("focus response");

        let frame = app.runtime().last_frame_identity();
        let handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(text_composition_message(
                "egui-composition-success",
                &frame,
                "egui.focused",
                "한",
                "界",
            ))
            .expect("composition request queued");
        let pending = app
            .runtime_mut()
            .take_pending_native_mcp_call()
            .expect("composition MCP receive")
            .expect("pending composition call");
        let lease = app
            .runtime()
            .take_debug_command_lease()
            .expect("composition bridge receive")
            .expect("composition lease");
        let token = 91;
        let span = EguiDebugInputSpan {
            token,
            start_event_index: 1,
            end_event_index: 3,
        };
        app.pending_native_physical =
            Some(PendingEguiNativePhysicalControl::WaitingForBackendTrace {
                pending,
                lease,
                origin: PendingEguiTraceOrigin::DebugComposition {
                    token,
                    span,
                    composition: PendingEguiComposition {
                        preflight: EguiCompositionPreflight {
                            target: WidgetId::from("egui.focused"),
                            target_slot: Some(WidgetSlotAddress::new(
                                WidgetId::from("egui.focused"),
                                0,
                            )),
                            selected_region: PresentationRegionId::from("text-focus"),
                            focused: true,
                            editable: true,
                        },
                    },
                },
            });
        let raw_input = egui::RawInput {
            events: vec![
                egui::Event::Copy,
                egui::Event::Ime(egui::ImeEvent::Preedit {
                    text: "한".to_string(),
                    active_range_chars: Some(0..1),
                }),
                egui::Event::Ime(egui::ImeEvent::Commit("界".to_string())),
                egui::Event::Copy,
            ],
            ..Default::default()
        };
        let _ = ctx.run_ui(raw_input, |ui| app.render_ui(ui));

        let response = handle
            .recv()
            .expect("composition response channel")
            .expect("composition response");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("composition payload");
        assert!(
            payload.contains(r#""product_kind":"text_composition_trace""#),
            "{payload}"
        );
        assert!(payload.contains(r#""completed":true"#), "{payload}");
        assert!(payload.contains(r#""before":"editable""#), "{payload}");
        assert!(payload.contains(r#""after":"editable界""#), "{payload}");
        assert!(payload.contains(EGUI_DEBUG_COMPOSITION_PASS), "{payload}");
        assert!(payload.contains(r#""event_index":1"#), "{payload}");
        assert!(payload.contains(r#""event_index":2"#), "{payload}");
    }

    #[test]
    fn step226_standard_egui_screenshot_refuses_presented_capture_without_fork() {
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            DefaultEguiBridge::new(),
            move |_, _messages: Vec<ProbeMessage>| {},
        );
        let mut wrong = app.runtime().last_frame_identity();
        wrong.frame_index += 9;
        let handle = app
            .runtime()
            .runtime_mcp_client_clone()
            .submit(screenshot_message("egui-capture-refusal", &wrong))
            .expect("screenshot refusal request queued");
        let (drained, error) = app.intake_pending_native_command(&egui::Context::default());
        assert_eq!((drained, error), (1, None));
        assert!(app.pending_presented_capture.is_none());
        let response = handle
            .recv()
            .expect("refusal response channel")
            .expect("refusal response");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("refusal payload");
        assert!(
            payload.contains("screenshot-presented-capture-unavailable"),
            "{payload}"
        );
        assert!(
            payload.contains("standard eframe/egui-wgpu does not expose request-scoped presented-surface readback"),
            "{payload}"
        );
    }

    #[cfg(any())]
    #[test]
    fn step223_enabled_but_idle_has_no_request_work() {
        let mut app = SlipwayEguiRuntimeApp::new(
            SlipwayRuntime::new(ProbeWidget::new("one"), ()),
            DefaultEguiBridge::new(),
            move |_, _messages: Vec<ProbeMessage>| {},
        );
        let mut raw_input = egui::RawInput {
            events: vec![egui::Event::Copy],
            ..Default::default()
        };
        let before = raw_input.events.clone();
        <SlipwayEguiRuntimeApp<_, _, _> as eframe::App>::raw_input_hook_with_slipway_debug(
            &mut app,
            &egui::Context::default(),
            &mut raw_input,
            None,
        );
        assert_eq!(raw_input.events, before);
        assert_eq!(app.next_native_debug_token, 1);
        assert!(app.pending_native_physical.is_none());
        assert!(app.pending_presented_capture.is_none());
        assert_eq!(app.drain_pending_presented_capture(), (false, None));
        assert!(!app.native_mcp_wake_pending);
    }

    #[cfg(any())]
    #[test]
    #[ignore = "requires a visible desktop and WGPU surface"]
    fn step223_live_egui_acquired_surface_capture() {
        const CHILD_ENV: &str = "SLIPWAY_STEP223_LIVE_EGUI_CHILD";
        if std::env::var_os(CHILD_ENV).is_some() {
            let runtime = SlipwayRuntime::new(ProbeWidget::new("live-egui"), ());
            let client = runtime.runtime_mcp_client_clone();
            let mut app = SlipwayEguiRuntimeApp::new(
                runtime,
                DefaultEguiBridge::new(),
                step223_probe_reducer,
            );
            app.live_continuous_repaint = true;
            thread::spawn(move || {
                let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    thread::sleep(Duration::from_millis(750));
                    let capture = |id: &str| {
                        let response = client
                            .submit(current_screenshot_message(id))
                            .expect("submit live current screenshot")
                            .recv()
                            .expect("live screenshot response channel")
                            .expect("live screenshot response");
                        response["result"]["content"][0]["text"]
                            .as_str()
                            .expect("live screenshot payload")
                            .to_string()
                    };

                    let first = capture("live-egui-current-1");
                    assert!(first.contains(r#""admission":"current""#), "{first}");
                    assert!(first.contains(r#""kind":"current""#), "{first}");
                    assert!(
                        first.contains("direct_acquired_surface_texture_copy"),
                        "{first}"
                    );
                    assert!(
                        first.contains("presented-pixels/direct-surface-copy"),
                        "{first}"
                    );
                    assert!(!first.contains("screenshot-frame-mismatch"), "{first}");
                    let first_frame = json_object_after(&first, r#""captured_frame":"#).to_string();
                    let first_index = json_u64_after(&first_frame, r#""frame_index":"#);

                    thread::sleep(Duration::from_millis(300));
                    let second = capture("live-egui-current-2");
                    let second_frame =
                        json_object_after(&second, r#""captured_frame":"#).to_string();
                    let second_index = json_u64_after(&second_frame, r#""frame_index":"#);
                    assert!(second.contains(r#""admission":"current""#), "{second}");
                    assert!(
                        second.contains("direct_acquired_surface_texture_copy"),
                        "{second}"
                    );
                    assert!(
                        second_index > first_index,
                        "{first_frame} then {second_frame}"
                    );

                    thread::sleep(Duration::from_millis(300));
                    for (id, forged) in [
                        ("live-egui-stale-exact", false),
                        ("live-egui-stale-forged", true),
                    ] {
                        let response = client
                            .submit(live_screenshot_object_message(id, &first_frame, forged))
                            .expect("submit stale exact screenshot")
                            .recv()
                            .expect("stale screenshot response channel")
                            .expect("stale screenshot response");
                        let payload = response["result"]["content"][0]["text"]
                            .as_str()
                            .expect("stale screenshot payload");
                        assert!(payload.contains(r#""admission":"exact""#), "{payload}");
                        assert!(payload.contains("screenshot-frame-mismatch"), "{payload}");
                    }
                }));
                std::process::exit(if outcome.is_ok() { 0 } else { 101 });
            });

            let mut event_loop_builder =
                winit::event_loop::EventLoop::<eframe::UserEvent>::with_user_event();
            #[cfg(target_os = "windows")]
            {
                use winit::platform::windows::EventLoopBuilderExtWindows as _;
                event_loop_builder.with_any_thread(true);
            }
            #[cfg(all(unix, not(target_os = "macos")))]
            {
                use winit::platform::x11::EventLoopBuilderExtX11 as _;
                event_loop_builder.with_any_thread(true);
            }
            let event_loop = event_loop_builder.build().expect("live egui event loop");
            event_loop.set_control_flow(winit::event_loop::ControlFlow::Wait);
            app.install_native_debug_proxy(eframe::NativeDebugProxy::new(&event_loop));
            app.mark_native_create_started();
            let mut eframe_app = eframe::create_native(
                "Step 223 live egui acquired-surface capture",
                eframe::NativeOptions {
                    renderer: eframe::Renderer::Wgpu,
                    ..Default::default()
                },
                Box::new(move |creation_context| {
                    let mut app = app;
                    app.record_native_create_phase();
                    app.prewarm_native_visible_cache(&creation_context.egui_ctx);
                    Ok(Box::new(app))
                }),
                &event_loop,
            );
            let result = event_loop.run_app(&mut eframe_app);
            panic!("live egui event loop exited before acceptance completed: {result:?}");
        }

        let mut child = Command::new(std::env::current_exe().expect("current test executable"))
            .arg("--ignored")
            .arg("--exact")
            .arg("tests::step223_live_egui_acquired_surface_capture")
            .env(CHILD_ENV, "1")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("launch live egui child process");
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(status) = child.try_wait().expect("poll live egui child") {
                assert!(status.success(), "live egui child failed with {status}");
                break;
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                panic!("live egui acceptance exceeded 30 seconds");
            }
            thread::sleep(Duration::from_millis(50));
        }
    }

    #[cfg(any())]
    #[test]
    fn step223_post_present_and_composition_anti_revert_source_guard() {
        let lib = include_str!("lib.rs");
        let runner = include_str!("native_runner.rs");
        assert!(lib.contains("raw_input_hook_with_slipway_debug"));
        assert!(lib.contains("try_send_input(egui::ViewportId::ROOT"));
        assert!(lib.contains("try_send_capture(egui::ViewportId::ROOT"));
        assert!(lib.contains("DirectAcquiredSurfaceTextureCopy"));
        assert!(lib.contains("selector: PresentedScreenshotSelector"));
        assert!(lib.contains("PresentedScreenshotSelector::Current { .. } => true"));
        assert!(lib.contains("egui_capture_frame_has_candidate_provenance"));
        assert!(lib.contains("screenshot-surface-acquire-failed"));
        assert!(lib.contains("last_successfully_presented: None"));
        assert!(lib.contains("rendered_frame_candidate = Some(frame_seed)"));
        assert!(
            !lib.contains(
                &[
                    "let last_successfully_presented = Some(runtime",
                    "last_frame_identity())"
                ]
                .join(".")
            )
        );
        assert!(
            !lib.contains(
                &[
                    "self.last_successfully_presented = Some(self.runtime",
                    "last_frame_identity())"
                ]
                .join(".")
            )
        );
        assert!(!lib.contains(&["event_index: native_phases", "len()"].join(".")));
        assert!(lib.contains("Renderer::Wgpu"));
        assert!(!lib.contains(&["raw_input.events", "extend"].join(".")));
        assert!(!lib.contains(&["request", "expected_frame"].join(".")));
        assert!(!lib.contains(&["requested", "frame:"].join("_")));
        assert!(!lib.contains(&["screenshot", "backend", "unsupported"].join("-")));
        assert!(!lib.contains(&["composition", "lifecycle", "unsupported"].join("-")));
        assert!(!lib.contains(&["Mu", "tex"].concat()));
        assert!(!lib.contains(&["Rw", "Lock"].concat()));
        assert!(!runner.contains(&["un", "safe"].concat()));
    }
}
