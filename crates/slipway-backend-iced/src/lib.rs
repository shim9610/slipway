use slipway_core::{
    BackendCapabilityReport, BackendInputEvent, BackendParityAdmission, BackendVisibleCapability,
    BackendVisibleCapabilityRequirement, BaselineShift, Capability, CapabilityProfileKind,
    CursorCapability, DeclaredEventDispatchKind, Diagnostic, EventOutcome, EvidenceSource,
    FocusRegionDeclaration, FocusTraversalMember, FontStyle, FontWeight, FrameIdentity,
    HitRegionDeclaration, HitRegionOrder, HitTestInput, InputEvent, LayoutConstraints, LayoutInput,
    LayoutIntentProbe, LayoutOutput, PaintLayerKey, PaintOp, PaintUnit, PathCommand,
    PathDeclaration, Point, PresentationGeometryIndex, PresentationRegionId, ProbeCollector,
    ProbeProduct, ProviderHitTestEvidence, ProviderSnapshotEvidence, ProviderSnapshotRequest,
    ProviderSurfaceKind, ProviderSurfaceRequest, Rect, RenderSurfaceDeclaration, ScrollEvent,
    ScrollRegionDeclaration, ShapeDeclaration, ShapeKind, Size, SlipwayAuthoredWidget,
    SlipwayBackendCapabilityProbe, SlipwayBackendParityAdmission, SlipwayCanvasProvider,
    SlipwayEventDispositionPolicy, SlipwayEventRoutingPolicy, SlipwayGpuSurfaceProvider,
    SlipwayLayoutIntent, SlipwayLogic, SlipwayMediaProvider, SlipwayPlotProvider,
    SlipwayProviderHitTestPolicy, SlipwayProviderSnapshotPolicy, SlipwayRenderSurfaces,
    SlipwayScrollableContainerCapability, SlipwaySsot, SlipwayTextInputCapability,
    SlipwayUnsupportedCapabilityEvidence, SlipwayView, SlipwayViewDefinition, SlipwayWidget,
    SlipwayWidgetTypes, StateObservation, TargetLocalRect, TextEditKind, TextLineMode,
    TextMeasurementEvidence, TextStyle, TopologyNode, UnsupportedCapabilityEvidence,
    ViewDefinition, ViewDefinitionInput, WidgetId, WidgetSlot, WidgetSlotAddress,
    expand_paint_unit_layers, paint_unit_sort_key, scroll_region_from_scrollable_capability,
    text_edit_focus_region_from_capability, view_definition_contract_diagnostics_for_capabilities,
    view_definition_has_blocking_contract_diagnostic,
};
use slipway_debug_bridge::{
    DebugCommand, DebugCommandKind, DebugFailure, DebugPhysicalControlDeclarationSelector,
    DebugReplyProduct,
};
use slipway_runtime::{
    SlipwayAssembledApp, SlipwayDebugMcpAttachment, SlipwayRuntime, SlipwayRuntimeConfig,
    SlipwayRuntimeDrainBudget, SlipwayRuntimeMcpTransport, SlipwayRuntimeMcpWakeReceiver,
};
use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::hash::{Hash, Hasher};
use std::net::SocketAddr;
use std::sync::Arc;

mod native_runner;

#[derive(Clone, Debug, PartialEq)]
pub struct IcedLayoutContext {
    pub viewport: Rect,
    pub constraints: LayoutConstraints,
    pub scale_factor: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IcedAdapterFrame {
    pub widget_id: WidgetId,
    pub layout: LayoutOutput,
    pub paint: Vec<PaintOp>,
    pub capabilities: Vec<Capability>,
    pub diagnostics: Vec<Diagnostic>,
    pub layout_intent: Option<LayoutIntentProbe>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IcedObservation {
    pub widget_id: WidgetId,
    pub topology: TopologyNode,
    pub state: Vec<StateObservation>,
    pub diagnostics: Vec<Diagnostic>,
    pub layout_intent: Option<LayoutIntentProbe>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IcedEventReceipt<M> {
    pub widget_id: WidgetId,
    pub outcome: EventOutcome<M>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcedVisibleLaunchStatus {
    AdapterBoundaryOnly,
}

pub const ICED_BACKEND_ID: &str = "slipway-backend-iced";
const ICED_OVERLAY_FORWARDING_REQUIREMENT: &str = "iced.lifecycle.overlay.forwarding";
const ICED_OVERLAY_MISSING_CONTRACT_REASON: &str = "iced Widget::overlay forwarding requires a stable authored overlay child/surface content contract, tree identity, layout, event routing, and a lifetime path suitable for iced overlay(); current core declarations expose overlay metadata only, so final visible lifecycle acceptance remains blocked";
const ICED_STANDALONE_CHILD_LIFECYCLE_REFUSAL: &str = "standalone SlipwayIcedWidget paths cannot host child-bearing widgets through official iced child lifecycle; use SlipwayRuntime with iced_runtime_widget instead";
const ICED_PROVIDER_SURFACE_REQUIREMENT: &str = "iced.provider_surface.native_wrapper";

/// Iced-side type contract for backends that can accept Slipway-authored
/// text-input styles.
///
/// This contract does not authorize reading iced theme defaults as Slipway
/// style data. It only proves that the iced `Theme::Class` type can carry a
/// style function whose returned values are built from Slipway declarations.
pub trait SlipwayIcedThemeContract:
    iced::widget::text::Catalog
    + iced::widget::text_input::Catalog
    + iced::widget::text_editor::Catalog
    + iced::widget::scrollable::Catalog
{
    fn slipway_text_input_class<'a>(
        style: iced::widget::text_input::StyleFn<'a, Self>,
    ) -> <Self as iced::widget::text_input::Catalog>::Class<'a>;
}

impl<T> SlipwayIcedThemeContract for T
where
    T: iced::widget::text::Catalog
        + iced::widget::text_input::Catalog
        + iced::widget::text_editor::Catalog
        + iced::widget::scrollable::Catalog,
    for<'a> <T as iced::widget::text_input::Catalog>::Class<'a>:
        From<iced::widget::text_input::StyleFn<'a, T>>,
{
    fn slipway_text_input_class<'a>(
        style: iced::widget::text_input::StyleFn<'a, Self>,
    ) -> <Self as iced::widget::text_input::Catalog>::Class<'a> {
        style.into()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum IcedChildTraversalOrder {
    BackToFront,
    FrontToBack,
}

#[derive(Clone, Debug, Default)]
pub struct IcedBackendAdmission;

pub fn iced_backend_admission() -> IcedBackendAdmission {
    IcedBackendAdmission
}

impl IcedBackendAdmission {
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
                backend_id: ICED_BACKEND_ID.to_string(),
                target: Some(view.target.clone()),
                capability: Capability::CapabilityAdmission,
                visible_capability: Some(BackendVisibleCapability::Custom(
                    "view_contract".to_string(),
                )),
                requirement_id: Some("view.contract".to_string()),
                reason: "view definition contract diagnostics contain blocking errors".to_string(),
                source: EvidenceSource::backend_presented(ICED_BACKEND_ID, "visible-admission"),
                diagnostics: contract_diagnostics.clone(),
            });
        }

        if view
            .hit_regions
            .iter()
            .any(|region| region.capture != slipway_core::PointerCaptureIntent::None)
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
            for scroll in &view.scroll_regions {
                if scroll.enabled && !scroll_region_has_real_child_ui(view, scroll) {
                    unsupported.push(scroll_without_real_child_refusal(scroll));
                }
            }
        }

        let paint_diagnostics = unsupported_visible_paint_diagnostics(&view.target, &view.paint);
        if !paint_diagnostics.is_empty() {
            push_view_requirement(
                &mut visible_requirements,
                "view.shape_path_clip",
                Some(view.target.clone()),
                BackendVisibleCapability::ShapePathClip,
            );
            unsupported.push(UnsupportedCapabilityEvidence {
                backend_id: ICED_BACKEND_ID.to_string(),
                target: Some(view.target.clone()),
                capability: Capability::Paint,
                visible_capability: Some(BackendVisibleCapability::ShapePathClip),
                requirement_id: Some("view.shape_path_clip".to_string()),
                reason:
                    "iced backend refuses visible shape/path/clip declarations it cannot present"
                        .to_string(),
                source: EvidenceSource::backend_presented(ICED_BACKEND_ID, "visible-admission"),
                diagnostics: paint_diagnostics,
            });
        }

        if view
            .paint
            .iter()
            .any(|op| paint_op_requires_font_installation(op))
        {
            push_view_requirement(
                &mut visible_requirements,
                "view.font_resource_installation",
                Some(view.target.clone()),
                BackendVisibleCapability::FontInstallation,
            );
            unsupported.push(unsupported_visible_capability(
                Some(view.target.clone()),
                Capability::ResourceResolutionPolicy,
                BackendVisibleCapability::FontInstallation,
                "view.font_resource_installation",
                "iced backend maps font descriptors but does not install declared font resources",
            ));
        }

        BackendParityAdmission {
            backend_id: ICED_BACKEND_ID.to_string(),
            accepted: unsupported.is_empty(),
            required_profiles: Vec::new(),
            visible_requirements,
            unsupported,
            source: EvidenceSource::backend_presented(ICED_BACKEND_ID, "visible-admission"),
            diagnostics: contract_diagnostics,
        }
    }
}

impl SlipwayBackendCapabilityProbe for IcedBackendAdmission {
    fn backend_capabilities(&self) -> BackendCapabilityReport {
        BackendCapabilityReport {
            backend_id: ICED_BACKEND_ID.to_string(),
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
                BackendVisibleCapability::BackendPresentedEvidence,
                iced_provider_surface_visible_capability(),
            ],
        }
    }
}

impl SlipwayUnsupportedCapabilityEvidence for IcedBackendAdmission {
    fn unsupported_capabilities(
        &self,
        required: &[Capability],
    ) -> Vec<UnsupportedCapabilityEvidence> {
        let report = self.backend_capabilities();
        required
            .iter()
            .filter(|capability| !report.capabilities.iter().any(|owned| owned == *capability))
            .map(|capability| UnsupportedCapabilityEvidence {
                backend_id: ICED_BACKEND_ID.to_string(),
                target: None,
                capability: capability.clone(),
                visible_capability: None,
                requirement_id: Some(format!("capability::{capability:?}")),
                reason: "capability is not declared by the iced visible backend".to_string(),
                source: EvidenceSource::backend_presented(ICED_BACKEND_ID, "capability-admission"),
                diagnostics: Vec::new(),
            })
            .collect()
    }
}

impl SlipwayBackendParityAdmission for IcedBackendAdmission {
    fn backend_parity_admission(
        &self,
        required_profiles: &[CapabilityProfileKind],
    ) -> BackendParityAdmission {
        backend_profile_admission(
            ICED_BACKEND_ID,
            &self.backend_capabilities(),
            required_profiles,
        )
    }
}

pub trait SlipwayIcedWidgetListVisitor<ExternalState, AppMessage> {
    fn set_iced_child_order_index(&mut self, _index: usize) {}

    fn visit_iced_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayIcedBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>;

    fn visit_iced_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayIcedNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>;
}

#[derive(Clone, Copy, Debug)]
pub struct IcedNativeWidgetContext<'a> {
    pub slot: &'a WidgetSlotAddress,
    pub frame_seed: Option<&'a FrameIdentity>,
    pub placement: Option<Rect>,
}

pub trait SlipwayIcedNativeChildWidget: SlipwayWidget + SlipwayViewDefinition {
    fn iced_native_element<'a, Theme, Renderer>(
        &'a self,
        external: &'a Self::ExternalState,
        local: &'a Self::LocalState,
        context: IcedNativeWidgetContext<'a>,
    ) -> iced::Element<'a, SlipwayIcedRuntimeMessage<Self::AppMessage>, Theme, Renderer>
    where
        Self::AppMessage: Clone + 'a,
        Theme: SlipwayIcedThemeContract + 'a,
        Renderer: iced::advanced::text::Renderer<Font = iced::Font>
            + iced::advanced::graphics::geometry::Renderer
            + 'a + 'static;
}

pub trait SlipwayIcedNativeWidgetSpec: SlipwayWidgetTypes {
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
    ) -> LayoutOutput {
        LayoutOutput {
            bounds: input.viewport,
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn observe_state(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        Vec::new()
    }

    fn iced_native_element<'a, Theme, Renderer>(
        &'a self,
        external: &'a Self::ExternalState,
        local: &'a Self::LocalState,
        context: IcedNativeWidgetContext<'a>,
    ) -> iced::Element<'a, SlipwayIcedRuntimeMessage<Self::AppMessage>, Theme, Renderer>
    where
        Self::AppMessage: Clone + 'a,
        Theme: SlipwayIcedThemeContract + 'a,
        Renderer: iced::advanced::text::Renderer<Font = iced::Font>
            + iced::advanced::graphics::geometry::Renderer
            + 'a + 'static;
}

pub trait SlipwayIcedProviderSurfaceSpec: SlipwayIcedNativeWidgetSpec {
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
            "iced.provider_surface.hit_test_unsupported",
            "iced provider surface did not implement backend-specific provider_hit_test",
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
            "iced.provider_surface.snapshot_unsupported",
            "iced provider surface did not implement backend-specific provider_snapshot",
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

pub struct IcedGpuSurfacePrepareContext<'a> {
    pub target: &'a WidgetId,
    pub provider_id: &'a str,
    pub frame: Option<&'a FrameIdentity>,
    pub bounds: Rect,
    pub viewport: Rect,
    pub device: &'a iced::wgpu::Device,
    pub queue: &'a iced::wgpu::Queue,
    pub format: iced::wgpu::TextureFormat,
}

pub struct IcedGpuSurfacePaintContext<'a, 'pass, Prepared> {
    pub target: &'a WidgetId,
    pub provider_id: &'a str,
    pub frame: Option<&'a FrameIdentity>,
    pub bounds: Rect,
    pub viewport: Rect,
    pub prepared: &'a Prepared,
    pub render_pass: &'a mut iced::wgpu::RenderPass<'pass>,
}

pub trait SlipwayIcedSplitGpuProviderSurfaceSpec: SlipwayIcedProviderSurfaceSpec {
    type PreparedFrame;

    fn prepare_iced_gpu_surface(
        &mut self,
        context: IcedGpuSurfacePrepareContext<'_>,
    ) -> Result<Self::PreparedFrame, Vec<Diagnostic>>;

    fn paint_prepared_iced_gpu_surface(
        &self,
        context: IcedGpuSurfacePaintContext<'_, '_, Self::PreparedFrame>,
    ) -> Result<(), Vec<Diagnostic>>;
}

pub trait SlipwayIcedSplitGpuSurfaceProvider: SlipwayGpuSurfaceProvider {
    type PreparedFrame;

    fn prepare_iced_gpu_surface(
        &mut self,
        context: IcedGpuSurfacePrepareContext<'_>,
    ) -> Result<Self::PreparedFrame, Vec<Diagnostic>>;

    fn paint_prepared_iced_gpu_surface(
        &self,
        context: IcedGpuSurfacePaintContext<'_, '_, Self::PreparedFrame>,
    ) -> Result<(), Vec<Diagnostic>>;
}

#[derive(Clone, Debug)]
pub struct SlipwayIcedNativeWidget<N> {
    native: N,
}

impl<N> SlipwayIcedNativeWidget<N> {
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

impl<N> SlipwayWidgetTypes for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedNativeWidgetSpec,
{
    type ExternalState = N::ExternalState;
    type LocalState = N::LocalState;
    type AppMessage = N::AppMessage;
}

impl<N> SlipwaySsot for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedNativeWidgetSpec,
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

impl<N> SlipwayLogic for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedNativeWidgetSpec,
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

impl<N> SlipwayView for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedNativeWidgetSpec,
{
    fn initial_local_state(&self) -> Self::LocalState {
        self.native.initial_local_state()
    }

    fn layout(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: LayoutInput,
    ) -> LayoutOutput {
        self.native.layout(external, local, input)
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

impl<N> SlipwayViewDefinition for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedNativeWidgetSpec,
{
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let layout = self.layout(external, local, input.layout_input);
        ViewDefinition {
            target: self.id(),
            frame: input.frame,
            layout,
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: self.unsupported(),
        }
    }
}

impl<N> SlipwayIcedNativeChildWidget for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedNativeWidgetSpec,
{
    fn iced_native_element<'a, Theme, Renderer>(
        &'a self,
        external: &'a Self::ExternalState,
        local: &'a Self::LocalState,
        context: IcedNativeWidgetContext<'a>,
    ) -> iced::Element<'a, SlipwayIcedRuntimeMessage<Self::AppMessage>, Theme, Renderer>
    where
        Self::AppMessage: Clone + 'a,
        Theme: SlipwayIcedThemeContract + 'a,
        Renderer: iced::advanced::text::Renderer<Font = iced::Font>
            + iced::advanced::graphics::geometry::Renderer
            + 'a + 'static,
    {
        self.native.iced_native_element(external, local, context)
    }
}

impl<N> SlipwayRenderSurfaces for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedProviderSurfaceSpec,
{
    fn render_surfaces(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<RenderSurfaceDeclaration> {
        vec![self.native.render_surface_declaration(external, local)]
    }
}

impl<N> SlipwayCanvasProvider for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedProviderSurfaceSpec,
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

impl<N> SlipwayGpuSurfaceProvider for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedProviderSurfaceSpec,
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

impl<N> SlipwayIcedSplitGpuSurfaceProvider for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedSplitGpuProviderSurfaceSpec,
{
    type PreparedFrame = N::PreparedFrame;

    fn prepare_iced_gpu_surface(
        &mut self,
        context: IcedGpuSurfacePrepareContext<'_>,
    ) -> Result<Self::PreparedFrame, Vec<Diagnostic>> {
        self.native.prepare_iced_gpu_surface(context)
    }

    fn paint_prepared_iced_gpu_surface(
        &self,
        context: IcedGpuSurfacePaintContext<'_, '_, Self::PreparedFrame>,
    ) -> Result<(), Vec<Diagnostic>> {
        self.native.paint_prepared_iced_gpu_surface(context)
    }
}

impl<N> SlipwayMediaProvider for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedProviderSurfaceSpec,
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

impl<N> SlipwayPlotProvider for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedProviderSurfaceSpec,
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

impl<N> SlipwayProviderHitTestPolicy for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedProviderSurfaceSpec,
{
    fn provider_hit_test(&self, request: HitTestInput) -> ProviderHitTestEvidence {
        self.native.provider_hit_test(request)
    }
}

impl<N> SlipwayProviderSnapshotPolicy for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedProviderSurfaceSpec,
{
    fn provider_snapshot(&mut self, request: ProviderSnapshotRequest) -> ProviderSnapshotEvidence {
        self.native.provider_snapshot(request)
    }
}

pub trait SlipwayIcedWidgetListEntry: SlipwayWidget + SlipwayViewDefinition {
    fn visit_iced_entry<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        slot: WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>;
}

impl<W> SlipwayIcedWidgetListEntry for W
where
    W: SlipwayIcedBackendChildWidget,
{
    fn visit_iced_entry<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        slot: WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        visitor.visit_iced_child(self, external, local, slot);
    }
}

impl<N> SlipwayIcedWidgetListEntry for SlipwayIcedNativeWidget<N>
where
    N: SlipwayIcedNativeWidgetSpec,
{
    fn visit_iced_entry<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        slot: WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        visitor.visit_iced_native_child(self, external, local, slot);
    }
}

pub trait SlipwayIcedWidgetList {
    type ExternalState;
    type LocalState;
    type AppMessage;

    fn visit_iced_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>;

    fn visit_iced_children_in_paint_order<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        frame: &FrameIdentity,
        placements: &[slipway_core::ChildPlacement],
        traversal_order: IcedChildTraversalOrder,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>;
}

impl SlipwayIcedWidgetList for () {
    type ExternalState = ();
    type LocalState = ();
    type AppMessage = ();

    fn visit_iced_children<V>(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
    }

    fn visit_iced_children_in_paint_order<V>(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _frame: &FrameIdentity,
        _placements: &[slipway_core::ChildPlacement],
        _traversal_order: IcedChildTraversalOrder,
        _visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
    }
}

macro_rules! impl_iced_widget_list_tuple {
    ($first:ident $first_index:tt $(, $widget:ident $index:tt)*) => {
        impl<$first $(, $widget)*> SlipwayIcedWidgetList for ($first, $($widget,)*)
        where
            $first: SlipwayIcedWidgetListEntry,
            $(
                $widget: SlipwayIcedWidgetListEntry<
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

            fn visit_iced_children<V>(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                visitor: &mut V,
            ) where
                V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
            {
                self.$first_index.visit_iced_entry(
                    external,
                    &local.$first_index,
                    parent_slot.child(self.$first_index.id(), $first_index),
                    visitor,
                );
                $(
                    self.$index.visit_iced_entry(
                        external,
                        &local.$index,
                        parent_slot.child(self.$index.id(), $index),
                        visitor,
                    );
                )*
            }

            fn visit_iced_children_in_paint_order<V>(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                frame: &FrameIdentity,
                placements: &[slipway_core::ChildPlacement],
                traversal_order: IcedChildTraversalOrder,
                visitor: &mut V,
            ) where
                V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
            {
                let mut order = vec![(
                    iced_child_paint_sort_key(
                        &self.$first_index,
                        external,
                        &local.$first_index,
                        parent_slot,
                        $first_index,
                        frame,
                        placements,
                    ),
                    $first_index,
                )];
                $(
                    order.push((
                        iced_child_paint_sort_key(
                            &self.$index,
                            external,
                            &local.$index,
                            parent_slot,
                            $index,
                            frame,
                            placements,
                        ),
                        $index,
                    ));
                )*
                order.sort_by_key(|(key, _)| *key);

                match traversal_order {
                    IcedChildTraversalOrder::BackToFront => {
                        for (order_index, (_, source_index)) in order.iter().enumerate() {
                            visitor.set_iced_child_order_index(order_index);
                            match *source_index {
                                $first_index => self.$first_index.visit_iced_entry(
                                    external,
                                    &local.$first_index,
                                    parent_slot.child(self.$first_index.id(), $first_index),
                                    visitor,
                                ),
                                $(
                                    $index => self.$index.visit_iced_entry(
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
                    IcedChildTraversalOrder::FrontToBack => {
                        for (order_index, (_, source_index)) in order.iter().enumerate().rev() {
                            visitor.set_iced_child_order_index(order_index);
                            match *source_index {
                                $first_index => self.$first_index.visit_iced_entry(
                                    external,
                                    &local.$first_index,
                                    parent_slot.child(self.$first_index.id(), $first_index),
                                    visitor,
                                ),
                                $(
                                    $index => self.$index.visit_iced_entry(
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
            }
        }
    };
}

impl_iced_widget_list_tuple!(A 0);
impl_iced_widget_list_tuple!(A 0, B 1);
impl_iced_widget_list_tuple!(A 0, B 1, C 2);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13, O 14);
impl_iced_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13, O 14, P 15);

pub trait SlipwayIcedAuthoredChildren: SlipwayAuthoredWidget {
    fn visit_iced_authored_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>;

    fn visit_iced_authored_children_in_paint_order<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        _frame: &FrameIdentity,
        _placements: &[slipway_core::ChildPlacement],
        _traversal_order: IcedChildTraversalOrder,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        self.visit_iced_authored_children(external, local, visitor);
    }
}

pub trait SlipwayIcedBackendContract:
    SlipwayWidget
    + SlipwayViewDefinition
    + SlipwayEventRoutingPolicy
    + SlipwayEventDispositionPolicy
    + SlipwayIcedAuthoredChildren
{
}

impl<W> SlipwayIcedBackendContract for W where
    W: SlipwayWidget
        + SlipwayViewDefinition
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy
        + SlipwayIcedAuthoredChildren
{
}

impl<A> SlipwayIcedAuthoredChildren for slipway_core::SlipwayAppWidget<A>
where
    A: slipway_core::SlipwayApp,
    A::Widgets: SlipwayIcedWidgetList<
            ExternalState = A::ExternalState,
            LocalState = <A::Widgets as slipway_core::SlipwayWidgetList>::LocalState,
            AppMessage = A::AppMessage,
        >,
{
    fn visit_iced_authored_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        self.app
            .widgets()
            .visit_iced_children(external, &local.widgets, &root_slot, visitor);
    }

    fn visit_iced_authored_children_in_paint_order<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        frame: &FrameIdentity,
        placements: &[slipway_core::ChildPlacement],
        traversal_order: IcedChildTraversalOrder,
        visitor: &mut V,
    ) where
        V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        self.app.widgets().visit_iced_children_in_paint_order(
            external,
            &local.widgets,
            &root_slot,
            frame,
            placements,
            traversal_order,
            visitor,
        );
    }
}

pub trait SlipwayIcedBackendChildWidget: SlipwayIcedBackendContract {}

impl<W> SlipwayIcedBackendChildWidget for W where W: SlipwayIcedBackendContract {}

pub trait SlipwayIcedTextInputBackendWidget:
    SlipwayIcedBackendChildWidget + SlipwayTextInputCapability
{
}

impl<W> SlipwayIcedTextInputBackendWidget for W where
    W: SlipwayIcedBackendChildWidget + SlipwayTextInputCapability
{
}

pub trait SlipwayIcedScrollableContainerBackendWidget:
    SlipwayIcedBackendChildWidget + SlipwayScrollableContainerCapability
{
}

impl<W> SlipwayIcedScrollableContainerBackendWidget for W where
    W: SlipwayIcedBackendChildWidget + SlipwayScrollableContainerCapability
{
}

#[allow(clippy::too_many_arguments)]
pub fn iced_text_edit_focus_region_from_capability<W>(
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
    W: SlipwayIcedTextInputBackendWidget,
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

pub fn iced_scroll_region_from_capability<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    layout: &LayoutOutput,
    region_id: Option<PresentationRegionId>,
    address: Option<WidgetSlotAddress>,
    enabled: bool,
) -> ScrollRegionDeclaration
where
    W: SlipwayIcedScrollableContainerBackendWidget,
{
    scroll_region_from_scrollable_capability(
        widget, external, local, layout, region_id, address, enabled,
    )
}

pub trait SlipwayIcedBackendWidget: SlipwayAuthoredWidget + SlipwayIcedBackendChildWidget {}

impl<W> SlipwayIcedBackendWidget for W where W: SlipwayAuthoredWidget + SlipwayIcedBackendChildWidget
{}

pub trait SlipwayIcedLayoutIntentBackendWidget:
    SlipwayIcedBackendWidget + SlipwayLayoutIntent
{
}

impl<W> SlipwayIcedLayoutIntentBackendWidget for W where
    W: SlipwayIcedBackendWidget + SlipwayLayoutIntent
{
}

pub trait SlipwayIcedLayoutIntentBackendChildWidget:
    SlipwayIcedBackendChildWidget + SlipwayLayoutIntent
{
}

impl<W> SlipwayIcedLayoutIntentBackendChildWidget for W where
    W: SlipwayIcedBackendChildWidget + SlipwayLayoutIntent
{
}

pub trait IcedSlipwayBridge<W: SlipwayAuthoredWidget> {
    fn layout_input(&mut self, context: IcedLayoutContext) -> LayoutInput;
    fn input_events(&mut self, widget_id: WidgetId) -> Vec<BackendInputEvent>;
    fn wants_observation(&self) -> bool {
        false
    }
    fn observe(&mut self, _observation: IcedObservation) {}
}

#[derive(Clone, Debug, Default)]
pub struct DefaultIcedBridge {
    events: Vec<BackendInputEvent>,
    probes: ProbeCollector,
    observe_next: bool,
}

impl DefaultIcedBridge {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn queue_event(&mut self, event: InputEvent) {
        self.events.push(BackendInputEvent::direct(event));
    }

    pub fn queue_backend_input_event(&mut self, event: BackendInputEvent) {
        self.events.push(event);
    }

    pub fn request_observation(&mut self) {
        self.observe_next = true;
    }

    pub fn take_probes(&mut self) -> Vec<ProbeProduct> {
        self.probes.take()
    }
}

impl<W: SlipwayAuthoredWidget> IcedSlipwayBridge<W> for DefaultIcedBridge {
    fn layout_input(&mut self, context: IcedLayoutContext) -> LayoutInput {
        let _scale_factor = context.scale_factor;
        LayoutInput {
            viewport: TargetLocalRect::new(context.viewport),
            constraints: context.constraints,
        }
    }

    fn input_events(&mut self, widget_id: WidgetId) -> Vec<BackendInputEvent> {
        let mut routed = Vec::new();
        let mut remaining = Vec::new();
        for event in self.events.drain(..) {
            if event.event.target() == &widget_id {
                routed.push(event);
            } else {
                remaining.push(event);
            }
        }
        self.events = remaining;
        routed
    }

    fn wants_observation(&self) -> bool {
        self.observe_next
    }

    fn observe(&mut self, observation: IcedObservation) {
        self.observe_next = false;
        let traversal = observation.topology.traverse_depth_first();
        self.probes
            .push(ProbeProduct::Topology(slipway_core::TopologyProbe {
                root: observation.topology,
                traversal,
            }));
        self.probes
            .push(ProbeProduct::State(slipway_core::StateProbe {
                target: observation.widget_id,
                observations: observation.state,
            }));
        if let Some(layout_intent) = observation.layout_intent {
            self.probes.push(ProbeProduct::LayoutIntent(layout_intent));
        }
        self.probes.extend(
            observation
                .diagnostics
                .into_iter()
                .map(ProbeProduct::Diagnostic),
        );
    }
}

pub struct IcedWidgetAdapter<W: SlipwayAuthoredWidget> {
    slot: WidgetSlot<W>,
}

impl<W: SlipwayAuthoredWidget> IcedWidgetAdapter<W> {
    pub fn new(widget: W) -> Self {
        Self {
            slot: WidgetSlot::new(widget),
        }
    }

    pub fn widget_id(&self) -> WidgetId {
        self.slot.widget.id()
    }

    pub fn local_state(&self) -> &W::LocalState {
        &self.slot.local_state
    }

    pub fn render_frame<B: IcedSlipwayBridge<W>>(
        &mut self,
        external: &W::ExternalState,
        bridge: &mut B,
        context: IcedLayoutContext,
    ) -> IcedAdapterFrame {
        let layout_input = bridge.layout_input(context);
        let layout = self
            .slot
            .widget
            .layout(external, &self.slot.local_state, layout_input);
        let paint = self
            .slot
            .widget
            .paint(external, &self.slot.local_state, &layout);

        IcedAdapterFrame {
            widget_id: self.widget_id(),
            layout,
            paint,
            capabilities: self.slot.widget.capabilities(),
            diagnostics: self.slot.widget.unsupported(),
            layout_intent: None,
        }
    }

    pub fn route_events<B: IcedSlipwayBridge<W>>(
        &mut self,
        external: &W::ExternalState,
        bridge: &mut B,
        _context: IcedLayoutContext,
        _frame: FrameIdentity,
    ) -> Vec<IcedEventReceipt<W::AppMessage>>
    where
        W: slipway_core::SlipwayEventRoutingPolicy
            + slipway_core::SlipwayEventDispositionPolicy
            + SlipwayViewDefinition,
    {
        let widget_id = self.widget_id();
        let mut receipts = Vec::new();
        for backend_event in bridge.input_events(widget_id.clone()) {
            let event = backend_event.event.clone();
            let declaration = slipway_core::declared_event_handling(
                &self.slot.widget,
                external,
                &self.slot.local_state,
                &event,
            );
            let outcome = if declaration.disposition.final_disposition.handled {
                let raw_outcome =
                    self.slot
                        .widget
                        .handle_event(external, &mut self.slot.local_state, event);
                let outcome = slipway_core::apply_physical_event_handling_declaration(
                    declaration,
                    raw_outcome,
                );
                outcome
            } else {
                slipway_core::refuse_event_declared_unhandled(declaration)
            };
            receipts.push(IcedEventReceipt {
                widget_id: widget_id.clone(),
                outcome,
            });
        }

        receipts
    }

    pub fn observe<B: IcedSlipwayBridge<W>>(
        &mut self,
        external: &W::ExternalState,
        bridge: &mut B,
    ) {
        if bridge.wants_observation() {
            bridge.observe(IcedObservation {
                widget_id: self.widget_id(),
                topology: self.slot.widget.topology(external),
                state: self
                    .slot
                    .widget
                    .observe_state(external, &self.slot.local_state),
                diagnostics: self.slot.widget.unsupported(),
                layout_intent: None,
            });
        }
    }
}

impl<W> IcedWidgetAdapter<W>
where
    W: SlipwayIcedLayoutIntentBackendWidget,
{
    pub fn render_frame_with_layout_intent<B: IcedSlipwayBridge<W>>(
        &mut self,
        external: &W::ExternalState,
        bridge: &mut B,
        context: IcedLayoutContext,
    ) -> IcedAdapterFrame {
        let layout_input = bridge.layout_input(context);
        let layout =
            self.slot
                .widget
                .layout(external, &self.slot.local_state, layout_input.clone());
        let paint = self
            .slot
            .widget
            .paint(external, &self.slot.local_state, &layout);
        let layout_intent =
            self.slot
                .widget
                .layout_intent(external, &self.slot.local_state, &layout_input);

        IcedAdapterFrame {
            widget_id: self.widget_id(),
            layout,
            paint,
            capabilities: self.slot.widget.capabilities(),
            diagnostics: self.slot.widget.unsupported(),
            layout_intent: Some(layout_intent),
        }
    }

    pub fn observe_with_layout_intent<B: IcedSlipwayBridge<W>>(
        &mut self,
        external: &W::ExternalState,
        bridge: &mut B,
        input: &LayoutInput,
    ) {
        if bridge.wants_observation() {
            bridge.observe(IcedObservation {
                widget_id: self.widget_id(),
                topology: self.slot.widget.topology(external),
                state: self
                    .slot
                    .widget
                    .observe_state(external, &self.slot.local_state),
                diagnostics: self.slot.widget.unsupported(),
                layout_intent: Some(self.slot.widget.layout_intent(
                    external,
                    &self.slot.local_state,
                    input,
                )),
            });
        }
    }
}

pub fn visible_launch_status() -> IcedVisibleLaunchStatus {
    IcedVisibleLaunchStatus::AdapterBoundaryOnly
}

pub fn iced_length_from_points(points: f32) -> iced::Length {
    iced::Length::Fixed(points.max(0.0))
}

pub fn iced_point(point: Point) -> iced::Point {
    iced::Point::new(point.x, point.y)
}

pub struct SlipwayIcedWidget<'a, W: SlipwayIcedBackendWidget> {
    widget: &'a W,
    external: &'a W::ExternalState,
    width: iced::Length,
    height: iced::Length,
}

pub struct SlipwayIcedLayoutIntentWidget<'a, W>
where
    W: SlipwayIcedLayoutIntentBackendWidget,
{
    widget: &'a W,
    external: &'a W::ExternalState,
    width: iced::Length,
    height: iced::Length,
    layout_intent_sink: Option<&'a mut Vec<LayoutIntentProbe>>,
}

pub struct SlipwayIcedRuntimeWidget<'a, W: SlipwayIcedBackendChildWidget> {
    widget: &'a W,
    external: &'a W::ExternalState,
    local: &'a W::LocalState,
    runtime_slot: Option<WidgetSlotAddress>,
    width: iced::Length,
    height: iced::Length,
    presented_viewport_sink: Option<&'a Cell<Rect>>,
    frame_seed: Option<FrameIdentity>,
    dirty_scopes: Option<Cow<'a, [IcedDirtyScope]>>,
}

pub struct SlipwayIcedRuntimeLayoutIntentWidget<'a, W>
where
    W: SlipwayIcedLayoutIntentBackendChildWidget,
{
    widget: &'a W,
    external: &'a W::ExternalState,
    local: &'a W::LocalState,
    runtime_slot: Option<WidgetSlotAddress>,
    width: iced::Length,
    height: iced::Length,
    layout_intent_sink: Option<&'a mut Vec<LayoutIntentProbe>>,
    frame_seed: Option<FrameIdentity>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IcedPreeditOverlayStyle {
    pub text_color: iced::Color,
    pub font: iced::Font,
    pub size: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SlipwayIcedRuntimeMessage<M> {
    BackendInput(BackendInputEvent),
    PreeditStyle(IcedPreeditOverlayStyle),
    App(M),
    DrainDebug,
    Noop,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SlipwayIcedRuntimeUpdate {
    Input {
        handled: bool,
        applied_messages: usize,
        diagnostics: Vec<Diagnostic>,
    },
    AppMessages {
        applied_messages: usize,
    },
    DrainDebug,
    Noop,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlipwayIcedRuntimeAppUpdate {
    pub runtime_update: Option<SlipwayIcedRuntimeUpdate>,
    pub debug_replies_drained: usize,
    pub debug_error: Option<String>,
}

pub struct SlipwayIcedRuntimeApp<W, F>
where
    W: SlipwayIcedBackendChildWidget,
{
    assembled: SlipwayAssembledApp<W>,
    apply_app_messages: F,
    debug_mcp_transport: Option<SlipwayRuntimeMcpTransport>,
    presented_viewport: Cell<Rect>,
}

impl<W, F> SlipwayIcedRuntimeApp<W, F>
where
    W: SlipwayIcedBackendWidget,
    W::LocalState: Clone,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
{
    pub fn new(assembled: SlipwayAssembledApp<W>, apply_app_messages: F) -> Self {
        let initial_viewport = assembled.runtime.last_frame_identity().viewport;
        let presented_viewport = Cell::new(initial_viewport);
        Self {
            assembled,
            apply_app_messages,
            debug_mcp_transport: None,
            presented_viewport,
        }
    }

    pub fn from_parts(widget: W, external: W::ExternalState, apply_app_messages: F) -> Self {
        Self::new(
            SlipwayAssembledApp::new(widget, external),
            apply_app_messages,
        )
    }

    pub fn assembled_app(&self) -> &SlipwayAssembledApp<W> {
        &self.assembled
    }

    pub fn assembled_app_mut(&mut self) -> &mut SlipwayAssembledApp<W> {
        &mut self.assembled
    }

    pub fn into_assembled_app(self) -> SlipwayAssembledApp<W> {
        self.assembled
    }

    pub fn runtime(&self) -> &SlipwayRuntime<W> {
        &self.assembled.runtime
    }

    pub fn runtime_mut(&mut self) -> &mut SlipwayRuntime<W> {
        &mut self.assembled.runtime
    }

    pub fn with_debug_mcp_transport(mut self, transport: SlipwayRuntimeMcpTransport) -> Self {
        self.debug_mcp_transport = Some(transport);
        self
    }

    pub fn title(&self) -> String {
        match self.debug_mcp_transport.as_ref() {
            Some(transport) => format!(
                "Slipway Backend Iced - Iced MCP: {}",
                transport.local_addr()
            ),
            None => "Slipway Backend Iced - Iced".to_string(),
        }
    }

    pub fn debug_mcp(&self) -> &SlipwayDebugMcpAttachment {
        &self.assembled.debug_mcp
    }

    pub fn update(
        &mut self,
        message: SlipwayIcedRuntimeMessage<W::AppMessage>,
    ) -> SlipwayIcedRuntimeAppUpdate {
        self.sync_presented_viewport();
        let (runtime_update, debug_replies_drained, debug_error) = match message {
            SlipwayIcedRuntimeMessage::DrainDebug => {
                let (drained, error) = self.drain_debug_pending();
                (Some(SlipwayIcedRuntimeUpdate::DrainDebug), drained, error)
            }
            SlipwayIcedRuntimeMessage::Noop => (Some(SlipwayIcedRuntimeUpdate::Noop), 0, None),
            message => (
                Some(apply_iced_runtime_message(
                    &mut self.assembled.runtime,
                    message,
                    &mut self.apply_app_messages,
                )),
                0,
                None,
            ),
        };

        SlipwayIcedRuntimeAppUpdate {
            runtime_update,
            debug_replies_drained,
            debug_error,
        }
    }

    pub fn update_without_debug_drain(
        &mut self,
        message: SlipwayIcedRuntimeMessage<W::AppMessage>,
    ) -> SlipwayIcedRuntimeAppUpdate {
        self.sync_presented_viewport();
        let runtime_update = match message {
            SlipwayIcedRuntimeMessage::DrainDebug => Some(SlipwayIcedRuntimeUpdate::DrainDebug),
            SlipwayIcedRuntimeMessage::Noop => Some(SlipwayIcedRuntimeUpdate::Noop),
            message => Some(apply_iced_runtime_message(
                &mut self.assembled.runtime,
                message,
                &mut self.apply_app_messages,
            )),
        };

        SlipwayIcedRuntimeAppUpdate {
            runtime_update,
            debug_replies_drained: 0,
            debug_error: None,
        }
    }

    pub fn drain_debug_pending(&mut self) -> (usize, Option<String>) {
        self.sync_presented_viewport();
        let mut intercept = iced_winit_physical_control_reply;
        match self
            .assembled
            .runtime
            .drain_live_debug_turn_with_app_reducer_and_interceptor(
                SlipwayRuntimeDrainBudget::default(),
                &mut self.apply_app_messages,
                &mut intercept,
            ) {
            Ok(report) => (
                report.debug_replies_drained + report.runtime_mcp_replies_drained,
                None,
            ),
            Err(error) => (0, Some(format!("{error:?}"))),
        }
    }

    pub fn handle_backend_presented_physical_control(
        &mut self,
        command: DebugCommand,
        backend_input: BackendInputEvent,
    ) -> DebugReplyProduct {
        self.sync_presented_viewport();
        self.assembled
            .runtime
            .handle_backend_presented_physical_control_with_app_reducer(
                command,
                backend_input,
                &mut self.apply_app_messages,
            )
    }

    fn sync_presented_viewport(&mut self) {
        self.assembled
            .runtime
            .record_presented_viewport(self.presented_viewport.get());
    }

    pub fn view<'a, Theme, Renderer>(
        &'a self,
    ) -> iced::Element<'a, SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>
    where
        W: SlipwayViewDefinition + 'a,
        W::ExternalState: 'a,
        W::LocalState: 'a,
        W::AppMessage: Clone + 'a,
        Theme: SlipwayIcedThemeContract + 'a,
        Renderer: iced::advanced::text::Renderer<Font = iced::Font>
            + iced::advanced::graphics::geometry::Renderer
            + 'a + 'static,
    {
        iced_runtime_widget(&self.assembled.runtime)
            .record_presented_viewport_in(&self.presented_viewport)
            .into()
    }

    pub fn subscription(&self) -> iced::Subscription<SlipwayIcedRuntimeMessage<W::AppMessage>>
    where
        W::AppMessage: 'static,
    {
        let mcp = match self.debug_mcp_transport.as_ref() {
            Some(transport) => iced_mcp_wake_subscription(transport)
                .map(|()| SlipwayIcedRuntimeMessage::DrainDebug),
            None => iced::Subscription::none(),
        };
        mcp
    }
}

fn iced_winit_physical_control_reply(command: &DebugCommand) -> Option<DebugReplyProduct> {
    let DebugCommandKind::PhysicalControl { .. } = &command.kind else {
        return None;
    };

    Some(DebugReplyProduct::Error(DebugFailure {
        code: "iced-winit-event-injection-required".to_string(),
        message: "slipway.debug.physical_control must enter iced through the iced_winit WindowEvent -> iced Event -> UserInterface::update path. The current iced::application runner exposes no supported injection seam, so this backend refuses instead of mutating runtime state or sending OS input.".to_string(),
        dispatch_evidence: None,
    }))
}

struct IcedMcpWakeSubscription {
    local_addr: SocketAddr,
    wake_rx: SlipwayRuntimeMcpWakeReceiver,
}

impl Hash for IcedMcpWakeSubscription {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.local_addr.hash(state);
    }
}

fn iced_mcp_wake_subscription(transport: &SlipwayRuntimeMcpTransport) -> iced::Subscription<()> {
    iced::Subscription::run_with(
        IcedMcpWakeSubscription {
            local_addr: transport.local_addr(),
            wake_rx: transport.wake_receiver(),
        },
        iced_mcp_wake_stream,
    )
}

fn iced_mcp_wake_stream(
    data: &IcedMcpWakeSubscription,
) -> iced::futures::stream::BoxStream<'static, ()> {
    use iced::futures::StreamExt;

    let wake_rx = data.wake_rx.clone();
    iced::stream::channel(1, async move |mut output| {
        let _ = std::thread::Builder::new()
            .name("slipway-iced-mcp-wake".to_string())
            .spawn(move || {
                while wake_rx.recv() {
                    let _ = output.try_send(());
                }
            });
    })
    .boxed()
}

struct IcedTreeState<L> {
    id: WidgetId,
    local: L,
    presentation: Option<IcedPresentationState>,
    hovered_region: Option<slipway_core::PresentationRegionId>,
    pressed_region: Option<IcedPressedPointerCapture>,
    focused_region: Option<slipway_core::PresentationRegionId>,
    layout_pass: u64,
}

struct IcedRuntimeTreeState {
    id: WidgetId,
    slot: Option<WidgetSlotAddress>,
    presentation: Option<IcedPresentationState>,
    text_edit_trees: Vec<iced::advanced::widget::Tree>,
    text_editor_trees: Vec<iced::advanced::widget::Tree>,
    scroll_region_trees: Vec<iced::advanced::widget::Tree>,
    hovered_region: Option<slipway_core::PresentationRegionId>,
    pressed_region: Option<IcedPressedPointerCapture>,
    focused_region: Option<slipway_core::PresentationRegionId>,
    layout_pass: u64,
}

#[derive(Clone, Debug, PartialEq)]
struct IcedDirtyScope {
    target: WidgetId,
    slot: Option<WidgetSlotAddress>,
}

#[derive(Clone, Debug, PartialEq)]
struct IcedPresentationState {
    target: WidgetId,
    frame: FrameIdentity,
    layout_input: LayoutInput,
    layout: LayoutOutput,
    geometry_index: PresentationGeometryIndex,
    root_child_placements: Arc<[slipway_core::ChildPlacement]>,
    native_scroll_regions: Arc<[ScrollRegionDeclaration]>,
    text_input_regions: Arc<[FocusRegionDeclaration]>,
    text_editor_regions: Arc<[FocusRegionDeclaration]>,
    paint: Vec<PaintOp>,
    local_paint: Vec<PaintOp>,
    explicit_layer_paint_units: Vec<PaintUnit>,
    paint_occlusion_regions: Vec<IcedPaintOcclusionRegion>,
    hit_regions: Vec<HitRegionDeclaration>,
    focus_regions: Vec<FocusRegionDeclaration>,
    scroll_regions: Vec<ScrollRegionDeclaration>,
    diagnostics: Vec<Diagnostic>,
    admission: BackendParityAdmission,
}

#[derive(Clone, Debug, PartialEq)]
struct IcedPaintOcclusionRegion {
    order: (i32, usize, usize),
    bounds: Rect,
}

impl IcedPresentationState {
    #[cfg(test)]
    fn from_view_definition(
        view: ViewDefinition,
        layout_input: LayoutInput,
        target: WidgetId,
    ) -> Self {
        Self::from_view_definition_with_capabilities_and_address(
            view,
            &[],
            layout_input,
            target,
            None,
            0,
        )
    }

    fn from_view_definition_with_capabilities_and_address(
        mut view: ViewDefinition,
        capabilities: &[Capability],
        layout_input: LayoutInput,
        target: WidgetId,
        address: Option<WidgetSlotAddress>,
        traversal_order: usize,
    ) -> Self {
        let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
        normalize_iced_visible_scroll_regions(&mut view, &geometry_index);
        let admission =
            iced_backend_admission().admit_view_definition_with_capabilities(capabilities, &view);
        let mut diagnostics = view.diagnostics.clone();
        for unsupported in &admission.unsupported {
            diagnostics.push(Diagnostic::unsupported(
                unsupported.target.clone().or_else(|| Some(target.clone())),
                unsupported
                    .requirement_id
                    .clone()
                    .unwrap_or_else(|| "iced.visible_admission.unsupported".to_string()),
                unsupported.reason.clone(),
            ));
            diagnostics.extend(unsupported.diagnostics.clone());
        }

        let scroll_indicator_paint = iced_scroll_indicator_paint(&view);
        if !scroll_indicator_paint.is_empty() {
            view.paint.push(PaintOp::keyed_layer_pass_through(
                PaintLayerKey::ordered(5, usize::MAX),
                scroll_indicator_paint,
            ));
        }
        let root_child_placements: Arc<[slipway_core::ChildPlacement]> =
            root_child_placements_excluding_native_scroll_for_layout(
                &view.layout,
                &geometry_index,
                &view.scroll_regions,
            )
            .into();
        let native_scroll_regions: Arc<[ScrollRegionDeclaration]> =
            native_iced_scroll_regions_for_layout(&view.layout, &view.scroll_regions).into();
        let text_input_regions: Arc<[FocusRegionDeclaration]> =
            text_input_focus_regions_for_view(&target, &view.focus_regions).into();
        let text_editor_regions: Arc<[FocusRegionDeclaration]> =
            text_editor_focus_regions_for_view(&target, &view.focus_regions).into();
        let paint = ordered_iced_presentation_paint(&view, traversal_order, address.as_ref());
        let local_paint = local_iced_presentation_paint(&view, traversal_order, address.as_ref());
        let explicit_layer_paint_units =
            iced_explicit_layer_paint_units(&view, traversal_order, address.as_ref());
        let paint_occlusion_regions = iced_paint_occlusion_regions(&explicit_layer_paint_units);
        Self {
            target,
            frame: view.frame,
            layout_input,
            layout: view.layout,
            geometry_index,
            root_child_placements,
            native_scroll_regions,
            text_input_regions,
            text_editor_regions,
            paint,
            local_paint,
            explicit_layer_paint_units,
            paint_occlusion_regions,
            hit_regions: view.hit_regions,
            focus_regions: view.focus_regions,
            scroll_regions: view.scroll_regions,
            diagnostics,
            admission,
        }
    }

    fn has_unsupported_visible_capability(&self, capability: BackendVisibleCapability) -> bool {
        self.admission
            .unsupported
            .iter()
            .any(|unsupported| unsupported.visible_capability.as_ref() == Some(&capability))
    }

    fn can_draw_visible_paint(&self) -> bool {
        !self.has_unsupported_visible_capability(BackendVisibleCapability::FontInstallation)
    }

    fn can_route_text_edit(&self) -> bool {
        !self.has_unsupported_visible_capability(BackendVisibleCapability::TextEditRegions)
    }
}

#[derive(Clone, Debug, PartialEq)]
struct IcedRoutedInput {
    input: Option<BackendInputEvent>,
    capture_event: bool,
    request_redraw: bool,
}

#[derive(Clone, Debug, PartialEq)]
struct IcedPressedPointerCapture {
    region_id: PresentationRegionId,
    layout_origin: Point,
}

impl IcedPressedPointerCapture {
    fn new(region_id: PresentationRegionId, layout_bounds: iced::Rectangle) -> Self {
        Self {
            region_id,
            layout_origin: Point {
                x: layout_bounds.x,
                y: layout_bounds.y,
            },
        }
    }

    fn layout_bounds_for_capture(&self, current: iced::Rectangle) -> iced::Rectangle {
        iced::Rectangle {
            x: self.layout_origin.x,
            y: self.layout_origin.y,
            width: current.width,
            height: current.height,
        }
    }
}

fn iced_layout_input_from_limits(limits: &iced::advanced::layout::Limits) -> LayoutInput {
    let viewport = rect_from_iced(iced::Rectangle {
        x: 0.0,
        y: 0.0,
        width: limits.max().width,
        height: limits.max().height,
    });

    LayoutInput {
        viewport: TargetLocalRect::new(viewport),
        constraints: LayoutConstraints {
            min: size_from_iced(limits.min()),
            max: size_from_iced(limits.max()),
        },
    }
}

fn iced_frame_identity(
    widget_id: &WidgetId,
    seed: Option<&FrameIdentity>,
    viewport: Rect,
    layout_pass: u64,
) -> FrameIdentity {
    if let Some(seed) = seed {
        return FrameIdentity {
            surface_id: seed.surface_id.clone(),
            surface_instance_id: seed.surface_instance_id.clone(),
            revision: seed.revision,
            frame_index: seed.frame_index,
            viewport: seed.viewport,
        };
    }

    FrameIdentity {
        surface_id: "iced-visible".to_string(),
        surface_instance_id: widget_id.as_str().to_string(),
        revision: 0,
        frame_index: layout_pass,
        viewport,
    }
}

fn iced_presentation_for_widget<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    input: LayoutInput,
    frame_seed: Option<&FrameIdentity>,
    layout_pass: u64,
    runtime_slot: Option<&WidgetSlotAddress>,
) -> IcedPresentationState
where
    W: SlipwayIcedBackendChildWidget,
{
    let target = widget.id();
    let frame = iced_frame_identity(&target, frame_seed, input.viewport.into_rect(), layout_pass);
    let mut view = widget.visible_backend_view_definition(
        external,
        local,
        ViewDefinitionInput {
            frame,
            layout_input: input.clone(),
        },
    );
    if let Some(slot) = runtime_slot {
        apply_runtime_slot_to_view_definition(&mut view, slot);
    }

    let capabilities = widget.capabilities();
    IcedPresentationState::from_view_definition_with_capabilities_and_address(
        view,
        &capabilities,
        input,
        target,
        runtime_slot.cloned(),
        runtime_slot.map(|slot| slot.ordinal).unwrap_or(0),
    )
}

fn iced_presentation_cache_matches(
    presentation: &IcedPresentationState,
    input: &LayoutInput,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
    target: &WidgetId,
    runtime_slot: Option<&WidgetSlotAddress>,
) -> bool {
    let expected_frame_viewport = frame_seed
        .map(|seed| seed.viewport)
        .unwrap_or_else(|| input.viewport.into_rect());
    if presentation.layout_input != *input || presentation.frame.viewport != expected_frame_viewport
    {
        return false;
    }

    match frame_seed {
        Some(seed) => {
            presentation.frame.surface_id == seed.surface_id
                && presentation.frame.surface_instance_id == seed.surface_instance_id
                && (presentation.frame.revision == seed.revision
                    || !iced_dirty_scopes_include_target(dirty_scopes, target, runtime_slot))
        }
        None => {
            presentation.frame.surface_id == "iced-visible"
                && presentation.frame.surface_instance_id == target.as_str()
                && presentation.frame.revision == 0
        }
    }
}

fn refresh_iced_presentation_frame(
    presentation: &mut IcedPresentationState,
    input: &LayoutInput,
    frame_seed: Option<&FrameIdentity>,
    target: &WidgetId,
    layout_pass: u64,
) {
    presentation.frame =
        iced_frame_identity(target, frame_seed, input.viewport.into_rect(), layout_pass);
}

fn iced_dirty_scopes_include_target(
    dirty_scopes: Option<&[IcedDirtyScope]>,
    target: &WidgetId,
    runtime_slot: Option<&WidgetSlotAddress>,
) -> bool {
    let Some(dirty_scopes) = dirty_scopes else {
        return true;
    };
    dirty_scopes.iter().any(|scope| {
        scope.target == *target
            && match (&scope.slot, runtime_slot) {
                (Some(dirty_slot), Some(runtime_slot)) => dirty_slot == runtime_slot,
                (Some(_), None) => false,
                (None, _) => true,
            }
    })
}

fn iced_dirty_scopes_from_runtime<W>(runtime: &SlipwayRuntime<W>) -> Option<Vec<IcedDirtyScope>>
where
    W: SlipwayIcedBackendChildWidget,
{
    let frame = runtime.last_frame_identity();
    let trace = runtime.last_backend_input_trace()?;
    if trace.revision_after != Some(frame.revision) {
        return None;
    }
    if !trace.handled || !trace.emitted_messages.is_empty() || trace.changes.is_empty() {
        return None;
    }
    Some(
        trace
            .changes
            .iter()
            .map(|change| IcedDirtyScope {
                target: change.target.clone(),
                slot: change.slot.clone(),
            })
            .collect(),
    )
}

fn apply_runtime_slot_to_view_definition(view: &mut ViewDefinition, slot: &WidgetSlotAddress) {
    for region in &mut view.hit_regions {
        mount_runtime_slot_address(&mut region.address, slot);
        mount_runtime_slot_address(&mut region.route.address, slot);
        region.route.path = mount_runtime_event_route_path(&region.route.path, slot);
    }
    for region in &mut view.focus_regions {
        mount_runtime_slot_address(&mut region.address, slot);
    }
    for region in &mut view.scroll_regions {
        mount_runtime_slot_address(&mut region.address, slot);
    }
}

fn mount_runtime_event_route_path(
    route_path: &[WidgetId],
    runtime_slot: &WidgetSlotAddress,
) -> Vec<WidgetId> {
    if route_path.first() == Some(&runtime_slot.widget) {
        let mut mounted = runtime_slot.path.clone();
        mounted.extend(route_path.iter().skip(1).cloned());
        mounted
    } else {
        let mut mounted = runtime_slot.path.clone();
        mounted.extend(route_path.iter().cloned());
        mounted
    }
}

fn mount_runtime_slot_address(
    address: &mut Option<WidgetSlotAddress>,
    runtime_slot: &WidgetSlotAddress,
) {
    *address = Some(
        address
            .take()
            .map(|address| mount_existing_runtime_slot(address, runtime_slot))
            .unwrap_or_else(|| runtime_slot.clone()),
    );
}

fn mount_existing_runtime_slot(
    slot: WidgetSlotAddress,
    runtime_slot: &WidgetSlotAddress,
) -> WidgetSlotAddress {
    if slot.path.starts_with(&runtime_slot.path) {
        return slot;
    }

    if slot.widget == runtime_slot.widget {
        return runtime_slot.clone();
    }

    let mut path = runtime_slot.path.clone();
    let mut suffix = slot.path;
    if suffix.first() == Some(&runtime_slot.widget) {
        suffix.remove(0);
    }
    path.extend(suffix);

    WidgetSlotAddress {
        widget: slot.widget,
        path,
        ordinal: slot.ordinal,
    }
}

fn iced_node_for_presentation(
    width: iced::Length,
    height: iced::Length,
    limits: &iced::advanced::layout::Limits,
    presentation: &IcedPresentationState,
) -> iced::advanced::layout::Node {
    iced_node_for_presentation_with_children(width, height, limits, presentation, Vec::new())
}

fn iced_node_for_presentation_with_children(
    width: iced::Length,
    height: iced::Length,
    limits: &iced::advanced::layout::Limits,
    presentation: &IcedPresentationState,
    children: Vec<iced::advanced::layout::Node>,
) -> iced::advanced::layout::Node {
    let intrinsic = iced::Size::new(
        presentation.layout.bounds.size.width.max(0.0),
        presentation.layout.bounds.size.height.max(0.0),
    );

    iced::advanced::layout::Node::with_children(limits.resolve(width, height, intrinsic), children)
}

fn assert_iced_standalone_no_topology_children<W>(widget: &W, external: &W::ExternalState)
where
    W: SlipwayAuthoredWidget,
{
    let topology = widget.topology(external);
    if !topology.children.is_empty() {
        panic!(
            "{}: widget `{}` declares {} child node(s)",
            ICED_STANDALONE_CHILD_LIFECYCLE_REFUSAL,
            topology.id.as_str(),
            topology.children.len()
        );
    }
}

fn assert_iced_standalone_no_layout_children(widget_id: &WidgetId, layout: &LayoutOutput) {
    if !layout.child_placements.is_empty() {
        panic!(
            "{}: widget `{}` produced {} child placement(s)",
            ICED_STANDALONE_CHILD_LIFECYCLE_REFUSAL,
            widget_id.as_str(),
            layout.child_placements.len()
        );
    }
}

fn iced_runtime_child_widget<'a, W>(
    widget: &'a W,
    external: &'a W::ExternalState,
    local: &'a W::LocalState,
    slot: WidgetSlotAddress,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&'a [IcedDirtyScope]>,
) -> SlipwayIcedRuntimeWidget<'a, W>
where
    W: SlipwayIcedBackendChildWidget,
{
    let child = SlipwayIcedRuntimeWidget::new(widget, external, local)
        .with_runtime_slot(slot)
        .with_dirty_scopes(dirty_scopes.map(Cow::Borrowed));
    if let Some(frame_seed) = frame_seed {
        child.with_frame_seed(frame_seed.clone())
    } else {
        child
    }
}

fn iced_child_placement_matches_slot(
    placement: &slipway_core::ChildPlacement,
    child: &WidgetId,
    child_slot: &WidgetSlotAddress,
) -> bool {
    if let Some(placement_slot) = &placement.local_state_slot {
        placement_slot == child_slot
    } else {
        placement.child == *child
    }
}

fn iced_child_placement_for_slot<'a>(
    placements: &'a [slipway_core::ChildPlacement],
    child: &WidgetId,
    child_slot: &WidgetSlotAddress,
) -> Option<&'a slipway_core::ChildPlacement> {
    placements
        .iter()
        .find(|placement| iced_child_placement_matches_slot(placement, child, child_slot))
}

fn iced_child_layout_input(bounds: Rect) -> LayoutInput {
    LayoutInput {
        viewport: TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: bounds.size,
        }),
        constraints: LayoutConstraints {
            min: slipway_core::Size {
                width: 0.0,
                height: 0.0,
            },
            max: bounds.size,
        },
    }
}

fn iced_child_paint_sort_key<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    parent_slot: &WidgetSlotAddress,
    source_order: usize,
    frame: &FrameIdentity,
    placements: &[slipway_core::ChildPlacement],
) -> (i32, usize, usize)
where
    W: SlipwayWidget + SlipwayViewDefinition,
{
    let child = widget.id();
    let child_slot = parent_slot.child(child.clone(), source_order);
    let Some(placement) = iced_child_placement_for_slot(placements, &child, &child_slot) else {
        return (0, source_order, source_order);
    };
    let view = widget.visible_backend_view_definition(
        external,
        local,
        ViewDefinitionInput {
            frame: frame.clone(),
            layout_input: iced_child_layout_input(placement.bounds.into_rect()),
        },
    );
    expand_paint_unit_layers(PaintUnit::from_view(view, source_order))
        .iter()
        .map(paint_unit_sort_key)
        .max()
        .unwrap_or((0, source_order, source_order))
}

#[derive(Clone, Debug, PartialEq)]
struct IcedPaintJob {
    unit: PaintUnit,
    origin: iced::Point,
    clip: Option<iced::Rectangle>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct IcedPaintQueueContext {
    deferred_translation: iced::Vector,
    clip: Option<iced::Rectangle>,
}

impl Default for IcedPaintQueueContext {
    fn default() -> Self {
        Self {
            deferred_translation: iced::Vector::new(0.0, 0.0),
            clip: None,
        }
    }
}

thread_local! {
    static ICED_GLOBAL_PAINT_QUEUE_ACTIVE: Cell<bool> = const { Cell::new(false) };
    static ICED_GLOBAL_PAINT_QUEUE: RefCell<Vec<IcedPaintJob>> = const { RefCell::new(Vec::new()) };
    static ICED_GLOBAL_PAINT_CONTEXT: Cell<IcedPaintQueueContext> = Cell::new(IcedPaintQueueContext::default());
}

fn iced_presentation_paint_units(
    view: &ViewDefinition,
    traversal_order: usize,
    address: Option<&WidgetSlotAddress>,
) -> Vec<PaintUnit> {
    let unit = PaintUnit {
        target: view.target.clone(),
        address: address.cloned(),
        order: view.paint_order.clone(),
        traversal_order,
        paint: view.paint.clone(),
    };
    expand_paint_unit_layers(unit)
}

fn ordered_iced_presentation_paint(
    view: &ViewDefinition,
    traversal_order: usize,
    address: Option<&WidgetSlotAddress>,
) -> Vec<PaintOp> {
    let mut units = iced_presentation_paint_units(view, traversal_order, address);
    units.sort_by_key(paint_unit_sort_key);
    units.into_iter().flat_map(|unit| unit.paint).collect()
}

fn local_iced_presentation_paint(
    view: &ViewDefinition,
    traversal_order: usize,
    address: Option<&WidgetSlotAddress>,
) -> Vec<PaintOp> {
    let unit_contains_extracted_layers = paint_ops_contain_layer(&view.paint);
    iced_presentation_paint_units(view, traversal_order, address)
        .into_iter()
        .filter(|unit| {
            !iced_paint_unit_requires_surface_global_flush(unit, unit_contains_extracted_layers)
        })
        .flat_map(|unit| unit.paint)
        .collect()
}

fn iced_explicit_layer_paint_units(
    view: &ViewDefinition,
    traversal_order: usize,
    address: Option<&WidgetSlotAddress>,
) -> Vec<PaintUnit> {
    let unit_contains_extracted_layers = paint_ops_contain_layer(&view.paint);
    iced_presentation_paint_units(view, traversal_order, address)
        .into_iter()
        .filter(|unit| {
            iced_paint_unit_requires_surface_global_flush(unit, unit_contains_extracted_layers)
        })
        .collect()
}

fn iced_paint_unit_requires_surface_global_flush(
    unit: &PaintUnit,
    unit_contains_extracted_layers: bool,
) -> bool {
    paint_ops_contain_layer(&unit.paint)
        || (!unit_contains_extracted_layers
            && unit.order.mode == slipway_core::PaintOrderMode::ExplicitLayered)
}

fn paint_ops_contain_layer(ops: &[PaintOp]) -> bool {
    ops.iter().any(|op| match op {
        PaintOp::Layer { .. } => true,
        PaintOp::Group { ops, .. } => paint_ops_contain_layer(ops),
        PaintOp::Fill { .. } | PaintOp::Stroke { .. } | PaintOp::Text { .. } => false,
    })
}

fn iced_paint_occlusion_regions(units: &[PaintUnit]) -> Vec<IcedPaintOcclusionRegion> {
    let mut regions = Vec::new();
    for unit in units {
        collect_iced_paint_occlusion_regions(
            &unit.paint,
            paint_unit_sort_key(unit),
            None,
            &mut regions,
        );
    }
    regions.sort_by_key(|region| region.order);
    regions
}

fn iced_scroll_indicator_paint(view: &ViewDefinition) -> Vec<PaintOp> {
    let mut ops = Vec::new();
    for region in &view.scroll_regions {
        if !region.enabled
            || scroll_region_has_real_child_placements(&view.layout, region)
            || !region.axes.vertical
            || region.content_bounds.size.height <= region.viewport.size.height + 0.5
        {
            continue;
        }
        let viewport = slipway_core::declared_region_root_local_rect(
            &view.layout,
            &region.target,
            region.address.as_ref(),
            *region.viewport,
        );
        let Some(track) = vertical_scroll_indicator_track(viewport) else {
            continue;
        };
        let thumb = vertical_scroll_indicator_thumb(
            track,
            region.viewport.size.height,
            region.content_bounds.size.height,
            region.offset.y,
        );
        ops.push(PaintOp::Fill {
            shape: ShapeDeclaration {
                id: Some(format!("{}:scroll-indicator-track", region.id.as_str())),
                kind: ShapeKind::RoundedRectangle,
                bounds: track,
                path: None,
                clip: None,
            },
            color: scroll_indicator_track_color(),
        });
        ops.push(PaintOp::Fill {
            shape: ShapeDeclaration {
                id: Some(format!("{}:scroll-indicator-thumb", region.id.as_str())),
                kind: ShapeKind::RoundedRectangle,
                bounds: thumb,
                path: None,
                clip: None,
            },
            color: scroll_indicator_thumb_color(),
        });
    }
    ops
}

fn vertical_scroll_indicator_track(viewport: Rect) -> Option<Rect> {
    if viewport.size.width <= 8.0 || viewport.size.height <= 12.0 {
        return None;
    }
    Some(Rect {
        origin: Point {
            x: viewport.origin.x + viewport.size.width - 9.0,
            y: viewport.origin.y + 4.0,
        },
        size: Size {
            width: 5.0,
            height: (viewport.size.height - 8.0).max(1.0),
        },
    })
}

fn vertical_scroll_indicator_thumb(
    track: Rect,
    viewport_height: f32,
    content_height: f32,
    offset_y: f32,
) -> Rect {
    let max_offset = (content_height - viewport_height).max(1.0);
    let ratio = (viewport_height / content_height.max(1.0)).clamp(0.0, 1.0);
    let track_height = track.size.height.max(0.0);
    let min_thumb_height = 18.0_f32.min(track_height);
    let thumb_height = (track_height * ratio).clamp(min_thumb_height, track_height);
    let travel = (track_height - thumb_height).max(0.0);
    let y = track.origin.y + travel * (offset_y.clamp(0.0, max_offset) / max_offset);
    Rect {
        origin: Point {
            x: track.origin.x,
            y,
        },
        size: Size {
            width: track.size.width,
            height: thumb_height,
        },
    }
}

fn scroll_indicator_track_color() -> slipway_core::Color {
    slipway_core::Color {
        red: 226.0 / 255.0,
        green: 232.0 / 255.0,
        blue: 240.0 / 255.0,
        alpha: 1.0,
    }
}

fn scroll_indicator_thumb_color() -> slipway_core::Color {
    slipway_core::Color {
        red: 100.0 / 255.0,
        green: 116.0 / 255.0,
        blue: 139.0 / 255.0,
        alpha: 1.0,
    }
}

fn collect_iced_paint_occlusion_regions(
    ops: &[PaintOp],
    order: (i32, usize, usize),
    clip: Option<Rect>,
    out: &mut Vec<IcedPaintOcclusionRegion>,
) {
    for op in ops {
        match op {
            PaintOp::Group {
                clip: group_clip,
                ops,
                ..
            } => {
                collect_iced_paint_occlusion_regions(
                    ops,
                    order,
                    combine_clip_rects(clip, group_clip.as_ref().map(|clip| clip.bounds)),
                    out,
                );
            }
            PaintOp::Layer {
                input_transparency,
                clip: layer_clip,
                ops,
                ..
            } => {
                let active_clip =
                    combine_clip_rects(clip, layer_clip.as_ref().map(|clip| clip.bounds));
                if *input_transparency == slipway_core::PaintInputTransparency::Opaque
                    && let Some(bounds) =
                        paint_ops_root_bounds(ops).and_then(|bounds| clip_rect(bounds, active_clip))
                {
                    out.push(IcedPaintOcclusionRegion { order, bounds });
                }
                collect_iced_paint_occlusion_regions(ops, order, active_clip, out);
            }
            PaintOp::Fill { .. } | PaintOp::Stroke { .. } | PaintOp::Text { .. } => {}
        }
    }
}

fn paint_ops_root_bounds(ops: &[PaintOp]) -> Option<Rect> {
    ops.iter()
        .filter_map(paint_op_root_bounds)
        .reduce(union_rects)
}

fn paint_op_root_bounds(op: &PaintOp) -> Option<Rect> {
    match op {
        PaintOp::Fill { shape, .. } | PaintOp::Stroke { shape, .. } => {
            Some(shape.clip.as_ref().map_or(shape.bounds, |clip| {
                rect_intersection(shape.bounds, clip.bounds).unwrap_or(clip.bounds)
            }))
        }
        PaintOp::Text { bounds, .. } => Some(*bounds),
        PaintOp::Group { clip, ops, .. } | PaintOp::Layer { clip, ops, .. } => {
            paint_ops_root_bounds(ops)
                .and_then(|bounds| clip_rect(bounds, clip.as_ref().map(|clip| clip.bounds)))
        }
    }
}

fn combine_clip_rects(current: Option<Rect>, next: Option<Rect>) -> Option<Rect> {
    match (current, next) {
        (Some(left), Some(right)) => rect_intersection(left, right),
        (Some(rect), None) | (None, Some(rect)) => Some(rect),
        (None, None) => None,
    }
}

fn clip_rect(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    clip.map_or(Some(rect), |clip| rect_intersection(rect, clip))
}

fn union_rects(a: Rect, b: Rect) -> Rect {
    let min_x = a.origin.x.min(b.origin.x);
    let min_y = a.origin.y.min(b.origin.y);
    let max_x = (a.origin.x + a.size.width.max(0.0)).max(b.origin.x + b.size.width.max(0.0));
    let max_y = (a.origin.y + a.size.height.max(0.0)).max(b.origin.y + b.size.height.max(0.0));
    Rect {
        origin: Point { x: min_x, y: min_y },
        size: Size {
            width: (max_x - min_x).max(0.0),
            height: (max_y - min_y).max(0.0),
        },
    }
}

fn rect_intersection(a: Rect, b: Rect) -> Option<Rect> {
    let min_x = a.origin.x.max(b.origin.x);
    let min_y = a.origin.y.max(b.origin.y);
    let max_x = (a.origin.x + a.size.width.max(0.0)).min(b.origin.x + b.size.width.max(0.0));
    let max_y = (a.origin.y + a.size.height.max(0.0)).min(b.origin.y + b.size.height.max(0.0));
    (min_x < max_x && min_y < max_y).then_some(Rect {
        origin: Point { x: min_x, y: min_y },
        size: Size {
            width: max_x - min_x,
            height: max_y - min_y,
        },
    })
}

fn iced_declared_rect_is_valid(rect: Rect) -> bool {
    rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.size.width >= 0.0
        && rect.size.height >= 0.0
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

fn root_rect_to_target_local_rect(
    geometry_index: &PresentationGeometryIndex,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    root_rect: Rect,
) -> Rect {
    let target_rect = slipway_core::declared_target_rect_for_region_address_with_geometry_index(
        geometry_index,
        target,
        address,
    );
    Rect {
        origin: Point {
            x: root_rect.origin.x - target_rect.origin.x,
            y: root_rect.origin.y - target_rect.origin.y,
        },
        size: root_rect.size,
    }
}

fn normalize_iced_visible_scroll_regions(
    view: &mut ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
) {
    let mut diagnostics = Vec::new();
    for region in &mut view.scroll_regions {
        normalize_iced_visible_scroll_region(region, geometry_index, &mut diagnostics);
    }
    view.diagnostics.extend(diagnostics);
}

fn normalize_iced_visible_scroll_region(
    region: &mut ScrollRegionDeclaration,
    geometry_index: &PresentationGeometryIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let target_root_bounds =
        slipway_core::declared_target_rect_for_region_address_with_geometry_index(
            geometry_index,
            &region.target,
            region.address.as_ref(),
        );
    let target_local_bounds =
        slipway_core::declared_target_local_bounds(target_root_bounds).into_rect();
    let viewport = region.viewport.into_rect();

    if !iced_declared_rect_is_valid(viewport) || !iced_declared_rect_is_valid(target_local_bounds) {
        let safe = safe_zero_rect_inside(target_local_bounds);
        region.viewport = TargetLocalRect::new(safe);
        region.content_bounds = TargetLocalRect::new(safe);
        region.offset = Point { x: 0.0, y: 0.0 };
        region.enabled = false;
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "iced.visible_scroll.normalized_invalid_geometry",
            "iced visible backend disabled an invalid scroll region instead of allowing it to break the visible surface",
        ));
        return;
    }

    let viewport_root = slipway_core::declared_region_root_local_rect_with_geometry_index(
        geometry_index,
        &region.target,
        region.address.as_ref(),
        viewport,
    );
    let Some(cropped_root_viewport) = rect_intersection(viewport_root, target_root_bounds) else {
        let safe = safe_zero_rect_inside(target_local_bounds);
        region.viewport = TargetLocalRect::new(safe);
        region.content_bounds = TargetLocalRect::new(safe);
        region.offset = Point { x: 0.0, y: 0.0 };
        region.enabled = false;
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "iced.visible_scroll.disabled_outside_layout",
            "iced visible backend disabled a scroll region whose viewport is fully outside the target layout bounds",
        ));
        return;
    };
    let cropped_viewport = root_rect_to_target_local_rect(
        geometry_index,
        &region.target,
        region.address.as_ref(),
        cropped_root_viewport,
    );

    if cropped_viewport != viewport {
        region.viewport = TargetLocalRect::new(cropped_viewport);
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "iced.visible_scroll.viewport_cropped_to_layout",
            "iced visible backend cropped a scroll viewport to the target layout bounds before visible admission",
        ));
    }

    let content_bounds = region.content_bounds.into_rect();
    let normalized_content = if iced_declared_rect_is_valid(content_bounds) {
        union_rects(content_bounds, cropped_viewport)
    } else {
        cropped_viewport
    };
    if normalized_content != content_bounds {
        region.content_bounds = TargetLocalRect::new(normalized_content);
        diagnostics.push(Diagnostic::warning(
            Some(region.target.clone()),
            "iced.visible_scroll.content_bounds_expanded_to_viewport",
            "iced visible backend expanded invalid or undersized scroll content bounds to contain the visible viewport",
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
            "iced.visible_scroll.offset_clamped",
            "iced visible backend clamped a scroll offset so the visible surface remains presentable",
        ));
    }
}

fn iced_point_minus_vector(point: iced::Point, vector: iced::Vector) -> iced::Point {
    iced::Point::new(point.x - vector.x, point.y - vector.y)
}

fn iced_intersect_clips(
    current: Option<iced::Rectangle>,
    next: iced::Rectangle,
) -> Option<iced::Rectangle> {
    match current {
        Some(current) => current.intersection(&next).or(Some(iced::Rectangle {
            x: next.x,
            y: next.y,
            width: 0.0,
            height: 0.0,
        })),
        None => Some(next),
    }
}

fn with_iced_global_paint_context<R>(next: IcedPaintQueueContext, f: impl FnOnce() -> R) -> R {
    let previous = ICED_GLOBAL_PAINT_CONTEXT.with(|context| {
        let previous = context.get();
        context.set(next);
        previous
    });
    let result = f();
    ICED_GLOBAL_PAINT_CONTEXT.with(|context| context.set(previous));
    result
}

fn with_iced_surface_global_paint_queue<Renderer>(
    renderer: &mut Renderer,
    f: impl FnOnce(&mut Renderer),
) where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    if ICED_GLOBAL_PAINT_QUEUE_ACTIVE.with(|active| active.get()) {
        f(renderer);
        return;
    }

    ICED_GLOBAL_PAINT_QUEUE_ACTIVE.with(|active| active.set(true));
    ICED_GLOBAL_PAINT_QUEUE.with(|queue| queue.borrow_mut().clear());
    ICED_GLOBAL_PAINT_CONTEXT.with(|context| context.set(IcedPaintQueueContext::default()));

    f(renderer);

    let mut jobs = ICED_GLOBAL_PAINT_QUEUE.with(|queue| std::mem::take(&mut *queue.borrow_mut()));
    ICED_GLOBAL_PAINT_CONTEXT.with(|context| context.set(IcedPaintQueueContext::default()));
    ICED_GLOBAL_PAINT_QUEUE_ACTIVE.with(|active| active.set(false));

    flush_iced_global_paint_jobs(renderer, &mut jobs);
}

fn queue_iced_explicit_layer_paint_jobs(
    presentation: &IcedPresentationState,
    local_origin: iced::Point,
) {
    if !presentation.can_draw_visible_paint()
        || presentation.explicit_layer_paint_units.is_empty()
        || !ICED_GLOBAL_PAINT_QUEUE_ACTIVE.with(|active| active.get())
    {
        return;
    }

    let context = ICED_GLOBAL_PAINT_CONTEXT.with(|context| context.get());
    let origin = iced_point_minus_vector(local_origin, context.deferred_translation);
    ICED_GLOBAL_PAINT_QUEUE.with(|queue| {
        let mut queue = queue.borrow_mut();
        queue.extend(
            presentation
                .explicit_layer_paint_units
                .iter()
                .cloned()
                .map(|unit| IcedPaintJob {
                    unit,
                    origin,
                    clip: context.clip,
                }),
        );
    });
}

fn flush_iced_global_paint_jobs<Renderer>(renderer: &mut Renderer, jobs: &mut Vec<IcedPaintJob>)
where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    jobs.sort_by_key(|job| paint_unit_sort_key(&job.unit));
    for job in jobs.drain(..) {
        if let Some(clip) = job.clip {
            if clip.width <= 0.0 || clip.height <= 0.0 {
                continue;
            }
            renderer.start_layer(clip);
            for op in &job.unit.paint {
                draw_paint_op(renderer, job.origin, op);
            }
            renderer.end_layer();
        } else {
            for op in &job.unit.paint {
                draw_paint_op(renderer, job.origin, op);
            }
        }
    }
}

fn scroll_content_paint_queue_context(
    layout: iced::advanced::Layout<'_>,
    viewport: &iced::Rectangle,
) -> IcedPaintQueueContext {
    let current = ICED_GLOBAL_PAINT_CONTEXT.with(|context| context.get());
    let bounds = layout.bounds();
    let scroll_translation = iced::Vector::new(viewport.x - bounds.x, viewport.y - bounds.y);
    let visible_clip = iced::Rectangle {
        x: viewport.x - scroll_translation.x,
        y: viewport.y - scroll_translation.y,
        width: viewport.width,
        height: viewport.height,
    };

    IcedPaintQueueContext {
        deferred_translation: iced::Vector::new(
            current.deferred_translation.x + scroll_translation.x,
            current.deferred_translation.y + scroll_translation.y,
        ),
        clip: iced_intersect_clips(current.clip, visible_clip),
    }
}

fn iced_child_layout_limits(
    placement: &slipway_core::ChildPlacement,
) -> iced::advanced::layout::Limits {
    let size = iced::Size::new(
        placement.bounds.size.width.max(0.0),
        placement.bounds.size.height.max(0.0),
    );
    iced::advanced::layout::Limits::new(size, size)
}

fn iced_unplaced_child_node() -> iced::advanced::layout::Node {
    iced::advanced::layout::Node::new(iced::Size::ZERO).move_to(iced::Point::new(0.0, 0.0))
}

fn iced_child_source_tree_index(slot: &WidgetSlotAddress, next_tree_index: &mut usize) -> usize {
    let index = slot.ordinal;
    *next_tree_index = (*next_tree_index).max(index + 1);
    index
}

struct IcedRuntimeChildTreeVisitor<'a, Theme, Renderer, AppMessage> {
    trees: Vec<iced::advanced::widget::Tree>,
    frame_seed: Option<&'a FrameIdentity>,
    dirty_scopes: Option<&'a [IcedDirtyScope]>,
    _phantom: std::marker::PhantomData<(Theme, Renderer, AppMessage)>,
}

impl<'a, Theme, Renderer, AppMessage> IcedRuntimeChildTreeVisitor<'a, Theme, Renderer, AppMessage> {
    fn new(
        frame_seed: Option<&'a FrameIdentity>,
        dirty_scopes: Option<&'a [IcedDirtyScope]>,
    ) -> Self {
        Self {
            trees: Vec::new(),
            frame_seed,
            dirty_scopes,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<ExternalState, AppMessage, Theme, Renderer>
    SlipwayIcedWidgetListVisitor<ExternalState, AppMessage>
    for IcedRuntimeChildTreeVisitor<'_, Theme, Renderer, AppMessage>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn visit_iced_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayIcedBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let child = iced_runtime_child_widget(
            widget,
            external,
            local,
            slot,
            self.frame_seed,
            self.dirty_scopes,
        );
        self.trees.push(iced::advanced::widget::Tree::new(
            &child
                as &dyn iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<AppMessage>,
                    Theme,
                    Renderer,
                >,
        ));
    }

    fn visit_iced_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayIcedNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let element = widget.iced_native_element::<Theme, Renderer>(
            external,
            local,
            IcedNativeWidgetContext {
                slot: &slot,
                frame_seed: self.frame_seed,
                placement: None,
            },
        );
        self.trees
            .push(iced::advanced::widget::Tree::new(element.as_widget()));
    }
}

fn iced_runtime_child_trees<W, Theme, Renderer>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
    order_context: Option<(&FrameIdentity, &[slipway_core::ChildPlacement])>,
) -> Vec<iced::advanced::widget::Tree>
where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let mut visitor = IcedRuntimeChildTreeVisitor::<Theme, Renderer, W::AppMessage>::new(
        frame_seed,
        dirty_scopes,
    );
    let _ = order_context;
    widget.visit_iced_authored_children(external, local, &mut visitor);
    visitor.trees
}

struct IcedRuntimeChildDiffVisitor<'a, 'b, Theme, Renderer, AppMessage> {
    trees: &'a mut Vec<iced::advanced::widget::Tree>,
    next_index: usize,
    frame_seed: Option<&'b FrameIdentity>,
    dirty_scopes: Option<&'b [IcedDirtyScope]>,
    _phantom: std::marker::PhantomData<(Theme, Renderer, AppMessage)>,
}

impl<'a, 'b, Theme, Renderer, AppMessage>
    IcedRuntimeChildDiffVisitor<'a, 'b, Theme, Renderer, AppMessage>
{
    fn new(
        trees: &'a mut Vec<iced::advanced::widget::Tree>,
        frame_seed: Option<&'b FrameIdentity>,
        dirty_scopes: Option<&'b [IcedDirtyScope]>,
    ) -> Self {
        Self {
            trees,
            next_index: 0,
            frame_seed,
            dirty_scopes,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<ExternalState, AppMessage, Theme, Renderer>
    SlipwayIcedWidgetListVisitor<ExternalState, AppMessage>
    for IcedRuntimeChildDiffVisitor<'_, '_, Theme, Renderer, AppMessage>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn visit_iced_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayIcedBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let child = iced_runtime_child_widget(
            widget,
            external,
            local,
            slot,
            self.frame_seed,
            self.dirty_scopes,
        );
        if let Some(tree) = self.trees.get_mut(self.next_index) {
            tree.diff(
                &child
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<AppMessage>,
                        Theme,
                        Renderer,
                    >,
            );
        } else {
            self.trees.push(iced::advanced::widget::Tree::new(
                &child
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<AppMessage>,
                        Theme,
                        Renderer,
                    >,
            ));
        }
        self.next_index += 1;
    }

    fn visit_iced_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayIcedNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let element = widget.iced_native_element::<Theme, Renderer>(
            external,
            local,
            IcedNativeWidgetContext {
                slot: &slot,
                frame_seed: self.frame_seed,
                placement: None,
            },
        );
        if let Some(tree) = self.trees.get_mut(self.next_index) {
            tree.diff(element.as_widget());
        } else {
            self.trees
                .push(iced::advanced::widget::Tree::new(element.as_widget()));
        }
        self.next_index += 1;
    }
}

fn reconcile_iced_runtime_child_trees<W, Theme, Renderer>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    trees: &mut Vec<iced::advanced::widget::Tree>,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
    order_context: Option<(&FrameIdentity, &[slipway_core::ChildPlacement])>,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let mut visitor = IcedRuntimeChildDiffVisitor::<Theme, Renderer, W::AppMessage>::new(
        trees,
        frame_seed,
        dirty_scopes,
    );
    let _ = order_context;
    widget.visit_iced_authored_children(external, local, &mut visitor);
    let visited = visitor.next_index;
    visitor.trees.truncate(visited);
}

fn text_input_focus_regions_for_view(
    target: &WidgetId,
    focus_regions: &[FocusRegionDeclaration],
) -> Vec<FocusRegionDeclaration> {
    focus_regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.target == *target
                && region
                    .text_edit
                    .as_ref()
                    .is_some_and(|text_edit| text_edit.line_mode == TextLineMode::SingleLine)
        })
        .cloned()
        .collect()
}

fn text_editor_focus_regions_for_view(
    target: &WidgetId,
    focus_regions: &[FocusRegionDeclaration],
) -> Vec<FocusRegionDeclaration> {
    focus_regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.target == *target
                && region
                    .text_edit
                    .as_ref()
                    .is_some_and(|text_edit| text_edit.line_mode == TextLineMode::MultiLine)
        })
        .cloned()
        .collect()
}

fn text_input_focus_regions(presentation: &IcedPresentationState) -> Arc<[FocusRegionDeclaration]> {
    Arc::clone(&presentation.text_input_regions)
}

fn text_editor_focus_regions(
    presentation: &IcedPresentationState,
) -> Arc<[FocusRegionDeclaration]> {
    Arc::clone(&presentation.text_editor_regions)
}

fn iced_text_input_id(region: &FocusRegionDeclaration) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(format!(
        "slipway-text-edit:{}:{}",
        region.target.as_str(),
        region.id.as_str()
    ))
}

fn preedit_overlay_style_for_region(
    region: &FocusRegionDeclaration,
) -> Option<IcedPreeditOverlayStyle> {
    region
        .text_edit
        .as_ref()
        .map(|text_edit| IcedPreeditOverlayStyle {
            text_color: iced_color(text_edit.visual_style.preedit_color),
            font: iced_font(&text_edit.typography.style),
            size: normalized_text_size(&text_edit.typography.style),
        })
}

fn iced_text_input_style_from_decl(
    visual_style: &slipway_core::TextInputVisualStyleDeclaration,
) -> iced::widget::text_input::Style {
    iced::widget::text_input::Style {
        background: iced::Background::Color(iced_color(visual_style.background_color)),
        border: iced::Border {
            color: iced_color(visual_style.border_color),
            width: visual_style.border_width,
            radius: iced::border::Radius::from(visual_style.border_radius),
        },
        icon: iced_color(visual_style.icon_color),
        placeholder: iced_color(visual_style.placeholder_color),
        value: iced_color(visual_style.value_color),
        selection: iced_color(visual_style.selection_color),
    }
}

fn iced_text_input_widget_for_region<'a, AppMessage, Theme, Renderer>(
    presentation: &'a IcedPresentationState,
    region: &'a FocusRegionDeclaration,
) -> iced::widget::TextInput<'a, SlipwayIcedRuntimeMessage<AppMessage>, Theme, Renderer>
where
    AppMessage: Clone + 'a,
    Theme: SlipwayIcedThemeContract + 'a,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'a + 'static,
{
    let text_edit = region
        .text_edit
        .as_ref()
        .expect("text input widget requires text edit region");
    let target = region.target.clone();
    let target_slot = region.address.clone();
    let selection_before = text_edit.selection.selection.clone();
    let frame = presentation.frame.clone();
    let geometry_index = presentation.geometry_index.clone();
    let focus_regions = presentation.focus_regions.clone();
    let selected_region = region.clone();
    let visual_style = text_edit.visual_style.clone();
    let input = iced::widget::text_input::<SlipwayIcedRuntimeMessage<AppMessage>, Theme, Renderer>(
        "",
        &text_edit.buffer.text,
    )
    .id(iced_text_input_id(region))
    .width(iced::Length::Fixed(region.bounds.size.width.max(0.0)))
    .padding(iced::Padding::ZERO)
    .font(iced_font(&text_edit.typography.style))
    .size(iced::Pixels(normalized_text_size(
        &text_edit.typography.style,
    )))
    .line_height(iced::advanced::text::LineHeight::Relative(1.2))
    .class(Theme::slipway_text_input_class(Box::new(
        move |_theme: &Theme, _status| iced_text_input_style_from_decl(&visual_style),
    )));

    if text_edit.selection.editable {
        input.on_input(move |value| {
            let event = InputEvent::TextEdit(slipway_core::TextEditEvent {
                target: target.clone(),
                target_slot: target_slot.clone(),
                kind: TextEditKind::ReplaceBuffer,
                text: Some(value),
                selection_before: selection_before.clone(),
                selection_after: None,
            });
            SlipwayIcedRuntimeMessage::BackendInput(backend_focus_input_event_from_parts(
                frame.clone(),
                &geometry_index,
                &focus_regions,
                &selected_region,
                DeclaredEventDispatchKind::Text,
                None,
                event,
            ))
        })
    } else {
        input
    }
}

fn reconcile_iced_text_input_trees<AppMessage, Theme, Renderer>(
    trees: &mut Vec<iced::advanced::widget::Tree>,
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
) where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    for (index, region) in regions.iter().enumerate() {
        let input =
            iced_text_input_widget_for_region::<AppMessage, Theme, Renderer>(presentation, region);
        if let Some(tree) = trees.get_mut(index) {
            tree.diff(
                &input
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<AppMessage>,
                        Theme,
                        Renderer,
                    >,
            );
        } else {
            trees.push(iced::advanced::widget::Tree::new(
                &input
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<AppMessage>,
                        Theme,
                        Renderer,
                    >,
            ));
        }
    }
    trees.truncate(regions.len());
}

fn layout_iced_text_input_regions<AppMessage, Theme, Renderer>(
    trees: &mut Vec<iced::advanced::widget::Tree>,
    renderer: &Renderer,
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
) -> Vec<iced::advanced::layout::Node>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    reconcile_iced_text_input_trees::<AppMessage, Theme, Renderer>(trees, presentation, regions);
    let mut nodes = Vec::with_capacity(regions.len());
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get_mut(index) else {
            continue;
        };
        let mut input =
            iced_text_input_widget_for_region::<AppMessage, Theme, Renderer>(presentation, region);
        let size = iced::Size::new(
            region.bounds.size.width.max(0.0),
            region.bounds.size.height.max(0.0),
        );
        let limits = iced::advanced::layout::Limits::new(size, size);
        let node = <iced::widget::TextInput<
            '_,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<SlipwayIcedRuntimeMessage<AppMessage>, Theme, Renderer>>::layout(
            &mut input,
            tree,
            renderer,
            &limits,
        )
        .move_to(iced_point(region_root_local_rect(
            presentation,
            &region.target,
            region.address.as_ref(),
            region.bounds.into_rect(),
        )
        .origin));
        nodes.push(node);
    }
    nodes
}

struct IcedRuntimeChildLayoutVisitor<'a, 'b, Theme, Renderer, AppMessage> {
    tree_children: &'a mut Vec<iced::advanced::widget::Tree>,
    renderer: &'a Renderer,
    placements: &'a [slipway_core::ChildPlacement],
    nodes: Vec<iced::advanced::layout::Node>,
    next_tree_index: usize,
    current_order_index: Option<usize>,
    frame_seed: Option<&'b FrameIdentity>,
    dirty_scopes: Option<&'b [IcedDirtyScope]>,
    _phantom: std::marker::PhantomData<(Theme, AppMessage)>,
}

impl<'a, 'b, Theme, Renderer, AppMessage>
    IcedRuntimeChildLayoutVisitor<'a, 'b, Theme, Renderer, AppMessage>
{
    fn new(
        tree_children: &'a mut Vec<iced::advanced::widget::Tree>,
        renderer: &'a Renderer,
        placements: &'a [slipway_core::ChildPlacement],
        frame_seed: Option<&'b FrameIdentity>,
        dirty_scopes: Option<&'b [IcedDirtyScope]>,
    ) -> Self {
        Self {
            tree_children,
            renderer,
            placements,
            nodes: Vec::new(),
            next_tree_index: 0,
            current_order_index: None,
            frame_seed,
            dirty_scopes,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<ExternalState, AppMessage, Theme, Renderer>
    SlipwayIcedWidgetListVisitor<ExternalState, AppMessage>
    for IcedRuntimeChildLayoutVisitor<'_, '_, Theme, Renderer, AppMessage>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn set_iced_child_order_index(&mut self, index: usize) {
        self.current_order_index = Some(index);
    }

    fn visit_iced_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayIcedBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);

        let placement = iced_child_placement_for_slot(self.placements, &widget.id(), &slot);
        let child = iced_runtime_child_widget(
            widget,
            external,
            local,
            slot,
            self.frame_seed,
            self.dirty_scopes,
        );
        if tree_index >= self.tree_children.len() {
            self.tree_children.push(iced::advanced::widget::Tree::new(
                &child
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<AppMessage>,
                        Theme,
                        Renderer,
                    >,
            ));
        }

        let Some(placement) = placement else {
            self.nodes.push(iced_unplaced_child_node());
            return;
        };

        let mut child = child;
        let limits = iced_child_layout_limits(placement);
        let node = <SlipwayIcedRuntimeWidget<'_, W> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::layout(
            &mut child,
            &mut self.tree_children[tree_index],
            self.renderer,
            &limits,
        )
        .move_to(iced_point(placement.bounds.origin));

        self.nodes.push(node);
    }

    fn visit_iced_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayIcedNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);

        let placement = iced_child_placement_for_slot(self.placements, &widget.id(), &slot);
        let element = widget.iced_native_element::<Theme, Renderer>(
            external,
            local,
            IcedNativeWidgetContext {
                slot: &slot,
                frame_seed: self.frame_seed,
                placement: placement.map(|placement| placement.bounds.into_rect()),
            },
        );
        if tree_index >= self.tree_children.len() {
            self.tree_children
                .push(iced::advanced::widget::Tree::new(element.as_widget()));
        }

        let Some(placement) = placement else {
            self.nodes.push(iced_unplaced_child_node());
            return;
        };

        let limits = iced_child_layout_limits(placement);
        let mut element = element;
        let node = element
            .as_widget_mut()
            .layout(&mut self.tree_children[tree_index], self.renderer, &limits)
            .move_to(iced_point(placement.bounds.origin));
        self.nodes.push(node);
    }
}

fn layout_iced_runtime_children<W, Theme, Renderer>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    tree_children: &mut Vec<iced::advanced::widget::Tree>,
    renderer: &Renderer,
    placements: &[slipway_core::ChildPlacement],
    frame: &FrameIdentity,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) -> Vec<iced::advanced::layout::Node>
where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    reconcile_iced_runtime_child_trees::<W, Theme, Renderer>(
        widget,
        external,
        local,
        tree_children,
        frame_seed,
        dirty_scopes,
        Some((frame, placements)),
    );
    let mut visitor = IcedRuntimeChildLayoutVisitor::<Theme, Renderer, W::AppMessage>::new(
        tree_children,
        renderer,
        placements,
        frame_seed,
        dirty_scopes,
    );
    let _ = frame;
    widget.visit_iced_authored_children(external, local, &mut visitor);
    visitor.nodes
}

struct IcedRuntimeChildDrawVisitor<'a, 'b, Theme, Renderer, AppMessage> {
    tree_children: &'a [iced::advanced::widget::Tree],
    layout: iced::advanced::Layout<'b>,
    renderer: &'a mut Renderer,
    theme: &'a Theme,
    style: &'a iced::advanced::renderer::Style,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &'a iced::Rectangle,
    placements: &'a [slipway_core::ChildPlacement],
    next_tree_index: usize,
    next_layout_index: usize,
    current_order_index: Option<usize>,
    frame_seed: Option<&'a FrameIdentity>,
    dirty_scopes: Option<&'a [IcedDirtyScope]>,
    _phantom: std::marker::PhantomData<AppMessage>,
}

impl<ExternalState, AppMessage, Theme, Renderer>
    SlipwayIcedWidgetListVisitor<ExternalState, AppMessage>
    for IcedRuntimeChildDrawVisitor<'_, '_, Theme, Renderer, AppMessage>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn set_iced_child_order_index(&mut self, index: usize) {
        self.current_order_index = Some(index);
    }

    fn visit_iced_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayIcedBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);
        let layout_index = tree_index;
        self.next_layout_index = self.next_layout_index.max(layout_index + 1);

        if iced_child_placement_for_slot(self.placements, &widget.id(), &slot).is_none() {
            return;
        }

        let Some(tree) = self.tree_children.get(tree_index) else {
            return;
        };

        if layout_index >= self.layout.children().len() {
            return;
        }

        let child = iced_runtime_child_widget(
            widget,
            external,
            local,
            slot,
            self.frame_seed,
            self.dirty_scopes,
        );
        draw_iced_runtime_widget_local::<W, Theme, Renderer>(
            &child,
            tree,
            self.renderer,
            self.theme,
            self.style,
            self.layout.child(layout_index),
            self.cursor,
            self.viewport,
        );
    }

    fn visit_iced_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayIcedNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);
        let layout_index = tree_index;
        self.next_layout_index = self.next_layout_index.max(layout_index + 1);

        let Some(placement) = iced_child_placement_for_slot(self.placements, &widget.id(), &slot)
        else {
            return;
        };

        let Some(tree) = self.tree_children.get(tree_index) else {
            return;
        };

        if layout_index >= self.layout.children().len() {
            return;
        }

        let element = widget.iced_native_element::<Theme, Renderer>(
            external,
            local,
            IcedNativeWidgetContext {
                slot: &slot,
                frame_seed: self.frame_seed,
                placement: Some(placement.bounds.into_rect()),
            },
        );
        element.as_widget().draw(
            tree,
            self.renderer,
            self.theme,
            self.style,
            self.layout.child(layout_index),
            self.cursor,
            self.viewport,
        );
    }
}

fn draw_iced_runtime_children<W, Theme, Renderer>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    tree_children: &[iced::advanced::widget::Tree],
    layout: iced::advanced::Layout<'_>,
    renderer: &mut Renderer,
    theme: &Theme,
    style: &iced::advanced::renderer::Style,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
    placements: &[slipway_core::ChildPlacement],
    frame: &FrameIdentity,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let mut visitor = IcedRuntimeChildDrawVisitor::<Theme, Renderer, W::AppMessage> {
        tree_children,
        layout,
        renderer,
        theme,
        style,
        cursor,
        viewport,
        placements,
        next_tree_index: 0,
        next_layout_index: 0,
        current_order_index: None,
        frame_seed,
        dirty_scopes,
        _phantom: std::marker::PhantomData,
    };
    let _ = frame;
    widget.visit_iced_authored_children(external, local, &mut visitor);
}

#[allow(clippy::too_many_arguments)]
fn draw_iced_runtime_widget_local<W, Theme, Renderer>(
    widget: &SlipwayIcedRuntimeWidget<'_, W>,
    tree: &iced::advanced::widget::Tree,
    renderer: &mut Renderer,
    theme: &Theme,
    style: &iced::advanced::renderer::Style,
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
    let bounds = layout.bounds();
    let Some(presentation) = state.presentation.as_ref() else {
        return;
    };

    let root_placements = root_child_placements_excluding_native_scroll(presentation);
    if presentation.can_draw_visible_paint() {
        widget.record_presented_viewport(presentation.layout_input.viewport.into_rect());
        for op in &presentation.local_paint {
            draw_paint_op(renderer, bounds.position(), op);
        }
        queue_iced_explicit_layer_paint_jobs(presentation, bounds.position());
    }
    draw_iced_runtime_children::<W, Theme, Renderer>(
        widget.widget,
        widget.external,
        widget.local,
        &tree.children,
        layout,
        renderer,
        theme,
        style,
        cursor,
        viewport,
        root_placements.as_ref(),
        &presentation.frame,
        widget.frame_seed.as_ref(),
        widget.dirty_scopes.as_deref(),
    );
    let text_input_regions = text_input_focus_regions(presentation);
    let text_editor_regions = text_editor_focus_regions(presentation);
    let text_region_count = text_input_regions.len() + text_editor_regions.len();
    draw_iced_scroll_regions::<W, Theme, Renderer>(
        &state.scroll_region_trees,
        widget.widget,
        widget.external,
        widget.local,
        presentation,
        layout,
        renderer,
        theme,
        style,
        cursor,
        viewport,
        text_region_count,
        widget.frame_seed.as_ref(),
        widget.dirty_scopes.as_deref(),
    );
    draw_iced_text_editor_regions::<W::AppMessage, Theme, Renderer>(
        &state.text_editor_trees,
        presentation,
        text_editor_regions.as_ref(),
        text_input_regions.len(),
        layout,
        renderer,
        theme,
        style,
        cursor,
        viewport,
    );
    draw_iced_text_input_regions::<W::AppMessage, Theme, Renderer>(
        &state.text_edit_trees,
        presentation,
        text_input_regions.as_ref(),
        layout,
        renderer,
        theme,
        style,
        cursor,
        viewport,
    );
}

fn draw_iced_standalone_presentation<Renderer>(
    renderer: &mut Renderer,
    origin: iced::Point,
    presentation: &IcedPresentationState,
) where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    if !presentation.can_draw_visible_paint() {
        return;
    }

    with_iced_surface_global_paint_queue(renderer, |renderer| {
        for op in &presentation.local_paint {
            draw_paint_op(renderer, origin, op);
        }
        queue_iced_explicit_layer_paint_jobs(presentation, origin);
    });
}

struct IcedRuntimeChildOperateVisitor<'a, 'b, Theme, Renderer, AppMessage> {
    tree_children: &'a mut [iced::advanced::widget::Tree],
    layout: iced::advanced::Layout<'b>,
    renderer: &'a Renderer,
    operation: &'a mut dyn iced::advanced::widget::Operation,
    placements: &'a [slipway_core::ChildPlacement],
    next_tree_index: usize,
    next_layout_index: usize,
    current_order_index: Option<usize>,
    frame_seed: Option<&'a FrameIdentity>,
    dirty_scopes: Option<&'a [IcedDirtyScope]>,
    _phantom: std::marker::PhantomData<(Theme, AppMessage)>,
}

impl<ExternalState, AppMessage, Theme, Renderer>
    SlipwayIcedWidgetListVisitor<ExternalState, AppMessage>
    for IcedRuntimeChildOperateVisitor<'_, '_, Theme, Renderer, AppMessage>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn set_iced_child_order_index(&mut self, index: usize) {
        self.current_order_index = Some(index);
    }

    fn visit_iced_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayIcedBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);
        let layout_index = tree_index;
        self.next_layout_index = self.next_layout_index.max(layout_index + 1);

        if iced_child_placement_for_slot(self.placements, &widget.id(), &slot).is_none() {
            return;
        }

        let Some(tree) = self.tree_children.get_mut(tree_index) else {
            return;
        };

        if layout_index >= self.layout.children().len() {
            return;
        }

        let mut child = iced_runtime_child_widget(
            widget,
            external,
            local,
            slot,
            self.frame_seed,
            self.dirty_scopes,
        );
        <SlipwayIcedRuntimeWidget<'_, W> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::operate(
            &mut child,
            tree,
            self.layout.child(layout_index),
            self.renderer,
            self.operation,
        );
    }

    fn visit_iced_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayIcedNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);
        let layout_index = tree_index;
        self.next_layout_index = self.next_layout_index.max(layout_index + 1);

        let Some(placement) = iced_child_placement_for_slot(self.placements, &widget.id(), &slot)
        else {
            return;
        };

        let Some(tree) = self.tree_children.get_mut(tree_index) else {
            return;
        };

        if layout_index >= self.layout.children().len() {
            return;
        }

        let mut element = widget.iced_native_element::<Theme, Renderer>(
            external,
            local,
            IcedNativeWidgetContext {
                slot: &slot,
                frame_seed: self.frame_seed,
                placement: Some(placement.bounds.into_rect()),
            },
        );
        element.as_widget_mut().operate(
            tree,
            self.layout.child(layout_index),
            self.renderer,
            self.operation,
        );
    }
}

fn operate_iced_runtime_children<W, Theme, Renderer>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    tree_children: &mut [iced::advanced::widget::Tree],
    layout: iced::advanced::Layout<'_>,
    renderer: &Renderer,
    operation: &mut dyn iced::advanced::widget::Operation,
    placements: &[slipway_core::ChildPlacement],
    frame: &FrameIdentity,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let mut visitor = IcedRuntimeChildOperateVisitor::<Theme, Renderer, W::AppMessage> {
        tree_children,
        layout,
        renderer,
        operation,
        placements,
        next_tree_index: 0,
        next_layout_index: 0,
        current_order_index: None,
        frame_seed,
        dirty_scopes,
        _phantom: std::marker::PhantomData,
    };
    widget.visit_iced_authored_children_in_paint_order(
        external,
        local,
        frame,
        placements,
        IcedChildTraversalOrder::BackToFront,
        &mut visitor,
    );
}

struct IcedRuntimeChildMouseVisitor<'a, 'b, Theme, Renderer, AppMessage> {
    tree_children: &'a [iced::advanced::widget::Tree],
    layout: iced::advanced::Layout<'b>,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &'a iced::Rectangle,
    renderer: &'a Renderer,
    placements: &'a [slipway_core::ChildPlacement],
    interaction: iced::advanced::mouse::Interaction,
    next_tree_index: usize,
    next_layout_index: usize,
    current_order_index: Option<usize>,
    frame_seed: Option<&'a FrameIdentity>,
    dirty_scopes: Option<&'a [IcedDirtyScope]>,
    _phantom: std::marker::PhantomData<(Theme, AppMessage)>,
}

impl<ExternalState, AppMessage, Theme, Renderer>
    SlipwayIcedWidgetListVisitor<ExternalState, AppMessage>
    for IcedRuntimeChildMouseVisitor<'_, '_, Theme, Renderer, AppMessage>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn set_iced_child_order_index(&mut self, index: usize) {
        self.current_order_index = Some(index);
    }

    fn visit_iced_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayIcedBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);
        let layout_index = tree_index;
        self.next_layout_index = self.next_layout_index.max(layout_index + 1);

        if self.interaction != iced::advanced::mouse::Interaction::None {
            return;
        }

        if iced_child_placement_for_slot(self.placements, &widget.id(), &slot).is_none() {
            return;
        }

        let Some(tree) = self.tree_children.get(tree_index) else {
            return;
        };

        if layout_index >= self.layout.children().len() {
            return;
        }

        let child = iced_runtime_child_widget(
            widget,
            external,
            local,
            slot,
            self.frame_seed,
            self.dirty_scopes,
        );
        self.interaction = <SlipwayIcedRuntimeWidget<'_, W> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::mouse_interaction(
            &child,
            tree,
            self.layout.child(layout_index),
            self.cursor,
            self.viewport,
            self.renderer,
        );
    }

    fn visit_iced_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayIcedNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);
        let layout_index = tree_index;
        self.next_layout_index = self.next_layout_index.max(layout_index + 1);

        if self.interaction != iced::advanced::mouse::Interaction::None {
            return;
        }

        let Some(placement) = iced_child_placement_for_slot(self.placements, &widget.id(), &slot)
        else {
            return;
        };

        let Some(tree) = self.tree_children.get(tree_index) else {
            return;
        };

        if layout_index >= self.layout.children().len() {
            return;
        }

        let element = widget.iced_native_element::<Theme, Renderer>(
            external,
            local,
            IcedNativeWidgetContext {
                slot: &slot,
                frame_seed: self.frame_seed,
                placement: Some(placement.bounds.into_rect()),
            },
        );
        self.interaction = element.as_widget().mouse_interaction(
            tree,
            self.layout.child(layout_index),
            self.cursor,
            self.viewport,
            self.renderer,
        );
    }
}

fn iced_runtime_children_mouse_interaction<W, Theme, Renderer>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    tree_children: &[iced::advanced::widget::Tree],
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
    renderer: &Renderer,
    placements: &[slipway_core::ChildPlacement],
    frame: &FrameIdentity,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) -> iced::advanced::mouse::Interaction
where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let mut visitor = IcedRuntimeChildMouseVisitor::<Theme, Renderer, W::AppMessage> {
        tree_children,
        layout,
        cursor,
        viewport,
        renderer,
        placements,
        interaction: iced::advanced::mouse::Interaction::None,
        next_tree_index: 0,
        next_layout_index: 0,
        current_order_index: None,
        frame_seed,
        dirty_scopes,
        _phantom: std::marker::PhantomData,
    };
    widget.visit_iced_authored_children_in_paint_order(
        external,
        local,
        frame,
        placements,
        IcedChildTraversalOrder::FrontToBack,
        &mut visitor,
    );
    visitor.interaction
}

struct IcedRuntimeChildUpdateVisitor<'a, 'b, 'c, Theme, Renderer, AppMessage> {
    tree_children: &'a mut [iced::advanced::widget::Tree],
    event: &'a iced::Event,
    layout: iced::advanced::Layout<'b>,
    cursor: iced::advanced::mouse::Cursor,
    renderer: &'a Renderer,
    clipboard: &'a mut dyn iced::advanced::Clipboard,
    shell: &'a mut iced::advanced::Shell<'c, SlipwayIcedRuntimeMessage<AppMessage>>,
    viewport: &'a iced::Rectangle,
    placements: &'a [slipway_core::ChildPlacement],
    next_tree_index: usize,
    next_layout_index: usize,
    current_order_index: Option<usize>,
    frame_seed: Option<&'a FrameIdentity>,
    dirty_scopes: Option<&'a [IcedDirtyScope]>,
    _phantom: std::marker::PhantomData<Theme>,
}

impl<ExternalState, AppMessage, Theme, Renderer>
    SlipwayIcedWidgetListVisitor<ExternalState, AppMessage>
    for IcedRuntimeChildUpdateVisitor<'_, '_, '_, Theme, Renderer, AppMessage>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn set_iced_child_order_index(&mut self, index: usize) {
        self.current_order_index = Some(index);
    }

    fn visit_iced_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayIcedBackendChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        if self.shell.is_event_captured() {
            return;
        }

        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);
        let layout_index = tree_index;
        self.next_layout_index = self.next_layout_index.max(layout_index + 1);

        if iced_child_placement_for_slot(self.placements, &widget.id(), &slot).is_none() {
            return;
        }

        let Some(tree) = self.tree_children.get_mut(tree_index) else {
            return;
        };

        if layout_index >= self.layout.children().len() {
            return;
        }

        let mut child = iced_runtime_child_widget(
            widget,
            external,
            local,
            slot,
            self.frame_seed,
            self.dirty_scopes,
        );
        <SlipwayIcedRuntimeWidget<'_, W> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::update(
            &mut child,
            tree,
            self.event,
            self.layout.child(layout_index),
            self.cursor,
            self.renderer,
            self.clipboard,
            self.shell,
            self.viewport,
        );
    }

    fn visit_iced_native_child<N>(
        &mut self,
        widget: &N,
        external: &ExternalState,
        local: &N::LocalState,
        slot: WidgetSlotAddress,
    ) where
        N: SlipwayIcedNativeChildWidget<ExternalState = ExternalState, AppMessage = AppMessage>,
    {
        if self.shell.is_event_captured() {
            return;
        }

        let _ = self.current_order_index.take();
        let tree_index = iced_child_source_tree_index(&slot, &mut self.next_tree_index);
        let layout_index = tree_index;
        self.next_layout_index = self.next_layout_index.max(layout_index + 1);

        let Some(placement) = iced_child_placement_for_slot(self.placements, &widget.id(), &slot)
        else {
            return;
        };

        let Some(tree) = self.tree_children.get_mut(tree_index) else {
            return;
        };

        if layout_index >= self.layout.children().len() {
            return;
        }

        let mut element = widget.iced_native_element::<Theme, Renderer>(
            external,
            local,
            IcedNativeWidgetContext {
                slot: &slot,
                frame_seed: self.frame_seed,
                placement: Some(placement.bounds.into_rect()),
            },
        );
        element.as_widget_mut().update(
            tree,
            self.event,
            self.layout.child(layout_index),
            self.cursor,
            self.renderer,
            self.clipboard,
            self.shell,
            self.viewport,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn update_iced_runtime_children<W, Theme, Renderer>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    tree_children: &mut [iced::advanced::widget::Tree],
    event: &iced::Event,
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    renderer: &Renderer,
    clipboard: &mut dyn iced::advanced::Clipboard,
    shell: &mut iced::advanced::Shell<'_, SlipwayIcedRuntimeMessage<W::AppMessage>>,
    viewport: &iced::Rectangle,
    placements: &[slipway_core::ChildPlacement],
    frame: &FrameIdentity,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let mut visitor = IcedRuntimeChildUpdateVisitor::<Theme, Renderer, W::AppMessage> {
        tree_children,
        event,
        layout,
        cursor,
        renderer,
        clipboard,
        shell,
        viewport,
        placements,
        next_tree_index: 0,
        next_layout_index: 0,
        current_order_index: None,
        frame_seed,
        dirty_scopes,
        _phantom: std::marker::PhantomData,
    };
    widget.visit_iced_authored_children_in_paint_order(
        external,
        local,
        frame,
        placements,
        IcedChildTraversalOrder::FrontToBack,
        &mut visitor,
    );
}

fn text_input_layout_start(
    layout: iced::advanced::Layout<'_>,
    region_count: usize,
) -> Option<usize> {
    let child_count = layout.children().len();
    (region_count <= child_count).then_some(child_count - region_count)
}

fn operate_iced_text_input_regions<AppMessage, Theme, Renderer>(
    trees: &mut [iced::advanced::widget::Tree],
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
    layout: iced::advanced::Layout<'_>,
    renderer: &Renderer,
    operation: &mut dyn iced::advanced::widget::Operation,
) where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let Some(start) = text_input_layout_start(layout, regions.len()) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get_mut(index) else {
            continue;
        };
        let mut input =
            iced_text_input_widget_for_region::<AppMessage, Theme, Renderer>(presentation, region);
        <iced::widget::TextInput<
            '_,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::operate(&mut input, tree, layout.child(start + index), renderer, operation);
    }
}

#[allow(clippy::too_many_arguments)]
fn update_iced_text_input_regions<AppMessage, Theme, Renderer>(
    trees: &mut [iced::advanced::widget::Tree],
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
    event: &iced::Event,
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    renderer: &Renderer,
    clipboard: &mut dyn iced::advanced::Clipboard,
    shell: &mut iced::advanced::Shell<'_, SlipwayIcedRuntimeMessage<AppMessage>>,
    viewport: &iced::Rectangle,
) where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let Some(start) = text_input_layout_start(layout, regions.len()) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        if shell.is_event_captured() {
            return;
        }
        let Some(tree) = trees.get_mut(index) else {
            continue;
        };
        let mut input =
            iced_text_input_widget_for_region::<AppMessage, Theme, Renderer>(presentation, region);
        let child_layout = layout.child(start + index);
        trace_iced_text_input_region_for_ime(
            event,
            region,
            child_layout.bounds(),
            cursor.position(),
            cursor.is_over(child_layout.bounds()),
        );
        let input_method_before = shell.input_method().clone();
        <iced::widget::TextInput<
            '_,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::update(
            &mut input,
            tree,
            event,
            child_layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.input_method() != &input_method_before
            && matches!(shell.input_method(), iced_core::InputMethod::Enabled { .. })
            && let Some(style) = preedit_overlay_style_for_region(region)
        {
            shell.publish(SlipwayIcedRuntimeMessage::PreeditStyle(style));
        }
    }
}

fn trace_iced_text_input_region_for_ime(
    event: &iced::Event,
    region: &FocusRegionDeclaration,
    bounds: iced::Rectangle,
    cursor: Option<iced::Point>,
    cursor_over: bool,
) {
    let interesting = matches!(
        event,
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_))
            | iced::Event::Mouse(iced::mouse::Event::ButtonReleased(_))
            | iced::Event::InputMethod(_)
            | iced::Event::Keyboard(_)
    );
    if !interesting || std::env::var_os("SLIPWAY_IME_TRACE").is_none() {
        return;
    }

    let message = format!(
        "[slipway-ime] text-input-region event={} target={} region={} bounds=({}, {}, {}, {}) cursor={:?} over={}",
        iced_event_kind_for_ime_trace(event),
        region.target.as_str(),
        region.id.as_str(),
        bounds.x,
        bounds.y,
        bounds.width,
        bounds.height,
        cursor,
        cursor_over
    );

    if let Some(path) = std::env::var_os("SLIPWAY_IME_TRACE_FILE") {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            use std::io::Write as _;
            let _ = writeln!(file, "{message}");
        }
    } else {
        eprintln!("{message}");
    }
}

fn iced_event_kind_for_ime_trace(event: &iced::Event) -> &'static str {
    match event {
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(_)) => "mouse.press",
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(_)) => "mouse.release",
        iced::Event::InputMethod(_) => "input-method",
        iced::Event::Keyboard(_) => "keyboard",
        _ => "other",
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_iced_text_input_regions<AppMessage, Theme, Renderer>(
    trees: &[iced::advanced::widget::Tree],
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
    layout: iced::advanced::Layout<'_>,
    renderer: &mut Renderer,
    theme: &Theme,
    style: &iced::advanced::renderer::Style,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
) where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let Some(start) = text_input_layout_start(layout, regions.len()) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get(index) else {
            continue;
        };
        let input =
            iced_text_input_widget_for_region::<AppMessage, Theme, Renderer>(presentation, region);
        <iced::widget::TextInput<
            '_,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::draw(
            &input,
            tree,
            renderer,
            theme,
            style,
            layout.child(start + index),
            cursor,
            viewport,
        );
    }
}

fn iced_text_input_regions_mouse_interaction<AppMessage, Theme, Renderer>(
    trees: &[iced::advanced::widget::Tree],
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
    renderer: &Renderer,
) -> iced::advanced::mouse::Interaction
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let Some(start) = text_input_layout_start(layout, regions.len()) else {
        return iced::advanced::mouse::Interaction::None;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get(index) else {
            continue;
        };
        let input =
            iced_text_input_widget_for_region::<AppMessage, Theme, Renderer>(presentation, region);
        let interaction = <iced::widget::TextInput<
            '_,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::mouse_interaction(
            &input,
            tree,
            layout.child(start + index),
            cursor,
            viewport,
            renderer,
        );
        if interaction != iced::advanced::mouse::Interaction::None {
            return interaction;
        }
    }
    iced::advanced::mouse::Interaction::None
}

struct IcedTextEditorRegionState<Renderer>
where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    id: PresentationRegionId,
    source_text: String,
    content: iced::widget::text_editor::Content<Renderer>,
    native_tree: iced::advanced::widget::Tree,
}

struct IcedTextEditorRegionWidget<'a, AppMessage> {
    presentation: &'a IcedPresentationState,
    region: &'a FocusRegionDeclaration,
    _phantom: std::marker::PhantomData<AppMessage>,
}

impl<'a, AppMessage> IcedTextEditorRegionWidget<'a, AppMessage> {
    fn new(presentation: &'a IcedPresentationState, region: &'a FocusRegionDeclaration) -> Self {
        Self {
            presentation,
            region,
            _phantom: std::marker::PhantomData,
        }
    }
}

fn iced_text_editor_id(region: &FocusRegionDeclaration) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(format!(
        "slipway-text-editor:{}:{}",
        region.target.as_str(),
        region.id.as_str()
    ))
}

fn iced_text_editor_text(region: &FocusRegionDeclaration) -> &str {
    region
        .text_edit
        .as_ref()
        .map(|text_edit| text_edit.buffer.text.as_str())
        .unwrap_or("")
}

fn iced_text_editor_widget_for_region<'a, AppMessage, Theme, Renderer, F>(
    region: &'a FocusRegionDeclaration,
    content: &'a iced::widget::text_editor::Content<Renderer>,
    on_action: F,
) -> iced::widget::TextEditor<
    'a,
    iced::advanced::text::highlighter::PlainText,
    SlipwayIcedRuntimeMessage<AppMessage>,
    Theme,
    Renderer,
>
where
    AppMessage: Clone + 'a,
    Theme: SlipwayIcedThemeContract + 'a,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'a + 'static,
    F: Fn(iced::advanced::text::editor::Action) -> SlipwayIcedRuntimeMessage<AppMessage> + 'a,
{
    let size = region.bounds.size;
    let text_style = region
        .text_edit
        .as_ref()
        .map(|text_edit| &text_edit.typography.style)
        .unwrap_or_else(|| panic!("text editor widget requires text edit typography declaration"));
    iced::widget::TextEditor::<
        iced::advanced::text::highlighter::PlainText,
        SlipwayIcedRuntimeMessage<AppMessage>,
        Theme,
        Renderer,
    >::new(content)
    .id(iced_text_editor_id(region))
    .width(iced::Pixels(size.width.max(0.0)))
    .height(iced::Length::Fixed(size.height.max(0.0)))
    .font(iced_font(text_style))
    .size(iced::Pixels(normalized_text_size(text_style)))
    .on_action(on_action)
}

fn sync_iced_text_editor_region_state<Renderer>(
    state: &mut IcedTextEditorRegionState<Renderer>,
    region: &FocusRegionDeclaration,
) where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let source_text = iced_text_editor_text(region);
    if state.id != region.id || state.source_text != source_text {
        state.id = region.id.clone();
        state.source_text = source_text.to_string();
        state.content = iced::widget::text_editor::Content::with_text(source_text);
    }
}

impl<AppMessage, Theme, Renderer>
    iced::advanced::Widget<SlipwayIcedRuntimeMessage<AppMessage>, Theme, Renderer>
    for IcedTextEditorRegionWidget<'_, AppMessage>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<IcedTextEditorRegionState<Renderer>>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        let content =
            iced::widget::text_editor::Content::with_text(iced_text_editor_text(self.region));
        let native_tree = {
            let editor = iced_text_editor_widget_for_region::<AppMessage, Theme, Renderer, _>(
                self.region,
                &content,
                |_| SlipwayIcedRuntimeMessage::Noop,
            );
            iced::advanced::widget::Tree::new(
                &editor
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<AppMessage>,
                        Theme,
                        Renderer,
                    >,
            )
        };
        iced::advanced::widget::tree::State::new(IcedTextEditorRegionState::<Renderer> {
            id: self.region.id.clone(),
            source_text: iced_text_editor_text(self.region).to_string(),
            content,
            native_tree,
        })
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        let state = tree
            .state
            .downcast_mut::<IcedTextEditorRegionState<Renderer>>();
        sync_iced_text_editor_region_state(state, self.region);
        let editor = iced_text_editor_widget_for_region::<AppMessage, Theme, Renderer, _>(
            self.region,
            &state.content,
            |_| SlipwayIcedRuntimeMessage::Noop,
        );
        state.native_tree.diff(
            &editor
                as &dyn iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<AppMessage>,
                    Theme,
                    Renderer,
                >,
        );
    }

    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size {
            width: iced::Length::Fixed(self.region.bounds.size.width.max(0.0)),
            height: iced::Length::Fixed(self.region.bounds.size.height.max(0.0)),
        }
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        _limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let state = tree
            .state
            .downcast_mut::<IcedTextEditorRegionState<Renderer>>();
        sync_iced_text_editor_region_state(state, self.region);
        let mut editor = iced_text_editor_widget_for_region::<AppMessage, Theme, Renderer, _>(
            self.region,
            &state.content,
            |_| SlipwayIcedRuntimeMessage::Noop,
        );
        let size = iced::Size::new(
            self.region.bounds.size.width.max(0.0),
            self.region.bounds.size.height.max(0.0),
        );
        let limits = iced::advanced::layout::Limits::new(size, size);
        <iced::widget::TextEditor<
            '_,
            iced::advanced::text::highlighter::PlainText,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<SlipwayIcedRuntimeMessage<AppMessage>, Theme, Renderer>>::layout(
            &mut editor,
            &mut state.native_tree,
            renderer,
            &limits,
        )
        .move_to(iced_point(region_root_local_rect(
            self.presentation,
            &self.region.target,
            self.region.address.as_ref(),
            self.region.bounds.into_rect(),
        )
        .origin))
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, SlipwayIcedRuntimeMessage<AppMessage>>,
        viewport: &iced::Rectangle,
    ) {
        let state = tree
            .state
            .downcast_mut::<IcedTextEditorRegionState<Renderer>>();
        sync_iced_text_editor_region_state(state, self.region);
        let captured_action: std::cell::RefCell<Option<iced::advanced::text::editor::Action>> =
            std::cell::RefCell::new(None);
        let target = self.region.target.clone();
        let target_slot = self.region.address.clone();
        let selection_before = self
            .region
            .text_edit
            .as_ref()
            .and_then(|text_edit| text_edit.selection.selection.clone());
        {
            let mut editor = iced_text_editor_widget_for_region::<AppMessage, Theme, Renderer, _>(
                self.region,
                &state.content,
                |action| {
                    *captured_action.borrow_mut() = Some(action);
                    SlipwayIcedRuntimeMessage::Noop
                },
            );
            <iced::widget::TextEditor<
                '_,
                iced::advanced::text::highlighter::PlainText,
                SlipwayIcedRuntimeMessage<AppMessage>,
                Theme,
                Renderer,
            > as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<AppMessage>,
                Theme,
                Renderer,
            >>::update(
                &mut editor,
                &mut state.native_tree,
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }
        if let Some(action) = captured_action.into_inner() {
            let before = state.content.text();
            state.content.perform(action);
            let after = state.content.text();
            state.source_text = after.clone();
            if before != after {
                let event = InputEvent::TextEdit(slipway_core::TextEditEvent {
                    target,
                    target_slot,
                    kind: TextEditKind::ReplaceBuffer,
                    text: Some(after),
                    selection_before,
                    selection_after: None,
                });
                shell.publish(SlipwayIcedRuntimeMessage::BackendInput(
                    backend_focus_input_event(
                        self.presentation,
                        self.region,
                        DeclaredEventDispatchKind::Text,
                        None,
                        event,
                    ),
                ));
                shell.request_redraw();
            }
        }
    }

    fn operate(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        let state = tree
            .state
            .downcast_mut::<IcedTextEditorRegionState<Renderer>>();
        sync_iced_text_editor_region_state(state, self.region);
        let mut editor = iced_text_editor_widget_for_region::<AppMessage, Theme, Renderer, _>(
            self.region,
            &state.content,
            |_| SlipwayIcedRuntimeMessage::Noop,
        );
        <iced::widget::TextEditor<
            '_,
            iced::advanced::text::highlighter::PlainText,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::operate(
            &mut editor,
            &mut state.native_tree,
            layout,
            renderer,
            operation,
        );
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        let state = tree
            .state
            .downcast_ref::<IcedTextEditorRegionState<Renderer>>();
        let editor = iced_text_editor_widget_for_region::<AppMessage, Theme, Renderer, _>(
            self.region,
            &state.content,
            |_| SlipwayIcedRuntimeMessage::Noop,
        );
        <iced::widget::TextEditor<
            '_,
            iced::advanced::text::highlighter::PlainText,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<SlipwayIcedRuntimeMessage<AppMessage>, Theme, Renderer>>::draw(
            &editor,
            &state.native_tree,
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        let state = tree
            .state
            .downcast_ref::<IcedTextEditorRegionState<Renderer>>();
        let editor = iced_text_editor_widget_for_region::<AppMessage, Theme, Renderer, _>(
            self.region,
            &state.content,
            |_| SlipwayIcedRuntimeMessage::Noop,
        );
        <iced::widget::TextEditor<
            '_,
            iced::advanced::text::highlighter::PlainText,
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<SlipwayIcedRuntimeMessage<AppMessage>, Theme, Renderer>>::mouse_interaction(
            &editor,
            &state.native_tree,
            layout,
            cursor,
            viewport,
            renderer,
        )
    }
}

fn reconcile_iced_text_editor_trees<AppMessage, Theme, Renderer>(
    trees: &mut Vec<iced::advanced::widget::Tree>,
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
) where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    for (index, region) in regions.iter().enumerate() {
        let widget = IcedTextEditorRegionWidget::<AppMessage>::new(presentation, region);
        if let Some(tree) = trees.get_mut(index) {
            tree.diff(
                &widget
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<AppMessage>,
                        Theme,
                        Renderer,
                    >,
            );
        } else {
            trees.push(iced::advanced::widget::Tree::new(
                &widget
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<AppMessage>,
                        Theme,
                        Renderer,
                    >,
            ));
        }
    }
    trees.truncate(regions.len());
}

fn layout_iced_text_editor_regions<AppMessage, Theme, Renderer>(
    trees: &mut Vec<iced::advanced::widget::Tree>,
    renderer: &Renderer,
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
) -> Vec<iced::advanced::layout::Node>
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    reconcile_iced_text_editor_trees::<AppMessage, Theme, Renderer>(trees, presentation, regions);
    let mut nodes = Vec::with_capacity(regions.len());
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get_mut(index) else {
            continue;
        };
        let mut widget = IcedTextEditorRegionWidget::<AppMessage>::new(presentation, region);
        let size = iced::Size::new(
            region.bounds.size.width.max(0.0),
            region.bounds.size.height.max(0.0),
        );
        let limits = iced::advanced::layout::Limits::new(size, size);
        nodes.push(
            <IcedTextEditorRegionWidget<'_, AppMessage> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<AppMessage>,
                Theme,
                Renderer,
            >>::layout(&mut widget, tree, renderer, &limits),
        );
    }
    nodes
}

fn text_editor_layout_start(
    layout: iced::advanced::Layout<'_>,
    singleline_count: usize,
    multiline_count: usize,
) -> Option<usize> {
    let child_count = layout.children().len();
    singleline_count
        .checked_add(multiline_count)
        .and_then(|tail_count| (tail_count <= child_count).then_some(child_count - tail_count))
}

fn operate_iced_text_editor_regions<AppMessage, Theme, Renderer>(
    trees: &mut [iced::advanced::widget::Tree],
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
    singleline_count: usize,
    layout: iced::advanced::Layout<'_>,
    renderer: &Renderer,
    operation: &mut dyn iced::advanced::widget::Operation,
) where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let Some(start) = text_editor_layout_start(layout, singleline_count, regions.len()) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get_mut(index) else {
            continue;
        };
        let mut widget = IcedTextEditorRegionWidget::<AppMessage>::new(presentation, region);
        <IcedTextEditorRegionWidget<'_, AppMessage> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::operate(
            &mut widget,
            tree,
            layout.child(start + index),
            renderer,
            operation,
        );
    }
}

fn update_iced_text_editor_regions<AppMessage, Theme, Renderer>(
    trees: &mut [iced::advanced::widget::Tree],
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
    singleline_count: usize,
    event: &iced::Event,
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    renderer: &Renderer,
    clipboard: &mut dyn iced::advanced::Clipboard,
    shell: &mut iced::advanced::Shell<'_, SlipwayIcedRuntimeMessage<AppMessage>>,
    viewport: &iced::Rectangle,
) where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let Some(start) = text_editor_layout_start(layout, singleline_count, regions.len()) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        if shell.is_event_captured() {
            return;
        }
        let Some(tree) = trees.get_mut(index) else {
            continue;
        };
        let mut widget = IcedTextEditorRegionWidget::<AppMessage>::new(presentation, region);
        let input_method_before = shell.input_method().clone();
        <IcedTextEditorRegionWidget<'_, AppMessage> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::update(
            &mut widget,
            tree,
            event,
            layout.child(start + index),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
        if shell.input_method() != &input_method_before
            && matches!(shell.input_method(), iced_core::InputMethod::Enabled { .. })
            && let Some(style) = preedit_overlay_style_for_region(region)
        {
            shell.publish(SlipwayIcedRuntimeMessage::PreeditStyle(style));
        }
    }
}

fn draw_iced_text_editor_regions<AppMessage, Theme, Renderer>(
    trees: &[iced::advanced::widget::Tree],
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
    singleline_count: usize,
    layout: iced::advanced::Layout<'_>,
    renderer: &mut Renderer,
    theme: &Theme,
    style: &iced::advanced::renderer::Style,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
) where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let Some(start) = text_editor_layout_start(layout, singleline_count, regions.len()) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get(index) else {
            continue;
        };
        let widget = IcedTextEditorRegionWidget::<AppMessage>::new(presentation, region);
        <IcedTextEditorRegionWidget<'_, AppMessage> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::draw(
            &widget,
            tree,
            renderer,
            theme,
            style,
            layout.child(start + index),
            cursor,
            viewport,
        );
    }
}

fn iced_text_editor_regions_mouse_interaction<AppMessage, Theme, Renderer>(
    trees: &[iced::advanced::widget::Tree],
    presentation: &IcedPresentationState,
    regions: &[FocusRegionDeclaration],
    singleline_count: usize,
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
    renderer: &Renderer,
) -> iced::advanced::mouse::Interaction
where
    AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let Some(start) = text_editor_layout_start(layout, singleline_count, regions.len()) else {
        return iced::advanced::mouse::Interaction::None;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get(index) else {
            continue;
        };
        let widget = IcedTextEditorRegionWidget::<AppMessage>::new(presentation, region);
        let interaction = <IcedTextEditorRegionWidget<'_, AppMessage> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<AppMessage>,
            Theme,
            Renderer,
        >>::mouse_interaction(
            &widget,
            tree,
            layout.child(start + index),
            cursor,
            viewport,
            renderer,
        );
        if interaction != iced::advanced::mouse::Interaction::None {
            return interaction;
        }
    }
    iced::advanced::mouse::Interaction::None
}

#[derive(Clone, Debug, PartialEq)]
struct IcedScrollContentTreeState {
    id: PresentationRegionId,
}

struct IcedScrollContentWidget<'a, W>
where
    W: SlipwayIcedBackendChildWidget,
{
    widget: &'a W,
    external: &'a W::ExternalState,
    local: &'a W::LocalState,
    presentation: &'a IcedPresentationState,
    scroll: &'a ScrollRegionDeclaration,
    frame_seed: Option<&'a FrameIdentity>,
    dirty_scopes: Option<&'a [IcedDirtyScope]>,
}

impl<'a, W, Theme, Renderer> From<IcedScrollContentWidget<'a, W>>
    for iced::Element<'a, SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>
where
    W: SlipwayIcedBackendChildWidget + 'a,
    W::ExternalState: 'a,
    W::LocalState: 'a,
    W::AppMessage: Clone + 'a,
    Theme: SlipwayIcedThemeContract + 'a,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'a + 'static,
{
    fn from(widget: IcedScrollContentWidget<'a, W>) -> Self {
        iced::Element::new(widget)
    }
}

impl<W, Theme, Renderer>
    iced::advanced::Widget<SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>
    for IcedScrollContentWidget<'_, W>
where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<IcedScrollContentTreeState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(IcedScrollContentTreeState {
            id: self.scroll.id.clone(),
        })
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        let placements = adjusted_scroll_child_placements(self.presentation, self.scroll);
        iced_runtime_child_trees::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            self.frame_seed,
            self.dirty_scopes,
            Some((&self.presentation.frame, &placements)),
        )
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        let state = tree.state.downcast_mut::<IcedScrollContentTreeState>();
        if state.id != self.scroll.id {
            state.id = self.scroll.id.clone();
            tree.children.clear();
        }
        reconcile_iced_runtime_child_trees::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            self.frame_seed,
            self.dirty_scopes,
            Some((
                &self.presentation.frame,
                &adjusted_scroll_child_placements(self.presentation, self.scroll),
            )),
        );
    }

    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size {
            width: iced::Length::Fixed(self.scroll.content_bounds.size.width.max(0.0)),
            height: iced::Length::Fixed(self.scroll.content_bounds.size.height.max(0.0)),
        }
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let placements = adjusted_scroll_child_placements(self.presentation, self.scroll);
        let child_nodes = layout_iced_runtime_children::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            renderer,
            &placements,
            &self.presentation.frame,
            self.frame_seed,
            self.dirty_scopes,
        );
        let intrinsic = iced::Size::new(
            self.scroll.content_bounds.size.width.max(0.0),
            self.scroll.content_bounds.size.height.max(0.0),
        );
        iced::advanced::layout::Node::with_children(
            limits.resolve(
                iced::Length::Fixed(intrinsic.width),
                iced::Length::Fixed(intrinsic.height),
                intrinsic,
            ),
            child_nodes,
        )
    }

    fn operate(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        let placements = adjusted_scroll_child_placements(self.presentation, self.scroll);
        operate_iced_runtime_children::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            layout,
            renderer,
            operation,
            &placements,
            &self.presentation.frame,
            self.frame_seed,
            self.dirty_scopes,
        );
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, SlipwayIcedRuntimeMessage<W::AppMessage>>,
        viewport: &iced::Rectangle,
    ) {
        let placements = adjusted_scroll_child_placements(self.presentation, self.scroll);
        update_iced_runtime_children::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
            &placements,
            &self.presentation.frame,
            self.frame_seed,
            self.dirty_scopes,
        );
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        let placements = adjusted_scroll_child_placements(self.presentation, self.scroll);
        let context = scroll_content_paint_queue_context(layout, viewport);
        with_iced_global_paint_context(context, || {
            draw_iced_runtime_children::<W, Theme, Renderer>(
                self.widget,
                self.external,
                self.local,
                &tree.children,
                layout,
                renderer,
                theme,
                style,
                cursor,
                viewport,
                &placements,
                &self.presentation.frame,
                self.frame_seed,
                self.dirty_scopes,
            );
        });
    }

    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        let placements = adjusted_scroll_child_placements(self.presentation, self.scroll);
        iced_runtime_children_mouse_interaction::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &tree.children,
            layout,
            cursor,
            viewport,
            renderer,
            &placements,
            &self.presentation.frame,
            self.frame_seed,
            self.dirty_scopes,
        )
    }
}

fn native_iced_scroll_regions_for_layout(
    layout: &LayoutOutput,
    scroll_regions: &[ScrollRegionDeclaration],
) -> Vec<ScrollRegionDeclaration> {
    scroll_regions
        .iter()
        .filter(|scroll| scroll.enabled && scroll_region_has_real_child_placements(layout, scroll))
        .cloned()
        .collect()
}

fn native_iced_scroll_regions(
    presentation: &IcedPresentationState,
) -> Arc<[ScrollRegionDeclaration]> {
    Arc::clone(&presentation.native_scroll_regions)
}

fn root_child_placements_excluding_native_scroll_for_layout(
    layout: &LayoutOutput,
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
) -> Vec<slipway_core::ChildPlacement> {
    layout
        .child_placements
        .iter()
        .filter(|placement| {
            !scroll_regions.iter().any(|scroll| {
                scroll.enabled
                    && scroll_contains_placement_for_layout(geometry_index, scroll, placement)
            })
        })
        .cloned()
        .collect()
}

fn root_child_placements_excluding_native_scroll(
    presentation: &IcedPresentationState,
) -> Arc<[slipway_core::ChildPlacement]> {
    Arc::clone(&presentation.root_child_placements)
}

fn adjusted_scroll_child_placements(
    presentation: &IcedPresentationState,
    scroll: &ScrollRegionDeclaration,
) -> Vec<slipway_core::ChildPlacement> {
    presentation
        .layout
        .child_placements
        .iter()
        .filter(|placement| scroll_contains_placement(presentation, scroll, placement))
        .cloned()
        .map(|mut placement| {
            let mut bounds = placement.bounds.into_rect();
            let content_bounds = region_root_local_rect(
                presentation,
                &scroll.target,
                scroll.address.as_ref(),
                scroll.content_bounds.into_rect(),
            );
            bounds.origin.x -= content_bounds.origin.x;
            bounds.origin.y -= content_bounds.origin.y;
            placement.bounds = slipway_core::ParentLocalRect::new(bounds);
            placement
        })
        .collect()
}

fn iced_scrollable_id(region: &ScrollRegionDeclaration) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(format!(
        "slipway-scroll:{}:{}",
        region.target.as_str(),
        region.id.as_str()
    ))
}

fn iced_scroll_direction(region: &ScrollRegionDeclaration) -> iced::widget::scrollable::Direction {
    match (region.axes.horizontal, region.axes.vertical) {
        (true, true) => iced::widget::scrollable::Direction::Both {
            horizontal: iced::widget::scrollable::Scrollbar::default(),
            vertical: iced::widget::scrollable::Scrollbar::default(),
        },
        (true, false) => iced::widget::scrollable::Direction::Horizontal(
            iced::widget::scrollable::Scrollbar::default(),
        ),
        (false, true) | (false, false) => iced::widget::scrollable::Direction::Vertical(
            iced::widget::scrollable::Scrollbar::default(),
        ),
    }
}

fn iced_scrollable_widget_for_region<'a, W, Theme, Renderer>(
    widget: &'a W,
    external: &'a W::ExternalState,
    local: &'a W::LocalState,
    presentation: &'a IcedPresentationState,
    region: &'a ScrollRegionDeclaration,
    frame_seed: Option<&'a FrameIdentity>,
    dirty_scopes: Option<&'a [IcedDirtyScope]>,
) -> iced::widget::Scrollable<'a, SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>
where
    W: SlipwayIcedBackendChildWidget + 'a,
    W::ExternalState: 'a,
    W::LocalState: 'a,
    W::AppMessage: Clone + 'a,
    Theme: SlipwayIcedThemeContract + 'a,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'a + 'static,
{
    let target = region.target.clone();
    let target_slot = region.address.clone();
    let region_id = region.id.clone();
    let declared_viewport = region.viewport;
    let declared_content = region.content_bounds;
    let frame = presentation.frame.clone();
    let scroll_regions = presentation.scroll_regions.clone();
    let selected_region = region.clone();
    let content = IcedScrollContentWidget {
        widget,
        external,
        local,
        presentation,
        scroll: region,
        frame_seed,
        dirty_scopes,
    };

    iced::widget::scrollable::<SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>(content)
        .id(iced_scrollable_id(region))
        .width(iced::Length::Fixed(region.viewport.size.width.max(0.0)))
        .height(iced::Length::Fixed(region.viewport.size.height.max(0.0)))
        .direction(iced_scroll_direction(region))
        .on_scroll(move |viewport| {
            let offset = viewport.absolute_offset();
            let event = InputEvent::Scroll(ScrollEvent {
                target: target.clone(),
                target_slot: target_slot.clone(),
                region_id: region_id.clone(),
                offset_x: offset.x,
                offset_y: offset.y,
                viewport: declared_viewport,
                content_bounds: declared_content,
            });
            SlipwayIcedRuntimeMessage::BackendInput(backend_scroll_input_event_from_parts(
                frame.clone(),
                &scroll_regions,
                &selected_region,
                event,
            ))
        })
}

fn reconcile_iced_scroll_region_trees<W, Theme, Renderer>(
    trees: &mut Vec<iced::advanced::widget::Tree>,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    presentation: &IcedPresentationState,
    regions: &[ScrollRegionDeclaration],
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    for (index, region) in regions.iter().enumerate() {
        let scrollable = iced_scrollable_widget_for_region::<W, Theme, Renderer>(
            widget,
            external,
            local,
            presentation,
            region,
            frame_seed,
            dirty_scopes,
        );
        if let Some(tree) = trees.get_mut(index) {
            tree.diff(
                &scrollable
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<W::AppMessage>,
                        Theme,
                        Renderer,
                    >,
            );
        } else {
            trees.push(iced::advanced::widget::Tree::new(
                &scrollable
                    as &dyn iced::advanced::Widget<
                        SlipwayIcedRuntimeMessage<W::AppMessage>,
                        Theme,
                        Renderer,
                    >,
            ));
        }
    }
    trees.truncate(regions.len());
}

fn layout_iced_scroll_regions<W, Theme, Renderer>(
    trees: &mut Vec<iced::advanced::widget::Tree>,
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    presentation: &IcedPresentationState,
    renderer: &Renderer,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) -> Vec<iced::advanced::layout::Node>
where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let regions = native_iced_scroll_regions(presentation);
    reconcile_iced_scroll_region_trees::<W, Theme, Renderer>(
        trees,
        widget,
        external,
        local,
        presentation,
        &regions,
        frame_seed,
        dirty_scopes,
    );
    regions
        .iter()
        .enumerate()
        .filter_map(|(index, region)| {
            let tree = trees.get_mut(index)?;
            let mut scrollable = iced_scrollable_widget_for_region::<W, Theme, Renderer>(
                widget,
                external,
                local,
                presentation,
                region,
                frame_seed,
                dirty_scopes,
            );
            let size = iced::Size::new(
                region.viewport.size.width.max(0.0),
                region.viewport.size.height.max(0.0),
            );
            let limits = iced::advanced::layout::Limits::new(size, size);
            Some(
                <iced::widget::Scrollable<
                    '_,
                    SlipwayIcedRuntimeMessage<W::AppMessage>,
                    Theme,
                    Renderer,
                > as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<W::AppMessage>,
                    Theme,
                    Renderer,
                >>::layout(&mut scrollable, tree, renderer, &limits)
                .move_to(iced_point(
                    region_root_local_rect(
                        presentation,
                        &region.target,
                        region.address.as_ref(),
                        region.viewport.into_rect(),
                    )
                    .origin,
                )),
            )
        })
        .collect()
}

fn scroll_layout_start(
    layout: iced::advanced::Layout<'_>,
    scroll_count: usize,
    text_count: usize,
) -> Option<usize> {
    let child_count = layout.children().len();
    scroll_count
        .checked_add(text_count)
        .and_then(|tail_count| (tail_count <= child_count).then_some(child_count - tail_count))
}

fn operate_iced_scroll_regions<W, Theme, Renderer>(
    trees: &mut [iced::advanced::widget::Tree],
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    presentation: &IcedPresentationState,
    layout: iced::advanced::Layout<'_>,
    renderer: &Renderer,
    operation: &mut dyn iced::advanced::widget::Operation,
    text_count: usize,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let regions = native_iced_scroll_regions(presentation);
    let Some(start) = scroll_layout_start(layout, regions.len(), text_count) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get_mut(index) else {
            continue;
        };
        let mut scrollable = iced_scrollable_widget_for_region::<W, Theme, Renderer>(
            widget,
            external,
            local,
            presentation,
            region,
            frame_seed,
            dirty_scopes,
        );
        <iced::widget::Scrollable<
            '_,
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        >>::operate(
            &mut scrollable,
            tree,
            layout.child(start + index),
            renderer,
            operation,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn update_iced_scroll_regions<W, Theme, Renderer>(
    trees: &mut [iced::advanced::widget::Tree],
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    presentation: &IcedPresentationState,
    event: &iced::Event,
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    renderer: &Renderer,
    clipboard: &mut dyn iced::advanced::Clipboard,
    shell: &mut iced::advanced::Shell<'_, SlipwayIcedRuntimeMessage<W::AppMessage>>,
    viewport: &iced::Rectangle,
    text_count: usize,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let regions = native_iced_scroll_regions(presentation);
    let Some(start) = scroll_layout_start(layout, regions.len(), text_count) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        if shell.is_event_captured() {
            return;
        }
        let Some(tree) = trees.get_mut(index) else {
            continue;
        };
        let mut scrollable = iced_scrollable_widget_for_region::<W, Theme, Renderer>(
            widget,
            external,
            local,
            presentation,
            region,
            frame_seed,
            dirty_scopes,
        );
        <iced::widget::Scrollable<
            '_,
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        >>::update(
            &mut scrollable,
            tree,
            event,
            layout.child(start + index),
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_iced_scroll_regions<W, Theme, Renderer>(
    trees: &[iced::advanced::widget::Tree],
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    presentation: &IcedPresentationState,
    layout: iced::advanced::Layout<'_>,
    renderer: &mut Renderer,
    theme: &Theme,
    style: &iced::advanced::renderer::Style,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
    text_count: usize,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let regions = native_iced_scroll_regions(presentation);
    let Some(start) = scroll_layout_start(layout, regions.len(), text_count) else {
        return;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get(index) else {
            continue;
        };
        let scrollable = iced_scrollable_widget_for_region::<W, Theme, Renderer>(
            widget,
            external,
            local,
            presentation,
            region,
            frame_seed,
            dirty_scopes,
        );
        <iced::widget::Scrollable<
            '_,
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        >>::draw(
            &scrollable,
            tree,
            renderer,
            theme,
            style,
            layout.child(start + index),
            cursor,
            viewport,
        );
    }
}

fn iced_scroll_regions_mouse_interaction<W, Theme, Renderer>(
    trees: &[iced::advanced::widget::Tree],
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    presentation: &IcedPresentationState,
    layout: iced::advanced::Layout<'_>,
    cursor: iced::advanced::mouse::Cursor,
    viewport: &iced::Rectangle,
    renderer: &Renderer,
    text_count: usize,
    frame_seed: Option<&FrameIdentity>,
    dirty_scopes: Option<&[IcedDirtyScope]>,
) -> iced::advanced::mouse::Interaction
where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let regions = native_iced_scroll_regions(presentation);
    let Some(start) = scroll_layout_start(layout, regions.len(), text_count) else {
        return iced::advanced::mouse::Interaction::None;
    };
    for (index, region) in regions.iter().enumerate() {
        let Some(tree) = trees.get(index) else {
            continue;
        };
        let scrollable = iced_scrollable_widget_for_region::<W, Theme, Renderer>(
            widget,
            external,
            local,
            presentation,
            region,
            frame_seed,
            dirty_scopes,
        );
        let interaction = <iced::widget::Scrollable<
            '_,
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        > as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        >>::mouse_interaction(
            &scrollable,
            tree,
            layout.child(start + index),
            cursor,
            viewport,
            renderer,
        );
        if interaction != iced::advanced::mouse::Interaction::None {
            return interaction;
        }
    }
    iced::advanced::mouse::Interaction::None
}

fn iced_widget_id(id: &WidgetId) -> iced::advanced::widget::Id {
    iced::advanced::widget::Id::from(id.as_str().to_string())
}

fn operate_iced_root_container(
    id: &WidgetId,
    layout: iced::advanced::Layout<'_>,
    operation: &mut dyn iced::advanced::widget::Operation,
) {
    let id = iced_widget_id(id);
    operation.container(Some(&id), layout.bounds());
}

pub fn iced_runtime_widget<W>(runtime: &SlipwayRuntime<W>) -> SlipwayIcedRuntimeWidget<'_, W>
where
    W: SlipwayIcedBackendWidget,
{
    SlipwayIcedRuntimeWidget::from_runtime(runtime)
}

pub fn iced_runtime_layout_intent_widget<W>(
    runtime: &SlipwayRuntime<W>,
) -> SlipwayIcedRuntimeLayoutIntentWidget<'_, W>
where
    W: SlipwayIcedLayoutIntentBackendChildWidget,
{
    SlipwayIcedRuntimeLayoutIntentWidget::from_runtime(runtime)
}

pub fn apply_iced_runtime_message<W, F>(
    runtime: &mut SlipwayRuntime<W>,
    message: SlipwayIcedRuntimeMessage<W::AppMessage>,
    apply_app_messages: &mut F,
) -> SlipwayIcedRuntimeUpdate
where
    W: SlipwayAuthoredWidget
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy
        + SlipwayViewDefinition,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
{
    match message {
        SlipwayIcedRuntimeMessage::BackendInput(event) => {
            let report =
                runtime.apply_backend_input_event_with_app_reducer(event, apply_app_messages);

            SlipwayIcedRuntimeUpdate::Input {
                handled: report.handled,
                applied_messages: report.applied_messages,
                diagnostics: report.diagnostics,
            }
        }
        SlipwayIcedRuntimeMessage::PreeditStyle(_) => SlipwayIcedRuntimeUpdate::Noop,
        SlipwayIcedRuntimeMessage::App(message) => {
            runtime.apply_app_messages(vec![message], apply_app_messages);
            SlipwayIcedRuntimeUpdate::AppMessages {
                applied_messages: 1,
            }
        }
        SlipwayIcedRuntimeMessage::DrainDebug => SlipwayIcedRuntimeUpdate::DrainDebug,
        SlipwayIcedRuntimeMessage::Noop => SlipwayIcedRuntimeUpdate::Noop,
    }
}

pub fn run_slipway_iced_runtime_app<W, F>(
    widget: W,
    external: W::ExternalState,
    apply_app_messages: F,
) -> iced::Result
where
    W: SlipwayIcedBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: Clone + std::fmt::Debug + Send + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    run_slipway_iced_runtime_app_with_config(
        widget,
        external,
        apply_app_messages,
        SlipwayRuntimeConfig::admitted_debug(),
    )
}

pub fn run_slipway_iced_runtime_app_with_config<W, F>(
    widget: W,
    external: W::ExternalState,
    apply_app_messages: F,
    config: SlipwayRuntimeConfig,
) -> iced::Result
where
    W: SlipwayIcedBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: Clone + std::fmt::Debug + Send + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    native_runner::run_slipway_iced_runtime_app_native(widget, external, apply_app_messages, config)
}

pub fn slipway_iced_runtime_app_view<W, F>(
    state: &SlipwayIcedRuntimeApp<W, F>,
) -> SlipwayIcedRuntimeWidget<'_, W>
where
    W: SlipwayIcedBackendWidget,
    W::LocalState: Clone,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
{
    iced_runtime_widget(state.runtime()).record_presented_viewport_in(&state.presented_viewport)
}

impl<'a, W: SlipwayIcedBackendChildWidget> SlipwayIcedRuntimeWidget<'a, W> {
    pub fn new(widget: &'a W, external: &'a W::ExternalState, local: &'a W::LocalState) -> Self {
        Self {
            widget,
            external,
            local,
            runtime_slot: None,
            width: iced::Length::Fill,
            height: iced::Length::Shrink,
            presented_viewport_sink: None,
            frame_seed: None,
            dirty_scopes: None,
        }
    }

    pub fn from_runtime(runtime: &'a SlipwayRuntime<W>) -> Self {
        let dirty_scopes = iced_dirty_scopes_from_runtime(runtime);
        Self::new(runtime.widget(), runtime.external(), runtime.local_state())
            .with_frame_seed(runtime.last_frame_identity())
            .with_dirty_scopes(dirty_scopes.map(Cow::Owned))
    }

    pub fn width(mut self, width: iced::Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: iced::Length) -> Self {
        self.height = height;
        self
    }

    pub fn record_presented_viewport_in(mut self, sink: &'a Cell<Rect>) -> Self {
        self.presented_viewport_sink = Some(sink);
        self
    }

    fn with_frame_seed(mut self, frame_seed: FrameIdentity) -> Self {
        self.frame_seed = Some(frame_seed);
        self
    }

    fn with_dirty_scopes(mut self, dirty_scopes: Option<Cow<'a, [IcedDirtyScope]>>) -> Self {
        self.dirty_scopes = dirty_scopes;
        self
    }

    fn with_runtime_slot(mut self, slot: WidgetSlotAddress) -> Self {
        self.runtime_slot = Some(slot);
        self
    }

    fn record_presented_viewport(&self, viewport: Rect) {
        if let Some(sink) = self.presented_viewport_sink {
            if sink.get() != viewport {
                sink.set(viewport);
            }
        }
    }

    pub fn runtime_message_for_iced_event(
        &self,
        _event: &iced::Event,
        _bounds: iced::Rectangle,
        _cursor: iced::advanced::mouse::Cursor,
    ) -> Option<SlipwayIcedRuntimeMessage<W::AppMessage>> {
        None
    }
}

impl<'a, W> SlipwayIcedRuntimeLayoutIntentWidget<'a, W>
where
    W: SlipwayIcedLayoutIntentBackendChildWidget,
{
    pub fn new(widget: &'a W, external: &'a W::ExternalState, local: &'a W::LocalState) -> Self {
        Self {
            widget,
            external,
            local,
            runtime_slot: None,
            width: iced::Length::Fill,
            height: iced::Length::Shrink,
            layout_intent_sink: None,
            frame_seed: None,
        }
    }

    pub fn from_runtime(runtime: &'a SlipwayRuntime<W>) -> Self {
        Self::new(runtime.widget(), runtime.external(), runtime.local_state())
            .with_frame_seed(runtime.last_frame_identity())
    }

    pub fn observe_layout_intent_into(mut self, sink: &'a mut Vec<LayoutIntentProbe>) -> Self {
        self.layout_intent_sink = Some(sink);
        self
    }

    fn with_frame_seed(mut self, frame_seed: FrameIdentity) -> Self {
        self.frame_seed = Some(frame_seed);
        self
    }

    pub fn width(mut self, width: iced::Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: iced::Length) -> Self {
        self.height = height;
        self
    }

    pub fn runtime_message_for_iced_event(
        &self,
        _event: &iced::Event,
        _bounds: iced::Rectangle,
        _cursor: iced::advanced::mouse::Cursor,
    ) -> Option<SlipwayIcedRuntimeMessage<W::AppMessage>> {
        None
    }

    fn push_layout_intent(&mut self, input: &LayoutInput) {
        if let Some(sink) = self.layout_intent_sink.as_mut() {
            sink.push(self.widget.layout_intent(self.external, self.local, input));
        }
    }
}

impl<'a, W: SlipwayIcedBackendWidget> SlipwayIcedWidget<'a, W> {
    pub fn new(widget: &'a W, external: &'a W::ExternalState) -> Self {
        assert_iced_standalone_no_topology_children(widget, external);
        Self {
            widget,
            external,
            width: iced::Length::Fill,
            height: iced::Length::Shrink,
        }
    }

    pub fn width(mut self, width: iced::Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: iced::Length) -> Self {
        self.height = height;
        self
    }
}

impl<'a, W> SlipwayIcedLayoutIntentWidget<'a, W>
where
    W: SlipwayIcedLayoutIntentBackendWidget,
{
    pub fn new(widget: &'a W, external: &'a W::ExternalState) -> Self {
        assert_iced_standalone_no_topology_children(widget, external);
        Self {
            widget,
            external,
            width: iced::Length::Fill,
            height: iced::Length::Shrink,
            layout_intent_sink: None,
        }
    }

    pub fn observe_layout_intent_into(mut self, sink: &'a mut Vec<LayoutIntentProbe>) -> Self {
        self.layout_intent_sink = Some(sink);
        self
    }

    pub fn width(mut self, width: iced::Length) -> Self {
        self.width = width;
        self
    }

    pub fn height(mut self, height: iced::Length) -> Self {
        self.height = height;
        self
    }

    fn push_layout_intent(&mut self, local: &W::LocalState, input: &LayoutInput) {
        if let Some(sink) = self.layout_intent_sink.as_mut() {
            sink.push(self.widget.layout_intent(self.external, local, input));
        }
    }
}

impl<'a, W, Theme, Renderer> From<SlipwayIcedRuntimeWidget<'a, W>>
    for iced::Element<'a, SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>
where
    W: SlipwayIcedBackendChildWidget + 'a,
    W::ExternalState: 'a,
    W::LocalState: 'a,
    W::AppMessage: Clone + 'a,
    Theme: SlipwayIcedThemeContract + 'a,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'a + 'static,
{
    fn from(widget: SlipwayIcedRuntimeWidget<'a, W>) -> Self {
        iced::Element::new(widget)
    }
}

impl<'a, W, Theme, Renderer> From<SlipwayIcedRuntimeLayoutIntentWidget<'a, W>>
    for iced::Element<'a, SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>
where
    W: SlipwayIcedLayoutIntentBackendChildWidget + 'a,
    W::ExternalState: 'a,
    W::LocalState: 'a,
    W::AppMessage: Clone + 'a,
    Theme: SlipwayIcedThemeContract + 'a,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'a + 'static,
{
    fn from(widget: SlipwayIcedRuntimeLayoutIntentWidget<'a, W>) -> Self {
        iced::Element::new(widget)
    }
}

impl<W, Theme, Renderer>
    iced::advanced::Widget<SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>
    for SlipwayIcedRuntimeWidget<'_, W>
where
    W: SlipwayIcedBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<IcedRuntimeTreeState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(IcedRuntimeTreeState {
            id: self.widget.id(),
            slot: self.runtime_slot.clone(),
            presentation: None,
            text_edit_trees: Vec::new(),
            text_editor_trees: Vec::new(),
            scroll_region_trees: Vec::new(),
            hovered_region: None,
            pressed_region: None,
            focused_region: None,
            layout_pass: 0,
        })
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        iced_runtime_child_trees::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            self.frame_seed.as_ref(),
            self.dirty_scopes.as_deref(),
            None,
        )
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        let topology = self.widget.topology(self.external);
        let id = topology.id.clone();
        let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
        if state.id != id || state.slot != self.runtime_slot {
            state.id = id;
            state.slot = self.runtime_slot.clone();
            state.presentation = None;
            state.text_edit_trees.clear();
            state.text_editor_trees.clear();
            state.scroll_region_trees.clear();
            state.hovered_region = None;
            state.pressed_region = None;
            state.focused_region = None;
            state.layout_pass = 0;
        }
        let previous_child_context = state.presentation.as_ref().map(|presentation| {
            (
                presentation.frame.clone(),
                root_child_placements_excluding_native_scroll(presentation),
            )
        });
        reconcile_iced_runtime_child_trees::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            self.frame_seed.as_ref(),
            self.dirty_scopes.as_deref(),
            previous_child_context
                .as_ref()
                .map(|(frame, placements)| (frame, placements.as_ref())),
        );
    }

    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let input = iced_layout_input_from_limits(limits);
        self.record_presented_viewport(input.viewport.into_rect());
        let root_placements = {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            state.id = self.widget.id();
            if state.presentation.as_ref().is_some_and(|presentation| {
                iced_presentation_cache_matches(
                    presentation,
                    &input,
                    self.frame_seed.as_ref(),
                    self.dirty_scopes.as_deref(),
                    &state.id,
                    self.runtime_slot.as_ref(),
                )
            }) {
                if let Some(presentation) = state.presentation.as_mut() {
                    refresh_iced_presentation_frame(
                        presentation,
                        &input,
                        self.frame_seed.as_ref(),
                        &state.id,
                        state.layout_pass,
                    );
                }
            } else {
                state.layout_pass = state.layout_pass.saturating_add(1);
                state.presentation = Some(iced_presentation_for_widget(
                    self.widget,
                    self.external,
                    self.local,
                    input,
                    self.frame_seed.as_ref(),
                    state.layout_pass,
                    self.runtime_slot.as_ref(),
                ));
            }
            state
                .presentation
                .as_ref()
                .map(root_child_placements_excluding_native_scroll)
                .unwrap_or_default()
        };
        let child_frame = tree
            .state
            .downcast_ref::<IcedRuntimeTreeState>()
            .presentation
            .as_ref()
            .expect("runtime layout must create presentation before child layout")
            .frame
            .clone();
        let mut child_nodes = layout_iced_runtime_children::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            renderer,
            root_placements.as_ref(),
            &child_frame,
            self.frame_seed.as_ref(),
            self.dirty_scopes.as_deref(),
        );
        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime layout must create presentation before scroll layout");
            child_nodes.extend(layout_iced_scroll_regions::<W, Theme, Renderer>(
                &mut state.scroll_region_trees,
                self.widget,
                self.external,
                self.local,
                presentation,
                renderer,
                self.frame_seed.as_ref(),
                self.dirty_scopes.as_deref(),
            ));
        }
        let (text_input_regions, text_editor_regions) = {
            let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
            let presentation = state.presentation.as_ref();
            (
                presentation
                    .map(text_input_focus_regions)
                    .unwrap_or_default(),
                presentation
                    .map(text_editor_focus_regions)
                    .unwrap_or_default(),
            )
        };
        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            if let Some(presentation) = state.presentation.as_ref() {
                child_nodes.extend(layout_iced_text_editor_regions::<
                    W::AppMessage,
                    Theme,
                    Renderer,
                >(
                    &mut state.text_editor_trees,
                    renderer,
                    presentation,
                    text_editor_regions.as_ref(),
                ));
                child_nodes.extend(layout_iced_text_input_regions::<
                    W::AppMessage,
                    Theme,
                    Renderer,
                >(
                    &mut state.text_edit_trees,
                    renderer,
                    presentation,
                    text_input_regions.as_ref(),
                ));
            }
        }
        let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
        let presentation = state
            .presentation
            .as_ref()
            .expect("runtime layout must create a presentation before building a node");
        let node = iced_node_for_presentation_with_children(
            self.width,
            self.height,
            limits,
            presentation,
            child_nodes,
        );
        node
    }

    fn operate(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        let (root_placements, text_input_regions, text_editor_regions, frame) = {
            let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime operate must have presentation before child operation");
            (
                root_child_placements_excluding_native_scroll(presentation),
                text_input_focus_regions(presentation),
                text_editor_focus_regions(presentation),
                presentation.frame.clone(),
            )
        };
        let text_region_count = text_input_regions.len() + text_editor_regions.len();
        operate_iced_root_container(&self.widget.id(), layout, operation);
        operation.traverse(&mut |operation| {
            operate_iced_runtime_children::<W, Theme, Renderer>(
                self.widget,
                self.external,
                self.local,
                &mut tree.children,
                layout,
                renderer,
                operation,
                root_placements.as_ref(),
                &frame,
                self.frame_seed.as_ref(),
                self.dirty_scopes.as_deref(),
            );
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            if let Some(presentation) = state.presentation.as_ref() {
                operate_iced_scroll_regions::<W, Theme, Renderer>(
                    &mut state.scroll_region_trees,
                    self.widget,
                    self.external,
                    self.local,
                    presentation,
                    layout,
                    renderer,
                    operation,
                    text_region_count,
                    self.frame_seed.as_ref(),
                    self.dirty_scopes.as_deref(),
                );
            }
            if let Some(presentation) = state.presentation.as_ref() {
                operate_iced_text_editor_regions::<W::AppMessage, Theme, Renderer>(
                    &mut state.text_editor_trees,
                    presentation,
                    text_editor_regions.as_ref(),
                    text_input_regions.len(),
                    layout,
                    renderer,
                    operation,
                );
            }
            if let Some(presentation) = state.presentation.as_ref() {
                operate_iced_text_input_regions::<W::AppMessage, Theme, Renderer>(
                    &mut state.text_edit_trees,
                    presentation,
                    text_input_regions.as_ref(),
                    layout,
                    renderer,
                    operation,
                );
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, SlipwayIcedRuntimeMessage<W::AppMessage>>,
        viewport: &iced::Rectangle,
    ) {
        if shell.is_event_captured() {
            return;
        }

        let (root_placements, text_input_regions, text_editor_regions, frame) = {
            let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime update must have presentation before child update");
            (
                root_child_placements_excluding_native_scroll(presentation),
                text_input_focus_regions(presentation),
                text_editor_focus_regions(presentation),
                presentation.frame.clone(),
            )
        };
        let text_region_count = text_input_regions.len() + text_editor_regions.len();

        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime update must have presentation before text editor update");
            update_iced_text_editor_regions::<W::AppMessage, Theme, Renderer>(
                &mut state.text_editor_trees,
                presentation,
                text_editor_regions.as_ref(),
                text_input_regions.len(),
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        if shell.is_event_captured() {
            return;
        }

        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime update must have presentation before text input update");
            update_iced_text_input_regions::<W::AppMessage, Theme, Renderer>(
                &mut state.text_edit_trees,
                presentation,
                text_input_regions.as_ref(),
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        if shell.is_event_captured() {
            return;
        }

        update_iced_runtime_children::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
            root_placements.as_ref(),
            &frame,
            self.frame_seed.as_ref(),
            self.dirty_scopes.as_deref(),
        );

        if shell.is_event_captured() {
            return;
        }

        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime update must have presentation before scroll update");
            update_iced_scroll_regions::<W, Theme, Renderer>(
                &mut state.scroll_region_trees,
                self.widget,
                self.external,
                self.local,
                presentation,
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
                text_region_count,
                self.frame_seed.as_ref(),
                self.dirty_scopes.as_deref(),
            );
        }

        if shell.is_event_captured() {
            return;
        }

        let routed = {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let mut hovered = state.hovered_region.clone();
            let mut pressed = state.pressed_region.clone();
            let mut focused = state.focused_region.clone();
            let routed = route_iced_event(
                event,
                layout.bounds(),
                cursor,
                state.presentation.as_ref(),
                &mut hovered,
                &mut pressed,
                &mut focused,
            );
            state.hovered_region = hovered;
            state.pressed_region = pressed;
            state.focused_region = focused;
            routed
        };
        let Some(routed) = routed else {
            return;
        };

        if let Some(input) = routed.input {
            shell.publish(SlipwayIcedRuntimeMessage::BackendInput(input));
        }

        if routed.capture_event {
            shell.capture_event();
        }
        if routed.request_redraw {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        with_iced_surface_global_paint_queue(renderer, |renderer| {
            draw_iced_runtime_widget_local::<W, Theme, Renderer>(
                self, tree, renderer, _theme, style, layout, cursor, viewport,
            );
        });
    }

    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
        let root_placements = state
            .presentation
            .as_ref()
            .map(root_child_placements_excluding_native_scroll)
            .unwrap_or_default();
        if let Some(presentation) = state.presentation.as_ref() {
            let child_interaction = iced_runtime_children_mouse_interaction::<W, Theme, Renderer>(
                self.widget,
                self.external,
                self.local,
                &tree.children,
                layout,
                cursor,
                viewport,
                renderer,
                root_placements.as_ref(),
                &presentation.frame,
                self.frame_seed.as_ref(),
                self.dirty_scopes.as_deref(),
            );
            if child_interaction != iced::advanced::mouse::Interaction::None {
                return child_interaction;
            }
        }
        let text_input_regions = state
            .presentation
            .as_ref()
            .map(text_input_focus_regions)
            .unwrap_or_default();
        let text_editor_regions = state
            .presentation
            .as_ref()
            .map(text_editor_focus_regions)
            .unwrap_or_default();
        let text_region_count = text_input_regions.len() + text_editor_regions.len();
        if let Some(presentation) = state.presentation.as_ref() {
            let scroll_interaction = iced_scroll_regions_mouse_interaction::<W, Theme, Renderer>(
                &state.scroll_region_trees,
                self.widget,
                self.external,
                self.local,
                presentation,
                layout,
                cursor,
                viewport,
                renderer,
                text_region_count,
                self.frame_seed.as_ref(),
                self.dirty_scopes.as_deref(),
            );
            if scroll_interaction != iced::advanced::mouse::Interaction::None {
                return scroll_interaction;
            }
        }
        let text_editor_interaction = state.presentation.as_ref().map_or(
            iced::advanced::mouse::Interaction::None,
            |presentation| {
                iced_text_editor_regions_mouse_interaction::<W::AppMessage, Theme, Renderer>(
                    &state.text_editor_trees,
                    presentation,
                    text_editor_regions.as_ref(),
                    text_input_regions.len(),
                    layout,
                    cursor,
                    viewport,
                    renderer,
                )
            },
        );
        if text_editor_interaction != iced::advanced::mouse::Interaction::None {
            return text_editor_interaction;
        }
        let text_interaction = state.presentation.as_ref().map_or(
            iced::advanced::mouse::Interaction::None,
            |presentation| {
                iced_text_input_regions_mouse_interaction::<W::AppMessage, Theme, Renderer>(
                    &state.text_edit_trees,
                    presentation,
                    text_input_regions.as_ref(),
                    layout,
                    cursor,
                    viewport,
                    renderer,
                )
            },
        );
        if text_interaction != iced::advanced::mouse::Interaction::None {
            return text_interaction;
        }
        mouse_interaction_for_presentation(layout.bounds(), cursor, state.presentation.as_ref())
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut iced::advanced::widget::Tree,
        _layout: iced::advanced::Layout<'b>,
        _renderer: &Renderer,
        _viewport: &iced::Rectangle,
        _translation: iced::Vector,
    ) -> Option<
        iced::advanced::overlay::Element<
            'b,
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        >,
    > {
        // Current core overlay declarations are metadata-only: they do not expose stable authored
        // overlay child/surface content, tree identity, layout, event routing, or an iced-safe
        // lifetime path. Returning an overlay here would fake official iced overlay forwarding.
        None
    }
}

impl<W, Theme, Renderer>
    iced::advanced::Widget<SlipwayIcedRuntimeMessage<W::AppMessage>, Theme, Renderer>
    for SlipwayIcedRuntimeLayoutIntentWidget<'_, W>
where
    W: SlipwayIcedLayoutIntentBackendChildWidget,
    W::AppMessage: Clone,
    Theme: SlipwayIcedThemeContract,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<IcedRuntimeTreeState>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        iced::advanced::widget::tree::State::new(IcedRuntimeTreeState {
            id: self.widget.id(),
            slot: self.runtime_slot.clone(),
            presentation: None,
            text_edit_trees: Vec::new(),
            text_editor_trees: Vec::new(),
            scroll_region_trees: Vec::new(),
            hovered_region: None,
            pressed_region: None,
            focused_region: None,
            layout_pass: 0,
        })
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        iced_runtime_child_trees::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            self.frame_seed.as_ref(),
            None,
            None,
        )
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        let topology = self.widget.topology(self.external);
        let id = topology.id.clone();
        let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
        if state.id != id || state.slot != self.runtime_slot {
            state.id = id;
            state.slot = self.runtime_slot.clone();
            state.presentation = None;
            state.text_edit_trees.clear();
            state.text_editor_trees.clear();
            state.scroll_region_trees.clear();
            state.hovered_region = None;
            state.pressed_region = None;
            state.focused_region = None;
            state.layout_pass = 0;
        }
        let previous_child_context = state.presentation.as_ref().map(|presentation| {
            (
                presentation.frame.clone(),
                root_child_placements_excluding_native_scroll(presentation),
            )
        });
        reconcile_iced_runtime_child_trees::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            self.frame_seed.as_ref(),
            None,
            previous_child_context
                .as_ref()
                .map(|(frame, placements)| (frame, placements.as_ref())),
        );
    }

    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        let input = iced_layout_input_from_limits(limits);
        let root_placements = {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            state.id = self.widget.id();
            if !state.presentation.as_ref().is_some_and(|presentation| {
                iced_presentation_cache_matches(
                    presentation,
                    &input,
                    self.frame_seed.as_ref(),
                    None,
                    &state.id,
                    self.runtime_slot.as_ref(),
                )
            }) {
                state.layout_pass = state.layout_pass.saturating_add(1);
                self.push_layout_intent(&input);
                state.presentation = Some(iced_presentation_for_widget(
                    self.widget,
                    self.external,
                    self.local,
                    input,
                    self.frame_seed.as_ref(),
                    state.layout_pass,
                    self.runtime_slot.as_ref(),
                ));
            }
            state
                .presentation
                .as_ref()
                .map(root_child_placements_excluding_native_scroll)
                .unwrap_or_default()
        };
        let child_frame = tree
            .state
            .downcast_ref::<IcedRuntimeTreeState>()
            .presentation
            .as_ref()
            .expect("runtime layout must create presentation before child layout")
            .frame
            .clone();
        let mut child_nodes = layout_iced_runtime_children::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            renderer,
            root_placements.as_ref(),
            &child_frame,
            self.frame_seed.as_ref(),
            None,
        );
        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime layout must create presentation before scroll layout");
            child_nodes.extend(layout_iced_scroll_regions::<W, Theme, Renderer>(
                &mut state.scroll_region_trees,
                self.widget,
                self.external,
                self.local,
                presentation,
                renderer,
                self.frame_seed.as_ref(),
                None,
            ));
        }
        let (text_input_regions, text_editor_regions) = {
            let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
            let presentation = state.presentation.as_ref();
            (
                presentation
                    .map(text_input_focus_regions)
                    .unwrap_or_default(),
                presentation
                    .map(text_editor_focus_regions)
                    .unwrap_or_default(),
            )
        };
        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            if let Some(presentation) = state.presentation.as_ref() {
                child_nodes.extend(layout_iced_text_editor_regions::<
                    W::AppMessage,
                    Theme,
                    Renderer,
                >(
                    &mut state.text_editor_trees,
                    renderer,
                    presentation,
                    text_editor_regions.as_ref(),
                ));
                child_nodes.extend(layout_iced_text_input_regions::<
                    W::AppMessage,
                    Theme,
                    Renderer,
                >(
                    &mut state.text_edit_trees,
                    renderer,
                    presentation,
                    text_input_regions.as_ref(),
                ));
            }
        }
        let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
        let presentation = state
            .presentation
            .as_ref()
            .expect("runtime layout must create a presentation before building a node");
        let node = iced_node_for_presentation_with_children(
            self.width,
            self.height,
            limits,
            presentation,
            child_nodes,
        );
        node
    }

    fn operate(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        let (root_placements, text_input_regions, text_editor_regions, frame) = {
            let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime operate must have presentation before child operation");
            (
                root_child_placements_excluding_native_scroll(presentation),
                text_input_focus_regions(presentation),
                text_editor_focus_regions(presentation),
                presentation.frame.clone(),
            )
        };
        let text_region_count = text_input_regions.len() + text_editor_regions.len();
        operate_iced_root_container(&self.widget.id(), layout, operation);
        operation.traverse(&mut |operation| {
            operate_iced_runtime_children::<W, Theme, Renderer>(
                self.widget,
                self.external,
                self.local,
                &mut tree.children,
                layout,
                renderer,
                operation,
                root_placements.as_ref(),
                &frame,
                self.frame_seed.as_ref(),
                None,
            );
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            if let Some(presentation) = state.presentation.as_ref() {
                operate_iced_scroll_regions::<W, Theme, Renderer>(
                    &mut state.scroll_region_trees,
                    self.widget,
                    self.external,
                    self.local,
                    presentation,
                    layout,
                    renderer,
                    operation,
                    text_region_count,
                    self.frame_seed.as_ref(),
                    None,
                );
            }
            if let Some(presentation) = state.presentation.as_ref() {
                operate_iced_text_editor_regions::<W::AppMessage, Theme, Renderer>(
                    &mut state.text_editor_trees,
                    presentation,
                    text_editor_regions.as_ref(),
                    text_input_regions.len(),
                    layout,
                    renderer,
                    operation,
                );
            }
            if let Some(presentation) = state.presentation.as_ref() {
                operate_iced_text_input_regions::<W::AppMessage, Theme, Renderer>(
                    &mut state.text_edit_trees,
                    presentation,
                    text_input_regions.as_ref(),
                    layout,
                    renderer,
                    operation,
                );
            }
        });
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        renderer: &Renderer,
        clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, SlipwayIcedRuntimeMessage<W::AppMessage>>,
        viewport: &iced::Rectangle,
    ) {
        if shell.is_event_captured() {
            return;
        }

        let (root_placements, text_input_regions, text_editor_regions, frame) = {
            let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime update must have presentation before child update");
            (
                root_child_placements_excluding_native_scroll(presentation),
                text_input_focus_regions(presentation),
                text_editor_focus_regions(presentation),
                presentation.frame.clone(),
            )
        };
        let text_region_count = text_input_regions.len() + text_editor_regions.len();

        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime update must have presentation before text editor update");
            update_iced_text_editor_regions::<W::AppMessage, Theme, Renderer>(
                &mut state.text_editor_trees,
                presentation,
                text_editor_regions.as_ref(),
                text_input_regions.len(),
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        if shell.is_event_captured() {
            return;
        }

        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime update must have presentation before text input update");
            update_iced_text_input_regions::<W::AppMessage, Theme, Renderer>(
                &mut state.text_edit_trees,
                presentation,
                text_input_regions.as_ref(),
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
            );
        }

        if shell.is_event_captured() {
            return;
        }

        update_iced_runtime_children::<W, Theme, Renderer>(
            self.widget,
            self.external,
            self.local,
            &mut tree.children,
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
            root_placements.as_ref(),
            &frame,
            self.frame_seed.as_ref(),
            None,
        );

        if shell.is_event_captured() {
            return;
        }

        {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let presentation = state
                .presentation
                .as_ref()
                .expect("runtime update must have presentation before scroll update");
            update_iced_scroll_regions::<W, Theme, Renderer>(
                &mut state.scroll_region_trees,
                self.widget,
                self.external,
                self.local,
                presentation,
                event,
                layout,
                cursor,
                renderer,
                clipboard,
                shell,
                viewport,
                text_region_count,
                self.frame_seed.as_ref(),
                None,
            );
        }

        if shell.is_event_captured() {
            return;
        }

        let routed = {
            let state = tree.state.downcast_mut::<IcedRuntimeTreeState>();
            let mut hovered = state.hovered_region.clone();
            let mut pressed = state.pressed_region.clone();
            let mut focused = state.focused_region.clone();
            let routed = route_iced_event(
                event,
                layout.bounds(),
                cursor,
                state.presentation.as_ref(),
                &mut hovered,
                &mut pressed,
                &mut focused,
            );
            state.hovered_region = hovered;
            state.pressed_region = pressed;
            state.focused_region = focused;
            routed
        };
        let Some(routed) = routed else {
            return;
        };

        if let Some(input) = routed.input {
            shell.publish(SlipwayIcedRuntimeMessage::BackendInput(input));
        }

        if routed.capture_event {
            shell.capture_event();
        }
        if routed.request_redraw {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        let runtime_widget = SlipwayIcedRuntimeWidget {
            widget: self.widget,
            external: self.external,
            local: self.local,
            runtime_slot: self.runtime_slot.clone(),
            width: self.width,
            height: self.height,
            presented_viewport_sink: None,
            frame_seed: self.frame_seed.clone(),
            dirty_scopes: None,
        };
        with_iced_surface_global_paint_queue(renderer, |renderer| {
            draw_iced_runtime_widget_local::<W, Theme, Renderer>(
                &runtime_widget,
                tree,
                renderer,
                _theme,
                style,
                layout,
                cursor,
                viewport,
            );
        });
    }

    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
        let root_placements = state
            .presentation
            .as_ref()
            .map(root_child_placements_excluding_native_scroll)
            .unwrap_or_default();
        if let Some(presentation) = state.presentation.as_ref() {
            let child_interaction = iced_runtime_children_mouse_interaction::<W, Theme, Renderer>(
                self.widget,
                self.external,
                self.local,
                &tree.children,
                layout,
                cursor,
                viewport,
                renderer,
                root_placements.as_ref(),
                &presentation.frame,
                self.frame_seed.as_ref(),
                None,
            );
            if child_interaction != iced::advanced::mouse::Interaction::None {
                return child_interaction;
            }
        }
        let text_input_regions = state
            .presentation
            .as_ref()
            .map(text_input_focus_regions)
            .unwrap_or_default();
        let text_editor_regions = state
            .presentation
            .as_ref()
            .map(text_editor_focus_regions)
            .unwrap_or_default();
        let text_region_count = text_input_regions.len() + text_editor_regions.len();
        if let Some(presentation) = state.presentation.as_ref() {
            let scroll_interaction = iced_scroll_regions_mouse_interaction::<W, Theme, Renderer>(
                &state.scroll_region_trees,
                self.widget,
                self.external,
                self.local,
                presentation,
                layout,
                cursor,
                viewport,
                renderer,
                text_region_count,
                self.frame_seed.as_ref(),
                None,
            );
            if scroll_interaction != iced::advanced::mouse::Interaction::None {
                return scroll_interaction;
            }
        }
        let text_editor_interaction = state.presentation.as_ref().map_or(
            iced::advanced::mouse::Interaction::None,
            |presentation| {
                iced_text_editor_regions_mouse_interaction::<W::AppMessage, Theme, Renderer>(
                    &state.text_editor_trees,
                    presentation,
                    text_editor_regions.as_ref(),
                    text_input_regions.len(),
                    layout,
                    cursor,
                    viewport,
                    renderer,
                )
            },
        );
        if text_editor_interaction != iced::advanced::mouse::Interaction::None {
            return text_editor_interaction;
        }
        let text_interaction = state.presentation.as_ref().map_or(
            iced::advanced::mouse::Interaction::None,
            |presentation| {
                iced_text_input_regions_mouse_interaction::<W::AppMessage, Theme, Renderer>(
                    &state.text_edit_trees,
                    presentation,
                    text_input_regions.as_ref(),
                    layout,
                    cursor,
                    viewport,
                    renderer,
                )
            },
        );
        if text_interaction != iced::advanced::mouse::Interaction::None {
            return text_interaction;
        }
        mouse_interaction_for_presentation(layout.bounds(), cursor, state.presentation.as_ref())
    }

    fn overlay<'b>(
        &'b mut self,
        _tree: &'b mut iced::advanced::widget::Tree,
        _layout: iced::advanced::Layout<'b>,
        _renderer: &Renderer,
        _viewport: &iced::Rectangle,
        _translation: iced::Vector,
    ) -> Option<
        iced::advanced::overlay::Element<
            'b,
            SlipwayIcedRuntimeMessage<W::AppMessage>,
            Theme,
            Renderer,
        >,
    > {
        // See SlipwayIcedRuntimeWidget::overlay for the current lifecycle gate.
        None
    }
}

impl<'a, W, Theme, Renderer> From<SlipwayIcedWidget<'a, W>>
    for iced::Element<'a, W::AppMessage, Theme, Renderer>
where
    W: SlipwayIcedBackendWidget + 'a,
    W::ExternalState: 'a,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'a,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'a + 'static,
{
    fn from(widget: SlipwayIcedWidget<'a, W>) -> Self {
        iced::Element::new(widget)
    }
}

impl<'a, W, Theme, Renderer> From<SlipwayIcedLayoutIntentWidget<'a, W>>
    for iced::Element<'a, W::AppMessage, Theme, Renderer>
where
    W: SlipwayIcedLayoutIntentBackendWidget + 'a,
    W::ExternalState: 'a,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'a,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'a + 'static,
{
    fn from(widget: SlipwayIcedLayoutIntentWidget<'a, W>) -> Self {
        iced::Element::new(widget)
    }
}

impl<W, Theme, Renderer> iced::advanced::Widget<W::AppMessage, Theme, Renderer>
    for SlipwayIcedWidget<'_, W>
where
    W: SlipwayIcedBackendWidget,
    W::LocalState: Clone + 'static,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<IcedTreeState<W::LocalState>>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        assert_iced_standalone_no_topology_children(self.widget, self.external);
        iced::advanced::widget::tree::State::new(IcedTreeState {
            id: self.widget.id(),
            local: self.widget.initial_local_state(),
            presentation: None,
            hovered_region: None,
            pressed_region: None,
            focused_region: None,
            layout_pass: 0,
        })
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        assert_iced_standalone_no_topology_children(self.widget, self.external);
        Vec::new()
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        assert_iced_standalone_no_topology_children(self.widget, self.external);
        let topology = self.widget.topology(self.external);
        let id = topology.id.clone();
        let state = tree.state.downcast_mut::<IcedTreeState<W::LocalState>>();
        if state.id != id {
            state.id = id;
            state.local = self.widget.initial_local_state();
            state.presentation = None;
            state.hovered_region = None;
            state.pressed_region = None;
            state.focused_region = None;
            state.layout_pass = 0;
        }
        tree.children.clear();
    }

    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        _renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        assert_iced_standalone_no_topology_children(self.widget, self.external);
        let state = tree.state.downcast_mut::<IcedTreeState<W::LocalState>>();
        state.layout_pass = state.layout_pass.saturating_add(1);

        let input = iced_layout_input_from_limits(limits);
        let presentation = iced_presentation_for_widget(
            self.widget,
            self.external,
            &state.local,
            input,
            None,
            state.layout_pass,
            None,
        );
        assert_iced_standalone_no_layout_children(&state.id, &presentation.layout);
        let node = iced_node_for_presentation(self.width, self.height, limits, &presentation);
        state.presentation = Some(presentation);
        node
    }

    fn operate(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        operate_iced_root_container(&self.widget.id(), layout, operation);
        operation.traverse(&mut |_operation| {});
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, W::AppMessage>,
        _viewport: &iced::Rectangle,
    ) {
        if shell.is_event_captured() {
            return;
        }

        let state = tree.state.downcast_mut::<IcedTreeState<W::LocalState>>();
        let Some(routed) = route_iced_event(
            event,
            layout.bounds(),
            cursor,
            state.presentation.as_ref(),
            &mut state.hovered_region,
            &mut state.pressed_region,
            &mut state.focused_region,
        ) else {
            return;
        };

        let Some(input) = routed.input else {
            if routed.request_redraw {
                shell.request_redraw();
            }
            return;
        };

        let event = input.event.clone();
        let declaration =
            slipway_core::declared_event_handling(self.widget, self.external, &state.local, &event);
        if !declaration.disposition.final_disposition.handled {
            return;
        }
        let raw_outcome = self
            .widget
            .handle_event(self.external, &mut state.local, event);
        let outcome =
            slipway_core::apply_physical_event_handling_declaration(declaration, raw_outcome);
        let handled = outcome.handled || !outcome.emitted_messages.is_empty();

        for message in outcome.emitted_messages {
            shell.publish(message.message);
        }

        if handled {
            shell.capture_event();
        }
        if handled || routed.request_redraw {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        let state = tree.state.downcast_ref::<IcedTreeState<W::LocalState>>();
        let bounds = layout.bounds();
        let Some(presentation) = state.presentation.as_ref() else {
            return;
        };

        draw_iced_standalone_presentation(renderer, bounds.position(), presentation);
    }

    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
        _renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        let state = tree.state.downcast_ref::<IcedTreeState<W::LocalState>>();
        mouse_interaction_for_presentation(layout.bounds(), cursor, state.presentation.as_ref())
    }
}

impl<W, Theme, Renderer> iced::advanced::Widget<W::AppMessage, Theme, Renderer>
    for SlipwayIcedLayoutIntentWidget<'_, W>
where
    W: SlipwayIcedLayoutIntentBackendWidget,
    W::LocalState: Clone + 'static,
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    fn tag(&self) -> iced::advanced::widget::tree::Tag {
        iced::advanced::widget::tree::Tag::of::<IcedTreeState<W::LocalState>>()
    }

    fn state(&self) -> iced::advanced::widget::tree::State {
        assert_iced_standalone_no_topology_children(self.widget, self.external);
        iced::advanced::widget::tree::State::new(IcedTreeState {
            id: self.widget.id(),
            local: self.widget.initial_local_state(),
            presentation: None,
            hovered_region: None,
            pressed_region: None,
            focused_region: None,
            layout_pass: 0,
        })
    }

    fn children(&self) -> Vec<iced::advanced::widget::Tree> {
        assert_iced_standalone_no_topology_children(self.widget, self.external);
        Vec::new()
    }

    fn diff(&self, tree: &mut iced::advanced::widget::Tree) {
        assert_iced_standalone_no_topology_children(self.widget, self.external);
        let topology = self.widget.topology(self.external);
        let id = topology.id.clone();
        let state = tree.state.downcast_mut::<IcedTreeState<W::LocalState>>();
        if state.id != id {
            state.id = id;
            state.local = self.widget.initial_local_state();
            state.presentation = None;
            state.hovered_region = None;
            state.pressed_region = None;
            state.focused_region = None;
            state.layout_pass = 0;
        }
        tree.children.clear();
    }

    fn size(&self) -> iced::Size<iced::Length> {
        iced::Size {
            width: self.width,
            height: self.height,
        }
    }

    fn layout(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        _renderer: &Renderer,
        limits: &iced::advanced::layout::Limits,
    ) -> iced::advanced::layout::Node {
        assert_iced_standalone_no_topology_children(self.widget, self.external);
        let state = tree.state.downcast_mut::<IcedTreeState<W::LocalState>>();
        state.layout_pass = state.layout_pass.saturating_add(1);

        let input = iced_layout_input_from_limits(limits);
        self.push_layout_intent(&state.local, &input);
        let presentation = iced_presentation_for_widget(
            self.widget,
            self.external,
            &state.local,
            input,
            None,
            state.layout_pass,
            None,
        );
        assert_iced_standalone_no_layout_children(&state.id, &presentation.layout);
        let node = iced_node_for_presentation(self.width, self.height, limits, &presentation);
        state.presentation = Some(presentation);
        node
    }

    fn operate(
        &mut self,
        _tree: &mut iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        _renderer: &Renderer,
        operation: &mut dyn iced::advanced::widget::Operation,
    ) {
        operate_iced_root_container(&self.widget.id(), layout, operation);
        operation.traverse(&mut |_operation| {});
    }

    fn update(
        &mut self,
        tree: &mut iced::advanced::widget::Tree,
        event: &iced::Event,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        _renderer: &Renderer,
        _clipboard: &mut dyn iced::advanced::Clipboard,
        shell: &mut iced::advanced::Shell<'_, W::AppMessage>,
        _viewport: &iced::Rectangle,
    ) {
        if shell.is_event_captured() {
            return;
        }

        let state = tree.state.downcast_mut::<IcedTreeState<W::LocalState>>();
        let Some(routed) = route_iced_event(
            event,
            layout.bounds(),
            cursor,
            state.presentation.as_ref(),
            &mut state.hovered_region,
            &mut state.pressed_region,
            &mut state.focused_region,
        ) else {
            return;
        };

        let Some(input) = routed.input else {
            if routed.request_redraw {
                shell.request_redraw();
            }
            return;
        };

        let event = input.event.clone();
        let declaration =
            slipway_core::declared_event_handling(self.widget, self.external, &state.local, &event);
        if !declaration.disposition.final_disposition.handled {
            return;
        }
        let raw_outcome = self
            .widget
            .handle_event(self.external, &mut state.local, event);
        let outcome =
            slipway_core::apply_physical_event_handling_declaration(declaration, raw_outcome);
        let handled = outcome.handled || !outcome.emitted_messages.is_empty();

        for message in outcome.emitted_messages {
            shell.publish(message.message);
        }

        if handled {
            shell.capture_event();
        }
        if handled || routed.request_redraw {
            shell.request_redraw();
        }
    }

    fn draw(
        &self,
        tree: &iced::advanced::widget::Tree,
        renderer: &mut Renderer,
        _theme: &Theme,
        _style: &iced::advanced::renderer::Style,
        layout: iced::advanced::Layout<'_>,
        _cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
    ) {
        let state = tree.state.downcast_ref::<IcedTreeState<W::LocalState>>();
        let bounds = layout.bounds();
        let Some(presentation) = state.presentation.as_ref() else {
            return;
        };

        draw_iced_standalone_presentation(renderer, bounds.position(), presentation);
    }

    fn mouse_interaction(
        &self,
        tree: &iced::advanced::widget::Tree,
        layout: iced::advanced::Layout<'_>,
        cursor: iced::advanced::mouse::Cursor,
        _viewport: &iced::Rectangle,
        _renderer: &Renderer,
    ) -> iced::advanced::mouse::Interaction {
        let state = tree.state.downcast_ref::<IcedTreeState<W::LocalState>>();
        mouse_interaction_for_presentation(layout.bounds(), cursor, state.presentation.as_ref())
    }
}

fn route_iced_event(
    event: &iced::Event,
    layout_bounds: iced::Rectangle,
    cursor: iced::advanced::mouse::Cursor,
    presentation: Option<&IcedPresentationState>,
    hovered_region: &mut Option<slipway_core::PresentationRegionId>,
    pressed_region: &mut Option<IcedPressedPointerCapture>,
    focused_region: &mut Option<slipway_core::PresentationRegionId>,
) -> Option<IcedRoutedInput> {
    let presentation = presentation?;
    let previous_hovered_region = hovered_region.clone();
    let hover_changed = sync_hover_region(presentation, layout_bounds, cursor, hovered_region);

    match event {
        iced::Event::Mouse(iced::mouse::Event::CursorMoved { position }) => {
            let position = cursor.position().unwrap_or(*position);
            if let Some(capture_state) = pressed_region.as_ref()
                && let Some(region) = hit_region_by_id(presentation, &capture_state.region_id)
            {
                let capture =
                    pointer_capture_for_region(region, slipway_core::PointerEventKind::Move, true);
                if capture {
                    let capture_layout_bounds =
                        capture_state.layout_bounds_for_capture(layout_bounds);
                    return Some(captured_pointer_event_for_hit_region(
                        region,
                        presentation,
                        capture_layout_bounds,
                        position,
                        slipway_core::PointerEventKind::Move,
                        None,
                        true,
                        true,
                    ));
                }
            }

            if paint_occlusion_at_root_local_point(
                presentation,
                iced_view_root_local_point(layout_bounds, position),
            )
            .is_some()
                && hit_region_at_point(presentation, layout_bounds, position).is_none()
            {
                return Some(IcedRoutedInput {
                    input: None,
                    capture_event: true,
                    request_redraw: hover_changed,
                });
            }

            hover_changed.then_some(IcedRoutedInput {
                input: None,
                capture_event: false,
                request_redraw: true,
            })
        }
        iced::Event::Mouse(iced::mouse::Event::CursorEntered) => {
            let Some(position) = cursor.position() else {
                return hover_changed.then_some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: true,
                });
            };
            let Some(region) = hit_region_at_point(presentation, layout_bounds, position) else {
                return hover_changed.then_some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: true,
                });
            };

            Some(pointer_event_for_hit_region(
                region,
                presentation,
                layout_bounds,
                position,
                slipway_core::PointerEventKind::Enter,
                None,
                false,
                true,
            ))
        }
        iced::Event::Mouse(iced::mouse::Event::CursorLeft) => {
            let previous = previous_hovered_region;
            *hovered_region = None;
            let Some(previous) = previous else {
                return hover_changed.then_some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: true,
                });
            };
            let Some(region) = hit_region_by_id(presentation, &previous) else {
                return Some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: true,
                });
            };
            let region_rect = region_root_local_rect(
                presentation,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
            );
            let position = iced::Point::new(
                layout_bounds.x + region_rect.origin.x,
                layout_bounds.y + region_rect.origin.y,
            );

            Some(pointer_event_for_hit_region(
                region,
                presentation,
                layout_bounds,
                position,
                slipway_core::PointerEventKind::Leave,
                None,
                false,
                true,
            ))
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonPressed(button)) => {
            let position = cursor.position()?;
            let focus_changed = focus_region_at_point(presentation, layout_bounds, position)
                .map(|region| set_region_id(focused_region, Some(region.id.clone())))
                .unwrap_or_else(|| set_region_id(focused_region, None));

            if let Some(region) = hit_region_at_point(presentation, layout_bounds, position) {
                *pressed_region = Some(IcedPressedPointerCapture::new(
                    region.id.clone(),
                    layout_bounds,
                ));
                let capture =
                    pointer_capture_for_region(region, slipway_core::PointerEventKind::Press, true);

                return Some(pointer_event_for_hit_region(
                    region,
                    presentation,
                    layout_bounds,
                    position,
                    slipway_core::PointerEventKind::Press,
                    Some(*button),
                    capture,
                    true,
                ));
            }

            if paint_occlusion_at_root_local_point(
                presentation,
                iced_view_root_local_point(layout_bounds, position),
            )
            .is_some()
            {
                return Some(IcedRoutedInput {
                    input: None,
                    capture_event: true,
                    request_redraw: focus_changed || hover_changed,
                });
            }

            let Some(region) = focus_region_at_point(presentation, layout_bounds, position) else {
                return (hover_changed || focus_changed).then_some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: true,
                });
            };

            Some(pointer_event_for_focus_region(
                region,
                presentation,
                layout_bounds,
                position,
                slipway_core::PointerEventKind::Press,
                Some(*button),
                true,
                true,
            ))
        }
        iced::Event::Mouse(iced::mouse::Event::ButtonReleased(button)) => {
            let position = cursor.position()?;
            let released_pressed_region = pressed_region.take();
            if let Some(capture_state) = released_pressed_region.as_ref()
                && let Some(region) = hit_region_by_id(presentation, &capture_state.region_id)
                && pointer_capture_for_region(
                    region,
                    slipway_core::PointerEventKind::Release,
                    false,
                )
            {
                let capture_layout_bounds = capture_state.layout_bounds_for_capture(layout_bounds);
                return Some(captured_pointer_event_for_hit_region(
                    region,
                    presentation,
                    capture_layout_bounds,
                    position,
                    slipway_core::PointerEventKind::Release,
                    Some(*button),
                    true,
                    true,
                ));
            }
            let hit_region =
                hit_region_at_point(presentation, layout_bounds, position).or_else(|| {
                    released_pressed_region
                        .as_ref()
                        .and_then(|capture| hit_region_by_id(presentation, &capture.region_id))
                });

            let Some(region) = hit_region else {
                if paint_occlusion_at_root_local_point(
                    presentation,
                    iced_view_root_local_point(layout_bounds, position),
                )
                .is_some()
                {
                    return Some(IcedRoutedInput {
                        input: None,
                        capture_event: true,
                        request_redraw: hover_changed,
                    });
                }
                return hover_changed.then_some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: true,
                });
            };
            let capture = released_pressed_region.is_some()
                || pointer_capture_for_region(
                    region,
                    slipway_core::PointerEventKind::Release,
                    false,
                );

            Some(pointer_event_for_hit_region(
                region,
                presentation,
                layout_bounds,
                position,
                slipway_core::PointerEventKind::Release,
                Some(*button),
                capture,
                released_pressed_region.is_some() || hover_changed,
            ))
        }
        iced::Event::Mouse(iced::mouse::Event::WheelScrolled { delta }) => {
            let position = cursor.position()?;
            let (delta_x, delta_y) = match delta {
                iced::mouse::ScrollDelta::Lines { x, y } => (*x, *y),
                iced::mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
            };
            let (dispatch, evidence) =
                slipway_core::resolve_declared_wheel_dispatch_with_evidence_and_geometry_index(
                    EvidenceSource::backend_presented(ICED_BACKEND_ID, "physical-input"),
                    presentation.frame.clone(),
                    &presentation.geometry_index,
                    &presentation.scroll_regions,
                    iced_view_root_local_point(layout_bounds, position),
                    delta_x,
                    delta_y,
                );
            let occlusion = paint_occlusion_at_root_local_point(
                presentation,
                iced_view_root_local_point(layout_bounds, position),
            );
            let Some(dispatch) = dispatch else {
                if occlusion.is_some() {
                    return Some(IcedRoutedInput {
                        input: None,
                        capture_event: true,
                        request_redraw: false,
                    });
                }
                return Some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: false,
                });
            };
            if let Some(occlusion) = occlusion
                && let Some(region) = presentation
                    .scroll_regions
                    .iter()
                    .find(|region| region.id == dispatch.selected_region)
                && paint_occlusion_blocks_scroll_order(occlusion, region)
            {
                return Some(IcedRoutedInput {
                    input: None,
                    capture_event: true,
                    request_redraw: false,
                });
            }

            Some(IcedRoutedInput {
                capture_event: dispatch.capture_event,
                request_redraw: dispatch.capture_event,
                input: Some(BackendInputEvent::declared(dispatch.input, evidence)),
            })
        }
        iced::Event::InputMethod(iced_core::input_method::Event::Commit(text)) => {
            if text.is_empty() {
                return Some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: false,
                });
            }
            if !presentation.can_route_text_edit() {
                return None;
            }
            let focus = focused_focus_region(presentation, focused_region.as_ref())?;
            let selection_before = focus
                .text_edit
                .as_ref()
                .and_then(|text_edit| text_edit.selection.selection.clone());
            let event = InputEvent::TextEdit(slipway_core::TextEditEvent {
                target: focus_target(focus),
                target_slot: focus.address.clone(),
                kind: TextEditKind::InsertText,
                text: Some(text.clone()),
                selection_before,
                selection_after: None,
            });
            Some(IcedRoutedInput {
                input: Some(backend_focus_input_event(
                    presentation,
                    focus,
                    DeclaredEventDispatchKind::Text,
                    None,
                    event,
                )),
                capture_event: true,
                request_redraw: true,
            })
        }
        iced::Event::Touch(iced::touch::Event::FingerPressed { id, position }) => {
            let Some(region) = hit_region_at_point(presentation, layout_bounds, *position) else {
                return hover_changed.then_some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: true,
                });
            };
            *pressed_region = Some(IcedPressedPointerCapture::new(
                region.id.clone(),
                layout_bounds,
            ));
            let capture =
                pointer_capture_for_region(region, slipway_core::PointerEventKind::Press, true);

            Some(pointer_event_for_hit_region_with_details(
                region,
                presentation,
                layout_bounds,
                *position,
                slipway_core::PointerEventKind::Press,
                None,
                touch_pointer_details(*id, true),
                capture,
                true,
            ))
        }
        iced::Event::Touch(iced::touch::Event::FingerMoved { id, position }) => {
            if let Some(capture_state) = pressed_region.as_ref()
                && let Some(region) = hit_region_by_id(presentation, &capture_state.region_id)
            {
                let capture =
                    pointer_capture_for_region(region, slipway_core::PointerEventKind::Move, true);
                if capture {
                    let capture_layout_bounds =
                        capture_state.layout_bounds_for_capture(layout_bounds);
                    return Some(captured_pointer_event_for_hit_region_with_details(
                        region,
                        presentation,
                        capture_layout_bounds,
                        *position,
                        slipway_core::PointerEventKind::Move,
                        None,
                        touch_pointer_details(*id, true),
                        true,
                        true,
                    ));
                }
            }
            let Some(region) = hit_region_at_point(presentation, layout_bounds, *position) else {
                return hover_changed.then_some(IcedRoutedInput {
                    input: None,
                    capture_event: false,
                    request_redraw: true,
                });
            };

            Some(pointer_event_for_hit_region_with_details(
                region,
                presentation,
                layout_bounds,
                *position,
                slipway_core::PointerEventKind::Move,
                None,
                touch_pointer_details(*id, true),
                false,
                hover_changed,
            ))
        }
        iced::Event::Touch(iced::touch::Event::FingerLifted { id, position }) => {
            let released_pressed_region = pressed_region.take();
            if let Some(capture_state) = released_pressed_region.as_ref()
                && let Some(region) = hit_region_by_id(presentation, &capture_state.region_id)
                && pointer_capture_for_region(
                    region,
                    slipway_core::PointerEventKind::Release,
                    false,
                )
            {
                let capture_layout_bounds = capture_state.layout_bounds_for_capture(layout_bounds);
                return Some(captured_pointer_event_for_hit_region_with_details(
                    region,
                    presentation,
                    capture_layout_bounds,
                    *position,
                    slipway_core::PointerEventKind::Release,
                    None,
                    touch_pointer_details(*id, false),
                    true,
                    true,
                ));
            }
            let region =
                hit_region_at_point(presentation, layout_bounds, *position).or_else(|| {
                    released_pressed_region
                        .as_ref()
                        .and_then(|capture| hit_region_by_id(presentation, &capture.region_id))
                })?;

            Some(pointer_event_for_hit_region_with_details(
                region,
                presentation,
                layout_bounds,
                *position,
                slipway_core::PointerEventKind::Release,
                None,
                touch_pointer_details(*id, false),
                released_pressed_region.is_some(),
                true,
            ))
        }
        iced::Event::Touch(iced::touch::Event::FingerLost { id, position }) => {
            let released_pressed_region = pressed_region.take();
            let capture_layout_bounds = released_pressed_region
                .as_ref()
                .map(|capture| capture.layout_bounds_for_capture(layout_bounds))
                .unwrap_or(layout_bounds);
            let region = released_pressed_region
                .as_ref()
                .and_then(|capture| hit_region_by_id(presentation, &capture.region_id))
                .or_else(|| hit_region_at_point(presentation, layout_bounds, *position))?;

            Some(captured_pointer_event_for_hit_region_with_details(
                region,
                presentation,
                capture_layout_bounds,
                *position,
                slipway_core::PointerEventKind::Cancel,
                None,
                touch_pointer_details(*id, false),
                true,
                true,
            ))
        }
        iced::Event::Keyboard(iced::keyboard::Event::KeyPressed {
            key,
            modifiers,
            text,
            ..
        }) => {
            if !presentation.can_route_text_edit() {
                return None;
            }
            let focus = focused_focus_region(presentation, focused_region.as_ref())?;
            if let Some(text) = text {
                let event = InputEvent::Text(slipway_core::TextInputEvent {
                    target: focus_target(focus),
                    target_slot: focus.address.clone(),
                    text: text.to_string(),
                });
                Some(IcedRoutedInput {
                    input: Some(backend_focus_input_event(
                        presentation,
                        focus,
                        DeclaredEventDispatchKind::Text,
                        None,
                        event,
                    )),
                    capture_event: true,
                    request_redraw: true,
                })
            } else {
                let event = InputEvent::Keyboard(slipway_core::KeyboardEvent {
                    target: focus_target(focus),
                    target_slot: focus.address.clone(),
                    key: iced_keyboard_key_label(key),
                    kind: slipway_core::KeyEventKind::Press,
                    modifiers: modifiers_from_iced(*modifiers),
                    details: slipway_core::KeyboardDetails {
                        logical_key: Some(iced_keyboard_key_label(key)),
                        physical_key: None,
                        text: None,
                        repeat: false,
                        location: slipway_core::KeyLocation::Unknown,
                    },
                });
                Some(IcedRoutedInput {
                    input: Some(backend_focus_input_event(
                        presentation,
                        focus,
                        DeclaredEventDispatchKind::Keyboard,
                        None,
                        event,
                    )),
                    capture_event: true,
                    request_redraw: false,
                })
            }
        }
        iced::Event::Keyboard(iced::keyboard::Event::KeyReleased { key, modifiers, .. }) => {
            if !presentation.can_route_text_edit() {
                return None;
            }
            let focus = focused_focus_region(presentation, focused_region.as_ref())?;
            let event = InputEvent::Keyboard(slipway_core::KeyboardEvent {
                target: focus_target(focus),
                target_slot: focus.address.clone(),
                key: iced_keyboard_key_label(key),
                kind: slipway_core::KeyEventKind::Release,
                modifiers: modifiers_from_iced(*modifiers),
                details: slipway_core::KeyboardDetails {
                    logical_key: Some(iced_keyboard_key_label(key)),
                    physical_key: None,
                    text: None,
                    repeat: false,
                    location: slipway_core::KeyLocation::Unknown,
                },
            });
            Some(IcedRoutedInput {
                input: Some(backend_focus_input_event(
                    presentation,
                    focus,
                    DeclaredEventDispatchKind::Keyboard,
                    None,
                    event,
                )),
                capture_event: true,
                request_redraw: false,
            })
        }
        _ => hover_changed.then_some(IcedRoutedInput {
            input: None,
            capture_event: false,
            request_redraw: true,
        }),
    }
}

fn sync_hover_region(
    presentation: &IcedPresentationState,
    layout_bounds: iced::Rectangle,
    cursor: iced::advanced::mouse::Cursor,
    hovered_region: &mut Option<slipway_core::PresentationRegionId>,
) -> bool {
    let next = cursor
        .position()
        .and_then(|position| interactive_region_id_at_point(presentation, layout_bounds, position));

    set_region_id(hovered_region, next)
}

fn set_region_id(
    current: &mut Option<slipway_core::PresentationRegionId>,
    next: Option<slipway_core::PresentationRegionId>,
) -> bool {
    if *current == next {
        false
    } else {
        *current = next;
        true
    }
}

fn interactive_region_id_at_point(
    presentation: &IcedPresentationState,
    layout_bounds: iced::Rectangle,
    position: iced::Point,
) -> Option<slipway_core::PresentationRegionId> {
    hit_region_at_point(presentation, layout_bounds, position)
        .map(|region| region.id.clone())
        .or_else(|| {
            focus_region_at_point(presentation, layout_bounds, position)
                .map(|region| region.id.clone())
        })
}

fn pointer_event_for_hit_region(
    region: &HitRegionDeclaration,
    presentation: &IcedPresentationState,
    layout_bounds: iced::Rectangle,
    position: iced::Point,
    kind: slipway_core::PointerEventKind,
    button: Option<iced::mouse::Button>,
    capture_event: bool,
    request_redraw: bool,
) -> IcedRoutedInput {
    pointer_event_for_hit_region_with_details(
        region,
        presentation,
        layout_bounds,
        position,
        kind,
        button.map(pointer_button),
        pointer_details(button),
        capture_event,
        request_redraw,
    )
}

fn captured_pointer_event_for_hit_region(
    region: &HitRegionDeclaration,
    presentation: &IcedPresentationState,
    layout_bounds: iced::Rectangle,
    position: iced::Point,
    kind: slipway_core::PointerEventKind,
    button: Option<iced::mouse::Button>,
    capture_event: bool,
    request_redraw: bool,
) -> IcedRoutedInput {
    captured_pointer_event_for_hit_region_with_details(
        region,
        presentation,
        layout_bounds,
        position,
        kind,
        button.map(pointer_button),
        pointer_details(button),
        capture_event,
        request_redraw,
    )
}

#[allow(clippy::too_many_arguments)]
fn captured_pointer_event_for_hit_region_with_details(
    region: &HitRegionDeclaration,
    presentation: &IcedPresentationState,
    layout_bounds: iced::Rectangle,
    position: iced::Point,
    kind: slipway_core::PointerEventKind,
    button: Option<slipway_core::PointerButton>,
    details: slipway_core::PointerDetails,
    capture_event: bool,
    request_redraw: bool,
) -> IcedRoutedInput {
    let view_root_local_position = iced_view_root_local_point(layout_bounds, position);
    let (dispatch, evidence) =
        slipway_core::resolve_declared_captured_pointer_dispatch_with_evidence_and_geometry_index(
            EvidenceSource::backend_presented(ICED_BACKEND_ID, "physical-input"),
            presentation.frame.clone(),
            &presentation.geometry_index,
            &presentation.hit_regions,
            &region.id,
            view_root_local_position,
            kind,
            button,
            details,
            capture_event,
        );
    let input = dispatch.map(|dispatch| BackendInputEvent::declared(dispatch.input, evidence));
    let resolved = input.is_some();
    IcedRoutedInput {
        input,
        capture_event: capture_event && resolved,
        request_redraw: request_redraw && resolved,
    }
}

fn pointer_event_for_hit_region_with_details(
    region: &HitRegionDeclaration,
    presentation: &IcedPresentationState,
    layout_bounds: iced::Rectangle,
    position: iced::Point,
    kind: slipway_core::PointerEventKind,
    button: Option<slipway_core::PointerButton>,
    details: slipway_core::PointerDetails,
    capture_event: bool,
    request_redraw: bool,
) -> IcedRoutedInput {
    let view_root_local_position = iced_view_root_local_point(layout_bounds, position);
    let (dispatch, evidence) =
        slipway_core::resolve_declared_pointer_dispatch_with_evidence_and_geometry_index(
            EvidenceSource::backend_presented(ICED_BACKEND_ID, "physical-input"),
            presentation.frame.clone(),
            &presentation.geometry_index,
            std::slice::from_ref(region),
            view_root_local_position,
            kind,
            button,
            details.clone(),
            capture_event,
        );
    let input = dispatch.map(|dispatch| BackendInputEvent::declared(dispatch.input, evidence));
    let resolved = input.is_some();
    IcedRoutedInput {
        input,
        capture_event: capture_event && resolved,
        request_redraw: request_redraw && resolved,
    }
}

fn pointer_event_for_focus_region(
    region: &FocusRegionDeclaration,
    presentation: &IcedPresentationState,
    layout_bounds: iced::Rectangle,
    position: iced::Point,
    kind: slipway_core::PointerEventKind,
    button: Option<iced::mouse::Button>,
    capture_event: bool,
    request_redraw: bool,
) -> IcedRoutedInput {
    let target_rect = target_rect_for_focus_region(presentation, region);
    let view_root_local_position = iced_view_root_local_point(layout_bounds, position);
    let event = InputEvent::Pointer(slipway_core::PointerEvent {
        target: focus_target(region),
        target_slot: region.address.clone(),
        position: target_local_point(layout_bounds, target_rect, position),
        target_bounds: Some(TargetLocalRect::new(target_local_bounds(target_rect))),
        kind,
        button: button.map(pointer_button),
        details: pointer_details(button),
    });
    IcedRoutedInput {
        input: Some(backend_focus_input_event(
            presentation,
            region,
            DeclaredEventDispatchKind::Pointer,
            Some(view_root_local_position),
            event,
        )),
        capture_event,
        request_redraw,
    }
}

fn focus_target(region: &FocusRegionDeclaration) -> WidgetId {
    region.target.clone()
}

fn backend_focus_input_event(
    presentation: &IcedPresentationState,
    region: &FocusRegionDeclaration,
    kind: DeclaredEventDispatchKind,
    input_position: Option<Point>,
    event: InputEvent,
) -> BackendInputEvent {
    backend_focus_input_event_from_parts(
        presentation.frame.clone(),
        &presentation.geometry_index,
        &presentation.focus_regions,
        region,
        kind,
        input_position,
        event,
    )
}

fn backend_focus_input_event_from_parts(
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    focus_regions: &[FocusRegionDeclaration],
    region: &FocusRegionDeclaration,
    kind: DeclaredEventDispatchKind,
    input_position: Option<Point>,
    event: InputEvent,
) -> BackendInputEvent {
    let evidence = slipway_core::declared_focus_text_dispatch_evidence_with_geometry_index(
        EvidenceSource::backend_presented(ICED_BACKEND_ID, "focused-input"),
        frame,
        geometry_index,
        focus_regions,
        Some(region),
        kind,
        input_position,
        event.clone(),
    );
    BackendInputEvent::declared(event, evidence)
}

fn backend_scroll_input_event_from_parts(
    frame: FrameIdentity,
    scroll_regions: &[ScrollRegionDeclaration],
    region: &ScrollRegionDeclaration,
    event: InputEvent,
) -> BackendInputEvent {
    let evidence = slipway_core::declared_scroll_dispatch_evidence(
        EvidenceSource::backend_presented(ICED_BACKEND_ID, "native-scroll"),
        frame,
        scroll_regions,
        Some(region),
        event.clone(),
    );
    BackendInputEvent::declared(event, evidence)
}

fn focus_region_for_native_physical_selector<'a>(
    presentation: &'a IcedPresentationState,
    selector: &DebugPhysicalControlDeclarationSelector,
) -> Option<&'a FocusRegionDeclaration> {
    match selector {
        DebugPhysicalControlDeclarationSelector::Target { target } => presentation
            .focus_regions
            .iter()
            .find(|region| region.enabled && &region.target == target),
        DebugPhysicalControlDeclarationSelector::Region { region } => presentation
            .focus_regions
            .iter()
            .find(|candidate| candidate.enabled && &candidate.id == region),
        DebugPhysicalControlDeclarationSelector::Position { position } => {
            slipway_core::select_declared_focus_region_at_root_local_point_with_geometry_index(
                &presentation.geometry_index,
                &presentation.focus_regions,
                *position,
            )
        }
    }
}

fn scroll_region_for_native_physical_selector<'a>(
    presentation: &'a IcedPresentationState,
    selector: &DebugPhysicalControlDeclarationSelector,
) -> Option<&'a ScrollRegionDeclaration> {
    match selector {
        DebugPhysicalControlDeclarationSelector::Target { target } => presentation
            .scroll_regions
            .iter()
            .find(|region| region.enabled && &region.target == target),
        DebugPhysicalControlDeclarationSelector::Region { region } => presentation
            .scroll_regions
            .iter()
            .find(|candidate| candidate.enabled && &candidate.id == region),
        DebugPhysicalControlDeclarationSelector::Position { position } => {
            slipway_core::select_declared_scroll_region_at_root_local_point_with_geometry_index(
                &presentation.geometry_index,
                &presentation.scroll_regions,
                *position,
            )
        }
    }
}

fn iced_focus_widget_id_for_region(
    region: &FocusRegionDeclaration,
) -> Option<iced::advanced::widget::Id> {
    match region
        .text_edit
        .as_ref()
        .map(|text_edit| text_edit.line_mode)
    {
        Some(TextLineMode::SingleLine) => Some(iced_text_input_id(region)),
        Some(TextLineMode::MultiLine) => Some(iced_text_editor_id(region)),
        None => None,
    }
}

fn backend_native_focus_input_event(
    presentation: &IcedPresentationState,
    region: &FocusRegionDeclaration,
    focused: bool,
) -> BackendInputEvent {
    let event = InputEvent::Focus(slipway_core::FocusEvent {
        target: region.target.clone(),
        target_slot: region.address.clone(),
        focused,
    });
    backend_focus_input_event(
        presentation,
        region,
        DeclaredEventDispatchKind::Focus,
        None,
        event,
    )
}

fn backend_native_text_caret_input_event(
    presentation: &IcedPresentationState,
    region: &FocusRegionDeclaration,
    selection_before: Option<slipway_core::TextSelectionRange>,
    selection_after: Option<slipway_core::TextSelectionRange>,
) -> BackendInputEvent {
    let event = InputEvent::TextEdit(slipway_core::TextEditEvent {
        target: region.target.clone(),
        target_slot: region.address.clone(),
        kind: TextEditKind::MoveCaret,
        text: None,
        selection_before,
        selection_after,
    });
    backend_focus_input_event(
        presentation,
        region,
        DeclaredEventDispatchKind::Text,
        None,
        event,
    )
}

fn backend_native_scroll_input_event(
    presentation: &IcedPresentationState,
    region: &ScrollRegionDeclaration,
    offset_x: f32,
    offset_y: f32,
) -> BackendInputEvent {
    let event = InputEvent::Scroll(ScrollEvent {
        target: region.target.clone(),
        target_slot: region.address.clone(),
        region_id: region.id.clone(),
        offset_x,
        offset_y,
        viewport: region.viewport,
        content_bounds: region.content_bounds,
    });
    backend_scroll_input_event_from_parts(
        presentation.frame.clone(),
        &presentation.scroll_regions,
        region,
        event,
    )
}

fn pointer_capture_for_region(
    region: &HitRegionDeclaration,
    kind: slipway_core::PointerEventKind,
    pointer_is_pressed: bool,
) -> bool {
    slipway_core::declared_pointer_capture_for_region(region, kind, pointer_is_pressed)
}

fn hit_region_at_point<'a>(
    presentation: &'a IcedPresentationState,
    layout_bounds: iced::Rectangle,
    position: iced::Point,
) -> Option<&'a HitRegionDeclaration> {
    let root_local_position = iced_view_root_local_point(layout_bounds, position);
    let selected = slipway_core::select_declared_hit_region_at_root_local_point_with_geometry_index(
        &presentation.geometry_index,
        &presentation.hit_regions,
        root_local_position,
    );
    let Some(occlusion) = paint_occlusion_at_root_local_point(presentation, root_local_position)
    else {
        return selected;
    };
    selected.filter(|region| !paint_occlusion_blocks_hit_order(occlusion, &region.order))
}

fn hit_region_by_id<'a>(
    presentation: &'a IcedPresentationState,
    id: &slipway_core::PresentationRegionId,
) -> Option<&'a HitRegionDeclaration> {
    presentation
        .hit_regions
        .iter()
        .find(|region| region.enabled && &region.id == id)
}

fn focus_region_at_point<'a>(
    presentation: &'a IcedPresentationState,
    layout_bounds: iced::Rectangle,
    position: iced::Point,
) -> Option<&'a FocusRegionDeclaration> {
    let root_local_position = iced_view_root_local_point(layout_bounds, position);
    if paint_occlusion_at_root_local_point(presentation, root_local_position).is_some()
        && hit_region_at_point(presentation, layout_bounds, position).is_none()
    {
        return None;
    }
    slipway_core::select_declared_focus_region_at_root_local_point_with_geometry_index(
        &presentation.geometry_index,
        &presentation.focus_regions,
        root_local_position,
    )
}

fn focused_focus_region<'a>(
    presentation: &'a IcedPresentationState,
    id: Option<&slipway_core::PresentationRegionId>,
) -> Option<&'a FocusRegionDeclaration> {
    let id = id?;
    presentation
        .focus_regions
        .iter()
        .find(|region| region.enabled && region.text_edit.is_some() && &region.id == id)
}

fn iced_view_root_local_point(layout_bounds: iced::Rectangle, position: iced::Point) -> Point {
    Point {
        x: position.x - layout_bounds.x,
        y: position.y - layout_bounds.y,
    }
}

fn paint_occlusion_at_root_local_point(
    presentation: &IcedPresentationState,
    position: Point,
) -> Option<&IcedPaintOcclusionRegion> {
    presentation
        .paint_occlusion_regions
        .iter()
        .filter(|region| rect_contains_root_local_point(region.bounds, position))
        .max_by_key(|region| region.order)
}

fn paint_occlusion_blocks_hit_order(
    occlusion: &IcedPaintOcclusionRegion,
    hit_order: &HitRegionOrder,
) -> bool {
    let (occlusion_z, occlusion_paint, _) = occlusion.order;
    occlusion_z > hit_order.z_index
        || (occlusion_z == hit_order.z_index && occlusion_paint > hit_order.paint_order)
}

fn paint_occlusion_blocks_scroll_order(
    occlusion: &IcedPaintOcclusionRegion,
    scroll: &ScrollRegionDeclaration,
) -> bool {
    paint_occlusion_blocks_hit_order(occlusion, &scroll.order)
}

fn rect_contains_root_local_point(rect: Rect, point: Point) -> bool {
    point.x >= rect.origin.x
        && point.y >= rect.origin.y
        && point.x < rect.origin.x + rect.size.width.max(0.0)
        && point.y < rect.origin.y + rect.size.height.max(0.0)
}

fn target_rect_for_focus_region(
    presentation: &IcedPresentationState,
    region: &FocusRegionDeclaration,
) -> Rect {
    target_rect_for_region_address(presentation, &region.target, region.address.as_ref())
}

fn target_rect_for_region_address(
    presentation: &IcedPresentationState,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
) -> Rect {
    slipway_core::declared_target_rect_for_region_address_with_geometry_index(
        &presentation.geometry_index,
        target,
        address,
    )
}

fn region_root_local_rect(
    presentation: &IcedPresentationState,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    target_local_rect: Rect,
) -> Rect {
    slipway_core::declared_region_root_local_rect_with_geometry_index(
        &presentation.geometry_index,
        target,
        address,
        target_local_rect,
    )
}

fn target_local_bounds(target_rect: Rect) -> Rect {
    Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: target_rect.size,
    }
}

fn target_local_point(
    layout_bounds: iced::Rectangle,
    target_rect: Rect,
    position: iced::Point,
) -> Point {
    Point {
        x: position.x - layout_bounds.x - target_rect.origin.x,
        y: position.y - layout_bounds.y - target_rect.origin.y,
    }
}

fn mouse_interaction_for_presentation(
    layout_bounds: iced::Rectangle,
    cursor: iced::advanced::mouse::Cursor,
    presentation: Option<&IcedPresentationState>,
) -> iced::advanced::mouse::Interaction {
    let Some(presentation) = presentation else {
        return iced::advanced::mouse::Interaction::None;
    };
    let Some(position) = cursor.position() else {
        return iced::advanced::mouse::Interaction::None;
    };

    if let Some(focus) = focus_region_at_point(presentation, layout_bounds, position) {
        if focus.text_edit.is_some() && presentation.can_route_text_edit() {
            return iced::advanced::mouse::Interaction::Text;
        }
    }

    hit_region_at_point(presentation, layout_bounds, position)
        .map(|region| interaction_for_cursor_capability(&region.cursor))
        .unwrap_or(iced::advanced::mouse::Interaction::None)
}

fn interaction_for_cursor_capability(
    capability: &CursorCapability,
) -> iced::advanced::mouse::Interaction {
    match capability {
        CursorCapability::Pointer => iced::advanced::mouse::Interaction::Pointer,
        CursorCapability::Text => iced::advanced::mouse::Interaction::Text,
        CursorCapability::Grab => iced::advanced::mouse::Interaction::Grab,
        CursorCapability::Grabbing => iced::advanced::mouse::Interaction::Grabbing,
        CursorCapability::Move => iced::advanced::mouse::Interaction::Move,
        CursorCapability::Crosshair => iced::advanced::mouse::Interaction::Crosshair,
        CursorCapability::NotAllowed => iced::advanced::mouse::Interaction::NotAllowed,
        CursorCapability::ResizeHorizontal => {
            iced::advanced::mouse::Interaction::ResizingHorizontally
        }
        CursorCapability::ResizeVertical => iced::advanced::mouse::Interaction::ResizingVertically,
        CursorCapability::ResizeBoth => iced::advanced::mouse::Interaction::ResizingDiagonallyDown,
        CursorCapability::Inherited | CursorCapability::Default | CursorCapability::Custom(_) => {
            iced::advanced::mouse::Interaction::None
        }
    }
}

fn draw_paint_op<Renderer>(renderer: &mut Renderer, origin: iced::Point, op: &PaintOp)
where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    draw_paint_op_with_clip(renderer, origin, op, None);
}

fn draw_paint_op_with_clip<Renderer>(
    renderer: &mut Renderer,
    origin: iced::Point,
    op: &PaintOp,
    active_clip: Option<iced::Rectangle>,
) where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    match op {
        PaintOp::Fill { shape, color } => {
            if draw_geometry_fill(renderer, origin, shape, *color) {
                return;
            }
            draw_supported_quad_shape(renderer, origin, shape, |renderer| {
                renderer.fill_quad(
                    quad(origin, shape, 0.0, iced::Color::TRANSPARENT),
                    iced_color(*color),
                );
            });
        }
        PaintOp::Stroke {
            shape,
            color,
            width,
        } => {
            if draw_geometry_stroke(renderer, origin, shape, *color, *width) {
                return;
            }
            draw_supported_quad_shape(renderer, origin, shape, |renderer| {
                renderer.fill_quad(
                    quad(origin, shape, *width, iced_color(*color)),
                    iced::Color::TRANSPARENT,
                );
            });
        }
        PaintOp::Text {
            bounds,
            content,
            color,
            style,
        } => draw_text_op_with_clip(
            renderer,
            origin,
            *bounds,
            content,
            *color,
            style,
            active_clip,
        ),
        PaintOp::Group { clip, ops, .. } | PaintOp::Layer { clip, ops, .. } => {
            draw_group_ops(renderer, origin, clip.as_ref(), ops, active_clip)
        }
    }
}

fn draw_supported_quad_shape<Renderer>(
    renderer: &mut Renderer,
    origin: iced::Point,
    shape: &ShapeDeclaration,
    draw: impl FnOnce(&mut Renderer),
) where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    if !supports_visible_quad_shape(shape) {
        return;
    }

    if let Some(clip) = shape.clip.as_ref() {
        renderer.start_layer(iced_rect(origin, clip.bounds));
        draw(renderer);
        renderer.end_layer();
    } else {
        draw(renderer);
    }
}

fn draw_geometry_fill<Renderer>(
    renderer: &mut Renderer,
    origin: iced::Point,
    shape: &ShapeDeclaration,
    color: slipway_core::Color,
) -> bool
where
    Renderer: iced::advanced::graphics::geometry::Renderer,
{
    let Some(path) = iced_geometry_path(origin, shape) else {
        return false;
    };
    draw_iced_geometry_shape(renderer, origin, shape, |frame| {
        if shape.kind == ShapeKind::Line {
            frame.stroke(
                &path,
                iced::advanced::graphics::geometry::Stroke::default()
                    .with_color(iced_color(color))
                    .with_width(1.0),
            );
        } else {
            frame.fill(&path, iced_color(color));
        }
    })
}

fn draw_geometry_stroke<Renderer>(
    renderer: &mut Renderer,
    origin: iced::Point,
    shape: &ShapeDeclaration,
    color: slipway_core::Color,
    width: f32,
) -> bool
where
    Renderer: iced::advanced::graphics::geometry::Renderer,
{
    let Some(path) = iced_geometry_path(origin, shape) else {
        return false;
    };
    draw_iced_geometry_shape(renderer, origin, shape, |frame| {
        frame.stroke(
            &path,
            iced::advanced::graphics::geometry::Stroke::default()
                .with_color(iced_color(color))
                .with_width(width.max(0.0)),
        );
    })
}

fn draw_iced_geometry_shape<Renderer>(
    renderer: &mut Renderer,
    origin: iced::Point,
    shape: &ShapeDeclaration,
    draw: impl FnOnce(&mut iced::advanced::graphics::geometry::Frame<Renderer>),
) -> bool
where
    Renderer: iced::advanced::graphics::geometry::Renderer,
{
    if shape.clip.as_ref().is_some_and(|clip| clip.path.is_some()) {
        return false;
    }

    let clip = shape
        .clip
        .as_ref()
        .map(|clip| iced_rect(origin, clip.bounds))
        .unwrap_or_else(|| iced_rect(origin, shape.bounds));
    if clip.width <= 0.0 || clip.height <= 0.0 {
        return false;
    }

    let mut frame = iced::advanced::graphics::geometry::Frame::with_bounds(renderer, clip);
    draw(&mut frame);
    renderer.draw_geometry(frame.into_geometry());
    true
}

fn iced_geometry_path(
    origin: iced::Point,
    shape: &ShapeDeclaration,
) -> Option<iced::advanced::graphics::geometry::Path> {
    match shape.kind {
        ShapeKind::Line => {
            let rect = iced_rect(origin, shape.bounds);
            Some(iced::advanced::graphics::geometry::Path::line(
                rect.position(),
                iced::Point::new(rect.x + rect.width, rect.y + rect.height),
            ))
        }
        ShapeKind::Path => iced_path_declaration(origin, shape.path.as_ref()?),
        _ => None,
    }
}

fn iced_path_declaration(
    origin: iced::Point,
    path: &PathDeclaration,
) -> Option<iced::advanced::graphics::geometry::Path> {
    let mut has_draw_command = false;
    let geometry_path = iced::advanced::graphics::geometry::Path::new(|builder| {
        for command in &path.commands {
            match command {
                PathCommand::MoveTo(point) => {
                    builder.move_to(iced_path_point(origin, *point));
                }
                PathCommand::LineTo(point) => {
                    has_draw_command = true;
                    builder.line_to(iced_path_point(origin, *point));
                }
                PathCommand::QuadraticTo { control, to } => {
                    has_draw_command = true;
                    builder.quadratic_curve_to(
                        iced_path_point(origin, *control),
                        iced_path_point(origin, *to),
                    );
                }
                PathCommand::CubicTo {
                    control_1,
                    control_2,
                    to,
                } => {
                    has_draw_command = true;
                    builder.bezier_curve_to(
                        iced_path_point(origin, *control_1),
                        iced_path_point(origin, *control_2),
                        iced_path_point(origin, *to),
                    );
                }
                PathCommand::Close => {
                    builder.close();
                }
            }
        }
    });
    has_draw_command.then_some(geometry_path)
}

fn draw_group_ops<Renderer>(
    renderer: &mut Renderer,
    origin: iced::Point,
    clip: Option<&slipway_core::ClipDeclaration>,
    ops: &[PaintOp],
    active_clip: Option<iced::Rectangle>,
) where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    if let Some(clip) = clip {
        if clip.path.is_some() {
            return;
        }

        let Some(effective_clip) = effective_iced_clip(active_clip, iced_rect(origin, clip.bounds))
        else {
            return;
        };
        renderer.start_layer(effective_clip);
        for op in ops {
            draw_paint_op_with_clip(renderer, origin, op, Some(effective_clip));
        }
        renderer.end_layer();
    } else {
        for op in ops {
            draw_paint_op_with_clip(renderer, origin, op, active_clip);
        }
    }
}

fn effective_iced_clip(
    active_clip: Option<iced::Rectangle>,
    next_clip: iced::Rectangle,
) -> Option<iced::Rectangle> {
    let clip = iced_intersect_clips(active_clip, next_clip)?;
    (clip.width > 0.0 && clip.height > 0.0).then_some(clip)
}

fn supports_visible_quad_shape(shape: &ShapeDeclaration) -> bool {
    shape.path.is_none()
        && shape.clip.as_ref().is_none_or(|clip| clip.path.is_none())
        && matches!(
            shape.kind,
            ShapeKind::Rectangle | ShapeKind::RoundedRectangle | ShapeKind::Circle
        )
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
        .map(|requirement| {
            let (capability, reason) =
                if is_iced_overlay_forwarding_requirement(&requirement.capability) {
                    (Capability::Overlay, ICED_OVERLAY_MISSING_CONTRACT_REASON)
                } else {
                    (
                        Capability::BackendCapabilityNegotiation,
                        "required visible backend capability is not declared",
                    )
                };

            unsupported_visible_capability(
                requirement.target.clone(),
                capability,
                requirement.capability.clone(),
                requirement.requirement_id.clone(),
                reason,
            )
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
        CapabilityProfileKind::Popup => vec![iced_overlay_forwarding_visible_capability()],
        CapabilityProfileKind::ProviderSurface => vec![iced_provider_surface_visible_capability()],
        CapabilityProfileKind::BackendAdapter => vec![
            BackendVisibleCapability::HitRegions,
            BackendVisibleCapability::Cursor,
            BackendVisibleCapability::FocusRegions,
            BackendVisibleCapability::ScrollRegions,
            BackendVisibleCapability::ShapePathClip,
            BackendVisibleCapability::FontInstallation,
            BackendVisibleCapability::BackendPresentedEvidence,
            iced_provider_surface_visible_capability(),
            iced_overlay_forwarding_visible_capability(),
        ],
        other => vec![BackendVisibleCapability::Custom(format!("{other:?}"))],
    }
}

fn iced_overlay_forwarding_visible_capability() -> BackendVisibleCapability {
    BackendVisibleCapability::Custom(ICED_OVERLAY_FORWARDING_REQUIREMENT.to_string())
}

fn iced_provider_surface_visible_capability() -> BackendVisibleCapability {
    BackendVisibleCapability::Custom(ICED_PROVIDER_SURFACE_REQUIREMENT.to_string())
}

fn is_iced_overlay_forwarding_requirement(capability: &BackendVisibleCapability) -> bool {
    matches!(
        capability,
        BackendVisibleCapability::Custom(name) if name == ICED_OVERLAY_FORWARDING_REQUIREMENT
    )
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

fn unsupported_visible_capability(
    target: Option<WidgetId>,
    capability: Capability,
    visible_capability: BackendVisibleCapability,
    requirement_id: impl Into<String>,
    reason: impl Into<String>,
) -> UnsupportedCapabilityEvidence {
    UnsupportedCapabilityEvidence {
        backend_id: ICED_BACKEND_ID.to_string(),
        target,
        capability,
        visible_capability: Some(visible_capability),
        requirement_id: Some(requirement_id.into()),
        reason: reason.into(),
        source: EvidenceSource::backend_presented(ICED_BACKEND_ID, "visible-admission"),
        diagnostics: Vec::new(),
    }
}

fn scroll_region_has_real_child_ui(
    view: &ViewDefinition,
    scroll: &ScrollRegionDeclaration,
) -> bool {
    scroll_region_has_real_child_placements(&view.layout, scroll)
}

fn scroll_region_has_real_child_placements(
    layout: &LayoutOutput,
    scroll: &ScrollRegionDeclaration,
) -> bool {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    layout
        .child_placements
        .iter()
        .any(|placement| scroll_contains_placement_for_layout(&geometry_index, scroll, placement))
}

fn scroll_contains_placement(
    presentation: &IcedPresentationState,
    scroll: &ScrollRegionDeclaration,
    placement: &slipway_core::ChildPlacement,
) -> bool {
    scroll_contains_placement_for_layout(&presentation.geometry_index, scroll, placement)
}

fn scroll_contains_placement_for_layout(
    geometry_index: &PresentationGeometryIndex,
    scroll: &ScrollRegionDeclaration,
    placement: &slipway_core::ChildPlacement,
) -> bool {
    if !scroll_owns_placement(scroll, placement) {
        return false;
    }

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

fn scroll_owns_placement(
    scroll: &ScrollRegionDeclaration,
    placement: &slipway_core::ChildPlacement,
) -> bool {
    match scroll.address.as_ref() {
        Some(address) => placement
            .local_state_slot
            .as_ref()
            .is_some_and(|slot| slot_is_descendant(address, slot)),
        None => placement
            .local_state_slot
            .as_ref()
            .is_some_and(|slot| slot.path.first() == Some(&scroll.target) && slot.path.len() > 1),
    }
}

fn slot_is_descendant(parent: &WidgetSlotAddress, child: &WidgetSlotAddress) -> bool {
    child.path.len() > parent.path.len() && child.path.starts_with(&parent.path)
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

fn scroll_without_real_child_refusal(
    scroll: &ScrollRegionDeclaration,
) -> UnsupportedCapabilityEvidence {
    let requirement_id = format!("view.scroll_regions.{}.real_child_ui", scroll.id.as_str());
    let diagnostic = Diagnostic::unsupported(
        Some(scroll.target.clone()),
        "iced.scroll_region.no_real_child_ui",
        "iced Scrollable requires matching authored child UI; no child placement was available for this scroll region",
    );

    UnsupportedCapabilityEvidence {
        backend_id: ICED_BACKEND_ID.to_string(),
        target: Some(scroll.target.clone()),
        capability: Capability::ScrollRegionPresentation,
        visible_capability: Some(BackendVisibleCapability::ScrollRegions),
        requirement_id: Some(requirement_id),
        reason: "scroll region cannot be presented as real iced Scrollable content without a matching authored child placement".to_string(),
        source: EvidenceSource::backend_presented(ICED_BACKEND_ID, "scroll-region-assembly"),
        diagnostics: vec![diagnostic],
    }
}

fn paint_op_requires_font_installation(op: &PaintOp) -> bool {
    match op {
        PaintOp::Text { style, .. } => text_style_requires_font_installation(style),
        PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
            ops.iter().any(paint_op_requires_font_installation)
        }
        PaintOp::Fill { .. } | PaintOp::Stroke { .. } => false,
    }
}

fn text_style_requires_font_installation(style: &TextStyle) -> bool {
    !matches!(
        style.font_family.trim().to_ascii_lowercase().as_str(),
        "" | "system-ui" | "sans-serif" | "serif" | "monospace"
    )
}

fn unsupported_visible_paint_diagnostics(target: &WidgetId, ops: &[PaintOp]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    collect_unsupported_visible_paint(target, ops, &mut diagnostics);
    diagnostics
}

fn collect_unsupported_visible_paint(
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
                        "iced.visible_paint.unsupported_shape_clip_path",
                        format!(
                            "iced visible renderer cannot draw path clips for ShapeKind::{:?}",
                            shape.kind
                        ),
                    ));
                }
            }
            PaintOp::Group { clip, ops, .. } => {
                if clip.as_ref().is_some_and(|clip| clip.path.is_some()) {
                    diagnostics.push(Diagnostic::unsupported(
                        Some(target.clone()),
                        "iced.visible_paint.unsupported_group_clip_path",
                        "iced visible renderer supports rectangular group clips only in this backend build",
                    ));
                }
                collect_unsupported_visible_paint(target, ops, diagnostics);
            }
            PaintOp::Layer { clip, ops, .. } => {
                if clip.as_ref().is_some_and(|clip| clip.path.is_some()) {
                    diagnostics.push(Diagnostic::unsupported(
                        Some(target.clone()),
                        "iced.visible_paint.unsupported_layer_clip_path",
                        "iced visible renderer supports rectangular paint layer clips only in this backend build",
                    ));
                }
                collect_unsupported_visible_paint(target, ops, diagnostics);
            }
            PaintOp::Text { .. } => {}
        }
    }
}

fn draw_text_op_with_clip<Renderer>(
    renderer: &mut Renderer,
    origin: iced::Point,
    bounds: Rect,
    content: &str,
    color: slipway_core::Color,
    style: &TextStyle,
    active_clip: Option<iced::Rectangle>,
) where
    Renderer: iced::advanced::text::Renderer<Font = iced::Font>
        + iced::advanced::graphics::geometry::Renderer
        + 'static,
{
    let rect = iced_rect(origin, bounds);
    let clip_bounds = match active_clip {
        Some(clip) => {
            let Some(visible_rect) = effective_iced_clip(Some(rect), clip) else {
                return;
            };
            if visible_rect != rect {
                return;
            }
            visible_rect
        }
        None => rect,
    };
    if clip_bounds.width <= 0.0 || clip_bounds.height <= 0.0 {
        return;
    }
    let text_color = iced_color(color);
    let font = iced_font(style);
    let size = iced::Pixels(iced_text_font_size(style));
    let line_height = iced::advanced::text::LineHeight::Relative(1.2);

    if !style.decoration.underline && !style.decoration.strikethrough {
        let text = iced::advanced::Text {
            content: content.to_string(),
            bounds: rect.size(),
            size,
            line_height,
            font,
            align_x: iced::advanced::text::Alignment::Left,
            align_y: iced::alignment::Vertical::Top,
            shaping: iced::advanced::text::Shaping::default(),
            wrapping: iced::advanced::text::Wrapping::Word,
        };
        renderer.fill_text(
            text,
            iced_text_position(rect, style),
            text_color,
            clip_bounds,
        );
        return;
    }

    let span: iced::advanced::text::Span<'_, (), iced::Font> =
        iced::advanced::text::Span::new(content)
            .size(size)
            .line_height(line_height)
            .font(font)
            .color(text_color)
            .underline(style.decoration.underline)
            .strikethrough(style.decoration.strikethrough);
    let spans = [span];
    let text = iced::advanced::Text {
        content: spans.as_slice(),
        bounds: rect.size(),
        size,
        line_height,
        font,
        align_x: iced::advanced::text::Alignment::Left,
        align_y: iced::alignment::Vertical::Top,
        shaping: iced::advanced::text::Shaping::default(),
        wrapping: iced::advanced::text::Wrapping::Word,
    };
    let paragraph =
        <Renderer::Paragraph as iced::advanced::text::Paragraph>::with_spans::<()>(text);

    renderer.fill_paragraph(
        &paragraph,
        iced_text_position(rect, style),
        text_color,
        clip_bounds,
    );
}

fn iced_font(style: &TextStyle) -> iced::Font {
    iced::Font {
        family: iced_font_family(&style.font_family),
        weight: iced_font_weight(style.font_weight),
        stretch: iced::font::Stretch::Normal,
        style: iced_font_style(style.font_style),
    }
}

fn iced_font_family(family: &str) -> iced::font::Family {
    match family.trim().to_ascii_lowercase().as_str() {
        "serif" => iced::font::Family::Serif,
        "monospace" | "monospaced" | "ui-monospace" => iced::font::Family::Monospace,
        "cursive" => iced::font::Family::Cursive,
        "fantasy" => iced::font::Family::Fantasy,
        "system-ui" | "sans" | "sans-serif" | "proportional" => iced::font::Family::SansSerif,
        _ => iced::font::Family::SansSerif,
    }
}

fn iced_font_weight(weight: FontWeight) -> iced::font::Weight {
    match font_weight_value(weight) {
        0..=150 => iced::font::Weight::Thin,
        151..=250 => iced::font::Weight::ExtraLight,
        251..=350 => iced::font::Weight::Light,
        351..=450 => iced::font::Weight::Normal,
        451..=550 => iced::font::Weight::Medium,
        551..=650 => iced::font::Weight::Semibold,
        651..=750 => iced::font::Weight::Bold,
        751..=850 => iced::font::Weight::ExtraBold,
        _ => iced::font::Weight::Black,
    }
}

fn iced_font_style(style: FontStyle) -> iced::font::Style {
    match style {
        FontStyle::Normal => iced::font::Style::Normal,
        FontStyle::Italic => iced::font::Style::Italic,
    }
}

fn iced_text_font_size(style: &TextStyle) -> f32 {
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

fn font_weight_value(weight: FontWeight) -> u16 {
    match weight {
        FontWeight::Normal => 400,
        FontWeight::Bold => 700,
        FontWeight::Weight(value) => value.clamp(1, 1000),
    }
}

fn iced_text_position(rect: iced::Rectangle, style: &TextStyle) -> iced::Point {
    let y_offset = match style.baseline {
        BaselineShift::Normal => 0.0,
        BaselineShift::Superscript => -normalized_text_size(style) * 0.35,
        BaselineShift::Subscript => normalized_text_size(style) * 0.2,
    };
    iced::Point::new(rect.x, rect.y + y_offset)
}

fn quad(
    origin: iced::Point,
    shape: &ShapeDeclaration,
    border_width: f32,
    border_color: iced::Color,
) -> iced::advanced::renderer::Quad {
    iced::advanced::renderer::Quad {
        bounds: iced_rect(origin, shape.bounds),
        border: iced::Border {
            color: border_color,
            width: border_width.max(0.0),
            radius: match shape.kind {
                ShapeKind::RoundedRectangle => iced::border::Radius::from(4.0),
                ShapeKind::Circle => iced::border::Radius::from(shape.bounds.size.width * 0.5),
                _ => iced::border::Radius::default(),
            },
        },
        shadow: iced::Shadow::default(),
        snap: true,
    }
}

fn iced_rect(origin: iced::Point, rect: Rect) -> iced::Rectangle {
    iced::Rectangle {
        x: origin.x + rect.origin.x,
        y: origin.y + rect.origin.y,
        width: rect.size.width.max(0.0),
        height: rect.size.height.max(0.0),
    }
}

fn iced_path_point(origin: iced::Point, point: Point) -> iced::Point {
    iced::Point::new(origin.x + point.x, origin.y + point.y)
}

fn rect_from_iced(rect: iced::Rectangle) -> Rect {
    Rect {
        origin: Point {
            x: rect.x,
            y: rect.y,
        },
        size: slipway_core::Size {
            width: rect.width,
            height: rect.height,
        },
    }
}

fn size_from_iced(size: iced::Size) -> slipway_core::Size {
    slipway_core::Size {
        width: size.width,
        height: size.height,
    }
}

fn iced_color(color: slipway_core::Color) -> iced::Color {
    iced::Color {
        r: color.red.clamp(0.0, 1.0),
        g: color.green.clamp(0.0, 1.0),
        b: color.blue.clamp(0.0, 1.0),
        a: color.alpha.clamp(0.0, 1.0),
    }
}

fn pointer_button(button: iced::mouse::Button) -> slipway_core::PointerButton {
    match button {
        iced::mouse::Button::Left => slipway_core::PointerButton::Primary,
        iced::mouse::Button::Right => slipway_core::PointerButton::Secondary,
        iced::mouse::Button::Middle | iced::mouse::Button::Back | iced::mouse::Button::Forward => {
            slipway_core::PointerButton::Auxiliary
        }
        iced::mouse::Button::Other(_) => slipway_core::PointerButton::Auxiliary,
    }
}

fn pointer_details(button: Option<iced::mouse::Button>) -> slipway_core::PointerDetails {
    let mut buttons = slipway_core::PointerButtons::default();
    match button {
        Some(iced::mouse::Button::Left) => buttons.primary = true,
        Some(iced::mouse::Button::Right) => buttons.secondary = true,
        Some(
            iced::mouse::Button::Middle
            | iced::mouse::Button::Back
            | iced::mouse::Button::Forward
            | iced::mouse::Button::Other(_),
        ) => buttons.auxiliary = true,
        None => {}
    }

    slipway_core::PointerDetails {
        pointer_id: None,
        device: slipway_core::PointerDeviceKind::Mouse,
        buttons,
        modifiers: slipway_core::Modifiers::default(),
        pressure: None,
        tilt_x: None,
        tilt_y: None,
        twist: None,
    }
}

fn touch_pointer_details(
    finger: iced::touch::Finger,
    active: bool,
) -> slipway_core::PointerDetails {
    let mut buttons = slipway_core::PointerButtons::default();
    buttons.primary = active;

    slipway_core::PointerDetails {
        pointer_id: Some(finger.0),
        device: slipway_core::PointerDeviceKind::Touch,
        buttons,
        modifiers: slipway_core::Modifiers::default(),
        pressure: None,
        tilt_x: None,
        tilt_y: None,
        twist: None,
    }
}

fn modifiers_from_iced(modifiers: iced::keyboard::Modifiers) -> slipway_core::Modifiers {
    slipway_core::Modifiers {
        shift: modifiers.shift(),
        control: modifiers.control(),
        alt: modifiers.alt(),
        meta: modifiers.logo(),
    }
}

fn iced_modifiers_from_slipway(modifiers: slipway_core::Modifiers) -> iced::keyboard::Modifiers {
    let mut iced = iced::keyboard::Modifiers::empty();
    if modifiers.shift {
        iced.insert(iced::keyboard::Modifiers::SHIFT);
    }
    if modifiers.control {
        iced.insert(iced::keyboard::Modifiers::CTRL);
    }
    if modifiers.alt {
        iced.insert(iced::keyboard::Modifiers::ALT);
    }
    if modifiers.meta {
        iced.insert(iced::keyboard::Modifiers::LOGO);
    }
    iced
}

fn iced_key_location_from_slipway(location: slipway_core::KeyLocation) -> iced::keyboard::Location {
    match location {
        slipway_core::KeyLocation::Left => iced::keyboard::Location::Left,
        slipway_core::KeyLocation::Right => iced::keyboard::Location::Right,
        slipway_core::KeyLocation::Numpad => iced::keyboard::Location::Numpad,
        slipway_core::KeyLocation::Standard | slipway_core::KeyLocation::Unknown => {
            iced::keyboard::Location::Standard
        }
    }
}

fn iced_keyboard_key_label(key: &iced::keyboard::Key) -> String {
    match key.as_ref() {
        iced::keyboard::Key::Named(named) => format!("{named:?}"),
        iced::keyboard::Key::Character(value) => value.to_string(),
        iced::keyboard::Key::Unidentified => "Unidentified".to_string(),
    }
}

fn iced_keyboard_key_from_label(label: &str) -> iced::keyboard::Key {
    use iced::keyboard::key::Named;

    let normalized = label
        .strip_prefix("Named(")
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(label);

    let named = match normalized {
        "Enter" => Some(Named::Enter),
        "Tab" => Some(Named::Tab),
        "Space" | " " => Some(Named::Space),
        "ArrowDown" | "Down" => Some(Named::ArrowDown),
        "ArrowLeft" | "Left" => Some(Named::ArrowLeft),
        "ArrowRight" | "Right" => Some(Named::ArrowRight),
        "ArrowUp" | "Up" => Some(Named::ArrowUp),
        "End" => Some(Named::End),
        "Home" => Some(Named::Home),
        "PageDown" => Some(Named::PageDown),
        "PageUp" => Some(Named::PageUp),
        "Backspace" => Some(Named::Backspace),
        "Delete" => Some(Named::Delete),
        "Insert" => Some(Named::Insert),
        "Escape" | "Esc" => Some(Named::Escape),
        "Copy" => Some(Named::Copy),
        "Cut" => Some(Named::Cut),
        "Paste" => Some(Named::Paste),
        "Undo" => Some(Named::Undo),
        "Redo" => Some(Named::Redo),
        _ => None,
    };

    named
        .map(iced::keyboard::Key::Named)
        .unwrap_or_else(|| iced::keyboard::Key::Character(normalized.to_string().into()))
}

fn iced_physical_key_from_label(label: Option<&str>) -> iced::keyboard::key::Physical {
    use iced::keyboard::key::{Code, NativeCode, Physical};

    let Some(label) = label else {
        return Physical::Unidentified(NativeCode::Unidentified);
    };
    let normalized = label
        .strip_prefix("Code(")
        .and_then(|value| value.strip_suffix(')'))
        .unwrap_or(label);

    let code = match normalized {
        "Enter" => Some(Code::Enter),
        "Tab" => Some(Code::Tab),
        "Space" | " " => Some(Code::Space),
        "ArrowDown" | "Down" => Some(Code::ArrowDown),
        "ArrowLeft" | "Left" => Some(Code::ArrowLeft),
        "ArrowRight" | "Right" => Some(Code::ArrowRight),
        "ArrowUp" | "Up" => Some(Code::ArrowUp),
        "End" => Some(Code::End),
        "Home" => Some(Code::Home),
        "PageDown" => Some(Code::PageDown),
        "PageUp" => Some(Code::PageUp),
        "Backspace" => Some(Code::Backspace),
        "Delete" => Some(Code::Delete),
        "Insert" => Some(Code::Insert),
        "Escape" | "Esc" => Some(Code::Escape),
        _ => None,
    };

    code.map(Physical::Code)
        .unwrap_or(Physical::Unidentified(NativeCode::Unidentified))
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::Widget;
    use slipway_core::{
        BaselineShift, CaretGeometryEvidence, CaretSet, Color, CommandEvent, EmittedMessage,
        FontStyle, FontWeight, FrameIdentity, ImeCompositionPolicyDeclaration, PaintOp,
        PointerEvent, PointerEventKind, ScrollEvent, ShapeDeclaration, ShapeKind, Size,
        SlipwayLogic, SlipwaySsot, SlipwayView, SlipwayWidgetListVisitor, SlipwayWidgetTypes,
        TextBufferSnapshot, TextDecoration, TextEditCommandDeclaration, TextEditKind,
        TextSelectionPolicyDeclaration, TextStyle,
    };
    use slipway_debug_bridge::{
        DebugCommand, DebugPhysicalControl, DebugReplyProduct, MessageDisposition,
    };
    use slipway_runtime::SlipwayRuntime;
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    fn test_rgb(red: u8, green: u8, blue: u8) -> slipway_core::Color {
        slipway_core::Color {
            red: f32::from(red) / 255.0,
            green: f32::from(green) / 255.0,
            blue: f32::from(blue) / 255.0,
            alpha: 1.0,
        }
    }

    #[test]
    fn iced_pointer_routing_hot_path_does_not_clone_all_hit_regions() {
        let source = include_str!("lib.rs");

        assert!(
            !source.contains(concat!("presentation.hit_regions", ".clone()")),
            "visible pointer routing must not clone the full hit-region list on the hot path"
        );
    }

    fn collect_paint_text_labels<'a>(ops: &'a [PaintOp], labels: &mut Vec<&'a str>) {
        for op in ops {
            match op {
                PaintOp::Text { content, .. } => labels.push(content.as_str()),
                PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                    collect_paint_text_labels(ops, labels);
                }
                PaintOp::Fill { .. } | PaintOp::Stroke { .. } => {}
            }
        }
    }

    fn paint_text_labels(ops: &[PaintOp]) -> Vec<&str> {
        let mut labels = Vec::new();
        collect_paint_text_labels(ops, &mut labels);
        labels
    }

    fn collect_paint_shape_ids<'a>(ops: &'a [PaintOp], labels: &mut Vec<&'a str>) {
        for op in ops {
            match op {
                PaintOp::Fill { shape, .. } | PaintOp::Stroke { shape, .. } => {
                    if let Some(id) = shape.id.as_deref() {
                        labels.push(id);
                    }
                }
                PaintOp::Group { ops, .. } | PaintOp::Layer { ops, .. } => {
                    collect_paint_shape_ids(ops, labels);
                }
                PaintOp::Text { .. } => {}
            }
        }
    }

    fn paint_shape_ids(ops: &[PaintOp]) -> Vec<&str> {
        let mut labels = Vec::new();
        collect_paint_shape_ids(ops, &mut labels);
        labels
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TestWidget;

    struct DragTestWidget;

    struct HoverTestWidget;

    struct CancelTestWidget;

    #[derive(Clone, Debug, PartialEq)]
    struct FocusedInputWidget;

    #[derive(Clone, Debug, PartialEq)]
    struct ScrollProbeWidget;

    #[derive(Clone, Debug, PartialEq)]
    struct InteractionCapabilityWidget {
        id: WidgetId,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Local {
        clicks: u32,
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Clicked,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct NativeIcedLabel;

    impl SlipwayWidgetTypes for NativeIcedLabel {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwayIcedNativeWidgetSpec for NativeIcedLabel {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.native-label")
        }

        fn initial_local_state(&self) -> Self::LocalState {}

        fn iced_native_element<'a, Theme, Renderer>(
            &'a self,
            _external: &'a Self::ExternalState,
            _local: &'a Self::LocalState,
            _context: IcedNativeWidgetContext<'a>,
        ) -> iced::Element<'a, SlipwayIcedRuntimeMessage<Self::AppMessage>, Theme, Renderer>
        where
            Self::AppMessage: Clone + 'a,
            Theme: iced::widget::text::Catalog
                + iced::widget::text_input::Catalog
                + iced::widget::text_editor::Catalog
                + iced::widget::scrollable::Catalog
                + 'a,
            Renderer: iced::advanced::text::Renderer<Font = iced::Font>
                + iced::advanced::graphics::geometry::Renderer
                + 'a + 'static,
        {
            iced::widget::text("native").into()
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct NativeIcedProviderSurface {
        kind: ProviderSurfaceKind,
    }

    impl SlipwayWidgetTypes for NativeIcedProviderSurface {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwayIcedNativeWidgetSpec for NativeIcedProviderSurface {
        fn id(&self) -> WidgetId {
            match self.kind {
                ProviderSurfaceKind::Canvas => WidgetId::from("iced.provider.canvas"),
                ProviderSurfaceKind::Gpu => WidgetId::from("iced.provider.gpu"),
                _ => WidgetId::from("iced.provider.other"),
            }
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::RenderSurface, Capability::ProviderSurfacePolicy]
        }

        fn initial_local_state(&self) -> Self::LocalState {}

        fn iced_native_element<'a, Theme, Renderer>(
            &'a self,
            _external: &'a Self::ExternalState,
            _local: &'a Self::LocalState,
            _context: IcedNativeWidgetContext<'a>,
        ) -> iced::Element<'a, SlipwayIcedRuntimeMessage<Self::AppMessage>, Theme, Renderer>
        where
            Self::AppMessage: Clone + 'a,
            Theme: iced::widget::text::Catalog
                + iced::widget::text_input::Catalog
                + iced::widget::text_editor::Catalog
                + iced::widget::scrollable::Catalog
                + 'a,
            Renderer: iced::advanced::text::Renderer<Font = iced::Font>
                + iced::advanced::graphics::geometry::Renderer
                + 'a + 'static,
        {
            iced::widget::text("provider").into()
        }
    }

    impl SlipwayIcedProviderSurfaceSpec for NativeIcedProviderSurface {
        fn provider_surface_request(&self) -> ProviderSurfaceRequest {
            ProviderSurfaceRequest {
                target: self.id(),
                provider_id: match self.kind {
                    ProviderSurfaceKind::Canvas => "iced.canvas.provider".to_string(),
                    ProviderSurfaceKind::Gpu => "iced.gpu.provider".to_string(),
                    _ => "iced.other.provider".to_string(),
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

    impl SlipwayIcedSplitGpuProviderSurfaceSpec for NativeIcedProviderSurface {
        type PreparedFrame = ();

        fn prepare_iced_gpu_surface(
            &mut self,
            _context: IcedGpuSurfacePrepareContext<'_>,
        ) -> Result<Self::PreparedFrame, Vec<Diagnostic>> {
            Ok(())
        }

        fn paint_prepared_iced_gpu_surface(
            &self,
            _context: IcedGpuSurfacePaintContext<'_, '_, Self::PreparedFrame>,
        ) -> Result<(), Vec<Diagnostic>> {
            Ok(())
        }
    }

    struct IcedNativeVisitCounter {
        normal: usize,
        native: usize,
    }

    impl SlipwayIcedWidgetListVisitor<(), Message> for IcedNativeVisitCounter {
        fn visit_iced_child<W>(
            &mut self,
            _widget: &W,
            _external: &(),
            _local: &W::LocalState,
            _slot: WidgetSlotAddress,
        ) where
            W: SlipwayIcedBackendChildWidget<ExternalState = (), AppMessage = Message>,
        {
            self.normal += 1;
        }

        fn visit_iced_native_child<N>(
            &mut self,
            _widget: &N,
            _external: &(),
            _local: &N::LocalState,
            _slot: WidgetSlotAddress,
        ) where
            N: SlipwayIcedNativeChildWidget<ExternalState = (), AppMessage = Message>,
        {
            self.native += 1;
        }
    }

    #[derive(Default)]
    struct IcedChildOrderTrace {
        current_order_index: Option<usize>,
        visits: Vec<(usize, WidgetId)>,
    }

    impl SlipwayIcedWidgetListVisitor<(), Message> for IcedChildOrderTrace {
        fn set_iced_child_order_index(&mut self, index: usize) {
            self.current_order_index = Some(index);
        }

        fn visit_iced_child<W>(
            &mut self,
            widget: &W,
            _external: &(),
            _local: &W::LocalState,
            _slot: WidgetSlotAddress,
        ) where
            W: SlipwayIcedBackendChildWidget<ExternalState = (), AppMessage = Message>,
        {
            self.visits.push((
                self.current_order_index
                    .take()
                    .expect("ordered visit supplies child order index"),
                widget.id(),
            ));
        }

        fn visit_iced_native_child<N>(
            &mut self,
            widget: &N,
            _external: &(),
            _local: &N::LocalState,
            _slot: WidgetSlotAddress,
        ) where
            N: SlipwayIcedNativeChildWidget<ExternalState = (), AppMessage = Message>,
        {
            self.visits.push((
                self.current_order_index
                    .take()
                    .expect("ordered native visit supplies child order index"),
                widget.id(),
            ));
        }
    }

    macro_rules! impl_iced_test_leaf_children {
        ($($type:ty),+ $(,)?) => {
            $(
                impl SlipwayIcedAuthoredChildren for $type {
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
            )+
        };
    }

    macro_rules! impl_iced_test_event_policy {
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

    impl_iced_test_leaf_children!(
        TestWidget,
        DragTestWidget,
        HoverTestWidget,
        CancelTestWidget,
        FocusedInputWidget,
        ScrollProbeWidget,
        TextEditUnsupportedWidget,
        ChildPaintWidget,
        LeafTopologyChildPlacementWidget,
        LifecycleProbeChild,
        UnsupportedPaintWidget,
        LayeredChild,
    );

    impl_iced_test_event_policy!(
        TestWidget,
        FocusedInputWidget,
        ScrollProbeWidget,
        TextEditUnsupportedWidget,
        ChildPaintWidget,
        ChildTreeWidget,
        ParentWithTextChildWidget,
        PartiallyPlacedChildTreeWidget,
        LeafTopologyChildPlacementWidget,
        LifecycleProbeWidget,
        LifecycleProbeChild,
        UnsupportedPaintWidget,
        UnsupportedPaintParentWithChildWidget,
        LayeredChild,
        ScrollLayerParent,
    );

    impl SlipwayWidgetTypes for InteractionCapabilityWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for InteractionCapabilityWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::PointerInput,
                Capability::FocusInput,
                Capability::TextInput,
                Capability::WheelInput,
                Capability::ScrollRegionPresentation,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id.clone())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for InteractionCapabilityWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for InteractionCapabilityWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            }
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

    impl slipway_core::SlipwayEventRoutingPolicy for InteractionCapabilityWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            slipway_core::EventRoutingPolicyDeclaration {
                target: self.id.clone(),
                event_target: event.target().clone(),
                route: slipway_core::EventRoute {
                    route_id: None,
                    address: event.target_slot().cloned(),
                    path: vec![self.id.clone()],
                    phase: slipway_core::EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for InteractionCapabilityWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            _route: &slipway_core::EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let disposition = slipway_core::EventDisposition {
                handled: event.target() == &self.id,
                propagate: event.target() != &self.id,
                default_action_allowed: true,
            };
            slipway_core::EventPropagationEvidence {
                target: self.id.clone(),
                event: event.clone(),
                steps: Vec::new(),
                final_disposition: disposition,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayTextBufferPolicy for InteractionCapabilityWidget {
        fn text_buffer(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TextBufferSnapshot {
            TextBufferSnapshot {
                target: self.id.clone(),
                text: String::new(),
                revision: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayTextSelectionPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayImeCompositionPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayCaretGeometryPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayTextEditCommandPolicy for InteractionCapabilityWidget {
        fn text_edit_commands(
            &self,
            _external: &Self::ExternalState,
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
                    enabled: true,
                },
                TextEditCommandDeclaration {
                    command_id: "replace-buffer".to_string(),
                    kind: TextEditKind::ReplaceBuffer,
                    enabled: true,
                },
            ]
        }
    }

    impl slipway_core::SlipwayTextInputVisualStylePolicy for InteractionCapabilityWidget {
        fn text_input_visual_style(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::TextInputVisualStyleDeclaration {
            slipway_core::TextInputVisualStyleDeclaration::explicit(
                self.id.clone(),
                test_rgb(15, 23, 42),
                test_rgb(100, 116, 139),
                test_rgb(15, 23, 42),
                test_rgb(191, 219, 254),
                test_rgb(255, 255, 255),
                test_rgb(203, 213, 225),
                1.0,
                4.0,
                test_rgb(15, 23, 42),
            )
        }
    }

    impl slipway_core::SlipwayTextInputTypographyPolicy for InteractionCapabilityWidget {
        fn text_input_typography(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::TextInputTypographyDeclaration {
            slipway_core::TextInputTypographyDeclaration::explicit(
                self.id.clone(),
                TextStyle::default().with_font_family("system-ui"),
            )
        }
    }

    impl slipway_core::SlipwayTextUndoRedoPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayTextFlowPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayTextMeasurementPolicy for InteractionCapabilityWidget {
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
                policy: self.text_measurement_policy(external, local, input),
                receipts: Vec::new(),
                cache: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayTextMeasurementCachePolicy for InteractionCapabilityWidget {
        fn text_measurement_cache_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> Vec<slipway_core::TextMeasurementCachePolicyDeclaration> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayCachedTextMeasurementPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayFocusTraversal for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwaySemantics for InteractionCapabilityWidget {
        fn semantics(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<slipway_core::SemanticNode> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayDebugEventTracePolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayContainerLayoutPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayChildConstraintPolicy for InteractionCapabilityWidget {
        fn child_constraints(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> Vec<slipway_core::ChildConstraintPolicyDeclaration> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayLayoutInvalidationPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayLayoutEvidencePolicy for InteractionCapabilityWidget {
        fn layout_evidence(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            output: &LayoutOutput,
        ) -> slipway_core::LayoutEvidence {
            slipway_core::LayoutEvidence {
                target: self.id.clone(),
                bounds: output.bounds,
                child_placements: output.child_placements.clone(),
                invalidated: false,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayScrollBehaviorPolicy for InteractionCapabilityWidget {
        fn scroll_behavior_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
        ) -> slipway_core::ScrollBehaviorPolicyDeclaration {
            let viewport = input.viewport;
            slipway_core::ScrollBehaviorPolicyDeclaration {
                target: self.id.clone(),
                region_id: None,
                address: None,
                axes: slipway_core::ScrollAxes {
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
                offset: Point { x: 0.0, y: 0.0 },
                consumption: slipway_core::ScrollConsumptionPolicy {
                    wheel: true,
                    drag: true,
                    keyboard: true,
                    programmatic: true,
                },
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayWheelRoutingPolicy for InteractionCapabilityWidget {
        fn wheel_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _wheel: &slipway_core::WheelEvent,
        ) -> slipway_core::WheelRoutingPolicyDeclaration {
            slipway_core::WheelRoutingPolicyDeclaration {
                target: self.id.clone(),
                routing: slipway_core::WheelRouting::SelfFirst,
                modifiers: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayViewportObservationPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayVirtualCollectionPolicy for InteractionCapabilityWidget {
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

    impl slipway_core::SlipwayHitTesting for InteractionCapabilityWidget {
        fn hit_test(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: slipway_core::HitTestInput,
        ) -> slipway_core::HitTestOutput {
            slipway_core::HitTestOutput {
                target: Some(input.target.clone()),
                local_point: Some(input.point),
                route: slipway_core::EventRoute {
                    route_id: None,
                    address: None,
                    path: vec![input.target],
                    phase: slipway_core::EventRoutePhase::Target,
                },
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayIcedAuthoredChildren for ChildTreeWidget {
        fn visit_iced_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = ChildPaintWidget;
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            visitor.visit_iced_child(&child, external, &(), parent_slot.child(child.id(), 0));
            visitor.visit_iced_child(&child, external, &(), parent_slot.child(child.id(), 1));
        }
    }

    impl SlipwayIcedAuthoredChildren for PartiallyPlacedChildTreeWidget {
        fn visit_iced_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = ChildPaintWidget;
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            visitor.visit_iced_child(
                &child,
                external,
                &(),
                parent_slot.clone().child(child.id(), 0),
            );
            visitor.visit_iced_child(&child, external, &(), parent_slot.child(child.id(), 1));
        }
    }

    impl SlipwayIcedAuthoredChildren for LifecycleProbeWidget {
        fn visit_iced_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = LifecycleProbeChild;
            visitor.visit_iced_child(
                &child,
                external,
                &(),
                WidgetSlotAddress::new(self.id(), 0).child(child.id(), 0),
            );
        }
    }

    impl SlipwayIcedAuthoredChildren for UnsupportedPaintParentWithChildWidget {
        fn visit_iced_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = ChildPaintWidget;
            visitor.visit_iced_child(
                &child,
                external,
                &(),
                WidgetSlotAddress::new(self.id(), 0).child(child.id(), 0),
            );
        }
    }

    #[derive(Default)]
    struct RecordingOperation {
        container_id: Option<iced::advanced::widget::Id>,
        container_bounds: Option<iced::Rectangle>,
        containers: Vec<(Option<iced::advanced::widget::Id>, iced::Rectangle)>,
        traversed: usize,
    }

    impl iced::advanced::widget::Operation for RecordingOperation {
        fn traverse(
            &mut self,
            operate: &mut dyn FnMut(&mut dyn iced::advanced::widget::Operation),
        ) {
            self.traversed += 1;
            operate(self);
        }

        fn container(&mut self, id: Option<&iced::advanced::widget::Id>, bounds: iced::Rectangle) {
            if self.container_id.is_none() {
                self.container_id = id.cloned();
                self.container_bounds = Some(bounds);
            }
            self.containers.push((id.cloned(), bounds));
        }
    }

    fn assert_root_operation(operation: &RecordingOperation) {
        assert_eq!(
            operation.container_id,
            Some(iced::advanced::widget::Id::from("iced.test".to_string()))
        );
        let bounds = operation.container_bounds.unwrap();
        assert_eq!(bounds.width, 100.0);
        assert_eq!(bounds.height, 40.0);
        assert!(operation.traversed > 0);
    }

    fn assert_precise_overlay_missing_contract_reason(reason: &str) {
        assert_eq!(reason, ICED_OVERLAY_MISSING_CONTRACT_REASON);
        for required_detail in [
            "stable authored overlay child/surface content",
            "tree identity",
            "layout",
            "event routing",
            "lifetime path suitable for iced overlay()",
            "current core declarations expose overlay metadata only",
            "final visible lifecycle acceptance remains blocked",
        ] {
            assert!(
                reason.contains(required_detail),
                "overlay refusal reason must mention {required_detail:?}: {reason}"
            );
        }
    }

    fn assert_standalone_child_lifecycle_refusal<T>(
        result: Result<T, Box<dyn std::any::Any + Send>>,
    ) {
        let Err(error) = result else {
            panic!("standalone child-bearing widget path should refuse");
        };
        let message = if let Some(message) = error.downcast_ref::<String>() {
            message.as_str()
        } else if let Some(message) = error.downcast_ref::<&str>() {
            *message
        } else {
            panic!("standalone refusal should use a string panic payload");
        };

        assert!(
            message.contains(ICED_STANDALONE_CHILD_LIFECYCLE_REFUSAL),
            "standalone refusal should explain official child lifecycle blocking: {message}"
        );
    }

    fn test_scroll_region_from_capability(
        target: WidgetId,
        id: &str,
        viewport: Rect,
        content_bounds: Rect,
        consumption: slipway_core::ScrollConsumptionPolicy,
    ) -> ScrollRegionDeclaration {
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(viewport),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let mut region = slipway_core::scroll_region_from_scrollable_capability(
            &InteractionCapabilityWidget { id: target.clone() },
            &(),
            &(),
            &layout,
            Some(PresentationRegionId::from(id)),
            Some(WidgetSlotAddress::new(target, 0)),
            true,
        );
        region.content_bounds = TargetLocalRect::new(content_bounds);
        region.consumption = consumption;
        region
    }

    impl SlipwayWidgetTypes for TestWidget {
        type ExternalState = ();
        type LocalState = Local;
        type AppMessage = Message;
    }

    impl SlipwaySsot for TestWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.test")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::PointerInput, Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for TestWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            local.clicks += 1;
            EventOutcome {
                handled: true,
                propagate: false,
                emitted_messages: vec![EmittedMessage {
                    target: self.id(),
                    name: "clicked".to_string(),
                    message: Message::Clicked,
                }],
                changes: Vec::new(),
                observations: Vec::new(),
                probes: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayView for TestWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            Local { clicks: 0 }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
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
                shape: ShapeDeclaration {
                    id: Some("body".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 1.0,
                    green: 0.0,
                    blue: 0.0,
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
                target: self.id(),
                slot: None,
                name: "clicks".to_string(),
                value: local.clicks.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for TestWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            let bounds = layout.bounds;
            let paint = self.paint(external, local, &layout);

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint,
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: vec![slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    slipway_core::PresentationRegionId::from("iced.test.hit"),
                    None,
                    bounds,
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    slipway_core::HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    Some("iced.test.route".to_string()),
                    CursorCapability::Pointer,
                    true,
                    slipway_core::PointerCaptureIntent::OnPress,
                )],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayWidgetTypes for DragTestWidget {
        type ExternalState = ();
        type LocalState = Local;
        type AppMessage = Message;
    }

    impl SlipwaySsot for DragTestWidget {
        fn id(&self) -> WidgetId {
            TestWidget.id()
        }

        fn capabilities(&self) -> Vec<Capability> {
            TestWidget.capabilities()
        }

        fn topology(&self, external: &Self::ExternalState) -> TopologyNode {
            TestWidget.topology(external)
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            TestWidget.unsupported()
        }
    }

    impl SlipwayLogic for DragTestWidget {
        fn handle_event(
            &self,
            external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match &event {
                InputEvent::Pointer(pointer)
                    if pointer.target == self.id()
                        && pointer.kind == slipway_core::PointerEventKind::Move =>
                {
                    TestWidget.handle_event(external, local, event)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl slipway_core::SlipwayEventRoutingPolicy for DragTestWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &slipway_core::InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            let id = self.id();
            slipway_core::EventRoutingPolicyDeclaration {
                target: id.clone(),
                event_target: event.target().clone(),
                route: slipway_core::EventRoute {
                    route_id: Some("iced.test.move.route".to_string()),
                    address: event.target_slot().cloned(),
                    path: vec![id],
                    phase: slipway_core::EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for DragTestWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &slipway_core::InputEvent,
            _route: &slipway_core::EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let id = self.id();
            let handled = matches!(
                event,
                slipway_core::InputEvent::Pointer(pointer)
                    if pointer.target == id
                        && pointer.kind == slipway_core::PointerEventKind::Move
            );
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

    impl SlipwayView for DragTestWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            TestWidget.initial_local_state()
        }

        fn layout(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            TestWidget.layout(external, local, input)
        }

        fn paint(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            TestWidget.paint(external, local, layout)
        }

        fn observe_state(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            TestWidget.observe_state(external, local)
        }
    }

    impl SlipwayViewDefinition for DragTestWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let mut view = TestWidget.view_definition(external, local, input);
            if let Some(region) = view.hit_regions.first_mut() {
                region.capture = slipway_core::PointerCaptureIntent::DuringDrag;
            }
            view
        }
    }

    impl SlipwayWidgetTypes for HoverTestWidget {
        type ExternalState = ();
        type LocalState = Local;
        type AppMessage = Message;
    }

    impl SlipwaySsot for HoverTestWidget {
        fn id(&self) -> WidgetId {
            TestWidget.id()
        }

        fn capabilities(&self) -> Vec<Capability> {
            TestWidget.capabilities()
        }

        fn topology(&self, external: &Self::ExternalState) -> TopologyNode {
            TestWidget.topology(external)
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            TestWidget.unsupported()
        }
    }

    impl SlipwayLogic for HoverTestWidget {
        fn handle_event(
            &self,
            external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match &event {
                InputEvent::Pointer(pointer)
                    if pointer.target == self.id()
                        && matches!(
                            pointer.kind,
                            slipway_core::PointerEventKind::Enter
                                | slipway_core::PointerEventKind::Leave
                        ) =>
                {
                    TestWidget.handle_event(external, local, event)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl slipway_core::SlipwayEventRoutingPolicy for HoverTestWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &slipway_core::InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            let id = self.id();
            slipway_core::EventRoutingPolicyDeclaration {
                target: id.clone(),
                event_target: event.target().clone(),
                route: slipway_core::EventRoute {
                    route_id: Some("iced.test.hover.route".to_string()),
                    address: event.target_slot().cloned(),
                    path: vec![id],
                    phase: slipway_core::EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for HoverTestWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &slipway_core::InputEvent,
            _route: &slipway_core::EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let id = self.id();
            let handled = matches!(
                event,
                slipway_core::InputEvent::Pointer(pointer)
                    if pointer.target == id
                        && matches!(
                            pointer.kind,
                            slipway_core::PointerEventKind::Enter
                                | slipway_core::PointerEventKind::Leave
                        )
            );
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

    impl SlipwayView for HoverTestWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            TestWidget.initial_local_state()
        }

        fn layout(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            TestWidget.layout(external, local, input)
        }

        fn paint(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            TestWidget.paint(external, local, layout)
        }

        fn observe_state(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            TestWidget.observe_state(external, local)
        }
    }

    impl SlipwayViewDefinition for HoverTestWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            TestWidget.view_definition(external, local, input)
        }
    }

    impl SlipwayWidgetTypes for CancelTestWidget {
        type ExternalState = ();
        type LocalState = Local;
        type AppMessage = Message;
    }

    impl SlipwaySsot for CancelTestWidget {
        fn id(&self) -> WidgetId {
            TestWidget.id()
        }

        fn capabilities(&self) -> Vec<Capability> {
            TestWidget.capabilities()
        }

        fn topology(&self, external: &Self::ExternalState) -> TopologyNode {
            TestWidget.topology(external)
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            TestWidget.unsupported()
        }
    }

    impl SlipwayLogic for CancelTestWidget {
        fn handle_event(
            &self,
            external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match &event {
                InputEvent::Pointer(pointer)
                    if pointer.target == self.id()
                        && pointer.kind == slipway_core::PointerEventKind::Cancel =>
                {
                    TestWidget.handle_event(external, local, event)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl slipway_core::SlipwayEventRoutingPolicy for CancelTestWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &slipway_core::InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            let id = self.id();
            slipway_core::EventRoutingPolicyDeclaration {
                target: id.clone(),
                event_target: event.target().clone(),
                route: slipway_core::EventRoute {
                    route_id: Some("iced.test.cancel.route".to_string()),
                    address: event.target_slot().cloned(),
                    path: vec![id],
                    phase: slipway_core::EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for CancelTestWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &slipway_core::InputEvent,
            _route: &slipway_core::EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let id = self.id();
            let handled = matches!(
                event,
                slipway_core::InputEvent::Pointer(pointer)
                    if pointer.target == id
                        && pointer.kind == slipway_core::PointerEventKind::Cancel
            );
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

    impl SlipwayView for CancelTestWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            TestWidget.initial_local_state()
        }

        fn layout(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            TestWidget.layout(external, local, input)
        }

        fn paint(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            TestWidget.paint(external, local, layout)
        }

        fn observe_state(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            TestWidget.observe_state(external, local)
        }
    }

    impl SlipwayViewDefinition for CancelTestWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            TestWidget.view_definition(external, local, input)
        }
    }

    impl SlipwayLayoutIntent for TestWidget {
        fn layout_intent(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
        ) -> LayoutIntentProbe {
            LayoutIntentProbe {
                target: self.id(),
                intrinsic_size: None,
                size_policy: Some(slipway_core::SizePolicyDeclaration {
                    target: self.id(),
                    width: slipway_core::SizePolicy::Fill { weight: 1.0 },
                    height: slipway_core::SizePolicy::FitContent,
                }),
                resize_policy: None,
                overflow_policy: Some(slipway_core::OverflowPolicyDeclaration {
                    target: self.id(),
                    x: slipway_core::OverflowBehavior::Clip,
                    y: slipway_core::OverflowBehavior::Scroll,
                }),
                auto_layout: None,
                responsive_variant: Some(slipway_core::ResponsiveVariant {
                    target: self.id(),
                    key: if input.viewport.size.width < 120.0 {
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

    impl SlipwayWidgetTypes for FocusedInputWidget {
        type ExternalState = ();
        type LocalState = Local;
        type AppMessage = Message;
    }

    impl SlipwaySsot for FocusedInputWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.focused")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::KeyboardInput,
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

    impl SlipwayLogic for FocusedInputWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            if event.target() != &self.id() {
                return EventOutcome::ignored();
            }
            local.clicks += 1;
            EventOutcome::message(self.id(), "focused", Message::Clicked)
        }
    }

    impl SlipwayView for FocusedInputWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            Local { clicks: 0 }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
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
                shape: ShapeDeclaration {
                    id: Some("focused-body".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 0.0,
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
                target: self.id(),
                slot: None,
                name: "clicks".to_string(),
                value: local.clicks.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for FocusedInputWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout_input = input.layout_input.clone();
            let layout = self.layout(external, local, layout_input.clone());
            let target = self.id();

            ViewDefinition {
                target: target.clone(),
                frame: input.frame,
                layout: layout.clone(),
                paint: self.paint(external, local, &layout),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
                hit_regions: Vec::new(),
                focus_regions: vec![slipway_core::text_edit_focus_region_from_capability(
                    &InteractionCapabilityWidget { id: target.clone() },
                    &(),
                    &(),
                    PresentationRegionId::from("iced.focused.focus"),
                    None,
                    layout.bounds,
                    None,
                    true,
                    &layout_input,
                    None,
                )],
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayWidgetTypes for ScrollProbeWidget {
        type ExternalState = ();
        type LocalState = Local;
        type AppMessage = Message;
    }

    impl SlipwaySsot for ScrollProbeWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.scroll")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
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

    impl SlipwayLogic for ScrollProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Scroll(scroll)
                    if scroll.target == self.id()
                        && scroll.region_id == PresentationRegionId::from("iced.scroll.region") =>
                {
                    local.clicks += 1;
                    EventOutcome::message(self.id(), "scrolled", Message::Clicked)
                }
                InputEvent::Wheel(wheel)
                    if wheel.target == self.id()
                        && wheel.target_slot == Some(WidgetSlotAddress::new(self.id(), 0))
                        && wheel.delta_x == 0.0
                        && wheel.delta_y == 7.0 =>
                {
                    local.clicks += 1;
                    EventOutcome::message(self.id(), "wheeled", Message::Clicked)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for ScrollProbeWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            Local { clicks: 0 }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
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
                shape: ShapeDeclaration {
                    id: Some("scroll-body".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 0.0,
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
                target: self.id(),
                slot: None,
                name: "scrolls".to_string(),
                value: local.clicks.to_string(),
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
            let layout = self.layout(external, local, input.layout_input);
            let viewport = layout.bounds.into_rect();
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: vec![test_scroll_region_from_capability(
                    self.id(),
                    "iced.scroll.region",
                    viewport,
                    Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: viewport.size.width,
                            height: viewport.size.height * 4.0,
                        },
                    },
                    slipway_core::ScrollConsumptionPolicy {
                        wheel: true,
                        drag: true,
                        keyboard: true,
                        programmatic: true,
                    },
                )],
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TextEditUnsupportedWidget;

    impl SlipwayWidgetTypes for TextEditUnsupportedWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for TextEditUnsupportedWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.text-edit-unsupported")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::Paint, Capability::TextInput]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for TextEditUnsupportedWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for TextEditUnsupportedWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
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
                shape: ShapeDeclaration {
                    id: Some("text-edit-panel".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 0.2,
                    green: 0.2,
                    blue: 0.2,
                    alpha: 1.0,
                },
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

    impl SlipwayViewDefinition for TextEditUnsupportedWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout_input = input.layout_input.clone();
            let layout = self.layout(external, local, layout_input.clone());
            let bounds = layout.bounds;
            let paint = self.paint(external, local, &layout);
            let target = self.id();

            ViewDefinition {
                target: target.clone(),
                frame: input.frame,
                layout,
                paint,
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: vec![slipway_core::text_edit_focus_region_from_capability(
                    &InteractionCapabilityWidget { id: target.clone() },
                    &(),
                    &(),
                    slipway_core::PresentationRegionId::from("text-edit-focus"),
                    None,
                    bounds,
                    None,
                    true,
                    &layout_input,
                    None,
                )],
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ChildPaintWidget;

    impl SlipwayWidgetTypes for ChildPaintWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for ChildPaintWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.child")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::PointerInput, Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for ChildPaintWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for ChildPaintWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
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
                shape: ShapeDeclaration {
                    id: Some("child-body".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 0.0,
                    green: 0.5,
                    blue: 1.0,
                    alpha: 1.0,
                },
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

    impl SlipwayViewDefinition for ChildPaintWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            let bounds = layout.bounds;
            let paint = self.paint(external, local, &layout);

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint,
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: vec![slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    slipway_core::PresentationRegionId::from("iced.child.hit"),
                    Some(slipway_core::WidgetSlotAddress::new(self.id(), 0)),
                    bounds,
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    slipway_core::HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    Some("iced.child.route".to_string()),
                    CursorCapability::Pointer,
                    true,
                    slipway_core::PointerCaptureIntent::None,
                )],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct LayeredChild {
        id: &'static str,
        z_index: i32,
        keyed_layer_z_index: Option<i32>,
        keyed_layer_order: Option<usize>,
        source_order: bool,
        include_default_paint: bool,
    }

    impl LayeredChild {
        fn new(id: &'static str, z_index: i32, keyed_layer_z_index: Option<i32>) -> Self {
            Self {
                id,
                z_index,
                keyed_layer_z_index,
                keyed_layer_order: Some(0),
                source_order: false,
                include_default_paint: keyed_layer_z_index.is_none(),
            }
        }

        fn with_source_order(mut self) -> Self {
            self.source_order = true;
            self
        }

        fn with_default_paint(mut self) -> Self {
            self.include_default_paint = true;
            self
        }

        fn with_keyed_layer_order(mut self, order: usize) -> Self {
            self.keyed_layer_order = Some(order);
            self
        }
    }

    impl SlipwayWidgetTypes for LayeredChild {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for LayeredChild {
        fn id(&self) -> WidgetId {
            WidgetId::from(self.id)
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

    impl SlipwayLogic for LayeredChild {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for LayeredChild {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            }
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            let text = |label: String| {
                PaintOp::text(
                    layout.bounds.into_rect(),
                    label,
                    Color {
                        red: 0.0,
                        green: 0.0,
                        blue: 0.0,
                        alpha: 1.0,
                    },
                )
            };
            let mut paint = Vec::new();
            if self.include_default_paint {
                paint.push(text(format!("{}:default", self.id)));
            }

            if let Some(z_index) = self.keyed_layer_z_index {
                let key = self.keyed_layer_order.map_or_else(
                    || slipway_core::PaintLayerKey::new(z_index),
                    |order| slipway_core::PaintLayerKey::ordered(z_index, order),
                );
                paint.push(PaintOp::keyed_layer(
                    key,
                    vec![text(format!("{}:layer", self.id))],
                ));
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

    impl SlipwayViewDefinition for LayeredChild {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            let paint = self.paint(external, local, &layout);

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint,
                paint_order: if self.source_order {
                    slipway_core::PaintOrderDeclaration::source_order(self.id())
                } else {
                    slipway_core::PaintOrderDeclaration::layered_order(self.id(), self.z_index, 0)
                },
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct LayeredApp {
        widgets: (LayeredChild, LayeredChild),
    }

    impl slipway_core::SlipwayApp for LayeredApp {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
        type Widgets = (LayeredChild, LayeredChild);

        fn id(&self) -> WidgetId {
            WidgetId::from("iced.layered-app")
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
            slipway_core::AppLayoutPlan {
                bounds: input.viewport,
                children: children
                    .into_iter()
                    .enumerate()
                    .map(|(index, seed)| {
                        let bounds = Rect {
                            origin: Point {
                                x: index as f32 * 10.0,
                                y: 0.0,
                            },
                            size: Size {
                                width: 10.0,
                                height: 10.0,
                            },
                        };
                        slipway_core::ChildLayoutPlan::placed_for_seed(
                            seed,
                            iced_child_layout_input(bounds),
                            slipway_core::ParentLocalRect::new(bounds),
                        )
                    })
                    .collect(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ScrollLayerParent {
        child: LayeredChild,
    }

    impl ScrollLayerParent {
        fn child_slot(&self) -> WidgetSlotAddress {
            WidgetSlotAddress::new(self.id(), 0).child(self.child.id(), 0)
        }
    }

    impl SlipwayWidgetTypes for ScrollLayerParent {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for ScrollLayerParent {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.scroll-layer-parent")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::ChildTraversal,
                Capability::Layout,
                Capability::Paint,
                Capability::ScrollRegionPresentation,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode {
                id: self.id(),
                local_state_slot: Some(WidgetSlotAddress::new(self.id(), 0)),
                children: vec![TopologyNode::leaf(self.child.id())],
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
            visitor.visit_child(&self.child, external, local, self.child_slot());
        }
    }

    impl SlipwayIcedAuthoredChildren for ScrollLayerParent {
        fn visit_iced_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            visitor.visit_iced_child(&self.child, external, local, self.child_slot());
        }
    }

    impl SlipwayLogic for ScrollLayerParent {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for ScrollLayerParent {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: vec![slipway_core::ChildPlacement {
                    child: self.child.id(),
                    bounds: slipway_core::ParentLocalRect::new(Rect {
                        origin: Point { x: 5.0, y: 30.0 },
                        size: Size {
                            width: 50.0,
                            height: 20.0,
                        },
                    }),
                    local_state_slot: Some(self.child_slot()),
                }],
                diagnostics: Vec::new(),
            }
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

    impl SlipwayViewDefinition for ScrollLayerParent {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            let viewport = Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 60.0,
                    height: 20.0,
                },
            };
            let content = Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 60.0,
                    height: 80.0,
                },
            };

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: vec![test_scroll_region_from_capability(
                    self.id(),
                    "iced.scroll-layer.region",
                    viewport,
                    content,
                    slipway_core::ScrollConsumptionPolicy {
                        wheel: true,
                        drag: true,
                        keyboard: true,
                        programmatic: true,
                    },
                )],
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    fn layered_app() -> slipway_core::SlipwayAppWidget<LayeredApp> {
        slipway_core::SlipwayAppWidget::new(LayeredApp {
            widgets: (
                LayeredChild::new("iced.layer.top", 10, None),
                LayeredChild::new("iced.layer.bottom", 0, None),
            ),
        })
    }

    fn keyed_layered_app() -> slipway_core::SlipwayAppWidget<LayeredApp> {
        slipway_core::SlipwayAppWidget::new(LayeredApp {
            widgets: (
                LayeredChild::new("iced.layer.top", 0, Some(20)),
                LayeredChild::new("iced.layer.bottom", 10, None),
            ),
        })
    }

    fn layered_child_placements() -> Vec<slipway_core::ChildPlacement> {
        let parent_slot = WidgetSlotAddress::new(WidgetId::from("iced.layered-app"), 0);
        [("iced.layer.top", 0, 0.0), ("iced.layer.bottom", 1, 10.0)]
            .into_iter()
            .map(|(id, slot_index, x)| {
                let bounds = Rect {
                    origin: Point { x, y: 0.0 },
                    size: Size {
                        width: 10.0,
                        height: 10.0,
                    },
                };
                slipway_core::ChildPlacement {
                    child: WidgetId::from(id),
                    bounds: slipway_core::ParentLocalRect::new(bounds),
                    local_state_slot: Some(parent_slot.child(WidgetId::from(id), slot_index)),
                }
            })
            .collect()
    }

    fn draw_layered_app(app: slipway_core::SlipwayAppWidget<LayeredApp>) -> RecordingRenderer {
        type LayeredRuntimeWidget<'a> =
            SlipwayIcedRuntimeWidget<'a, slipway_core::SlipwayAppWidget<LayeredApp>>;

        let runtime = SlipwayRuntime::new(app, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <LayeredRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::tag(&widget),
            state: <LayeredRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::state(&widget),
            children: <LayeredRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::children(&widget),
        };
        let mut renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 100.0),
        );
        let node = <LayeredRuntimeWidget<'_> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::layout(&mut widget, &mut tree, &renderer, &limits);
        let style = iced::advanced::renderer::Style {
            text_color: iced::Color::BLACK,
        };
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        <LayeredRuntimeWidget<'_> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::draw(
            &widget,
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            iced::advanced::Layout::new(&node),
            iced::advanced::mouse::Cursor::Unavailable,
            &viewport,
        );
        renderer
    }

    fn recorded_text_labels(renderer: &RecordingRenderer) -> Vec<&str> {
        renderer
            .text_calls
            .iter()
            .map(|draw| draw.paragraph.content.as_str())
            .collect()
    }

    fn draw_scroll_layer_parent_after_wheel(wheel_y: f32) -> RecordingRenderer {
        type ScrollLayerRuntimeWidget<'a> = SlipwayIcedRuntimeWidget<'a, ScrollLayerParent>;

        let runtime = SlipwayRuntime::new(
            ScrollLayerParent {
                child: LayeredChild::new("iced.scroll-layer.child", 0, Some(20))
                    .with_default_paint(),
            },
            (),
        );
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <ScrollLayerRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::tag(&widget),
            state: <ScrollLayerRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::state(&widget),
            children: <ScrollLayerRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::children(&widget),
        };
        let mut renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 100.0),
        );
        let node = <ScrollLayerRuntimeWidget<'_> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::layout(&mut widget, &mut tree, &renderer, &limits);
        let layout = iced::advanced::Layout::new(&node);
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 100.0,
        };
        let mut clipboard = iced::advanced::clipboard::Null;
        let mut messages = Vec::new();
        let mut shell = iced::advanced::Shell::new(&mut messages);
        <ScrollLayerRuntimeWidget<'_> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::update(
            &mut widget,
            &mut tree,
            &iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: wheel_y },
            }),
            layout,
            iced::advanced::mouse::Cursor::Available(iced::Point::new(5.0, 5.0)),
            &renderer,
            &mut clipboard,
            &mut shell,
            &viewport,
        );

        let style = iced::advanced::renderer::Style {
            text_color: iced::Color::BLACK,
        };
        <ScrollLayerRuntimeWidget<'_> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::draw(
            &widget,
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            layout,
            iced::advanced::mouse::Cursor::Unavailable,
            &viewport,
        );
        renderer
    }

    #[test]
    fn iced_authored_children_visit_back_to_front_in_declared_paint_order() {
        let app = layered_app();
        let local = app.initial_local_state();
        let frame = frame(111);
        let placements = layered_child_placements();
        let mut trace = IcedChildOrderTrace::default();

        app.visit_iced_authored_children_in_paint_order(
            &(),
            &local,
            &frame,
            &placements,
            IcedChildTraversalOrder::BackToFront,
            &mut trace,
        );

        assert_eq!(
            trace.visits,
            vec![
                (0, WidgetId::from("iced.layer.bottom")),
                (1, WidgetId::from("iced.layer.top")),
            ]
        );
    }

    #[test]
    fn iced_authored_children_visit_front_to_back_but_keep_tree_indices() {
        let app = layered_app();
        let local = app.initial_local_state();
        let frame = frame(112);
        let placements = layered_child_placements();
        let mut trace = IcedChildOrderTrace::default();

        app.visit_iced_authored_children_in_paint_order(
            &(),
            &local,
            &frame,
            &placements,
            IcedChildTraversalOrder::FrontToBack,
            &mut trace,
        );

        assert_eq!(
            trace.visits,
            vec![
                (1, WidgetId::from("iced.layer.top")),
                (0, WidgetId::from("iced.layer.bottom")),
            ]
        );
    }

    #[test]
    fn iced_child_paint_sort_key_uses_highest_expanded_keyed_layer() {
        let app = keyed_layered_app();
        let local = app.initial_local_state();
        let frame = frame(113);
        let placements = layered_child_placements();
        let mut trace = IcedChildOrderTrace::default();

        app.visit_iced_authored_children_in_paint_order(
            &(),
            &local,
            &frame,
            &placements,
            IcedChildTraversalOrder::BackToFront,
            &mut trace,
        );

        assert_eq!(
            trace.visits,
            vec![
                (0, WidgetId::from("iced.layer.bottom")),
                (1, WidgetId::from("iced.layer.top")),
            ]
        );
    }

    #[test]
    fn runtime_widget_layout_keeps_layered_child_tree_and_layout_source_slot_order() {
        type LayeredRuntimeWidget<'a> =
            SlipwayIcedRuntimeWidget<'a, slipway_core::SlipwayAppWidget<LayeredApp>>;

        let app = layered_app();
        let local = app.initial_local_state();
        let frame = frame(114);
        let placements = layered_child_placements();
        let mut trace = IcedChildOrderTrace::default();
        app.visit_iced_authored_children_in_paint_order(
            &(),
            &local,
            &frame,
            &placements,
            IcedChildTraversalOrder::BackToFront,
            &mut trace,
        );
        assert_eq!(
            trace.visits,
            vec![
                (0, WidgetId::from("iced.layer.bottom")),
                (1, WidgetId::from("iced.layer.top")),
            ],
            "visual traversal can still differ from source order"
        );

        let runtime = SlipwayRuntime::new(app, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <LayeredRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <LayeredRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: <LayeredRuntimeWidget<'_> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&widget),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let node = <LayeredRuntimeWidget<'_> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        assert_eq!(tree.children.len(), 2);
        assert_eq!(node.children().len(), 2);
        assert_eq!(
            tree.children[0]
                .state
                .downcast_ref::<IcedRuntimeTreeState>()
                .id,
            WidgetId::from("iced.layer.top")
        );
        assert_eq!(
            tree.children[1]
                .state
                .downcast_ref::<IcedRuntimeTreeState>()
                .id,
            WidgetId::from("iced.layer.bottom")
        );
        assert_eq!(node.children()[0].bounds().x, 0.0);
        assert_eq!(node.children()[1].bounds().x, 10.0);
    }

    #[test]
    fn iced_child_default_paint_is_not_raised_with_extracted_keyed_layer() {
        let app = slipway_core::SlipwayAppWidget::new(LayeredApp {
            widgets: (
                LayeredChild::new("iced.layer.with-default", 0, Some(20)).with_default_paint(),
                LayeredChild::new("iced.layer.later-normal", 0, None).with_source_order(),
            ),
        });

        let renderer = draw_layered_app(app);

        assert_eq!(
            recorded_text_labels(&renderer),
            vec![
                "iced.layer.with-default:default",
                "iced.layer.later-normal:default",
                "iced.layer.with-default:layer",
            ],
            "the earlier child's default paint stays in the local/source pass while its extracted layer is flushed globally"
        );
    }

    #[test]
    fn iced_earlier_child_keyed_layer_paints_after_later_normal_sibling() {
        let app = slipway_core::SlipwayAppWidget::new(LayeredApp {
            widgets: (
                LayeredChild::new("iced.layer.earlier-keyed", 0, Some(20)),
                LayeredChild::new("iced.layer.later-normal", 0, None).with_source_order(),
            ),
        });

        let renderer = draw_layered_app(app);

        assert_eq!(
            recorded_text_labels(&renderer),
            vec![
                "iced.layer.later-normal:default",
                "iced.layer.earlier-keyed:layer",
            ],
            "the earlier source child layer must escape the child local pass and flush above a later normal sibling"
        );
    }

    #[test]
    fn iced_extracted_keyed_layer_labels_sort_globally_across_siblings() {
        let app = slipway_core::SlipwayAppWidget::new(LayeredApp {
            widgets: (
                LayeredChild::new("iced.layer.high-key", 0, Some(30)).with_keyed_layer_order(5),
                LayeredChild::new("iced.layer.low-key", 0, Some(20)).with_keyed_layer_order(0),
            ),
        });

        let renderer = draw_layered_app(app);

        assert_eq!(
            recorded_text_labels(&renderer),
            vec!["iced.layer.low-key:layer", "iced.layer.high-key:layer"],
            "extracted keyed layers from sibling presentations must share one paint_unit_sort_key order"
        );
    }

    #[test]
    fn iced_scroll_child_keyed_layer_uses_root_queue_with_clip_and_offset() {
        let renderer = draw_scroll_layer_parent_after_wheel(-12.0);
        let draw = renderer
            .text_calls
            .iter()
            .find(|draw| draw.paragraph.content == "iced.scroll-layer.child:layer")
            .expect("scroll-hosted child keyed layer is drawn by the root flush");

        let expected_clip = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 20.0,
        };
        assert!(
            draw.active_layers
                .iter()
                .any(|layer| *layer == expected_clip),
            "the queued scroll child layer must be clipped to the scroll viewport: {:?}",
            draw.active_layers
        );
        assert!(
            (draw.position.y - 18.0).abs() < 0.01,
            "the queued scroll child layer must preserve the scroll offset; got y={}",
            draw.position.y
        );
    }

    #[test]
    fn iced_scroll_child_default_paint_draws_under_native_scroll_layer() {
        let renderer = draw_scroll_layer_parent_after_wheel(-12.0);
        let draw = renderer
            .text_calls
            .iter()
            .find(|draw| draw.paragraph.content == "iced.scroll-layer.child:default")
            .expect("scroll-hosted child default paint is drawn immediately in the child pass");

        let expected_clip = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 60.0,
            height: 20.0,
        };
        assert!(
            draw.active_layers
                .iter()
                .any(|layer| *layer == expected_clip),
            "immediate child local paint must be clipped to the scroll viewport: {:?}",
            draw.active_layers
        );
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ChildTreeWidget;

    impl SlipwayWidgetTypes for ChildTreeWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for ChildTreeWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.parent")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::ChildTraversal, Capability::Layout]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode {
                id: self.id(),
                local_state_slot: Some(slipway_core::WidgetSlotAddress::new(self.id(), 0)),
                children: vec![
                    TopologyNode {
                        id: WidgetId::from("iced.child"),
                        local_state_slot: Some(
                            slipway_core::WidgetSlotAddress::new(self.id(), 0)
                                .child(WidgetId::from("iced.child"), 0),
                        ),
                        children: Vec::new(),
                    },
                    TopologyNode {
                        id: WidgetId::from("iced.child"),
                        local_state_slot: Some(
                            slipway_core::WidgetSlotAddress::new(self.id(), 0)
                                .child(WidgetId::from("iced.child"), 1),
                        ),
                        children: Vec::new(),
                    },
                ],
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }

        fn visit_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = ChildPaintWidget;
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            visitor.visit_child(&child, external, &(), parent_slot.child(child.id(), 0));
            visitor.visit_child(&child, external, &(), parent_slot.child(child.id(), 1));
        }
    }

    impl SlipwayLogic for ChildTreeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for ChildTreeWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: vec![
                    slipway_core::ChildPlacement {
                        child: WidgetId::from("iced.child"),
                        local_state_slot: Some(
                            slipway_core::WidgetSlotAddress::new(self.id(), 0)
                                .child(WidgetId::from("iced.child"), 0),
                        ),
                        bounds: slipway_core::ParentLocalRect::new(Rect {
                            origin: Point { x: 4.0, y: 6.0 },
                            size: Size {
                                width: 20.0,
                                height: 10.0,
                            },
                        }),
                    },
                    slipway_core::ChildPlacement {
                        child: WidgetId::from("iced.child"),
                        local_state_slot: Some(
                            slipway_core::WidgetSlotAddress::new(self.id(), 0)
                                .child(WidgetId::from("iced.child"), 1),
                        ),
                        bounds: slipway_core::ParentLocalRect::new(Rect {
                            origin: Point { x: 30.0, y: 8.0 },
                            size: Size {
                                width: 18.0,
                                height: 12.0,
                            },
                        }),
                    },
                ],
                diagnostics: Vec::new(),
            }
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

    impl SlipwayViewDefinition for ChildTreeWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayLayoutIntent for ChildTreeWidget {
        fn layout_intent(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> LayoutIntentProbe {
            LayoutIntentProbe {
                target: self.id(),
                intrinsic_size: None,
                size_policy: None,
                resize_policy: None,
                overflow_policy: None,
                auto_layout: None,
                responsive_variant: None,
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

    #[derive(Clone, Debug, PartialEq)]
    struct ParentWithTextChildWidget;

    impl SlipwayWidgetTypes for ParentWithTextChildWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for ParentWithTextChildWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.parent-with-text")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::ChildTraversal, Capability::Layout]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            let child = TextEditUnsupportedWidget.id();
            TopologyNode {
                id: self.id(),
                local_state_slot: Some(slipway_core::WidgetSlotAddress::new(self.id(), 0)),
                children: vec![TopologyNode {
                    id: child.clone(),
                    local_state_slot: Some(
                        slipway_core::WidgetSlotAddress::new(self.id(), 0).child(child, 0),
                    ),
                    children: Vec::new(),
                }],
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }

        fn visit_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = TextEditUnsupportedWidget;
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            visitor.visit_child(&child, external, &(), parent_slot.child(child.id(), 0));
        }
    }

    impl SlipwayIcedAuthoredChildren for ParentWithTextChildWidget {
        fn visit_iced_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayIcedWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = TextEditUnsupportedWidget;
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            visitor.visit_iced_child(&child, external, &(), parent_slot.child(child.id(), 0));
        }
    }

    impl SlipwayLogic for ParentWithTextChildWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for ParentWithTextChildWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: vec![slipway_core::ChildPlacement {
                    child: TextEditUnsupportedWidget.id(),
                    local_state_slot: Some(
                        slipway_core::WidgetSlotAddress::new(self.id(), 0)
                            .child(TextEditUnsupportedWidget.id(), 0),
                    ),
                    bounds: slipway_core::ParentLocalRect::new(Rect {
                        origin: Point { x: 20.0, y: 12.0 },
                        size: Size {
                            width: 70.0,
                            height: 24.0,
                        },
                    }),
                }],
                diagnostics: Vec::new(),
            }
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

    impl SlipwayViewDefinition for ParentWithTextChildWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct PartiallyPlacedChildTreeWidget;

    impl SlipwayWidgetTypes for PartiallyPlacedChildTreeWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for PartiallyPlacedChildTreeWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.partial-parent")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::ChildTraversal, Capability::Layout]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            TopologyNode {
                id: self.id(),
                local_state_slot: Some(parent_slot.clone()),
                children: vec![
                    TopologyNode {
                        id: WidgetId::from("iced.child"),
                        local_state_slot: Some(
                            parent_slot.clone().child(WidgetId::from("iced.child"), 0),
                        ),
                        children: Vec::new(),
                    },
                    TopologyNode {
                        id: WidgetId::from("iced.child"),
                        local_state_slot: Some(parent_slot.child(WidgetId::from("iced.child"), 1)),
                        children: Vec::new(),
                    },
                ],
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }

        fn visit_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = ChildPaintWidget;
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            visitor.visit_child(
                &child,
                external,
                &(),
                parent_slot.clone().child(child.id(), 0),
            );
            visitor.visit_child(&child, external, &(), parent_slot.child(child.id(), 1));
        }
    }

    impl SlipwayLogic for PartiallyPlacedChildTreeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for PartiallyPlacedChildTreeWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: vec![slipway_core::ChildPlacement {
                    child: WidgetId::from("iced.child"),
                    local_state_slot: Some(
                        WidgetSlotAddress::new(self.id(), 0).child(WidgetId::from("iced.child"), 1),
                    ),
                    bounds: slipway_core::ParentLocalRect::new(Rect {
                        origin: Point { x: 30.0, y: 8.0 },
                        size: Size {
                            width: 18.0,
                            height: 12.0,
                        },
                    }),
                }],
                diagnostics: Vec::new(),
            }
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

    impl SlipwayViewDefinition for PartiallyPlacedChildTreeWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct LeafTopologyChildPlacementWidget;

    impl SlipwayWidgetTypes for LeafTopologyChildPlacementWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for LeafTopologyChildPlacementWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.leaf-with-placement")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::Layout]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for LeafTopologyChildPlacementWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for LeafTopologyChildPlacementWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: vec![slipway_core::ChildPlacement {
                    child: WidgetId::from("iced.synthetic-child"),
                    local_state_slot: None,
                    bounds: slipway_core::ParentLocalRect::new(Rect {
                        origin: Point { x: 2.0, y: 3.0 },
                        size: Size {
                            width: 10.0,
                            height: 11.0,
                        },
                    }),
                }],
                diagnostics: Vec::new(),
            }
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

    impl SlipwayViewDefinition for LeafTopologyChildPlacementWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Default)]
    struct LifecycleCounterState {
        parent_views: std::cell::Cell<u32>,
        parent_layouts: std::cell::Cell<u32>,
        parent_paints: std::cell::Cell<u32>,
        child_views: std::cell::Cell<u32>,
        child_layouts: std::cell::Cell<u32>,
        child_paints: std::cell::Cell<u32>,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct LifecycleProbeWidget;

    #[derive(Clone, Debug, PartialEq)]
    struct LifecycleProbeChild;

    fn increment(cell: &std::cell::Cell<u32>) {
        cell.set(cell.get() + 1);
    }

    fn lifecycle_counts(counters: &LifecycleCounterState) -> (u32, u32, u32, u32, u32, u32) {
        (
            counters.parent_views.get(),
            counters.parent_layouts.get(),
            counters.parent_paints.get(),
            counters.child_views.get(),
            counters.child_layouts.get(),
            counters.child_paints.get(),
        )
    }

    impl SlipwayWidgetTypes for LifecycleProbeWidget {
        type ExternalState = LifecycleCounterState;
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for LifecycleProbeWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.lifecycle.parent")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::ChildTraversal,
                Capability::Layout,
                Capability::Paint,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            TopologyNode {
                id: self.id(),
                local_state_slot: Some(parent_slot.clone()),
                children: vec![TopologyNode {
                    id: WidgetId::from("iced.lifecycle.child"),
                    local_state_slot: Some(
                        parent_slot.child(WidgetId::from("iced.lifecycle.child"), 0),
                    ),
                    children: Vec::new(),
                }],
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }

        fn visit_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = LifecycleProbeChild;
            visitor.visit_child(
                &child,
                external,
                &(),
                WidgetSlotAddress::new(self.id(), 0).child(child.id(), 0),
            );
        }
    }

    impl SlipwayLogic for LifecycleProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for LifecycleProbeWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            increment(&external.parent_layouts);
            LayoutOutput {
                bounds: input.viewport,
                child_placements: vec![slipway_core::ChildPlacement {
                    child: WidgetId::from("iced.lifecycle.child"),
                    local_state_slot: Some(
                        WidgetSlotAddress::new(self.id(), 0)
                            .child(WidgetId::from("iced.lifecycle.child"), 0),
                    ),
                    bounds: slipway_core::ParentLocalRect::new(Rect {
                        origin: Point { x: 8.0, y: 9.0 },
                        size: Size {
                            width: 30.0,
                            height: 14.0,
                        },
                    }),
                }],
                diagnostics: Vec::new(),
            }
        }

        fn paint(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            _layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            increment(&external.parent_paints);
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

    impl SlipwayViewDefinition for LifecycleProbeWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            increment(&external.parent_views);
            let layout = self.layout(external, local, input.layout_input);
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
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayWidgetTypes for LifecycleProbeChild {
        type ExternalState = LifecycleCounterState;
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for LifecycleProbeChild {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.lifecycle.child")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::PointerInput, Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for LifecycleProbeChild {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for LifecycleProbeChild {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            increment(&external.child_layouts);
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            }
        }

        fn paint(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            increment(&external.child_paints);
            vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some("lifecycle-child-body".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 0.1,
                    green: 0.4,
                    blue: 0.7,
                    alpha: 1.0,
                },
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

    impl SlipwayViewDefinition for LifecycleProbeChild {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            increment(&external.child_views);
            let layout = self.layout(external, local, input.layout_input);
            let bounds = layout.bounds;
            let paint = self.paint(external, local, &layout);

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                layout,
                paint,
                paint_order: slipway_core::PaintOrderDeclaration::source_order(self.id()),
                hit_regions: vec![slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    slipway_core::PresentationRegionId::from("iced.lifecycle.child.hit"),
                    None,
                    bounds,
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    slipway_core::HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    Some("iced.lifecycle.child.route".to_string()),
                    CursorCapability::Pointer,
                    true,
                    slipway_core::PointerCaptureIntent::None,
                )],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct UnsupportedPaintWidget;

    impl SlipwayWidgetTypes for UnsupportedPaintWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for UnsupportedPaintWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.unsupported")
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

    impl SlipwayLogic for UnsupportedPaintWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for UnsupportedPaintWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            }
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Stroke {
                shape: ShapeDeclaration {
                    id: Some("path".to_string()),
                    kind: ShapeKind::Path,
                    bounds: layout.bounds.into_rect(),
                    path: Some(slipway_core::PathDeclaration {
                        commands: vec![
                            slipway_core::PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                            slipway_core::PathCommand::LineTo(Point { x: 12.0, y: 12.0 }),
                        ],
                    }),
                    clip: None,
                },
                color: Color {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 1.0,
                },
                width: 1.0,
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

    impl SlipwayViewDefinition for UnsupportedPaintWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
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
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct UnsupportedPaintParentWithChildWidget;

    impl SlipwayWidgetTypes for UnsupportedPaintParentWithChildWidget {
        type ExternalState = ();
        type LocalState = ();
        type AppMessage = Message;
    }

    impl SlipwaySsot for UnsupportedPaintParentWithChildWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("iced.unsupported-parent")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::ChildTraversal,
                Capability::Layout,
                Capability::Paint,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            let parent_slot = WidgetSlotAddress::new(self.id(), 0);
            TopologyNode {
                id: self.id(),
                local_state_slot: Some(parent_slot.clone()),
                children: vec![TopologyNode {
                    id: WidgetId::from("iced.child"),
                    local_state_slot: Some(parent_slot.child(WidgetId::from("iced.child"), 0)),
                    children: Vec::new(),
                }],
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }

        fn visit_authored_children<V>(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            visitor: &mut V,
        ) where
            V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
        {
            let child = ChildPaintWidget;
            visitor.visit_child(
                &child,
                external,
                &(),
                WidgetSlotAddress::new(self.id(), 0).child(child.id(), 0),
            );
        }
    }

    impl SlipwayLogic for UnsupportedPaintParentWithChildWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for UnsupportedPaintParentWithChildWidget {
        fn initial_local_state(&self) -> Self::LocalState {}

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: vec![slipway_core::ChildPlacement {
                    child: WidgetId::from("iced.child"),
                    local_state_slot: Some(
                        WidgetSlotAddress::new(self.id(), 0).child(WidgetId::from("iced.child"), 0),
                    ),
                    bounds: slipway_core::ParentLocalRect::new(Rect {
                        origin: Point { x: 4.0, y: 6.0 },
                        size: Size {
                            width: 20.0,
                            height: 10.0,
                        },
                    }),
                }],
                diagnostics: Vec::new(),
            }
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Stroke {
                shape: ShapeDeclaration {
                    id: Some("unsupported-parent-path".to_string()),
                    kind: ShapeKind::Path,
                    bounds: layout.bounds.into_rect(),
                    path: Some(slipway_core::PathDeclaration {
                        commands: vec![
                            slipway_core::PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                            slipway_core::PathCommand::LineTo(Point { x: 12.0, y: 12.0 }),
                        ],
                    }),
                    clip: None,
                },
                color: Color {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 1.0,
                },
                width: 1.0,
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

    impl SlipwayViewDefinition for UnsupportedPaintParentWithChildWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
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
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    fn context() -> IcedLayoutContext {
        IcedLayoutContext {
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 40.0,
                },
            },
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
            scale_factor: 1.0,
        }
    }

    fn frame(index: u64) -> FrameIdentity {
        FrameIdentity {
            surface_id: "iced-runtime-shell".to_string(),
            surface_instance_id: "test-instance".to_string(),
            revision: 1,
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

    fn pressed_capture(id: &str) -> IcedPressedPointerCapture {
        pressed_capture_with_origin(id, 0.0, 0.0)
    }

    fn pressed_capture_with_origin(id: &str, x: f32, y: f32) -> IcedPressedPointerCapture {
        IcedPressedPointerCapture {
            region_id: PresentationRegionId::from(id),
            layout_origin: Point { x, y },
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

    fn status_message(id: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.status","arguments":{{"frame":{}}}}}}}"#,
            id,
            frame_json(frame),
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

    fn physical_pointer_phase_message(
        id: &str,
        frame: &FrameIdentity,
        x: f32,
        y: f32,
        phase: &str,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"pointer","phase":"{}","position":{{"x":{},"y":{}}},"device":"mouse"}}}}}}}}"#,
            id,
            frame_json(frame),
            phase,
            x,
            y,
        )
    }

    fn physical_touch_phase_message(
        id: &str,
        frame: &FrameIdentity,
        x: f32,
        y: f32,
        phase: &str,
        pointer_id: u64,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"pointer","phase":"{}","position":{{"x":{},"y":{}}},"device":"touch","pointer_id":{},"pointer_is_pressed":false}}}}}}}}"#,
            id,
            frame_json(frame),
            phase,
            x,
            y,
            pointer_id,
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

    fn physical_text_edit_message(
        id: &str,
        frame: &FrameIdentity,
        target: &str,
        text: &str,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"text_edit","target":"{}","edit_kind":"replace_selection","text":"{}"}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
            text,
        )
    }

    fn physical_command_message(
        id: &str,
        frame: &FrameIdentity,
        target: &str,
        command: &str,
        payload_ref: &str,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"command","target":"{}","command":"{}","payload_ref":"{}"}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
            command,
            payload_ref,
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

    fn probe_event_message(id: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.probe","arguments":{{"frame":{},"kinds":["event"]}}}}}}"#,
            id,
            frame_json(frame),
        )
    }

    fn declared_iced_test_press(
        runtime: &SlipwayRuntime<TestWidget>,
        frame: FrameIdentity,
        position: Point,
    ) -> BackendInputEvent {
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
        let view = TestWidget.visible_backend_view_definition(
            &(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::backend_presented(ICED_BACKEND_ID, "physical-input"),
            frame,
            &view.layout,
            &view.hit_regions,
            position,
            PointerEventKind::Press,
            Some(slipway_core::PointerButton::Primary),
            pointer_details(Some(iced::mouse::Button::Left)),
            true,
        );
        BackendInputEvent::declared(
            dispatch
                .expect("iced test press resolves declared hit region")
                .input,
            evidence,
        )
    }

    #[derive(Clone, Debug, PartialEq)]
    struct RecordedParagraph {
        content: String,
        bounds: iced::Size,
        size: iced::Pixels,
        font: iced::Font,
        line_height: iced::advanced::text::LineHeight,
        align_x: iced::advanced::text::Alignment,
        align_y: iced::alignment::Vertical,
        wrapping: iced::advanced::text::Wrapping,
        shaping: iced::advanced::text::Shaping,
        underline: bool,
        strikethrough: bool,
        color: Option<iced::Color>,
    }

    impl Default for RecordedParagraph {
        fn default() -> Self {
            Self {
                content: String::new(),
                bounds: iced::Size::ZERO,
                size: iced::Pixels(16.0),
                font: iced::Font::DEFAULT,
                line_height: iced::advanced::text::LineHeight::default(),
                align_x: iced::advanced::text::Alignment::default(),
                align_y: iced::alignment::Vertical::Top,
                wrapping: iced::advanced::text::Wrapping::default(),
                shaping: iced::advanced::text::Shaping::default(),
                underline: false,
                strikethrough: false,
                color: None,
            }
        }
    }

    impl iced::advanced::text::Paragraph for RecordedParagraph {
        type Font = iced::Font;

        fn with_text(text: iced::advanced::Text<&str, Self::Font>) -> Self {
            Self {
                content: text.content.to_string(),
                bounds: text.bounds,
                size: text.size,
                font: text.font,
                line_height: text.line_height,
                align_x: text.align_x,
                align_y: text.align_y,
                wrapping: text.wrapping,
                shaping: text.shaping,
                ..Self::default()
            }
        }

        fn with_spans<Link>(
            text: iced::advanced::Text<
                &[iced::advanced::text::Span<'_, Link, Self::Font>],
                Self::Font,
            >,
        ) -> Self {
            let first = text.content.first();
            Self {
                content: text
                    .content
                    .iter()
                    .map(|span| span.text.as_ref())
                    .collect::<String>(),
                bounds: text.bounds,
                size: first.and_then(|span| span.size).unwrap_or(text.size),
                font: first.and_then(|span| span.font).unwrap_or(text.font),
                line_height: first
                    .and_then(|span| span.line_height)
                    .unwrap_or(text.line_height),
                align_x: text.align_x,
                align_y: text.align_y,
                wrapping: text.wrapping,
                shaping: text.shaping,
                underline: first.is_some_and(|span| span.underline),
                strikethrough: first.is_some_and(|span| span.strikethrough),
                color: first.and_then(|span| span.color),
            }
        }

        fn resize(&mut self, new_bounds: iced::Size) {
            self.bounds = new_bounds;
        }

        fn compare(
            &self,
            _text: iced::advanced::Text<(), Self::Font>,
        ) -> iced::advanced::text::Difference {
            iced::advanced::text::Difference::None
        }

        fn size(&self) -> iced::Pixels {
            self.size
        }

        fn font(&self) -> Self::Font {
            self.font
        }

        fn line_height(&self) -> iced::advanced::text::LineHeight {
            self.line_height
        }

        fn align_x(&self) -> iced::advanced::text::Alignment {
            self.align_x
        }

        fn align_y(&self) -> iced::alignment::Vertical {
            self.align_y
        }

        fn wrapping(&self) -> iced::advanced::text::Wrapping {
            self.wrapping
        }

        fn shaping(&self) -> iced::advanced::text::Shaping {
            self.shaping
        }

        fn bounds(&self) -> iced::Size {
            self.bounds
        }

        fn min_bounds(&self) -> iced::Size {
            self.bounds
        }

        fn hit_test(&self, _point: iced::Point) -> Option<iced::advanced::text::Hit> {
            None
        }

        fn hit_span(&self, _point: iced::Point) -> Option<usize> {
            None
        }

        fn span_bounds(&self, _index: usize) -> Vec<iced::Rectangle> {
            Vec::new()
        }

        fn grapheme_position(&self, _line: usize, _index: usize) -> Option<iced::Point> {
            None
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct RecordedParagraphDraw {
        paragraph: RecordedParagraph,
        position: iced::Point,
        color: iced::Color,
        clip_bounds: iced::Rectangle,
        active_layers: Vec<iced::Rectangle>,
    }

    #[derive(Default)]
    struct RecordingRenderer {
        paragraphs: Vec<RecordedParagraphDraw>,
        text_calls: Vec<RecordedParagraphDraw>,
        active_layers: Vec<iced::Rectangle>,
        layers: Vec<iced::Rectangle>,
        quads: usize,
        geometries: usize,
    }

    impl iced::advanced::Renderer for RecordingRenderer {
        fn start_layer(&mut self, bounds: iced::Rectangle) {
            self.layers.push(bounds);
            self.active_layers.push(bounds);
        }

        fn end_layer(&mut self) {
            let _ = self.active_layers.pop();
        }

        fn start_transformation(&mut self, _transformation: iced::Transformation) {}

        fn end_transformation(&mut self) {}

        fn fill_quad(
            &mut self,
            _quad: iced::advanced::renderer::Quad,
            _background: impl Into<iced::Background>,
        ) {
            self.quads += 1;
        }

        fn reset(&mut self, _new_bounds: iced::Rectangle) {}

        fn allocate_image(
            &mut self,
            _handle: &iced::advanced::image::Handle,
            callback: impl FnOnce(
                Result<iced::advanced::image::Allocation, iced::advanced::image::Error>,
            ) + Send
            + 'static,
        ) {
            callback(Err(iced::advanced::image::Error::Unsupported));
        }
    }

    impl iced::advanced::graphics::geometry::Renderer for RecordingRenderer {
        type Geometry = ();
        type Frame = ();

        fn new_frame(&self, _bounds: iced::Rectangle) -> Self::Frame {}

        fn draw_geometry(&mut self, _geometry: Self::Geometry) {
            self.geometries += 1;
        }
    }

    impl iced::advanced::text::Renderer for RecordingRenderer {
        type Font = iced::Font;
        type Paragraph = RecordedParagraph;
        type Editor = ();

        const ICON_FONT: Self::Font = iced::Font::DEFAULT;
        const CHECKMARK_ICON: char = '0';
        const ARROW_DOWN_ICON: char = '0';
        const SCROLL_UP_ICON: char = '0';
        const SCROLL_DOWN_ICON: char = '0';
        const SCROLL_LEFT_ICON: char = '0';
        const SCROLL_RIGHT_ICON: char = '0';
        const ICED_LOGO: char = '0';

        fn default_font(&self) -> Self::Font {
            iced::Font::DEFAULT
        }

        fn default_size(&self) -> iced::Pixels {
            iced::Pixels(16.0)
        }

        fn fill_paragraph(
            &mut self,
            text: &Self::Paragraph,
            position: iced::Point,
            color: iced::Color,
            clip_bounds: iced::Rectangle,
        ) {
            self.paragraphs.push(RecordedParagraphDraw {
                paragraph: text.clone(),
                position,
                color,
                clip_bounds,
                active_layers: self.active_layers.clone(),
            });
        }

        fn fill_editor(
            &mut self,
            _editor: &Self::Editor,
            _position: iced::Point,
            _color: iced::Color,
            _clip_bounds: iced::Rectangle,
        ) {
        }

        fn fill_text(
            &mut self,
            text: iced::advanced::Text<String, Self::Font>,
            position: iced::Point,
            color: iced::Color,
            clip_bounds: iced::Rectangle,
        ) {
            self.text_calls.push(RecordedParagraphDraw {
                paragraph: <RecordedParagraph as iced::advanced::text::Paragraph>::with_text(
                    text.as_ref(),
                ),
                position,
                color,
                clip_bounds,
                active_layers: self.active_layers.clone(),
            });
        }
    }

    #[test]
    fn iced_native_wrapper_participates_in_child_list_entry_traversal() {
        let children = (SlipwayIcedNativeWidget::new(NativeIcedLabel),);
        let local = ((),);
        let root_slot = WidgetSlotAddress::new(WidgetId::from("iced.native-root"), 0);
        let mut counter = IcedNativeVisitCounter {
            normal: 0,
            native: 0,
        };

        children.visit_iced_children(&(), &local, &root_slot, &mut counter);

        assert_eq!(counter.normal, 0);
        assert_eq!(counter.native, 1);
    }

    #[test]
    fn iced_provider_surface_wrapper_exposes_canvas_and_gpu_slots() {
        fn assert_provider_surface<W: slipway_core::SlipwayProviderSurfaceCapability>(_widget: &W) {
        }

        let canvas = SlipwayIcedNativeWidget::new(NativeIcedProviderSurface {
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

        let mut gpu = SlipwayIcedNativeWidget::new(NativeIcedProviderSurface {
            kind: ProviderSurfaceKind::Gpu,
        });
        assert_provider_surface(&gpu);
        assert!(gpu.canvas_surfaces().is_empty());
        assert_eq!(gpu.gpu_surfaces().len(), 1);
        assert_eq!(gpu.gpu_surfaces()[0].provider_id, "iced.gpu.provider");
        assert_eq!(
            gpu.render_surfaces(&(), &())[0].capabilities,
            vec!["gpu".to_string()]
        );

        let hit = gpu.provider_hit_test(HitTestInput {
            target: WidgetId::from("iced.provider.gpu"),
            point: Point { x: 2.0, y: 3.0 },
            pointer: slipway_core::PointerDetails::default(),
        });
        assert_eq!(hit.provider_id, "iced.gpu.provider");
        assert_eq!(hit.hit, None);
        assert!(hit.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "iced.provider_surface.hit_test_unsupported"
                && diagnostic.severity == slipway_core::DiagnosticSeverity::Unsupported
        }));

        let snapshot = gpu.provider_snapshot(ProviderSnapshotRequest {
            target: WidgetId::from("iced.provider.gpu"),
            provider_id: "iced.gpu.provider".to_string(),
            bounds: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 64.0,
                    height: 32.0,
                },
            },
            frame: frame(212),
        });
        assert_eq!(snapshot.provider_id, "iced.gpu.provider");
        assert!(snapshot.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "iced.provider_surface.snapshot_unsupported"
                && diagnostic.severity == slipway_core::DiagnosticSeverity::Unsupported
        }));
    }

    #[test]
    fn iced_split_gpu_provider_contract_is_backend_specific_and_mut_prepare_only() {
        fn assert_split_gpu<W: SlipwayIcedSplitGpuSurfaceProvider<PreparedFrame = ()>>(
            _widget: &W,
        ) {
        }

        let mut gpu = SlipwayIcedNativeWidget::new(NativeIcedProviderSurface {
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
    fn iced_provider_surface_profile_is_admitted() {
        let admission = iced_backend_admission()
            .backend_parity_admission(&[CapabilityProfileKind::ProviderSurface]);

        assert!(admission.accepted);
        assert!(admission.visible_requirements.iter().any(|requirement| {
            requirement.capability == iced_provider_surface_visible_capability()
        }));
    }

    #[test]
    fn adapter_frame_uses_authored_layout_and_paint() {
        let mut adapter = IcedWidgetAdapter::new(TestWidget);
        let mut bridge = DefaultIcedBridge::new();
        let frame = adapter.render_frame(&(), &mut bridge, context());
        assert_eq!(frame.widget_id.as_str(), "iced.test");
        assert_eq!(frame.paint.len(), 1);
    }

    #[test]
    fn adapter_frame_layout_intent_is_explicit_opt_in() {
        let mut adapter = IcedWidgetAdapter::new(TestWidget);
        let mut bridge = DefaultIcedBridge::new();

        let ordinary = adapter.render_frame(&(), &mut bridge, context());
        assert!(ordinary.layout_intent.is_none());

        let with_intent = adapter.render_frame_with_layout_intent(&(), &mut bridge, context());
        assert_eq!(
            with_intent
                .layout_intent
                .as_ref()
                .map(|intent| intent.target.as_str()),
            Some("iced.test")
        );
    }

    #[test]
    fn queued_events_route_through_authored_logic() {
        let mut adapter = IcedWidgetAdapter::new(TestWidget);
        let mut bridge = DefaultIcedBridge::new();
        let frame = frame(1);
        let direct_event = InputEvent::Pointer(PointerEvent {
            target: WidgetId::from("iced.test"),
            target_slot: None,
            position: Point { x: 1.0, y: 1.0 },
            target_bounds: None,
            kind: PointerEventKind::Press,
            button: None,
            details: slipway_core::PointerDetails::default(),
        });

        bridge.queue_event(direct_event);
        let receipts = adapter.route_events(&(), &mut bridge, context(), frame.clone());
        assert_eq!(receipts.len(), 1);
        assert_eq!(adapter.local_state().clicks, 1);
        assert!(receipts[0].outcome.handled);

        let mut layout_bridge = DefaultIcedBridge::new();
        let layout_input = <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
            &mut layout_bridge,
            context(),
        );
        let view = TestWidget.view_definition(
            &(),
            adapter.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::backend_presented(ICED_BACKEND_ID, "adapter-test"),
            frame.clone(),
            &view.layout,
            &view.hit_regions,
            Point { x: 1.0, y: 1.0 },
            PointerEventKind::Press,
            None,
            slipway_core::PointerDetails::default(),
            true,
        );
        let dispatch = dispatch.expect("test hit region should resolve");
        bridge.queue_backend_input_event(BackendInputEvent::declared(dispatch.input, evidence));
        let receipts = adapter.route_events(&(), &mut bridge, context(), frame);
        assert_eq!(receipts.len(), 1);
        assert_eq!(adapter.local_state().clicks, 2);
    }

    #[test]
    fn observation_is_request_scoped() {
        let mut adapter = IcedWidgetAdapter::new(TestWidget);
        let mut bridge = DefaultIcedBridge::new();
        bridge.request_observation();
        adapter.observe(&(), &mut bridge);
        assert!(!bridge.take_probes().is_empty());
        assert!(bridge.take_probes().is_empty());
    }

    #[test]
    fn observe_with_layout_intent_is_request_scoped() {
        let mut adapter = IcedWidgetAdapter::new(TestWidget);
        let mut bridge = DefaultIcedBridge::new();
        let input = <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
            &mut bridge,
            context(),
        );

        adapter.observe_with_layout_intent(&(), &mut bridge, &input);
        assert!(bridge.take_probes().is_empty());

        bridge.request_observation();
        adapter.observe_with_layout_intent(&(), &mut bridge, &input);

        let probes = bridge.take_probes();
        assert_eq!(probes.len(), 3);
        assert!(
            probes
                .iter()
                .any(|probe| matches!(probe, ProbeProduct::LayoutIntent(_)))
        );
        assert!(bridge.take_probes().is_empty());
    }

    #[test]
    fn iced_default_text_style_maps_to_plain_text() {
        let style = TextStyle::default();
        let font = iced_font(&style);

        assert_eq!(font.family, iced::font::Family::SansSerif);
        assert_eq!(font.weight, iced::font::Weight::Normal);
        assert_eq!(font.style, iced::font::Style::Normal);
        assert_eq!(
            iced_text_font_size(&style),
            slipway_core::DEFAULT_TEXT_FONT_SIZE
        );
        assert_eq!(iced_font_family("Inter"), iced::font::Family::SansSerif);
    }

    #[test]
    fn iced_preedit_overlay_uses_text_input_typography() {
        let target = WidgetId::from("text");
        let style = TextStyle::default()
            .with_font_family("serif")
            .with_font_size(22.0)
            .with_font_weight(FontWeight::Bold);
        let bounds = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 120.0,
                height: 32.0,
            },
        });
        let input = LayoutInput {
            viewport: bounds,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: bounds.size,
            },
        };
        let mut region = text_edit_focus_region_from_capability(
            &InteractionCapabilityWidget { id: target },
            &(),
            &(),
            PresentationRegionId::from("focus"),
            None,
            bounds,
            None,
            true,
            &input,
            None,
        );
        region
            .text_edit
            .as_mut()
            .expect("capability helper emits text edit")
            .typography
            .style = style;

        let overlay = preedit_overlay_style_for_region(&region)
            .expect("text edit region yields preedit overlay style");
        assert_eq!(overlay.font.family, iced::font::Family::Serif);
        assert_eq!(overlay.font.weight, iced::font::Weight::Bold);
        assert_eq!(overlay.size, 22.0);
    }

    #[test]
    fn iced_draws_text_through_renderer_paragraph_api() {
        let style = TextStyle {
            font_family: "serif".to_string(),
            font_size: 20.0,
            font_weight: FontWeight::Weight(800),
            font_style: FontStyle::Italic,
            decoration: TextDecoration {
                underline: true,
                strikethrough: true,
            },
            baseline: BaselineShift::Subscript,
        };
        let op = PaintOp::Text {
            bounds: Rect {
                origin: Point { x: 10.0, y: 12.0 },
                size: Size {
                    width: 80.0,
                    height: 24.0,
                },
            },
            content: "iced text".to_string(),
            color: Color {
                red: 0.2,
                green: 0.3,
                blue: 0.4,
                alpha: 1.0,
            },
            style,
        };
        let mut renderer = RecordingRenderer::default();

        draw_paint_op(&mut renderer, iced::Point::new(5.0, 6.0), &op);

        assert_eq!(renderer.paragraphs.len(), 1);
        assert!(renderer.text_calls.is_empty());
        let draw = &renderer.paragraphs[0];
        assert_eq!(draw.paragraph.content, "iced text");
        assert_eq!(draw.paragraph.font.family, iced::font::Family::Serif);
        assert_eq!(draw.paragraph.font.weight, iced::font::Weight::ExtraBold);
        assert_eq!(draw.paragraph.font.style, iced::font::Style::Italic);
        assert_eq!(draw.paragraph.size, iced::Pixels(15.0));
        assert!(draw.paragraph.underline);
        assert!(draw.paragraph.strikethrough);
        assert_eq!(
            draw.paragraph.color,
            Some(iced_color(Color {
                red: 0.2,
                green: 0.3,
                blue: 0.4,
                alpha: 1.0,
            }))
        );
        assert_eq!(draw.position, iced::Point::new(15.0, 22.0));
        assert_eq!(
            draw.clip_bounds,
            iced::Rectangle {
                x: 15.0,
                y: 18.0,
                width: 80.0,
                height: 24.0,
            }
        );
    }

    #[test]
    fn nested_group_text_outside_effective_clip_is_not_drawn() {
        let mut renderer = RecordingRenderer::default();
        let outer_clip = slipway_core::ClipDeclaration {
            id: Some("outer".to_string()),
            bounds: Rect {
                origin: Point { x: 0.0, y: 50.0 },
                size: Size {
                    width: 120.0,
                    height: 60.0,
                },
            },
            path: None,
        };
        let inner_clip = slipway_core::ClipDeclaration {
            id: Some("inner".to_string()),
            bounds: Rect {
                origin: Point { x: 0.0, y: 20.0 },
                size: Size {
                    width: 120.0,
                    height: 60.0,
                },
            },
            path: None,
        };
        let op = PaintOp::Group {
            id: Some("outer-group".to_string()),
            clip: Some(outer_clip),
            ops: vec![PaintOp::Group {
                id: Some("inner-group".to_string()),
                clip: Some(inner_clip),
                ops: vec![PaintOp::Text {
                    bounds: Rect {
                        origin: Point { x: 8.0, y: 24.0 },
                        size: Size {
                            width: 90.0,
                            height: 18.0,
                        },
                    },
                    content: "leaky row".to_string(),
                    color: test_rgb(0, 0, 0),
                    style: TextStyle::default(),
                }],
            }],
        };

        draw_paint_op(&mut renderer, iced::Point::new(0.0, 0.0), &op);

        assert!(
            renderer.text_calls.is_empty(),
            "text fully outside the effective nested group clip must not reach the renderer"
        );
    }

    #[test]
    fn nested_group_text_inside_effective_clip_is_drawn() {
        let mut renderer = RecordingRenderer::default();
        let outer_clip = slipway_core::ClipDeclaration {
            id: Some("outer".to_string()),
            bounds: Rect {
                origin: Point { x: 0.0, y: 50.0 },
                size: Size {
                    width: 120.0,
                    height: 60.0,
                },
            },
            path: None,
        };
        let inner_clip = slipway_core::ClipDeclaration {
            id: Some("inner".to_string()),
            bounds: Rect {
                origin: Point { x: 0.0, y: 20.0 },
                size: Size {
                    width: 120.0,
                    height: 60.0,
                },
            },
            path: None,
        };
        let op = PaintOp::Group {
            id: Some("outer-group".to_string()),
            clip: Some(outer_clip),
            ops: vec![PaintOp::Group {
                id: Some("inner-group".to_string()),
                clip: Some(inner_clip),
                ops: vec![PaintOp::Text {
                    bounds: Rect {
                        origin: Point { x: 8.0, y: 54.0 },
                        size: Size {
                            width: 90.0,
                            height: 18.0,
                        },
                    },
                    content: "visible row".to_string(),
                    color: test_rgb(0, 0, 0),
                    style: TextStyle::default(),
                }],
            }],
        };

        draw_paint_op(&mut renderer, iced::Point::new(0.0, 0.0), &op);

        assert_eq!(renderer.text_calls.len(), 1);
        assert_eq!(
            renderer.text_calls[0].clip_bounds,
            iced::Rectangle {
                x: 8.0,
                y: 54.0,
                width: 90.0,
                height: 18.0,
            }
        );
    }

    #[test]
    fn iced_presentation_splits_keyed_paint_layers_for_local_draw_and_evidence() {
        let target = WidgetId::from("iced.layered-paint");
        let bounds = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 100.0,
                height: 40.0,
            },
        });
        let layout_input = LayoutInput {
            viewport: bounds,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: bounds.size,
            },
        };
        let text = |label: &str| {
            PaintOp::text(
                bounds.into_rect(),
                label,
                Color {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 1.0,
                },
            )
        };
        let view = ViewDefinition {
            target: target.clone(),
            frame: frame(311),
            layout: LayoutOutput {
                bounds,
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            },
            paint: vec![
                PaintOp::keyed_layer(
                    slipway_core::PaintLayerKey::ordered(20, 0),
                    vec![text("top-key")],
                ),
                text("default"),
                PaintOp::keyed_layer(
                    slipway_core::PaintLayerKey::ordered(10, 0),
                    vec![text("middle-key")],
                ),
            ],
            paint_order: slipway_core::PaintOrderDeclaration::layered_order(target.clone(), 15, 0),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let presentation = IcedPresentationState::from_view_definition(view, layout_input, target);

        assert_eq!(
            paint_text_labels(&presentation.paint),
            vec!["middle-key", "default", "top-key"]
        );
        assert_eq!(
            paint_text_labels(&presentation.local_paint),
            vec!["default"]
        );

        let mut renderer = RecordingRenderer::default();
        draw_iced_standalone_presentation(&mut renderer, iced::Point::new(0.0, 0.0), &presentation);

        let drawn = renderer
            .text_calls
            .iter()
            .map(|draw| draw.paragraph.content.as_str())
            .collect::<Vec<_>>();
        assert_eq!(drawn, vec!["default", "middle-key", "top-key"]);
    }

    #[test]
    fn iced_backend_widget_trait_captures_visible_runtime_requirements() {
        fn assert_backend_widget<W: SlipwayIcedBackendWidget>() {}
        fn assert_layout_intent_widget<W: SlipwayIcedLayoutIntentBackendWidget>() {}
        fn assert_child_widget<W: SlipwayIcedBackendChildWidget>() {}

        assert_backend_widget::<TestWidget>();
        assert_layout_intent_widget::<TestWidget>();
        assert_child_widget::<TestWidget>();
    }

    #[test]
    fn iced_backend_admission_accepts_supported_rectangular_view() {
        let widget = TestWidget;
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput {
            frame: frame(12),
            layout_input: <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
                &mut DefaultIcedBridge::new(),
                context(),
            ),
        };
        let view = widget.view_definition(&(), &local, input);

        let admission = iced_backend_admission().admit_view_definition(&view);

        assert!(admission.accepted);
        assert!(admission.unsupported.is_empty());
        assert_eq!(admission.source.label(), "backend_presented");
        assert_eq!(
            admission.source.backend_id.as_deref(),
            Some(ICED_BACKEND_ID)
        );
        assert!(
            admission
                .visible_requirements
                .iter()
                .any(|requirement| requirement.capability == BackendVisibleCapability::HitRegions)
        );
        assert!(admission.visible_requirements.iter().any(|requirement| {
            requirement.capability == BackendVisibleCapability::BackendPresentedEvidence
        }));
    }

    #[test]
    fn iced_backend_admission_refuses_blocking_view_contract_errors() {
        let widget = TestWidget;
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput {
            frame: frame(21),
            layout_input: <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
                &mut DefaultIcedBridge::new(),
                context(),
            ),
        };
        let mut view = widget.view_definition(&(), &local, input);
        view.hit_regions[0].route.path.clear();

        let admission = iced_backend_admission().admit_view_definition(&view);

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
    fn iced_backend_admission_refuses_text_input_without_text_edit_focus_region() {
        let widget = TestWidget;
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput {
            frame: frame(23),
            layout_input: <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
                &mut DefaultIcedBridge::new(),
                context(),
            ),
        };
        let mut view = widget.view_definition(&(), &local, input);
        view.focus_regions.clear();

        let admission = iced_backend_admission()
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
    fn iced_backend_admission_accepts_multiline_text_edit_with_native_editor() {
        let widget = TextEditUnsupportedWidget;
        let local = widget.initial_local_state();
        let iced_context = context();
        let input = ViewDefinitionInput {
            frame: frame(22),
            layout_input: LayoutInput {
                viewport: TargetLocalRect::new(iced_context.viewport),
                constraints: iced_context.constraints,
            },
        };
        let mut view = widget.view_definition(&(), &local, input);
        view.focus_regions[0]
            .text_edit
            .as_mut()
            .expect("fixture carries a text edit declaration")
            .line_mode = TextLineMode::MultiLine;

        let admission = iced_backend_admission().admit_view_definition(&view);

        assert!(admission.accepted);
        assert!(admission.unsupported.iter().all(|entry| {
            entry.requirement_id.as_deref() != Some("view.text_edit_regions.multiline")
        }));
        assert!(admission.visible_requirements.iter().any(|requirement| {
            requirement.capability == BackendVisibleCapability::TextEditRegions
        }));
    }

    #[test]
    fn iced_backend_admission_accepts_scroll_regions_with_real_child_placement() {
        let widget = TestWidget;
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput {
            frame: frame(23),
            layout_input: <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
                &mut DefaultIcedBridge::new(),
                context(),
            ),
        };
        let mut view = widget.view_definition(&(), &local, input);
        view.layout
            .child_placements
            .push(slipway_core::ChildPlacement {
                child: WidgetId::from("iced.test.scroll-child"),
                bounds: slipway_core::ParentLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 80.0,
                        height: 120.0,
                    },
                }),
                local_state_slot: Some(
                    WidgetSlotAddress::new(WidgetId::from("iced.test"), 0)
                        .child(WidgetId::from("iced.test.scroll-child"), 0),
                ),
            });
        view.scroll_regions.push(test_scroll_region_from_capability(
            widget.id(),
            "test-scroll",
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 30.0,
                },
            },
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 120.0,
                },
            },
            slipway_core::ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
        ));

        let admission = iced_backend_admission().admit_view_definition(&view);

        assert!(admission.accepted);
        assert!(
            admission.visible_requirements.iter().any(
                |requirement| requirement.capability == BackendVisibleCapability::ScrollRegions
            )
        );
        assert!(
            admission
                .unsupported
                .iter()
                .all(|entry| entry.visible_capability
                    != Some(BackendVisibleCapability::ScrollRegions))
        );
    }

    #[test]
    fn native_iced_scroll_regions_create_scrollable_and_skip_root_child_lifecycle() {
        let widget = TestWidget;
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput {
            frame: frame(24),
            layout_input: <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
                &mut DefaultIcedBridge::new(),
                context(),
            ),
        };
        let mut view = widget.view_definition(&(), &local, input.clone());
        let child_slot = WidgetSlotAddress::new(WidgetId::from("iced.test"), 0)
            .child(WidgetId::from("iced.test.scroll-child"), 0);
        view.layout
            .child_placements
            .push(slipway_core::ChildPlacement {
                child: WidgetId::from("iced.test.scroll-child"),
                bounds: slipway_core::ParentLocalRect::new(Rect {
                    origin: Point { x: 4.0, y: 24.0 },
                    size: Size {
                        width: 80.0,
                        height: 120.0,
                    },
                }),
                local_state_slot: Some(child_slot),
            });
        view.scroll_regions.push(test_scroll_region_from_capability(
            widget.id(),
            "test-scroll",
            Rect {
                origin: Point { x: 0.0, y: 20.0 },
                size: Size {
                    width: 90.0,
                    height: 40.0,
                },
            },
            Rect {
                origin: Point { x: 0.0, y: 20.0 },
                size: Size {
                    width: 90.0,
                    height: 160.0,
                },
            },
            slipway_core::ScrollConsumptionPolicy {
                wheel: true,
                drag: true,
                keyboard: false,
                programmatic: false,
            },
        ));
        let presentation =
            IcedPresentationState::from_view_definition(view, input.layout_input, widget.id());

        let native_scroll = native_iced_scroll_regions(&presentation);
        assert_eq!(native_scroll.len(), 1);
        assert!(root_child_placements_excluding_native_scroll(&presentation).is_empty());
        let adjusted = adjusted_scroll_child_placements(&presentation, &native_scroll[0]);
        assert_eq!(adjusted.len(), 1);
        assert_eq!(adjusted[0].bounds.origin.x, 4.0);
        assert_eq!(adjusted[0].bounds.origin.y, 4.0);

        let _scrollable: iced::widget::Scrollable<
            '_,
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        > = iced_scrollable_widget_for_region::<TestWidget, iced::Theme, RecordingRenderer>(
            &widget,
            &(),
            &local,
            &presentation,
            &native_scroll[0],
            None,
            None,
        );
    }

    fn scroll_ownership_test_input(frame_number: u64) -> ViewDefinitionInput {
        ViewDefinitionInput {
            frame: frame(frame_number),
            layout_input: <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
                &mut DefaultIcedBridge::new(),
                context(),
            ),
        }
    }

    fn scroll_ownership_test_region(
        target: WidgetId,
        address: WidgetSlotAddress,
    ) -> ScrollRegionDeclaration {
        let mut region = test_scroll_region_from_capability(
            target,
            "test-scroll-ownership",
            Rect {
                origin: Point { x: 0.0, y: 20.0 },
                size: Size {
                    width: 90.0,
                    height: 40.0,
                },
            },
            Rect {
                origin: Point { x: 0.0, y: 20.0 },
                size: Size {
                    width: 90.0,
                    height: 160.0,
                },
            },
            slipway_core::ScrollConsumptionPolicy {
                wheel: true,
                drag: true,
                keyboard: false,
                programmatic: false,
            },
        );
        region.address = Some(address);
        region
    }

    fn scroll_ownership_test_presentation(
        frame_number: u64,
        placements: Vec<slipway_core::ChildPlacement>,
        scroll: ScrollRegionDeclaration,
    ) -> IcedPresentationState {
        let widget = TestWidget;
        let local = widget.initial_local_state();
        let input = scroll_ownership_test_input(frame_number);
        let mut view = widget.view_definition(&(), &local, input.clone());
        view.layout.child_placements = placements;
        view.scroll_regions = vec![scroll];
        IcedPresentationState::from_view_definition(view, input.layout_input, widget.id())
    }

    fn scroll_ownership_slots() -> (
        WidgetId,
        WidgetSlotAddress,
        WidgetId,
        WidgetSlotAddress,
        WidgetId,
        WidgetSlotAddress,
    ) {
        let root = WidgetId::from("iced.test");
        let scroll_target = WidgetId::from("iced.test.scroll-list");
        let overlay = WidgetId::from("iced.test.movable-overlay");
        let owned_child = WidgetId::from("iced.test.scroll-child");
        let root_slot = WidgetSlotAddress::new(root, 0);
        let scroll_slot = root_slot.child(scroll_target.clone(), 0);
        let overlay_slot = root_slot.child(overlay.clone(), 1);
        let owned_slot = scroll_slot.child(owned_child.clone(), 0);
        (
            scroll_target,
            scroll_slot,
            overlay,
            overlay_slot,
            owned_child,
            owned_slot,
        )
    }

    fn overlapping_sibling_scroll_fixture() -> (
        slipway_core::ChildPlacement,
        ScrollRegionDeclaration,
        WidgetId,
    ) {
        let (scroll_target, scroll_slot, overlay, overlay_slot, _, _) = scroll_ownership_slots();
        let sibling = slipway_core::ChildPlacement {
            child: overlay.clone(),
            bounds: slipway_core::ParentLocalRect::new(Rect {
                origin: Point { x: 4.0, y: 24.0 },
                size: Size {
                    width: 80.0,
                    height: 32.0,
                },
            }),
            local_state_slot: Some(overlay_slot),
        };
        (
            sibling,
            scroll_ownership_test_region(scroll_target, scroll_slot),
            overlay,
        )
    }

    #[test]
    fn scroll_region_does_not_own_geometrically_overlapping_sibling_child() {
        let (sibling, scroll, _) = overlapping_sibling_scroll_fixture();
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 200.0,
                },
            }),
            child_placements: vec![sibling.clone()],
            diagnostics: Vec::new(),
        };
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);

        assert!(
            rects_intersect(
                scroll.content_bounds.into_rect(),
                sibling.bounds.into_rect()
            ),
            "fixture must overlap geometrically to catch the old predicate"
        );
        assert!(!scroll_contains_placement_for_layout(
            &geometry_index,
            &scroll,
            &sibling
        ));
    }

    #[test]
    fn root_child_placements_keep_overlapping_sibling_outside_native_scroll() {
        let (sibling, scroll, overlay) = overlapping_sibling_scroll_fixture();
        let presentation = scroll_ownership_test_presentation(25, vec![sibling], scroll);

        let root_placements = root_child_placements_excluding_native_scroll(&presentation);

        assert_eq!(root_placements.len(), 1);
        assert_eq!(root_placements[0].child, overlay);
    }

    #[test]
    fn native_iced_scroll_regions_omit_region_with_only_overlapping_sibling() {
        let (sibling, scroll, _) = overlapping_sibling_scroll_fixture();
        let presentation = scroll_ownership_test_presentation(26, vec![sibling], scroll);

        assert!(native_iced_scroll_regions(&presentation).is_empty());
    }

    #[test]
    fn native_iced_scroll_region_still_owns_descendant_child_placement() {
        let (scroll_target, scroll_slot, overlay, overlay_slot, owned_child, owned_slot) =
            scroll_ownership_slots();
        let sibling = slipway_core::ChildPlacement {
            child: overlay.clone(),
            bounds: slipway_core::ParentLocalRect::new(Rect {
                origin: Point { x: 4.0, y: 24.0 },
                size: Size {
                    width: 80.0,
                    height: 32.0,
                },
            }),
            local_state_slot: Some(overlay_slot),
        };
        let owned = slipway_core::ChildPlacement {
            child: owned_child.clone(),
            bounds: slipway_core::ParentLocalRect::new(Rect {
                origin: Point { x: 8.0, y: 36.0 },
                size: Size {
                    width: 70.0,
                    height: 40.0,
                },
            }),
            local_state_slot: Some(owned_slot),
        };
        let scroll = scroll_ownership_test_region(scroll_target, scroll_slot);
        let presentation = scroll_ownership_test_presentation(27, vec![sibling, owned], scroll);

        let native_scroll = native_iced_scroll_regions(&presentation);
        assert_eq!(native_scroll.len(), 1);
        let adjusted = adjusted_scroll_child_placements(&presentation, &native_scroll[0]);
        assert_eq!(adjusted.len(), 1);
        assert_eq!(adjusted[0].child, owned_child);
        assert_eq!(adjusted[0].bounds.origin.x, 8.0);
        assert_eq!(adjusted[0].bounds.origin.y, 16.0);

        let root_placements = root_child_placements_excluding_native_scroll(&presentation);
        assert_eq!(root_placements.len(), 1);
        assert_eq!(root_placements[0].child, overlay);
    }

    #[test]
    fn iced_backend_admission_refuses_scroll_region_without_real_child_placement() {
        let widget = TestWidget;
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput {
            frame: frame(23),
            layout_input: <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
                &mut DefaultIcedBridge::new(),
                context(),
            ),
        };
        let mut view = widget.view_definition(&(), &local, input);
        view.layout.child_placements.clear();
        view.scroll_regions.push(test_scroll_region_from_capability(
            widget.id(),
            "test-scroll",
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 30.0,
                },
            },
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 120.0,
                },
            },
            slipway_core::ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
        ));

        let admission = iced_backend_admission().admit_view_definition(&view);

        assert!(!admission.accepted);
        let unsupported = admission
            .unsupported
            .iter()
            .find(|entry| {
                entry
                    .requirement_id
                    .as_deref()
                    .is_some_and(|id| id == "view.scroll_regions.test-scroll.real_child_ui")
            })
            .expect(
                "scroll region without hosted child UI must produce precise unsupported evidence",
            );
        assert_eq!(
            unsupported.visible_capability,
            Some(BackendVisibleCapability::ScrollRegions)
        );
        assert_eq!(unsupported.capability, Capability::ScrollRegionPresentation);
    }

    #[test]
    fn iced_backend_admission_accepts_visible_path_shape() {
        let widget = TestWidget;
        let local = widget.initial_local_state();
        let input = ViewDefinitionInput {
            frame: frame(13),
            layout_input: <DefaultIcedBridge as IcedSlipwayBridge<TestWidget>>::layout_input(
                &mut DefaultIcedBridge::new(),
                context(),
            ),
        };
        let mut view = widget.view_definition(&(), &local, input);
        view.paint = vec![PaintOp::Stroke {
            shape: ShapeDeclaration {
                id: Some("visible-path".to_string()),
                kind: ShapeKind::Path,
                bounds: view.layout.bounds.into_rect(),
                path: Some(slipway_core::PathDeclaration {
                    commands: vec![
                        slipway_core::PathCommand::MoveTo(Point { x: 0.0, y: 0.0 }),
                        slipway_core::PathCommand::LineTo(Point { x: 10.0, y: 10.0 }),
                    ],
                }),
                clip: None,
            },
            color: Color {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 1.0,
            },
            width: 1.0,
        }];

        let admission = iced_backend_admission().admit_view_definition(&view);

        assert!(
            admission.accepted,
            "iced geometry renderer must admit visible Path shapes"
        );
        assert!(
            admission.unsupported.iter().all(|entry| {
                entry.visible_capability != Some(BackendVisibleCapability::ShapePathClip)
            }),
            "Path shape must not poison the whole visible paint set"
        );
    }

    #[test]
    fn iced_backend_adapter_profile_exposes_unsupported_visible_contracts() {
        let admission = iced_backend_admission()
            .backend_parity_admission(&[CapabilityProfileKind::BackendAdapter]);

        assert!(!admission.accepted);
        assert_eq!(admission.source.label(), "backend_presented");
        assert!(admission.unsupported.iter().any(|entry| {
            entry.visible_capability == Some(BackendVisibleCapability::ShapePathClip)
        }));
        assert!(admission.unsupported.iter().any(|entry| {
            entry.visible_capability == Some(BackendVisibleCapability::FontInstallation)
        }));
        let overlay = admission
            .unsupported
            .iter()
            .find(|entry| {
                entry.visible_capability == Some(iced_overlay_forwarding_visible_capability())
            })
            .expect("official iced overlay forwarding must block adapter profile acceptance");
        assert_eq!(overlay.capability, Capability::Overlay);
        assert_eq!(overlay.source.label(), "backend_presented");
        assert_precise_overlay_missing_contract_reason(&overlay.reason);
    }

    #[test]
    fn iced_popup_profile_refuses_overlay_without_core_authoring_contract() {
        let admission =
            iced_backend_admission().backend_parity_admission(&[CapabilityProfileKind::Popup]);

        assert!(!admission.accepted);
        assert_eq!(
            admission.required_profiles,
            vec![CapabilityProfileKind::Popup]
        );
        let overlay = admission
            .unsupported
            .iter()
            .find(|entry| {
                entry.capability == Capability::Overlay
                    && entry.visible_capability
                        == Some(iced_overlay_forwarding_visible_capability())
            })
            .expect("popup profile must be blocked by missing iced overlay forwarding contract");
        assert_eq!(
            overlay.requirement_id.as_deref(),
            Some("profile::Popup::Custom(\"iced.lifecycle.overlay.forwarding\")")
        );
        assert_eq!(overlay.source.label(), "backend_presented");
        assert_precise_overlay_missing_contract_reason(&overlay.reason);
    }

    #[test]
    fn iced_runtime_visible_widget_draws_path_paint_before_draw() {
        let runtime = SlipwayRuntime::new(UnsupportedPaintWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::state(&widget),
            children: Vec::new(),
        };
        let mut renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let presentation = tree
            .state
            .downcast_ref::<IcedRuntimeTreeState>()
            .presentation
            .as_ref()
            .expect("visible presentation is stored after layout");
        assert!(presentation.admission.accepted);
        assert!(presentation.admission.unsupported.iter().all(|entry| {
            entry.visible_capability != Some(BackendVisibleCapability::ShapePathClip)
        }));

        let layout = iced::advanced::Layout::new(&node);
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let style = iced::advanced::renderer::Style {
            text_color: iced::Color::BLACK,
        };
        <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::draw(
            &widget,
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            layout,
            iced::advanced::mouse::Cursor::Unavailable,
            &viewport,
        );

        assert_eq!(renderer.geometries, 1);
        assert_eq!(renderer.quads, 0);
        assert!(renderer.paragraphs.is_empty());
        assert!(renderer.text_calls.is_empty());
    }

    #[test]
    fn iced_runtime_visible_widget_draws_supported_paint_with_native_text_edit() {
        let runtime = SlipwayRuntime::new(TextEditUnsupportedWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::state(&widget),
            children: Vec::new(),
        };
        let mut renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let presentation = tree
            .state
            .downcast_ref::<IcedRuntimeTreeState>()
            .presentation
            .as_ref()
            .expect("visible presentation is stored after layout");
        assert_eq!(
            tree.state
                .downcast_ref::<IcedRuntimeTreeState>()
                .text_edit_trees
                .len(),
            1
        );
        assert!(presentation.admission.accepted);
        assert!(presentation.admission.unsupported.iter().all(|entry| {
            entry.visible_capability != Some(BackendVisibleCapability::TextEditRegions)
        }));
        assert!(presentation.can_draw_visible_paint());

        let layout = iced::advanced::Layout::new(&node);
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let style = iced::advanced::renderer::Style {
            text_color: iced::Color::BLACK,
        };
        <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::draw(
            &widget,
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            layout,
            iced::advanced::mouse::Cursor::Unavailable,
            &viewport,
        );

        assert!(renderer.quads >= 2);
    }

    #[test]
    fn iced_runtime_native_text_input_focus_requests_input_method_on_redraw() {
        let runtime = SlipwayRuntime::new(TextEditUnsupportedWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::state(&widget),
            children: Vec::new(),
        };
        let renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::layout(&mut widget, &mut tree, &renderer, &limits);
        let layout = iced::advanced::Layout::new(&node);
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let mut clipboard = iced::advanced::clipboard::Null;
        let mut messages = Vec::new();

        {
            let event =
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
            let mut shell = iced::advanced::Shell::new(&mut messages);
            <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::update(
                &mut widget,
                &mut tree,
                &event,
                layout,
                iced::advanced::mouse::Cursor::Available(iced::Point::new(10.0, 10.0)),
                &renderer,
                &mut clipboard,
                &mut shell,
                &viewport,
            );
        }

        let mut redraw_messages = Vec::new();
        let mut redraw_shell = iced::advanced::Shell::new(&mut redraw_messages);
        <SlipwayIcedRuntimeWidget<'_, TextEditUnsupportedWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::update(
            &mut widget,
            &mut tree,
            &iced::Event::Window(iced::window::Event::RedrawRequested(
                std::time::Instant::now(),
            )),
            layout,
            iced::advanced::mouse::Cursor::Available(iced::Point::new(10.0, 10.0)),
            &renderer,
            &mut clipboard,
            &mut redraw_shell,
            &viewport,
        );

        assert!(
            matches!(
                redraw_shell.input_method(),
                iced_core::InputMethod::Enabled { .. }
            ),
            "focused Slipway text edit region must keep the native iced TextInput focused so Windows IME can emit preedit/commit events"
        );
    }

    #[test]
    fn iced_runtime_child_native_text_input_focus_requests_input_method_on_redraw() {
        let runtime = SlipwayRuntime::new(ParentWithTextChildWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, ParentWithTextChildWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, ParentWithTextChildWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, ParentWithTextChildWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::children(&widget),
        };
        let renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, ParentWithTextChildWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::layout(&mut widget, &mut tree, &renderer, &limits);
        let layout = iced::advanced::Layout::new(&node);
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let mut clipboard = iced::advanced::clipboard::Null;
        let mut messages = Vec::new();

        {
            let event =
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
            let mut shell = iced::advanced::Shell::new(&mut messages);
            <SlipwayIcedRuntimeWidget<'_, ParentWithTextChildWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::update(
                &mut widget,
                &mut tree,
                &event,
                layout,
                iced::advanced::mouse::Cursor::Available(iced::Point::new(25.0, 17.0)),
                &renderer,
                &mut clipboard,
                &mut shell,
                &viewport,
            );
        }

        let mut redraw_messages = Vec::new();
        let mut redraw_shell = iced::advanced::Shell::new(&mut redraw_messages);
        <SlipwayIcedRuntimeWidget<'_, ParentWithTextChildWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::update(
            &mut widget,
            &mut tree,
            &iced::Event::Window(iced::window::Event::RedrawRequested(
                std::time::Instant::now(),
            )),
            layout,
            iced::advanced::mouse::Cursor::Available(iced::Point::new(25.0, 17.0)),
            &renderer,
            &mut clipboard,
            &mut redraw_shell,
            &viewport,
        );

        assert!(
            matches!(
                redraw_shell.input_method(),
                iced_core::InputMethod::Enabled { .. }
            ),
            "child text edit regions must mount a real native iced TextInput that can request IME"
        );
    }

    #[test]
    fn iced_runtime_user_interface_cache_preserves_child_text_input_focus_for_ime() {
        let runtime = SlipwayRuntime::new(ParentWithTextChildWidget, ());
        let mut renderer = RecordingRenderer::default();
        let mut cache = iced_winit::runtime::user_interface::Cache::new();
        let bounds = iced::Size::new(100.0, 40.0);
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let cursor = iced::advanced::mouse::Cursor::Available(iced::Point::new(25.0, 17.0));
        let mut clipboard = iced::advanced::clipboard::Null;

        {
            let mut messages = Vec::new();
            let mut ui = iced_winit::runtime::user_interface::UserInterface::<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >::build(
                SlipwayIcedRuntimeWidget::from_runtime(&runtime),
                bounds,
                cache,
                &mut renderer,
            );
            let _ = ui.update(
                &[iced::Event::Mouse(iced::mouse::Event::ButtonPressed(
                    iced::mouse::Button::Left,
                ))],
                cursor,
                &mut renderer,
                &mut clipboard,
                &mut messages,
            );
            cache = ui.into_cache();
        }

        let mut messages = Vec::new();
        let mut ui = iced_winit::runtime::user_interface::UserInterface::<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >::build(
            SlipwayIcedRuntimeWidget::from_runtime(&runtime),
            bounds,
            cache,
            &mut renderer,
        );
        let (state, _) = ui.update(
            &[iced::Event::Window(iced::window::Event::RedrawRequested(
                std::time::Instant::now(),
            ))],
            cursor,
            &mut renderer,
            &mut clipboard,
            &mut messages,
        );
        let iced_winit::runtime::user_interface::State::Updated { input_method, .. } = state else {
            panic!("user interface should remain updated after redraw");
        };

        assert!(
            matches!(input_method, iced_core::InputMethod::Enabled { .. }),
            "runner-style UserInterface cache rebuild must preserve child native text input focus"
        );
        let _ = viewport;
    }

    #[test]
    fn native_layout_intent_widget_uses_explicit_sink() {
        let widget = TestWidget;
        let external = ();
        let local = widget.initial_local_state();
        let iced_context = context();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(iced_context.viewport),
            constraints: iced_context.constraints,
        };
        let mut sink = Vec::new();

        {
            let mut native = SlipwayIcedLayoutIntentWidget::new(&widget, &external)
                .observe_layout_intent_into(&mut sink);
            native.push_layout_intent(&local, &input);
        }

        assert_eq!(sink.len(), 1);
        assert_eq!(sink[0].target, WidgetId::from("iced.test"));
        assert!(sink[0].size_policy.is_some());
    }

    #[test]
    fn runtime_widget_creates_authored_child_runtime_tree_states() {
        let runtime = SlipwayRuntime::new(ChildTreeWidget, ());
        let widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&widget),
        };

        assert_eq!(tree.children.len(), 2);
        let first = tree.children[0]
            .state
            .downcast_ref::<IcedRuntimeTreeState>();
        let second = tree.children[1]
            .state
            .downcast_ref::<IcedRuntimeTreeState>();
        assert_eq!(first.id, WidgetId::from("iced.child"));
        assert_eq!(second.id, WidgetId::from("iced.child"));

        <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::diff(&widget, &mut tree);

        assert_eq!(tree.children.len(), 2);
        let first_after = tree.children[0]
            .state
            .downcast_ref::<IcedRuntimeTreeState>();
        let second_after = tree.children[1]
            .state
            .downcast_ref::<IcedRuntimeTreeState>();
        assert_eq!(first_after.id, WidgetId::from("iced.child"));
        assert_eq!(second_after.id, WidgetId::from("iced.child"));
    }

    #[test]
    fn runtime_widget_layout_exposes_child_nodes_from_child_placements() {
        let runtime = SlipwayRuntime::new(ChildTreeWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&widget),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let node = <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        assert_eq!(node.children().len(), 2);
        assert_eq!(node.children()[0].bounds().x, 4.0);
        assert_eq!(node.children()[0].bounds().y, 6.0);
        assert_eq!(node.children()[0].bounds().width, 20.0);
        assert_eq!(node.children()[1].bounds().x, 30.0);
        assert_eq!(node.children()[1].bounds().height, 12.0);
    }

    #[test]
    fn runtime_widget_layout_keeps_unplaced_authored_child_slot() {
        let runtime = SlipwayRuntime::new(PartiallyPlacedChildTreeWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, PartiallyPlacedChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, PartiallyPlacedChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, PartiallyPlacedChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&widget),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let node = <SlipwayIcedRuntimeWidget<'_, PartiallyPlacedChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        assert_eq!(tree.children.len(), 2);
        assert_eq!(node.children().len(), 2);
        assert_eq!(node.children()[0].bounds().width, 0.0);
        assert_eq!(node.children()[0].bounds().height, 0.0);
        assert_eq!(node.children()[1].bounds().x, 30.0);
        assert_eq!(node.children()[1].bounds().height, 12.0);
        assert!(
            tree.children[0]
                .state
                .downcast_ref::<IcedRuntimeTreeState>()
                .presentation
                .is_none()
        );
        assert!(
            tree.children[1]
                .state
                .downcast_ref::<IcedRuntimeTreeState>()
                .presentation
                .is_some()
        );
    }

    #[test]
    fn runtime_widget_draw_delegates_to_authored_child_widgets() {
        let runtime = SlipwayRuntime::new(ChildTreeWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::children(&widget),
        };
        let mut renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let node = <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        assert!(
            tree.children[0]
                .state
                .downcast_ref::<IcedRuntimeTreeState>()
                .presentation
                .is_some()
        );
        assert!(
            tree.children[1]
                .state
                .downcast_ref::<IcedRuntimeTreeState>()
                .presentation
                .is_some()
        );

        let style = iced::advanced::renderer::Style {
            text_color: iced::Color::BLACK,
        };
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };

        <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::draw(
            &widget,
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            iced::advanced::Layout::new(&node),
            iced::advanced::mouse::Cursor::Unavailable,
            &viewport,
        );

        assert_eq!(renderer.quads, 2);
    }

    #[test]
    fn runtime_widget_draws_parent_path_paint_and_still_delegates_children() {
        let runtime = SlipwayRuntime::new(UnsupportedPaintParentWithChildWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag:
                <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintParentWithChildWidget> as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    RecordingRenderer,
                >>::tag(&widget),
            state:
                <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintParentWithChildWidget> as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    RecordingRenderer,
                >>::state(&widget),
            children:
                <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintParentWithChildWidget> as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    RecordingRenderer,
                >>::children(&widget),
        };
        let mut renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let node =
            <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintParentWithChildWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let parent_presentation = tree
            .state
            .downcast_ref::<IcedRuntimeTreeState>()
            .presentation
            .as_ref()
            .expect("parent presentation should be stored");
        assert!(parent_presentation.can_draw_visible_paint());
        let child_presentation = tree.children[0]
            .state
            .downcast_ref::<IcedRuntimeTreeState>()
            .presentation
            .as_ref()
            .expect("child presentation should be stored");
        assert!(child_presentation.can_draw_visible_paint());

        let style = iced::advanced::renderer::Style {
            text_color: iced::Color::BLACK,
        };
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };

        <SlipwayIcedRuntimeWidget<'_, UnsupportedPaintParentWithChildWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::draw(
            &widget,
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            iced::advanced::Layout::new(&node),
            iced::advanced::mouse::Cursor::Unavailable,
            &viewport,
        );

        assert_eq!(renderer.geometries, 1);
        assert_eq!(renderer.quads, 1);
    }

    #[test]
    fn runtime_widget_lifecycle_methods_reuse_layout_presentation_and_preserve_child_tree_state() {
        let runtime = SlipwayRuntime::new(LifecycleProbeWidget, LifecycleCounterState::default());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, LifecycleProbeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, LifecycleProbeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::state(&widget),
            children:
                <SlipwayIcedRuntimeWidget<'_, LifecycleProbeWidget> as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    RecordingRenderer,
                >>::children(&widget),
        };
        let mut renderer = RecordingRenderer::default();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, LifecycleProbeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let after_layout = lifecycle_counts(runtime.external());
        assert_eq!(after_layout, (1, 1, 1, 1, 1, 1));
        let child_state_after_layout = tree.children[0]
            .state
            .downcast_ref::<IcedRuntimeTreeState>();
        let child_id_after_layout = child_state_after_layout.id.clone();
        let child_presentation_after_layout = child_state_after_layout.presentation.clone();
        assert_eq!(child_state_after_layout.layout_pass, 1);
        assert!(child_state_after_layout.presentation.is_some());

        let mut operation = RecordingOperation::default();
        <SlipwayIcedRuntimeWidget<'_, LifecycleProbeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::operate(
            &mut widget,
            &mut tree,
            iced::advanced::Layout::new(&node),
            &renderer,
            &mut operation,
        );
        assert_eq!(lifecycle_counts(runtime.external()), after_layout);

        let style = iced::advanced::renderer::Style {
            text_color: iced::Color::BLACK,
        };
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        <SlipwayIcedRuntimeWidget<'_, LifecycleProbeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::draw(
            &widget,
            &tree,
            &mut renderer,
            &iced::Theme::Light,
            &style,
            iced::advanced::Layout::new(&node),
            iced::advanced::mouse::Cursor::Unavailable,
            &viewport,
        );
        assert_eq!(lifecycle_counts(runtime.external()), after_layout);

        let interaction = <SlipwayIcedRuntimeWidget<'_, LifecycleProbeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            RecordingRenderer,
        >>::mouse_interaction(
            &widget,
            &tree,
            iced::advanced::Layout::new(&node),
            iced::advanced::mouse::Cursor::Available(iced::Point::new(10.0, 11.0)),
            &viewport,
            &renderer,
        );
        assert_eq!(interaction, iced::advanced::mouse::Interaction::Pointer);
        assert_eq!(lifecycle_counts(runtime.external()), after_layout);

        let event =
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
        let mut clipboard = iced::advanced::clipboard::Null;
        let mut messages = Vec::new();
        {
            let mut shell = iced::advanced::Shell::new(&mut messages);
            <SlipwayIcedRuntimeWidget<'_, LifecycleProbeWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                RecordingRenderer,
            >>::update(
                &mut widget,
                &mut tree,
                &event,
                iced::advanced::Layout::new(&node),
                iced::advanced::mouse::Cursor::Available(iced::Point::new(10.0, 11.0)),
                &renderer,
                &mut clipboard,
                &mut shell,
                &viewport,
            );
            assert!(!shell.is_event_captured());
            assert_eq!(
                shell.redraw_request(),
                iced::window::RedrawRequest::NextFrame
            );
        }

        assert_eq!(messages.len(), 1);
        assert_eq!(lifecycle_counts(runtime.external()), after_layout);
        let child_state_after_update = tree.children[0]
            .state
            .downcast_ref::<IcedRuntimeTreeState>();
        assert_eq!(child_state_after_update.id, child_id_after_layout);
        assert_eq!(
            child_state_after_update.presentation,
            child_presentation_after_layout
        );
        assert_eq!(child_state_after_update.layout_pass, 1);
        assert_eq!(
            child_state_after_update.hovered_region,
            Some(slipway_core::PresentationRegionId::from(
                "iced.lifecycle.child.hit"
            ))
        );
        assert_eq!(
            child_state_after_update.pressed_region,
            Some(pressed_capture_with_origin(
                "iced.lifecycle.child.hit",
                8.0,
                9.0,
            ))
        );
    }

    #[test]
    fn runtime_widget_overlay_is_explicitly_refused_by_profile_gate() {
        let runtime = SlipwayRuntime::new(ChildTreeWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&widget),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };

        let overlay = <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::overlay(
            &mut widget,
            &mut tree,
            iced::advanced::Layout::new(&node),
            &renderer,
            &viewport,
            iced::Vector::new(0.0, 0.0),
        );
        assert!(overlay.is_none());

        let admission = iced_backend_admission()
            .backend_parity_admission(&[CapabilityProfileKind::BackendAdapter]);
        let overlay_refusal = admission
            .unsupported
            .iter()
            .find(|entry| {
                entry.capability == Capability::Overlay
                    && entry.visible_capability
                        == Some(iced_overlay_forwarding_visible_capability())
            })
            .expect("backend adapter profile must be blocked by missing iced overlay contract");
        assert_precise_overlay_missing_contract_reason(&overlay_refusal.reason);
    }

    #[test]
    fn runtime_widget_operate_and_mouse_delegate_to_authored_children() {
        let runtime = SlipwayRuntime::new(ChildTreeWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&widget),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);
        let layout = iced::advanced::Layout::new(&node);

        let mut operation = RecordingOperation::default();
        <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::operate(&mut widget, &mut tree, layout, &renderer, &mut operation);

        assert_eq!(operation.containers.len(), 3);
        assert_eq!(
            operation.containers[0].0,
            Some(iced::advanced::widget::Id::from("iced.parent".to_string()))
        );
        assert_eq!(
            operation.containers[1].0,
            Some(iced::advanced::widget::Id::from("iced.child".to_string()))
        );
        assert_eq!(
            operation.containers[2].0,
            Some(iced::advanced::widget::Id::from("iced.child".to_string()))
        );

        let interaction = <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::mouse_interaction(
            &widget,
            &tree,
            layout,
            iced::advanced::mouse::Cursor::Available(iced::Point::new(5.0, 7.0)),
            &iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            },
            &renderer,
        );

        assert_eq!(interaction, iced::advanced::mouse::Interaction::Pointer);
    }

    #[test]
    fn runtime_widget_update_delegates_to_authored_child_and_preserves_slot_identity() {
        let runtime = SlipwayRuntime::new(ChildTreeWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&widget),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let event =
            iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
        let cursor = iced::advanced::mouse::Cursor::Available(iced::Point::new(31.0, 9.0));
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let mut clipboard = iced::advanced::clipboard::Null;
        let mut messages = Vec::new();

        {
            let mut shell = iced::advanced::Shell::new(&mut messages);
            <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::update(
                &mut widget,
                &mut tree,
                &event,
                iced::advanced::Layout::new(&node),
                cursor,
                &renderer,
                &mut clipboard,
                &mut shell,
                &viewport,
            );
        }

        assert_eq!(messages.len(), 1);
        let SlipwayIcedRuntimeMessage::BackendInput(input) = messages.remove(0) else {
            panic!("expected child pointer input message");
        };
        assert_eq!(
            input
                .dispatch_evidence
                .as_ref()
                .map(|evidence| evidence.source.clone()),
            Some(EvidenceSource::backend_presented(
                ICED_BACKEND_ID,
                "physical-input"
            ))
        );
        let InputEvent::Pointer(pointer) = input.event else {
            panic!("expected child pointer input event");
        };
        assert_eq!(pointer.target, WidgetId::from("iced.child"));
        assert_eq!(
            pointer.target_slot,
            Some(
                WidgetSlotAddress::new(WidgetId::from("iced.parent"), 0)
                    .child(WidgetId::from("iced.child"), 1)
            )
        );
        assert_eq!(pointer.position, Point { x: 1.0, y: 1.0 });
        assert_eq!(
            pointer.target_bounds,
            Some(TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 18.0,
                    height: 12.0,
                },
            }))
        );
    }

    #[test]
    fn runtime_widget_cursor_move_without_capture_does_not_publish_input() {
        let runtime = SlipwayRuntime::new(ChildTreeWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&widget),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );
        let node = <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let event = iced::Event::Mouse(iced::mouse::Event::CursorMoved {
            position: iced::Point::new(31.0, 9.0),
        });
        let cursor = iced::advanced::mouse::Cursor::Available(iced::Point::new(31.0, 9.0));
        let viewport = iced::Rectangle {
            x: 0.0,
            y: 0.0,
            width: 100.0,
            height: 40.0,
        };
        let mut clipboard = iced::advanced::clipboard::Null;
        let mut messages = Vec::new();

        {
            let mut shell = iced::advanced::Shell::new(&mut messages);
            <SlipwayIcedRuntimeWidget<'_, ChildTreeWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::update(
                &mut widget,
                &mut tree,
                &event,
                iced::advanced::Layout::new(&node),
                cursor,
                &renderer,
                &mut clipboard,
                &mut shell,
                &viewport,
            );
            assert_eq!(
                shell.redraw_request(),
                iced::window::RedrawRequest::NextFrame
            );
        }

        assert!(messages.is_empty());
    }

    #[test]
    fn runtime_widgets_emit_root_operation_container() {
        let runtime = SlipwayRuntime::new(TestWidget, ());
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let mut runtime_widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut runtime_tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&runtime_widget),
            state: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&runtime_widget),
            children: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::children(&runtime_widget),
        };
        let runtime_node =
            <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::layout(&mut runtime_widget, &mut runtime_tree, &renderer, &limits);
        let mut runtime_operation = RecordingOperation::default();
        <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::operate(
            &mut runtime_widget,
            &mut runtime_tree,
            iced::advanced::Layout::new(&runtime_node),
            &renderer,
            &mut runtime_operation,
        );
        assert_root_operation(&runtime_operation);

        let mut intent_widget = SlipwayIcedRuntimeLayoutIntentWidget::from_runtime(&runtime);
        let mut intent_tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeLayoutIntentWidget<'_, TestWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&intent_widget),
            state:
                <SlipwayIcedRuntimeLayoutIntentWidget<'_, TestWidget> as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    (),
                >>::state(&intent_widget),
            children:
                <SlipwayIcedRuntimeLayoutIntentWidget<'_, TestWidget> as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    (),
                >>::children(&intent_widget),
        };
        let intent_node =
            <SlipwayIcedRuntimeLayoutIntentWidget<'_, TestWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::layout(&mut intent_widget, &mut intent_tree, &renderer, &limits);
        let mut intent_operation = RecordingOperation::default();
        <SlipwayIcedRuntimeLayoutIntentWidget<'_, TestWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::operate(
            &mut intent_widget,
            &mut intent_tree,
            iced::advanced::Layout::new(&intent_node),
            &renderer,
            &mut intent_operation,
        );
        assert_root_operation(&intent_operation);
    }

    #[test]
    fn standalone_widgets_refuse_child_bearing_public_path() {
        let widget_model = ChildTreeWidget;
        let external = ();

        assert_standalone_child_lifecycle_refusal(std::panic::catch_unwind(|| {
            SlipwayIcedWidget::new(&widget_model, &external);
        }));
        assert_standalone_child_lifecycle_refusal(std::panic::catch_unwind(|| {
            SlipwayIcedLayoutIntentWidget::new(&widget_model, &external);
        }));
    }

    #[test]
    fn standalone_widget_refuses_child_placements_without_child_lifecycle() {
        let widget_model = LeafTopologyChildPlacementWidget;
        let external = ();
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let mut widget = SlipwayIcedWidget::new(&widget_model, &external);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedWidget<'_, LeafTopologyChildPlacementWidget> as iced::advanced::Widget<
                Message,
                (),
                (),
            >>::tag(&widget),
            state:
                <SlipwayIcedWidget<'_, LeafTopologyChildPlacementWidget> as iced::advanced::Widget<
                    Message,
                    (),
                    (),
                >>::state(&widget),
            children:
                <SlipwayIcedWidget<'_, LeafTopologyChildPlacementWidget> as iced::advanced::Widget<
                    Message,
                    (),
                    (),
                >>::children(&widget),
        };

        assert!(tree.children.is_empty());
        assert_standalone_child_lifecycle_refusal(std::panic::catch_unwind(
            std::panic::AssertUnwindSafe(|| {
                <SlipwayIcedWidget<'_, LeafTopologyChildPlacementWidget> as Widget<
                    Message,
                    (),
                    (),
                >>::layout(&mut widget, &mut tree, &renderer, &limits);
            }),
        ));
    }

    #[test]
    fn standalone_widgets_emit_root_operation_container() {
        let widget_model = TestWidget;
        let external = ();
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let mut widget = SlipwayIcedWidget::new(&widget_model, &external);
        let mut tree =
            iced::advanced::widget::Tree {
                tag: <SlipwayIcedWidget<'_, TestWidget> as iced::advanced::Widget<
                    Message,
                    (),
                    (),
                >>::tag(&widget),
                state: <SlipwayIcedWidget<'_, TestWidget> as iced::advanced::Widget<
                    Message,
                    (),
                    (),
                >>::state(&widget),
                children: <SlipwayIcedWidget<'_, TestWidget> as iced::advanced::Widget<
                    Message,
                    (),
                    (),
                >>::children(&widget),
            };
        assert!(tree.children.is_empty());
        let node = <SlipwayIcedWidget<'_, TestWidget> as Widget<Message, (), ()>>::layout(
            &mut widget,
            &mut tree,
            &renderer,
            &limits,
        );
        assert!(node.children().is_empty());
        let mut operation = RecordingOperation::default();
        <SlipwayIcedWidget<'_, TestWidget> as Widget<Message, (), ()>>::operate(
            &mut widget,
            &mut tree,
            iced::advanced::Layout::new(&node),
            &renderer,
            &mut operation,
        );
        assert_root_operation(&operation);

        let mut intent_widget = SlipwayIcedLayoutIntentWidget::new(&widget_model, &external);
        let mut intent_tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedLayoutIntentWidget<'_, TestWidget> as iced::advanced::Widget<
                Message,
                (),
                (),
            >>::tag(&intent_widget),
            state: <SlipwayIcedLayoutIntentWidget<'_, TestWidget> as iced::advanced::Widget<
                Message,
                (),
                (),
            >>::state(&intent_widget),
            children: <SlipwayIcedLayoutIntentWidget<'_, TestWidget> as iced::advanced::Widget<
                Message,
                (),
                (),
            >>::children(&intent_widget),
        };
        assert!(intent_tree.children.is_empty());
        let intent_node = <SlipwayIcedLayoutIntentWidget<'_, TestWidget> as Widget<
            Message,
            (),
            (),
        >>::layout(
            &mut intent_widget, &mut intent_tree, &renderer, &limits
        );
        assert!(intent_node.children().is_empty());
        let mut intent_operation = RecordingOperation::default();
        <SlipwayIcedLayoutIntentWidget<'_, TestWidget> as Widget<Message, (), ()>>::operate(
            &mut intent_widget,
            &mut intent_tree,
            iced::advanced::Layout::new(&intent_node),
            &renderer,
            &mut intent_operation,
        );
        assert_root_operation(&intent_operation);
    }

    #[test]
    fn runtime_widget_update_publishes_input_and_helper_mutates_runtime_state() {
        let mut runtime = SlipwayRuntime::new(TestWidget, ());
        runtime.record_presented_viewport(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 100.0,
                height: 40.0,
            },
        });

        let messages = {
            let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
            let mut tree = iced::advanced::widget::Tree {
                tag: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    (),
                >>::tag(&widget),
                state: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    (),
                >>::state(&widget),
                children: Vec::new(),
            };
            let renderer = ();
            let limits = iced::advanced::layout::Limits::new(
                iced::Size::new(0.0, 0.0),
                iced::Size::new(100.0, 40.0),
            );
            let node = <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::layout(&mut widget, &mut tree, &renderer, &limits);
            let layout = iced::advanced::Layout::new(&node);
            let event =
                iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left));
            let cursor = iced::advanced::mouse::Cursor::Available(iced::Point::new(4.0, 4.0));
            let mut clipboard = iced::advanced::clipboard::Null;
            let viewport = iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 40.0,
            };
            let mut messages = Vec::new();

            {
                let mut shell = iced::advanced::Shell::new(&mut messages);
                <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
                    SlipwayIcedRuntimeMessage<Message>,
                    iced::Theme,
                    (),
                >>::update(
                    &mut widget,
                    &mut tree,
                    &event,
                    layout,
                    cursor,
                    &renderer,
                    &mut clipboard,
                    &mut shell,
                    &viewport,
                );

                assert!(shell.is_event_captured());
            }

            assert_eq!(runtime.local_state().clicks, 0);
            messages
        };

        assert_eq!(messages.len(), 1);
        assert_eq!(runtime.debug_render_calls(), 0);

        let mut applied_batches = Vec::new();
        let update = apply_iced_runtime_message(
            &mut runtime,
            messages.into_iter().next().expect("runtime input message"),
            &mut |_, messages| applied_batches.push(messages),
        );

        assert_eq!(runtime.local_state().clicks, 1, "update: {update:?}");
        assert_eq!(runtime.debug_render_calls(), 0);
        assert_eq!(applied_batches, vec![vec![Message::Clicked]]);
        assert_eq!(
            update,
            SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            }
        );
    }

    #[test]
    fn opaque_paint_layer_blocks_lower_hit_region_input() {
        let target = WidgetId::from("opaque-layer-input-test");
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 120.0,
                height: 80.0,
            },
        };
        let lower_hit = slipway_core::hit_region_from_pointer_capability(
            &InteractionCapabilityWidget { id: target.clone() },
            &(),
            &(),
            slipway_core::PresentationRegionId::from("lower-hit"),
            None,
            TargetLocalRect::new(bounds),
            slipway_core::PointerEventCoordinateSpace::TargetLocal,
            slipway_core::HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
            Some("lower-hit-route".to_string()),
            CursorCapability::Pointer,
            true,
            slipway_core::PointerCaptureIntent::OnPress,
        );
        let view = ViewDefinition {
            target: target.clone(),
            frame: frame(612),
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(bounds),
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            },
            paint: vec![PaintOp::keyed_layer(
                slipway_core::PaintLayerKey::ordered(10, 0),
                vec![PaintOp::Fill {
                    shape: ShapeDeclaration {
                        id: Some("opaque-card".to_string()),
                        kind: ShapeKind::Rectangle,
                        bounds,
                        path: None,
                        clip: None,
                    },
                    color: test_rgb(255, 255, 255),
                }],
            )],
            paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: vec![lower_hit],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: bounds.size,
                },
            },
            target,
        );

        assert_eq!(presentation.paint_occlusion_regions.len(), 1);
        let mut hovered_region = None;
        let mut pressed_region = None;
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: bounds.size.width,
                height: bounds.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(12.0, 12.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("opaque paint layer returns a routed absorption result");

        assert!(routed.capture_event);
        assert!(
            routed.input.is_none(),
            "opaque paint without a declared hit target must absorb, not dispatch to a lower hit region"
        );
    }

    #[test]
    fn opaque_paint_layer_allows_its_own_same_layer_hit_region_input() {
        let target = WidgetId::from("opaque-layer-own-input-test");
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 120.0,
                height: 80.0,
            },
        };
        let own_hit = slipway_core::hit_region_from_pointer_capability(
            &InteractionCapabilityWidget { id: target.clone() },
            &(),
            &(),
            slipway_core::PresentationRegionId::from("own-hit"),
            None,
            TargetLocalRect::new(bounds),
            slipway_core::PointerEventCoordinateSpace::TargetLocal,
            slipway_core::HitRegionOrder {
                z_index: 10,
                paint_order: 0,
                traversal_order: 0,
            },
            Some("own-hit-route".to_string()),
            CursorCapability::Grab,
            true,
            slipway_core::PointerCaptureIntent::DuringDrag,
        );
        let view = ViewDefinition {
            target: target.clone(),
            frame: frame(613),
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(bounds),
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            },
            paint: vec![PaintOp::keyed_layer(
                slipway_core::PaintLayerKey::ordered(10, 0),
                vec![PaintOp::Fill {
                    shape: ShapeDeclaration {
                        id: Some("opaque-card".to_string()),
                        kind: ShapeKind::Rectangle,
                        bounds,
                        path: None,
                        clip: None,
                    },
                    color: test_rgb(255, 255, 255),
                }],
            )],
            paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: vec![own_hit],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: bounds.size,
                },
            },
            target,
        );

        let mut hovered_region = None;
        let mut pressed_region = None;
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::ButtonPressed(iced::mouse::Button::Left)),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: bounds.size.width,
                height: bounds.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(12.0, 12.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("same-layer hit routes through opaque paint");

        assert!(routed.capture_event);
        let input = routed
            .input
            .expect("opaque layer must not block its own same-key hit region");
        let InputEvent::Pointer(_) = input.event else {
            panic!("expected pointer event");
        };
        assert_eq!(
            input
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("own-hit"))
        );
    }

    #[test]
    fn captured_drag_move_routes_to_pressed_region_outside_current_hit_area() {
        let target = WidgetId::from("captured-drag-move-test");
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 220.0,
                height: 140.0,
            },
        };
        let titlebar_bounds = Rect {
            origin: Point { x: 20.0, y: 12.0 },
            size: Size {
                width: 110.0,
                height: 24.0,
            },
        };
        let titlebar_hit = slipway_core::hit_region_from_pointer_capability(
            &InteractionCapabilityWidget { id: target.clone() },
            &(),
            &(),
            PresentationRegionId::from("titlebar-hit"),
            None,
            TargetLocalRect::new(titlebar_bounds),
            slipway_core::PointerEventCoordinateSpace::TargetLocal,
            slipway_core::HitRegionOrder {
                z_index: 12,
                paint_order: 0,
                traversal_order: 0,
            },
            Some("titlebar-route".to_string()),
            CursorCapability::Grab,
            true,
            slipway_core::PointerCaptureIntent::DuringDrag,
        );
        let view = ViewDefinition {
            target: target.clone(),
            frame: frame(615),
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(bounds),
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            },
            paint: vec![PaintOp::keyed_layer(
                slipway_core::PaintLayerKey::ordered(12, 0),
                vec![PaintOp::Fill {
                    shape: ShapeDeclaration {
                        id: Some("overlay-window".to_string()),
                        kind: ShapeKind::Rectangle,
                        bounds,
                        path: None,
                        clip: None,
                    },
                    color: test_rgb(255, 255, 255),
                }],
            )],
            paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: vec![titlebar_hit],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        let presentation = IcedPresentationState::from_view_definition(
            view.clone(),
            LayoutInput {
                viewport: TargetLocalRect::new(bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: bounds.size,
                },
            },
            target,
        );

        let mut hovered_region = None;
        let mut pressed_region = Some(pressed_capture("titlebar-hit"));
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: iced::Point::new(170.0, 96.0),
            }),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: bounds.size.width,
                height: bounds.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(170.0, 96.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("captured cursor move outside titlebar still routes");

        assert!(routed.capture_event);
        let input = routed
            .input
            .expect("captured cursor move must produce backend input");
        let InputEvent::Pointer(pointer) = &input.event else {
            panic!("expected pointer event");
        };
        assert_eq!(pointer.kind, slipway_core::PointerEventKind::Move);
        assert_eq!(pointer.target, WidgetId::from("captured-drag-move-test"));
        assert_eq!(pointer.position, Point { x: 170.0, y: 96.0 });
        let evidence = input
            .dispatch_evidence
            .as_ref()
            .expect("captured pointer input carries dispatch evidence");
        assert_eq!(
            evidence.selected_region,
            Some(PresentationRegionId::from("titlebar-hit"))
        );
        assert_eq!(
            evidence.candidate_regions,
            vec![PresentationRegionId::from("titlebar-hit")]
        );
        assert!(evidence.capture_event);
        let diagnostics = slipway_core::backend_input_dispatch_evidence_contract_diagnostics(
            &view,
            &input,
            Some(slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED),
            Some(ICED_BACKEND_ID),
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn captured_drag_release_routes_to_pressed_region_outside_current_hit_area() {
        let target = WidgetId::from("captured-drag-release-test");
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 220.0,
                height: 140.0,
            },
        };
        let titlebar_bounds = Rect {
            origin: Point { x: 20.0, y: 12.0 },
            size: Size {
                width: 110.0,
                height: 24.0,
            },
        };
        let titlebar_hit = slipway_core::hit_region_from_pointer_capability(
            &InteractionCapabilityWidget { id: target.clone() },
            &(),
            &(),
            PresentationRegionId::from("titlebar-hit"),
            None,
            TargetLocalRect::new(titlebar_bounds),
            slipway_core::PointerEventCoordinateSpace::TargetLocal,
            slipway_core::HitRegionOrder {
                z_index: 12,
                paint_order: 0,
                traversal_order: 0,
            },
            Some("titlebar-route".to_string()),
            CursorCapability::Grab,
            true,
            slipway_core::PointerCaptureIntent::DuringDrag,
        );
        let view = ViewDefinition {
            target: target.clone(),
            frame: frame(616),
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(bounds),
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            },
            paint: vec![PaintOp::keyed_layer(
                slipway_core::PaintLayerKey::ordered(12, 0),
                vec![PaintOp::Fill {
                    shape: ShapeDeclaration {
                        id: Some("overlay-window".to_string()),
                        kind: ShapeKind::Rectangle,
                        bounds,
                        path: None,
                        clip: None,
                    },
                    color: test_rgb(255, 255, 255),
                }],
            )],
            paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: vec![titlebar_hit],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        let presentation = IcedPresentationState::from_view_definition(
            view.clone(),
            LayoutInput {
                viewport: TargetLocalRect::new(bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: bounds.size,
                },
            },
            target,
        );

        let mut hovered_region = None;
        let mut pressed_region = Some(pressed_capture("titlebar-hit"));
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::ButtonReleased(
                iced::mouse::Button::Left,
            )),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: bounds.size.width,
                height: bounds.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(170.0, 96.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("captured release outside titlebar still routes");

        assert!(routed.capture_event);
        assert_eq!(pressed_region, None);
        let input = routed
            .input
            .expect("captured release must produce backend input");
        let InputEvent::Pointer(pointer) = &input.event else {
            panic!("expected pointer event");
        };
        assert_eq!(pointer.kind, slipway_core::PointerEventKind::Release);
        assert_eq!(pointer.target, WidgetId::from("captured-drag-release-test"));
        assert_eq!(pointer.position, Point { x: 170.0, y: 96.0 });
        let evidence = input
            .dispatch_evidence
            .as_ref()
            .expect("captured pointer input carries dispatch evidence");
        assert_eq!(
            evidence.selected_region,
            Some(PresentationRegionId::from("titlebar-hit"))
        );
        assert_eq!(
            evidence.candidate_regions,
            vec![PresentationRegionId::from("titlebar-hit")]
        );
        assert!(evidence.capture_event);
        let diagnostics = slipway_core::backend_input_dispatch_evidence_contract_diagnostics(
            &view,
            &input,
            Some(slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED),
            Some(ICED_BACKEND_ID),
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn captured_drag_move_uses_press_layout_origin_when_scroll_content_origin_changes() {
        let target = WidgetId::from("captured-drag-scroll-origin-test");
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 220.0,
                height: 140.0,
            },
        };
        let titlebar_hit = slipway_core::hit_region_from_pointer_capability(
            &InteractionCapabilityWidget { id: target.clone() },
            &(),
            &(),
            PresentationRegionId::from("titlebar-hit"),
            None,
            TargetLocalRect::new(Rect {
                origin: Point { x: 20.0, y: 12.0 },
                size: Size {
                    width: 110.0,
                    height: 24.0,
                },
            }),
            slipway_core::PointerEventCoordinateSpace::TargetLocal,
            slipway_core::HitRegionOrder {
                z_index: 12,
                paint_order: 0,
                traversal_order: 0,
            },
            Some("titlebar-route".to_string()),
            CursorCapability::Grab,
            true,
            slipway_core::PointerCaptureIntent::DuringDrag,
        );
        let view = ViewDefinition {
            target: target.clone(),
            frame: frame(618),
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(bounds),
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            },
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: vec![titlebar_hit],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: bounds.size,
                },
            },
            target,
        );

        let mut hovered_region = None;
        let mut pressed_region = Some(pressed_capture_with_origin("titlebar-hit", 0.0, 160.0));
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: iced::Point::new(520.0, -546.0),
            }),
            iced::Rectangle {
                x: 0.0,
                y: 876.0,
                width: bounds.size.width,
                height: bounds.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(520.0, 330.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("captured move must route through the retained press frame");
        let input = routed.input.expect("captured move emits backend input");
        let InputEvent::Pointer(pointer) = &input.event else {
            panic!("expected pointer event");
        };
        assert_eq!(pointer.position, Point { x: 520.0, y: 170.0 });
    }

    #[test]
    fn captured_drag_uses_child_target_local_coordinates_with_window_offset() {
        let root = WidgetId::from("captured-drag-root");
        let child = WidgetId::from("captured-drag-child");
        let child_slot = WidgetSlotAddress::new(root.clone(), 0).child(child.clone(), 0);
        let root_bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 360.0,
                height: 260.0,
            },
        };
        let child_bounds = Rect {
            origin: Point { x: 84.0, y: 72.0 },
            size: Size {
                width: 180.0,
                height: 120.0,
            },
        };
        let titlebar_bounds = Rect {
            origin: Point { x: 24.0, y: 16.0 },
            size: Size {
                width: 100.0,
                height: 24.0,
            },
        };
        let titlebar_hit = slipway_core::hit_region_from_pointer_capability(
            &InteractionCapabilityWidget { id: child.clone() },
            &(),
            &(),
            PresentationRegionId::from("child-titlebar-hit"),
            Some(child_slot.clone()),
            TargetLocalRect::new(titlebar_bounds),
            slipway_core::PointerEventCoordinateSpace::TargetLocal,
            slipway_core::HitRegionOrder {
                z_index: 12,
                paint_order: 0,
                traversal_order: 0,
            },
            Some("child-titlebar-route".to_string()),
            CursorCapability::Grab,
            true,
            slipway_core::PointerCaptureIntent::DuringDrag,
        );
        let view = ViewDefinition {
            target: root.clone(),
            frame: frame(617),
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(root_bounds),
                child_placements: vec![slipway_core::ChildPlacement {
                    child: child.clone(),
                    bounds: slipway_core::ParentLocalRect::new(child_bounds),
                    local_state_slot: Some(child_slot.clone()),
                }],
                diagnostics: Vec::new(),
            },
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(root.clone()),
            hit_regions: vec![titlebar_hit],
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        let presentation = IcedPresentationState::from_view_definition(
            view.clone(),
            LayoutInput {
                viewport: TargetLocalRect::new(root_bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: root_bounds.size,
                },
            },
            root,
        );

        let window_bounds = iced::Rectangle {
            x: 20.0,
            y: 30.0,
            width: root_bounds.size.width,
            height: root_bounds.size.height,
        };
        let cursor = iced::Point::new(
            20.0 + child_bounds.origin.x + 140.0,
            30.0 + child_bounds.origin.y + 80.0,
        );
        let mut hovered_region = None;
        let mut pressed_region = Some(pressed_capture_with_origin(
            "child-titlebar-hit",
            20.0,
            30.0,
        ));
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::CursorMoved { position: cursor }),
            window_bounds,
            iced::advanced::mouse::Cursor::Available(cursor),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("captured child drag routes even when the window has a non-zero origin");

        let input = routed
            .input
            .expect("captured child drag must produce backend input");
        let InputEvent::Pointer(pointer) = &input.event else {
            panic!("expected pointer event");
        };
        assert_eq!(pointer.target, child);
        assert_eq!(pointer.target_slot, Some(child_slot));
        assert_eq!(
            pointer.position,
            Point { x: 140.0, y: 80.0 },
            "TargetLocal pointer coordinates must not include window or parent origin"
        );
        assert_eq!(
            pointer.target_bounds,
            Some(TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: child_bounds.size,
            }))
        );
        let diagnostics = slipway_core::backend_input_dispatch_evidence_contract_diagnostics(
            &view,
            &input,
            Some(slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED),
            Some(ICED_BACKEND_ID),
        );
        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn childless_declared_scroll_region_gets_iced_indicator_paint() {
        let target = WidgetId::from("childless-scroll-indicator-test");
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 180.0,
                height: 120.0,
            },
        };
        let mut view = ViewDefinition {
            target: target.clone(),
            frame: frame(614),
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(bounds),
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            },
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        view.scroll_regions
            .push(slipway_core::ScrollRegionDeclaration::explicit(
                slipway_core::PresentationRegionId::from("childless-scroll"),
                target.clone(),
                None,
                TargetLocalRect::new(Rect {
                    origin: Point { x: 16.0, y: 16.0 },
                    size: Size {
                        width: 84.0,
                        height: 48.0,
                    },
                }),
                TargetLocalRect::new(Rect {
                    origin: Point { x: 16.0, y: 16.0 },
                    size: Size {
                        width: 84.0,
                        height: 144.0,
                    },
                }),
                Point { x: 0.0, y: 24.0 },
                slipway_core::ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                slipway_core::WheelRouting::SelfFirst,
                slipway_core::HitRegionOrder {
                    z_index: 0,
                    paint_order: 0,
                    traversal_order: 0,
                },
                slipway_core::ScrollConsumptionPolicy::exclusive_wheel(),
                true,
            ));

        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: bounds.size,
                },
            },
            target,
        );

        assert!(
            native_iced_scroll_regions(&presentation).is_empty(),
            "the region has no authored child placement, so it cannot become a native iced Scrollable"
        );
        let indicator_shapes = presentation
            .explicit_layer_paint_units
            .iter()
            .flat_map(|unit| paint_shape_ids(&unit.paint))
            .into_iter()
            .filter(|id| id.contains("scroll-indicator"))
            .collect::<Vec<_>>();
        assert_eq!(
            indicator_shapes.len(),
            2,
            "custom/internal scroll regions still need visible scroll evidence in a pass-through paint layer"
        );
    }

    #[test]
    fn iced_visible_scroll_normalization_crops_bad_viewport_before_admission() {
        let target = WidgetId::from("root");
        let layout_bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 100.0,
                height: 100.0,
            },
        };
        let child = WidgetId::from("scroll-child");
        let child_slot = WidgetSlotAddress::new(target.clone(), 0).child(child.clone(), 0);
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(layout_bounds),
            child_placements: vec![slipway_core::ChildPlacement {
                child,
                bounds: slipway_core::ParentLocalRect::new(Rect {
                    origin: Point { x: 4.0, y: 92.0 },
                    size: Size {
                        width: 80.0,
                        height: 120.0,
                    },
                }),
                local_state_slot: Some(child_slot),
            }],
            diagnostics: Vec::new(),
        };
        let mut cropped_scroll = test_scroll_region_from_capability(
            target.clone(),
            "cropped-scroll",
            Rect {
                origin: Point { x: -4.0, y: 92.0 },
                size: Size {
                    width: 120.0,
                    height: 16.0,
                },
            },
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 10.0,
                    height: 10.0,
                },
            },
            slipway_core::ScrollConsumptionPolicy::exclusive_wheel(),
        );
        cropped_scroll.offset = Point { x: 7.0, y: 999.0 };
        let mut disabled_scroll = test_scroll_region_from_capability(
            target.clone(),
            "disabled-scroll",
            Rect {
                origin: Point { x: 0.0, y: 140.0 },
                size: Size {
                    width: 40.0,
                    height: 20.0,
                },
            },
            Rect {
                origin: Point { x: 0.0, y: 140.0 },
                size: Size {
                    width: 40.0,
                    height: 60.0,
                },
            },
            slipway_core::ScrollConsumptionPolicy::exclusive_wheel(),
        );
        disabled_scroll.offset = Point { x: 0.0, y: 8.0 };
        let view = ViewDefinition {
            target: target.clone(),
            frame: frame(618),
            layout,
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: vec![cropped_scroll, disabled_scroll],
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let original = iced_backend_admission().admit_view_definition(&view);
        assert!(
            !original.accepted,
            "bad scroll geometry must fail before visible backend normalization"
        );

        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(layout_bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: layout_bounds.size,
                },
            },
            target,
        );

        assert!(
            presentation.admission.accepted,
            "iced presentation must normalize scroll geometry before visible admission: {:?}",
            presentation.admission.unsupported
        );
        assert_eq!(
            presentation.scroll_regions[0].viewport.into_rect(),
            Rect {
                origin: Point { x: 0.0, y: 92.0 },
                size: Size {
                    width: 100.0,
                    height: 8.0,
                },
            }
        );
        assert_eq!(
            presentation.scroll_regions[0].offset,
            Point { x: 0.0, y: 92.0 }
        );
        assert!(!presentation.scroll_regions[1].enabled);
        assert_eq!(
            presentation.scroll_regions[1].offset,
            Point { x: 0.0, y: 0.0 }
        );
        let codes = presentation
            .diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();
        assert!(codes.contains(&"iced.visible_scroll.viewport_cropped_to_layout"));
        assert!(codes.contains(&"iced.visible_scroll.disabled_outside_layout"));
        assert!(codes.contains(&"iced.visible_scroll.content_bounds_expanded_to_viewport"));
        assert!(codes.contains(&"iced.visible_scroll.offset_clamped"));
    }

    #[test]
    fn same_slot_internal_scroll_region_gets_indicator_not_native_scrollable() {
        let root = WidgetId::from("same-slot-scroll-root");
        let child = WidgetId::from("same-slot-scroll-child");
        let child_slot = WidgetSlotAddress::new(root.clone(), 0).child(child.clone(), 0);
        let bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 240.0,
                height: 180.0,
            },
        };
        let mut view = ViewDefinition {
            target: root.clone(),
            frame: frame(615),
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(bounds),
                child_placements: vec![slipway_core::ChildPlacement {
                    child: child.clone(),
                    bounds: slipway_core::ParentLocalRect::new(Rect {
                        origin: Point { x: 24.0, y: 32.0 },
                        size: Size {
                            width: 180.0,
                            height: 120.0,
                        },
                    }),
                    local_state_slot: Some(child_slot.clone()),
                }],
                diagnostics: Vec::new(),
            },
            paint: Vec::new(),
            paint_order: slipway_core::PaintOrderDeclaration::source_order(root.clone()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        view.scroll_regions
            .push(slipway_core::ScrollRegionDeclaration::explicit(
                slipway_core::PresentationRegionId::from("same-slot-inner-scroll"),
                child,
                Some(child_slot),
                TargetLocalRect::new(Rect {
                    origin: Point { x: 12.0, y: 16.0 },
                    size: Size {
                        width: 84.0,
                        height: 48.0,
                    },
                }),
                TargetLocalRect::new(Rect {
                    origin: Point { x: 12.0, y: 16.0 },
                    size: Size {
                        width: 84.0,
                        height: 144.0,
                    },
                }),
                Point { x: 0.0, y: 24.0 },
                slipway_core::ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                slipway_core::WheelRouting::SelfFirst,
                slipway_core::HitRegionOrder {
                    z_index: 0,
                    paint_order: 0,
                    traversal_order: 0,
                },
                slipway_core::ScrollConsumptionPolicy::exclusive_wheel(),
                true,
            ));

        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: bounds.size,
                },
            },
            root,
        );

        assert!(
            native_iced_scroll_regions(&presentation).is_empty(),
            "an internal scroll declaration must not treat the owning widget's same slot as native scroll content"
        );
        let indicator_shapes = presentation
            .explicit_layer_paint_units
            .iter()
            .flat_map(|unit| paint_shape_ids(&unit.paint))
            .into_iter()
            .filter(|id| id.contains("scroll-indicator"))
            .collect::<Vec<_>>();
        assert_eq!(indicator_shapes.len(), 2);
    }

    #[test]
    fn tiny_scroll_indicator_thumb_stays_inside_track_without_panic() {
        let track = Rect {
            origin: Point { x: 10.0, y: 4.0 },
            size: Size {
                width: 5.0,
                height: 14.0,
            },
        };

        let thumb = vertical_scroll_indicator_thumb(track, 22.0, 220.0, 10_000.0);

        assert_eq!(thumb.origin.x, track.origin.x);
        assert!(thumb.origin.y >= track.origin.y);
        assert!(thumb.size.height <= track.size.height);
        assert!(thumb.origin.y + thumb.size.height <= track.origin.y + track.size.height);
        assert!(thumb.origin.y.is_finite());
        assert!(thumb.size.height.is_finite());
    }

    #[test]
    fn iced_hit_region_coordinate_space_controls_pointer_position() {
        let target = WidgetId::from("coordinate-test");
        let target_bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 500.0,
                height: 90.0,
            },
        };
        let layout_bounds = iced::Rectangle {
            x: 100.0,
            y: 50.0,
            width: 500.0,
            height: 90.0,
        };
        let region_bounds = Rect {
            origin: Point { x: 30.0, y: 3.0 },
            size: Size {
                width: 56.0,
                height: 32.0,
            },
        };
        let mut region = slipway_core::hit_region_from_pointer_capability(
            &InteractionCapabilityWidget { id: target.clone() },
            &(),
            &(),
            slipway_core::PresentationRegionId::from("coordinate-test-region"),
            None,
            TargetLocalRect::new(region_bounds),
            slipway_core::PointerEventCoordinateSpace::TargetLocal,
            slipway_core::HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
            Some("coordinate-test-route".to_string()),
            CursorCapability::Pointer,
            true,
            slipway_core::PointerCaptureIntent::None,
        );
        let presentation = IcedPresentationState::from_view_definition(
            ViewDefinition {
                target: target.clone(),
                frame: frame(1),
                layout: LayoutOutput {
                    bounds: TargetLocalRect::new(target_bounds),
                    child_placements: Vec::new(),
                    diagnostics: Vec::new(),
                },
                paint: Vec::new(),
                paint_order: slipway_core::PaintOrderDeclaration::source_order(target.clone()),
                hit_regions: vec![region.clone()],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            },
            LayoutInput {
                viewport: TargetLocalRect::new(target_bounds),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: target_bounds.size,
                },
            },
            target.clone(),
        );

        let target_local = pointer_event_for_hit_region(
            &region,
            &presentation,
            layout_bounds,
            iced::Point::new(140.0, 60.0),
            slipway_core::PointerEventKind::Press,
            Some(iced::mouse::Button::Left),
            true,
            true,
        );

        let target_local_backend_input = target_local.input.clone().expect("pointer event");
        let target_local_input = target_local_backend_input.event.clone();
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::backend_presented(ICED_BACKEND_ID, "physical-input"),
            frame(1),
            &presentation.layout,
            &[region.clone()],
            Point { x: 40.0, y: 10.0 },
            slipway_core::PointerEventKind::Press,
            Some(slipway_core::PointerButton::Primary),
            pointer_details(Some(iced::mouse::Button::Left)),
            true,
        );
        assert_eq!(
            target_local_backend_input.dispatch_evidence,
            Some(evidence.clone())
        );
        assert_eq!(
            dispatch.as_ref().map(|dispatch| dispatch.input.clone()),
            Some(target_local_input.clone())
        );
        assert_eq!(
            evidence.source,
            EvidenceSource::backend_presented(ICED_BACKEND_ID, "physical-input")
        );
        assert_eq!(
            evidence.selected_region,
            Some(slipway_core::PresentationRegionId::from(
                "coordinate-test-region"
            ))
        );
        assert_eq!(evidence.generated_event, Some(target_local_input.clone()));
        assert_eq!(evidence.capture_event, false);

        let InputEvent::Pointer(pointer) = target_local_input else {
            panic!("expected pointer event");
        };
        assert_eq!(pointer.position, Point { x: 40.0, y: 10.0 });
        assert_eq!(
            pointer.target_bounds,
            Some(TargetLocalRect::new(target_bounds))
        );

        region.event_coordinate_space = slipway_core::PointerEventCoordinateSpace::RegionLocal;
        let region_local = pointer_event_for_hit_region(
            &region,
            &presentation,
            layout_bounds,
            iced::Point::new(140.0, 60.0),
            slipway_core::PointerEventKind::Press,
            Some(iced::mouse::Button::Left),
            true,
            true,
        );

        let InputEvent::Pointer(pointer) = region_local.input.expect("pointer event").event else {
            panic!("expected pointer event");
        };
        assert_eq!(pointer.position, Point { x: 10.0, y: 7.0 });
        assert_eq!(
            pointer.target_bounds,
            Some(TargetLocalRect::new(target_bounds))
        );
    }

    #[test]
    fn runtime_widget_layout_reuses_presentation_for_same_revision_and_input() {
        let runtime = SlipwayRuntime::new(TestWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: Vec::new(),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let _ = <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);
        let _ = <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
        assert_eq!(state.layout_pass, 1);
    }

    #[test]
    fn presentation_cache_reuses_unrelated_dirty_scope_across_revision_change() {
        let widget = TestWidget;
        let viewport = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 100.0,
                height: 40.0,
            },
        };
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(viewport),
            constraints: slipway_core::LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: Size {
                    width: 100.0,
                    height: 40.0,
                },
            },
        };
        let target = widget.id();
        let initial_frame = frame(1);
        let view = widget.view_definition(
            &(),
            &Local { clicks: 0 },
            ViewDefinitionInput {
                frame: initial_frame,
                layout_input: layout_input.clone(),
            },
        );
        let mut presentation =
            IcedPresentationState::from_view_definition(view, layout_input.clone(), target.clone());
        let mut next_frame = frame(2);
        next_frame.revision = presentation.frame.revision.saturating_add(1);

        let unrelated_dirty = [IcedDirtyScope {
            target: WidgetId::from("iced.other"),
            slot: None,
        }];
        assert!(iced_presentation_cache_matches(
            &presentation,
            &layout_input,
            Some(&next_frame),
            Some(&unrelated_dirty),
            &target,
            None,
        ));
        refresh_iced_presentation_frame(
            &mut presentation,
            &layout_input,
            Some(&next_frame),
            &target,
            1,
        );
        assert_eq!(presentation.frame.revision, next_frame.revision);

        let mut dirty_frame = next_frame.clone();
        dirty_frame.revision = dirty_frame.revision.saturating_add(1);
        let matching_dirty = [IcedDirtyScope {
            target: target.clone(),
            slot: None,
        }];
        assert!(!iced_presentation_cache_matches(
            &presentation,
            &layout_input,
            Some(&dirty_frame),
            Some(&matching_dirty),
            &target,
            None,
        ));
    }

    #[test]
    fn runtime_widget_layout_skips_rebuild_for_unrelated_dirty_scope() {
        let runtime = SlipwayRuntime::new(TestWidget, ());
        let mut widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);
        let mut tree = iced::advanced::widget::Tree {
            tag: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::tag(&widget),
            state: <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
                SlipwayIcedRuntimeMessage<Message>,
                iced::Theme,
                (),
            >>::state(&widget),
            children: Vec::new(),
        };
        let renderer = ();
        let limits = iced::advanced::layout::Limits::new(
            iced::Size::new(0.0, 0.0),
            iced::Size::new(100.0, 40.0),
        );

        let _ = <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let first_frame = tree
            .state
            .downcast_ref::<IcedRuntimeTreeState>()
            .presentation
            .as_ref()
            .expect("first layout creates presentation")
            .frame
            .clone();
        let mut next_frame = first_frame.clone();
        next_frame.revision = next_frame.revision.saturating_add(1);
        widget.frame_seed = Some(next_frame);
        widget.dirty_scopes = Some(Cow::Owned(vec![IcedDirtyScope {
            target: WidgetId::from("iced.other"),
            slot: None,
        }]));

        let _ = <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
        assert_eq!(state.layout_pass, 1);
        assert_eq!(
            state.presentation.as_ref().unwrap().frame.revision,
            first_frame.revision.saturating_add(1)
        );

        let mut dirty_frame = state.presentation.as_ref().unwrap().frame.clone();
        dirty_frame.revision = dirty_frame.revision.saturating_add(1);
        widget.frame_seed = Some(dirty_frame);
        widget.dirty_scopes = Some(Cow::Owned(vec![IcedDirtyScope {
            target: WidgetId::from("iced.test"),
            slot: None,
        }]));

        let _ = <SlipwayIcedRuntimeWidget<'_, TestWidget> as Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::layout(&mut widget, &mut tree, &renderer, &limits);

        let state = tree.state.downcast_ref::<IcedRuntimeTreeState>();
        assert_eq!(state.layout_pass, 2);
    }

    #[test]
    fn runtime_widget_tree_state_is_marker_only_not_local_state_authority() {
        let runtime = SlipwayRuntime::new(TestWidget, ());
        let widget = SlipwayIcedRuntimeWidget::from_runtime(&runtime);

        let runtime_tag = <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::tag(&widget);
        assert_eq!(
            runtime_tag,
            iced::advanced::widget::tree::Tag::of::<IcedRuntimeTreeState>()
        );
        assert_ne!(
            runtime_tag,
            iced::advanced::widget::tree::Tag::of::<IcedTreeState<Local>>()
        );

        let tree_state = <SlipwayIcedRuntimeWidget<'_, TestWidget> as iced::advanced::Widget<
            SlipwayIcedRuntimeMessage<Message>,
            iced::Theme,
            (),
        >>::state(&widget);
        let marker = tree_state.downcast_ref::<IcedRuntimeTreeState>();

        assert_eq!(marker.id, WidgetId::from("iced.test"));
        assert_eq!(runtime.local_state().clicks, 0);
    }

    #[test]
    fn app_shell_drain_debug_message_uses_runtime_bridge_without_render() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let _pending = shell
            .debug_mcp()
            .begin_bridge_message(&status_message("status", &frame(1)));

        let update = shell.update(SlipwayIcedRuntimeMessage::DrainDebug);

        assert_eq!(
            update,
            SlipwayIcedRuntimeAppUpdate {
                runtime_update: Some(SlipwayIcedRuntimeUpdate::DrainDebug),
                debug_replies_drained: 1,
                debug_error: None,
            }
        );
        assert_eq!(shell.runtime().debug_render_calls(), 0);
    }

    #[test]
    fn app_shell_backend_input_does_not_drain_debug_hot_path() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let _pending = shell
            .debug_mcp()
            .begin_bridge_message(&status_message("status", &frame(1)));
        let frame = shell.runtime().last_frame_identity();
        let input =
            declared_iced_test_press(shell.runtime(), frame.clone(), Point { x: 1.0, y: 1.0 });

        let input_update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(input));

        assert!(matches!(
            input_update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input { handled: true, .. })
        ));
        assert_eq!(
            input_update.debug_replies_drained, 0,
            "visible backend input must not drain MCP/debug queues"
        );

        let drain_update = shell.update(SlipwayIcedRuntimeMessage::DrainDebug);

        assert_eq!(drain_update.debug_replies_drained, 1);
        assert_eq!(drain_update.debug_error, None);
    }

    #[test]
    fn app_shell_drain_debug_respects_default_debug_bridge_budget() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let budget = SlipwayRuntimeDrainBudget::default();
        assert!(budget.debug_bridge > 0);
        let handles = (0..=budget.debug_bridge)
            .map(|index| {
                shell
                    .runtime()
                    .bridge_client()
                    .submit(DebugCommand::status(
                        format!("iced-budget-debug-{index}"),
                        frame(index as u64),
                    ))
                    .expect("submit debug command")
            })
            .collect::<Vec<_>>();

        let (drained, error) = shell.drain_debug_pending();

        assert_eq!(drained, budget.debug_bridge);
        assert_eq!(error, None);
        for handle in handles.iter().take(budget.debug_bridge) {
            assert!(handle.try_recv().expect("reply channel ok").is_some());
        }
        assert!(
            handles[budget.debug_bridge]
                .try_recv()
                .expect("reply channel ok")
                .is_none()
        );

        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert!(
            handles[budget.debug_bridge]
                .try_recv()
                .expect("reply channel ok")
                .is_some()
        );
    }

    #[test]
    fn app_shell_drain_debug_respects_default_runtime_mcp_budget() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let budget = SlipwayRuntimeDrainBudget::default();
        assert!(budget.runtime_mcp > 0);
        let handles = (0..=budget.runtime_mcp)
            .map(|index| {
                shell
                    .debug_mcp()
                    .submit_runtime_request(status_message(
                        &format!("iced-budget-mcp-{index}"),
                        &frame(index as u64),
                    ))
                    .expect("submit runtime MCP request")
            })
            .collect::<Vec<_>>();

        let (drained, error) = shell.drain_debug_pending();

        assert_eq!(drained, budget.runtime_mcp);
        assert_eq!(error, None);
        for handle in handles.iter().take(budget.runtime_mcp) {
            assert!(handle.try_recv().expect("response channel ok").is_some());
        }
        assert!(
            handles[budget.runtime_mcp]
                .try_recv()
                .expect("response channel ok")
                .is_none()
        );

        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert!(
            handles[budget.runtime_mcp]
                .try_recv()
                .expect("response channel ok")
                .is_some()
        );
    }

    #[test]
    fn app_shell_subscription_follows_runtime_mcp_transport() {
        let shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        assert_eq!(shell.subscription().units(), 0);

        let transport = shell
            .runtime()
            .start_debug_mcp_transport()
            .expect("runtime MCP transport starts");
        let shell = shell.with_debug_mcp_transport(transport);

        assert_eq!(shell.subscription().units(), 1);
    }

    #[test]
    fn app_shell_drain_debug_completes_tcp_runtime_mcp_request() {
        let shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let transport = shell
            .runtime()
            .start_debug_mcp_transport()
            .expect("runtime MCP transport starts");
        let addr = transport.local_addr();
        let wake_rx = transport.wake_receiver();
        let mut shell = shell.with_debug_mcp_transport(transport);

        let mut stream = TcpStream::connect(addr).expect("connect to iced runtime MCP transport");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        writeln!(stream, "{}", status_message("tcp-status", &frame(42)))
            .expect("write JSON-RPC line");
        stream.flush().expect("flush JSON-RPC line");

        assert!(wake_rx.recv());
        let update = shell.update(SlipwayIcedRuntimeMessage::DrainDebug);

        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::DrainDebug)
        );
        assert_eq!(update.debug_replies_drained, 1);
        assert_eq!(update.debug_error, None);

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .expect("read JSON-RPC response line");
        assert!(response_line.contains(r#""id":"tcp-status""#));
        assert!(response_line.contains(r#""result""#));
    }

    #[test]
    fn app_shell_debug_drain_uses_message_reducer_for_traced_control() {
        let mut applied_batches = Vec::new();
        let mut shell =
            SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, messages: Vec<Message>| {
                applied_batches.push(messages);
            });
        let handle = shell
            .runtime()
            .bridge_client()
            .submit(DebugCommand::control_with_trace(
                "iced-trace",
                frame(9),
                InputEvent::Command(CommandEvent {
                    target: WidgetId::from("iced.test"),
                    target_slot: None,
                    command: "click".to_string(),
                    payload_ref: None,
                    source: None,
                }),
            ))
            .expect("submit traced control");

        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert_eq!(applied_batches, vec![vec![Message::Clicked]]);

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
    fn app_shell_mcp_physical_control_requires_iced_winit_event_injection_runner() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        let handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_pointer_message(
                "iced-native-missing-window",
                &frame,
                8.0,
                8.0,
            ))
            .expect("runtime MCP physical request queued");

        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        assert_eq!(shell.runtime().local_state().clicks, 0);
        assert!(shell.runtime().last_backend_input_trace().is_none());

        let response = handle
            .recv()
            .expect("transport response arrives")
            .expect("physical response sent");
        let payload: serde_json::Value = serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("tool result text"),
        )
        .expect("tool result payload is JSON");
        assert_eq!(payload["product_kind"], "error");
        assert_eq!(
            payload["product"]["code"],
            "iced-winit-event-injection-required"
        );
    }

    #[test]
    fn app_shell_accepts_backend_presented_physical_control_from_native_runner() {
        let mut applied_batches = Vec::new();
        let mut shell =
            SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, messages: Vec<Message>| {
                applied_batches.push(messages);
            });
        let frame = shell.runtime().last_frame_identity();
        let backend_input =
            declared_iced_test_press(shell.runtime(), frame.clone(), Point { x: 1.0, y: 1.0 });

        let product = shell.handle_backend_presented_physical_control(
            DebugCommand::physical_control_with_trace(
                "iced-native-runner-press",
                frame,
                DebugPhysicalControl::Pointer {
                    position: Point { x: 1.0, y: 1.0 },
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: pointer_details(Some(iced::mouse::Button::Left)),
                    pointer_is_pressed: true,
                },
            ),
            backend_input,
        );

        assert_eq!(shell.runtime().local_state().clicks, 1);
        assert_eq!(applied_batches, vec![vec![Message::Clicked]]);
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
            EvidenceSource::backend_presented(ICED_BACKEND_ID, "physical-input")
        );
    }

    #[test]
    fn app_shell_visible_backend_input_requires_declared_dispatch_evidence() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        let input =
            declared_iced_test_press(shell.runtime(), frame.clone(), Point { x: 1.0, y: 1.0 });

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(input));

        assert_eq!(shell.runtime().local_state().clicks, 1);
        assert_eq!(shell.runtime().debug_render_calls(), 0);
        let trace = shell
            .runtime()
            .last_backend_input_trace()
            .expect("visible backend input is recorded as a backend input trace");
        let evidence = trace
            .input
            .dispatch_evidence
            .as_ref()
            .expect("visible backend input carries declared dispatch evidence");
        assert_eq!(
            evidence.source.label(),
            slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED
        );
        assert_eq!(
            evidence.selected_region,
            Some(PresentationRegionId::from("iced.test.hit"))
        );
        assert!(trace.handled);
        assert_eq!(
            update,
            SlipwayIcedRuntimeAppUpdate {
                runtime_update: Some(SlipwayIcedRuntimeUpdate::Input {
                    handled: true,
                    applied_messages: 1,
                    diagnostics: Vec::new(),
                }),
                debug_replies_drained: 0,
                debug_error: None,
            }
        );
    }

    #[test]
    fn iced_backend_focus_input_event_evidence_matches_runtime_contract() {
        let widget = TextEditUnsupportedWidget;
        let external = ();
        let local = ();
        let frame = frame(81);
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
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let region = view
            .focus_regions
            .first()
            .expect("text edit widget declares focus region");
        let event = InputEvent::Keyboard(slipway_core::KeyboardEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            key: "Enter".to_string(),
            kind: slipway_core::KeyEventKind::Press,
            modifiers: slipway_core::Modifiers::default(),
            details: slipway_core::KeyboardDetails::default(),
        });

        let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
        let input = backend_focus_input_event_from_parts(
            frame,
            &geometry_index,
            &view.focus_regions,
            region,
            DeclaredEventDispatchKind::Keyboard,
            None,
            event,
        );
        let diagnostics = slipway_core::backend_input_dispatch_evidence_contract_diagnostics(
            &view,
            &input,
            Some(slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED),
            Some(ICED_BACKEND_ID),
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
        let evidence = input
            .dispatch_evidence
            .as_ref()
            .expect("focused backend input carries evidence");
        assert_eq!(
            evidence.candidate_regions,
            vec![PresentationRegionId::from("text-edit-focus")]
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_keyboard_input_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(FocusedInputWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = FocusedInputWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let region = view
            .focus_regions
            .first()
            .expect("focused widget declares focus region");
        let event = InputEvent::Keyboard(slipway_core::KeyboardEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            key: "Enter".to_string(),
            kind: slipway_core::KeyEventKind::Press,
            modifiers: slipway_core::Modifiers::default(),
            details: slipway_core::KeyboardDetails::default(),
        });
        let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
        let backend_input = backend_focus_input_event_from_parts(
            frame.clone(),
            &geometry_index,
            &view.focus_regions,
            region,
            DeclaredEventDispatchKind::Keyboard,
            None,
            event,
        );

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );
        assert_eq!(shell.runtime().local_state().clicks, 1);

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_keyboard_message(
                "iced-shell-keyboard",
                &frame,
                "iced.focused",
                "Enter",
            ))
            .expect("runtime MCP physical keyboard request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical keyboard response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-keyboard-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");

        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.focused.focus""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend keyboard input share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_text_edit_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(FocusedInputWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = FocusedInputWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let region = view
            .focus_regions
            .first()
            .expect("focused widget declares text-edit focus region");
        let event = InputEvent::TextEdit(slipway_core::TextEditEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            kind: TextEditKind::ReplaceSelection,
            text: Some("abc".to_string()),
            selection_before: None,
            selection_after: None,
        });
        let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
        let backend_input = backend_focus_input_event_from_parts(
            frame.clone(),
            &geometry_index,
            &view.focus_regions,
            region,
            DeclaredEventDispatchKind::Text,
            None,
            event,
        );

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );
        assert_eq!(shell.runtime().local_state().clicks, 1);

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_text_edit_message(
                "iced-shell-text-edit",
                &frame,
                "iced.focused",
                "abc",
            ))
            .expect("runtime MCP physical text-edit request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical text-edit response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-text-edit-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");

        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.focused.focus""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""summary":"text_edit:ReplaceSelection""#),
            "probe must preserve text-edit shape: {payload}"
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend text-edit input share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_command_input_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(FocusedInputWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = FocusedInputWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let region = view
            .focus_regions
            .first()
            .expect("focused widget declares focus region");
        let event = InputEvent::Command(CommandEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            command: "submit".to_string(),
            payload_ref: Some("payload-1".to_string()),
            source: None,
        });
        let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
        let backend_input = backend_focus_input_event_from_parts(
            frame.clone(),
            &geometry_index,
            &view.focus_regions,
            region,
            DeclaredEventDispatchKind::Command,
            None,
            event,
        );

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );
        assert_eq!(shell.runtime().local_state().clicks, 1);

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_command_message(
                "iced-shell-command",
                &frame,
                "iced.focused",
                "submit",
                "payload-1",
            ))
            .expect("runtime MCP physical command request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical command response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-command-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");

        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.focused.focus""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend command input share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_wheel_input_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(ScrollProbeWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = ScrollProbeWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(frame.viewport),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: frame.viewport.size,
                },
            },
            widget.id(),
        );
        let mut hovered_region = None;
        let mut pressed_region = None;
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::WheelScrolled {
                delta: iced::mouse::ScrollDelta::Pixels { x: 0.0, y: 7.0 },
            }),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: frame.viewport.size.width,
                height: frame.viewport.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(1.0, 1.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("iced wheel event routes through backend converter");
        let backend_input = routed
            .input
            .expect("iced wheel event yields declared backend input");

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );
        assert_eq!(shell.runtime().local_state().clicks, 1);

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_wheel_message(
                "iced-shell-wheel",
                &frame,
                1.0,
                1.0,
                0.0,
                7.0,
            ))
            .expect("runtime MCP physical wheel request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical wheel response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-wheel-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");

        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.scroll.region""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend wheel input share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_scroll_input_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(ScrollProbeWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = ScrollProbeWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let region = view
            .scroll_regions
            .first()
            .expect("scroll widget declares scroll region");
        let event = InputEvent::Scroll(ScrollEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            region_id: region.id.clone(),
            offset_x: 0.0,
            offset_y: 24.0,
            viewport: region.viewport,
            content_bounds: region.content_bounds,
        });
        let backend_input = backend_scroll_input_event_from_parts(
            frame.clone(),
            &view.scroll_regions,
            region,
            event,
        );

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );
        assert_eq!(shell.runtime().local_state().clicks, 1);

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_scroll_message(
                "iced-shell-scroll",
                &frame,
                "iced.scroll",
                0.0,
                24.0,
            ))
            .expect("runtime MCP physical scroll request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical scroll response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-scroll-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");

        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.scroll.region""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend scroll input share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    fn app_shell_delivers_forged_declared_backend_input_without_runtime_evidence_gate() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        let mut input =
            declared_iced_test_press(shell.runtime(), frame.clone(), Point { x: 1.0, y: 1.0 });
        if let Some(evidence) = input.dispatch_evidence.as_mut() {
            evidence.selected_region = Some(PresentationRegionId::from("forged-hit"));
        }

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(input));

        assert_eq!(shell.runtime().local_state().clicks, 1);
        let trace = shell
            .runtime()
            .last_backend_input_trace()
            .expect("visible backend input is recorded as a backend input trace");
        assert!(trace.handled);
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: trace.diagnostics.clone(),
            })
        );
    }

    #[test]
    fn app_shell_delivers_stale_declared_backend_input_without_runtime_evidence_gate() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let stale_frame = frame(777);
        let input =
            declared_iced_test_press(shell.runtime(), stale_frame, Point { x: 1.0, y: 1.0 });

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(input));

        assert_eq!(shell.runtime().local_state().clicks, 1);
        let trace = shell
            .runtime()
            .last_backend_input_trace()
            .expect("stale backend input is recorded");
        assert!(trace.handled);
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: trace.diagnostics.clone(),
            })
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_move_input_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(DragTestWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = DragTestWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(frame.viewport),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: frame.viewport.size,
                },
            },
            widget.id(),
        );
        let mut hovered_region = Some(PresentationRegionId::from("iced.test.hit"));
        let mut pressed_region = Some(pressed_capture("iced.test.hit"));
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::CursorMoved {
                position: iced::Point::new(1.0, 1.0),
            }),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: frame.viewport.size.width,
                height: frame.viewport.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(1.0, 1.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("iced captured cursor move routes through backend converter");
        let backend_input = routed
            .input
            .expect("iced captured cursor move yields declared backend input");

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_pointer_phase_message(
                "iced-shell-move",
                &frame,
                1.0,
                1.0,
                "move",
            ))
            .expect("runtime MCP physical pointer move request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical pointer move response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-move-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.test.hit""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend pointer move share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_enter_input_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(HoverTestWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = HoverTestWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(frame.viewport),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: frame.viewport.size,
                },
            },
            widget.id(),
        );
        let mut hovered_region = None;
        let mut pressed_region = None;
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::CursorEntered),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: frame.viewport.size.width,
                height: frame.viewport.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(1.0, 1.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("iced cursor enter routes through backend converter");
        let backend_input = routed
            .input
            .expect("iced cursor enter yields declared backend input");

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_pointer_phase_message(
                "iced-shell-enter",
                &frame,
                1.0,
                1.0,
                "enter",
            ))
            .expect("runtime MCP physical pointer enter request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical pointer enter response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-enter-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.test.hit""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend pointer enter share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_leave_input_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(HoverTestWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = HoverTestWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(frame.viewport),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: frame.viewport.size,
                },
            },
            widget.id(),
        );
        let mut hovered_region = Some(PresentationRegionId::from("iced.test.hit"));
        let mut pressed_region = None;
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Mouse(iced::mouse::Event::CursorLeft),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: frame.viewport.size.width,
                height: frame.viewport.size.height,
            },
            iced::advanced::mouse::Cursor::Unavailable,
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("iced cursor leave routes through backend converter");
        let backend_input = routed
            .input
            .expect("iced cursor leave yields declared backend input");

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_pointer_phase_message(
                "iced-shell-leave",
                &frame,
                0.0,
                0.0,
                "leave",
            ))
            .expect("runtime MCP physical pointer leave request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical pointer leave response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-leave-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.test.hit""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend pointer leave share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_touch_cancel_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(CancelTestWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let widget = CancelTestWidget;
        let view = widget.visible_backend_view_definition(
            shell.runtime().external(),
            shell.runtime().local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            },
        );
        let presentation = IcedPresentationState::from_view_definition(
            view,
            LayoutInput {
                viewport: TargetLocalRect::new(frame.viewport),
                constraints: LayoutConstraints {
                    min: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                    max: frame.viewport.size,
                },
            },
            widget.id(),
        );
        let mut hovered_region = Some(PresentationRegionId::from("iced.test.hit"));
        let mut pressed_region = Some(pressed_capture("iced.test.hit"));
        let mut focused_region = None;
        let routed = route_iced_event(
            &iced::Event::Touch(iced::touch::Event::FingerLost {
                id: iced::touch::Finger(7),
                position: iced::Point::new(1.0, 1.0),
            }),
            iced::Rectangle {
                x: 0.0,
                y: 0.0,
                width: frame.viewport.size.width,
                height: frame.viewport.size.height,
            },
            iced::advanced::mouse::Cursor::Available(iced::Point::new(1.0, 1.0)),
            Some(&presentation),
            &mut hovered_region,
            &mut pressed_region,
            &mut focused_region,
        )
        .expect("iced touch cancel routes through backend converter");
        let backend_input = routed
            .input
            .expect("iced touch cancel yields declared backend input");

        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_touch_phase_message(
                "iced-shell-touch-cancel",
                &frame,
                1.0,
                1.0,
                "cancel",
                7,
            ))
            .expect("runtime MCP physical touch cancel request queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical touch cancel response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message(
                "iced-shell-touch-cancel-events",
                &frame,
            ))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.test.hit""#)
                .count(),
            4
        );
        assert!(
            payload.contains(r#""summary":"pointer:Cancel""#),
            "event probe must expose pointer cancel generated events: {payload}"
        );
        assert!(
            payload.contains(r#""kind":"Cancel""#),
            "event probe must expose pointer cancel kind: {payload}"
        );
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend touch cancel share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control no longer synthesizes a backend-presented event inside runtime"]
    fn app_shell_mcp_physical_and_visible_backend_input_share_event_probe_surface() {
        let mut shell = SlipwayIcedRuntimeApp::from_parts(TestWidget, (), |_, _| {});
        let frame = shell.runtime().last_frame_identity();
        shell
            .runtime_mut()
            .record_presented_viewport(frame.viewport);

        let backend_input =
            declared_iced_test_press(shell.runtime(), frame.clone(), Point { x: 1.0, y: 1.0 });
        let update = shell.update(SlipwayIcedRuntimeMessage::BackendInput(backend_input));
        assert_eq!(
            update.runtime_update,
            Some(SlipwayIcedRuntimeUpdate::Input {
                handled: true,
                applied_messages: 1,
                diagnostics: Vec::new(),
            })
        );

        let physical_handle = shell
            .debug_mcp()
            .submit_runtime_request(physical_pointer_message(
                "iced-shell-physical",
                &frame,
                1.0,
                1.0,
            ))
            .expect("runtime MCP physical request queued");

        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical control response sent");
        assert_eq!(shell.runtime().local_state().clicks, 2);

        let probe_handle = shell
            .debug_mcp()
            .submit_runtime_request(probe_event_message("iced-shell-events", &frame))
            .expect("runtime MCP event probe queued");
        let (drained, error) = shell.drain_debug_pending();
        assert_eq!(drained, 1);
        assert_eq!(error, None);
        let response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert_eq!(payload.matches(r#""kind":"event""#).count(), 2);
        assert!(payload.contains(r#""label":"debug_mcp""#));
        assert!(payload.contains(r#""label":"backend_presented""#));
        assert_eq!(payload.matches(r#""dispatch_identity""#).count(), 2);
        assert_eq!(payload.matches(r#""result_identity""#).count(), 2);
        assert_eq!(
            payload
                .matches(r#""selected_region":"iced.test.hit""#)
                .count(),
            4
        );
        assert_eq!(payload.matches(r#""handled":true"#).count(), 4);
        assert!(
            payload.contains(r#""code":"event_equivalence.identity_match""#),
            "event probe must prove MCP and backend physical input share dispatch/result identity: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.dispatch_identity_mismatch""#),
            "happy path must not report dispatch mismatch: {payload}"
        );
        assert!(
            !payload.contains(r#""code":"event_equivalence.result_identity_mismatch""#),
            "happy path must not report result mismatch: {payload}"
        );
    }
}
