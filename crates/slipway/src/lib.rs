//! Public facade crate for Slipway.
//!
//! Most users should depend on this crate instead of depending on individual
//! workspace crates. Backend support is selected with Cargo features:
//!
//! ```toml
//! slipway = { git = "https://github.com/shim9610/slipway.git", features = ["iced"] }
//! ```
//!
//! Ordinary app authors should import `slipway::prelude::*`. The crate root
//! intentionally exposes lower-level extension APIs for backend authors and
//! provider wrappers; do not treat `use slipway::*` as the normal authoring
//! surface.
//!
//! Backend input evidence is not available through the authoring prelude:
//!
//! ```compile_fail
//! use slipway::prelude::*;
//!
//! let _ = core::mem::size_of::<BackendInputEvent>();
//! ```
//!
//! Text paint has no implicit backend/default style:
//!
//! ```compile_fail
//! let _ = slipway::core::TextStyle::default();
//! ```
//!
//! ```compile_fail
//! use slipway::prelude::*;
//!
//! let _ = PaintOp::text;
//! ```

pub use slipway_core as core;
pub use slipway_debug_bridge as debug_bridge;
pub use slipway_debug_mcp as debug_mcp;
pub use slipway_debug_renderer as debug_renderer;
pub use slipway_runtime as runtime;

pub use slipway_core::*;
pub use slipway_runtime::{SlipwayImePolicy, SlipwayRuntime, SlipwayRuntimeConfig};

#[cfg(feature = "iced")]
pub use slipway_backend_iced as backend_iced;

#[cfg(feature = "egui")]
pub use slipway_backend_egui as backend_egui;

pub mod prelude {
    //! Common imports for authoring Slipway apps.
    //!
    //! This module is the ordinary authoring surface. It covers every type
    //! and helper the public docs' mandatory declarations require
    //! (`docs/public/llm-contract-checklist.md`, "What Must Be Declared" and
    //! "Style Rules"): the widget trio, the declaration structs, the
    //! capability helpers that construct them, the load-bearing policy
    //! traits, and the pre-flight admission check.
    //!
    //! The doctest below is the sufficiency proof for the checklist's
    //! prelude claim: it authors a widget with pointer, focus, scroll,
    //! paint-order, wheel-transparency, and text-style declarations from
    //! these imports alone and asserts a clean admission pre-flight. If a
    //! checklist-required item leaves the prelude, this test fails.
    //!
    //! ```
    //! use slipway::prelude::*;
    //!
    //! struct Panel;
    //!
    //! impl SlipwayWidgetTypes for Panel {
    //!     type ExternalState = ();
    //!     type LocalState = ();
    //!     type AppMessage = ();
    //! }
    //!
    //! impl SlipwaySsot for Panel {
    //!     fn id(&self) -> WidgetId {
    //!         WidgetId::from("panel")
    //!     }
    //!     fn capabilities(&self) -> Vec<Capability> {
    //!         vec![
    //!             Capability::PointerInput,
    //!             Capability::FocusInput,
    //!             Capability::WheelInput,
    //!         ]
    //!     }
    //!     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
    //!         TopologyNode::leaf(self.id())
    //!     }
    //!     fn unsupported(&self) -> Vec<Diagnostic> {
    //!         Vec::new()
    //!     }
    //! }
    //!
    //! impl SlipwayLogic for Panel {
    //!     fn handle_event(
    //!         &self,
    //!         _external: &Self::ExternalState,
    //!         _local: &mut Self::LocalState,
    //!         _event: InputEvent,
    //!     ) -> EventOutcome<Self::AppMessage> {
    //!         EventOutcome::ignored()
    //!     }
    //! }
    //!
    //! impl SlipwayView for Panel {
    //!     fn initial_local_state(&self) -> Self::LocalState {}
    //!     fn layout(
    //!         &self,
    //!         _external: &Self::ExternalState,
    //!         _local: &Self::LocalState,
    //!         input: LayoutInput,
    //!     ) -> LayoutOutput {
    //!         LayoutOutput {
    //!             bounds: input.viewport,
    //!             child_placements: Vec::new(),
    //!             diagnostics: Vec::new(),
    //!         }
    //!     }
    //!     fn paint(
    //!         &self,
    //!         _external: &Self::ExternalState,
    //!         _local: &Self::LocalState,
    //!         layout: &LayoutOutput,
    //!     ) -> Vec<PaintOp> {
    //!         let text = PaintOp::styled_text(
    //!             layout.bounds.into_rect(),
    //!             "panel",
    //!             Color { red: 0.1, green: 0.1, blue: 0.1, alpha: 1.0 },
    //!             TextStyle::plain(),
    //!         );
    //!         // Pointer-opaque but wheel-transparent overlay layer.
    //!         vec![
    //!             PaintOp::keyed_layer(PaintLayerKey::ordered(10, 0), vec![text])
    //!                 .with_wheel_transparency(PaintInputTransparency::PassThrough),
    //!         ]
    //!     }
    //!     fn observe_state(
    //!         &self,
    //!         _external: &Self::ExternalState,
    //!         _local: &Self::LocalState,
    //!     ) -> Vec<StateObservation> {
    //!         Vec::new()
    //!     }
    //! }
    //!
    //! impl SlipwayEventRoutingPolicy for Panel {
    //!     fn event_routing_policy(
    //!         &self,
    //!         _external: &Self::ExternalState,
    //!         _local: &Self::LocalState,
    //!         event: &InputEvent,
    //!     ) -> EventRoutingPolicyDeclaration {
    //!         EventRoutingPolicyDeclaration {
    //!             target: self.id(),
    //!             event_target: event.target().clone(),
    //!             route: EventRoute {
    //!                 route_id: None,
    //!                 address: None,
    //!                 path: vec![self.id()],
    //!                 phase: EventRoutePhase::Target,
    //!             },
    //!             capture: Vec::new(),
    //!             diagnostics: Vec::new(),
    //!         }
    //!     }
    //! }
    //!
    //! impl SlipwayEventDispositionPolicy for Panel {
    //!     fn event_disposition(
    //!         &self,
    //!         _external: &Self::ExternalState,
    //!         _local: &Self::LocalState,
    //!         event: &InputEvent,
    //!         route: &EventRoute,
    //!     ) -> EventPropagationEvidence {
    //!         let disposition = EventDisposition {
    //!             handled: true,
    //!             propagate: false,
    //!             default_action_allowed: true,
    //!         };
    //!         EventPropagationEvidence {
    //!             target: self.id(),
    //!             event: event.clone(),
    //!             steps: vec![EventPropagationStep {
    //!                 stage: EventPropagationStage::Target,
    //!                 node: route.path.last().cloned(),
    //!                 disposition,
    //!                 emitted_messages: Vec::new(),
    //!                 changes: Vec::new(),
    //!             }],
    //!             final_disposition: disposition,
    //!             diagnostics: Vec::new(),
    //!         }
    //!     }
    //! }
    //!
    //! impl SlipwayScrollBehaviorPolicy for Panel {
    //!     fn scroll_behavior_policy(
    //!         &self,
    //!         _external: &Self::ExternalState,
    //!         _local: &Self::LocalState,
    //!         input: &LayoutInput,
    //!     ) -> ScrollBehaviorPolicyDeclaration {
    //!         let viewport = input.viewport;
    //!         let content = Rect {
    //!             origin: Point { x: 0.0, y: 0.0 },
    //!             size: Size {
    //!                 width: viewport.size.width,
    //!                 height: viewport.size.height * 2.0,
    //!             },
    //!         };
    //!         ScrollBehaviorPolicyDeclaration {
    //!             target: self.id(),
    //!             region_id: None,
    //!             address: None,
    //!             axes: ScrollAxes { horizontal: false, vertical: true },
    //!             extent: content.size,
    //!             viewport,
    //!             content_bounds: TargetLocalRect::new(content),
    //!             offset: Point { x: 0.0, y: 0.0 },
    //!             consumption: ScrollConsumptionPolicy {
    //!                 wheel: true,
    //!                 drag: false,
    //!                 keyboard: false,
    //!                 programmatic: false,
    //!             },
    //!             diagnostics: Vec::new(),
    //!         }
    //!     }
    //! }
    //!
    //! impl SlipwayWheelRoutingPolicy for Panel {
    //!     fn wheel_routing_policy(
    //!         &self,
    //!         _external: &Self::ExternalState,
    //!         _local: &Self::LocalState,
    //!         _region: &PresentationRegionId,
    //!     ) -> WheelRoutingPolicyDeclaration {
    //!         WheelRoutingPolicyDeclaration {
    //!             target: self.id(),
    //!             routing: WheelRouting::NearestScrollable,
    //!             modifiers: None,
    //!             diagnostics: Vec::new(),
    //!         }
    //!     }
    //! }
    //!
    //! // Every RESERVED bundle bound in one line.
    //! reserved_policy_defaults!(Panel);
    //!
    //! // The text-input helper and the plain scroll helper are also
    //! // prelude-reachable (a full text widget is out of scope here).
    //! #[allow(unused_imports)]
    //! use slipway::prelude::{
    //!     scroll_region_from_scrollable_capability, text_edit_focus_region_from_capability,
    //! };
    //!
    //! let panel = Panel;
    //! let external = ();
    //! let local = ();
    //! let viewport = Rect {
    //!     origin: Point { x: 0.0, y: 0.0 },
    //!     size: Size { width: 240.0, height: 120.0 },
    //! };
    //! let layout = panel.layout(
    //!     &external,
    //!     &local,
    //!     LayoutInput {
    //!         viewport: TargetLocalRect::new(viewport),
    //!         constraints: LayoutConstraints {
    //!             min: Size { width: 0.0, height: 0.0 },
    //!             max: viewport.size,
    //!         },
    //!     },
    //! );
    //!
    //! let hit = hit_region_from_pointer_capability(
    //!     &panel,
    //!     &external,
    //!     &local,
    //!     PresentationRegionId::new("panel:hit"),
    //!     None,
    //!     layout.bounds,
    //!     PointerEventCoordinateSpace::TargetLocal,
    //!     HitRegionOrder::default(),
    //!     None,
    //!     CursorCapability::Default,
    //!     true,
    //!     PointerCaptureIntent::None,
    //! );
    //! let focus = focus_region_from_focus_capability(
    //!     &panel,
    //!     &external,
    //!     &local,
    //!     PresentationRegionId::new("panel:focus"),
    //!     None,
    //!     layout.bounds,
    //!     true,
    //! );
    //! let scroll = scroll_region_from_scrollable_capability_with_order(
    //!     &panel,
    //!     &external,
    //!     &local,
    //!     &layout,
    //!     None,
    //!     None,
    //!     true,
    //!     HitRegionOrder { z_index: 0, paint_order: 0, traversal_order: 1 },
    //! );
    //! let commands = vec![TextEditCommandDeclaration {
    //!     command_id: "insert-text".to_string(),
    //!     kind: TextEditKind::InsertText,
    //!     enabled: true,
    //! }];
    //! assert_eq!(commands.len(), 1);
    //!
    //! let view = ViewDefinition {
    //!     target: panel.id(),
    //!     frame: FrameIdentity {
    //!         surface_id: "prelude-doctest".to_string(),
    //!         surface_instance_id: "instance".to_string(),
    //!         revision: 0,
    //!         frame_index: 0,
    //!         viewport,
    //!     },
    //!     paint: panel.paint(&external, &local, &layout),
    //!     layout,
    //!     paint_order: PaintOrderDeclaration::source_order(panel.id()),
    //!     hit_regions: vec![hit],
    //!     focus_regions: vec![focus],
    //!     scroll_regions: vec![scroll],
    //!     semantic_slots: Vec::new(),
    //!     probe_metadata: Vec::new(),
    //!     diagnostics: Vec::new(),
    //! };
    //! let diagnostics =
    //!     view_definition_contract_diagnostics_for_capabilities(&view, &panel.capabilities());
    //! assert!(
    //!     !view_definition_has_blocking_contract_diagnostic(&diagnostics),
    //!     "{diagnostics:?}"
    //! );
    //! ```

    // Identity, widget trio, and app composition.
    pub use slipway_core::{
        AppLayoutPlan, Capability, ChangeEvidence, ChildLayoutPlan, ChildLayoutSeed,
        ChildPlacement, Diagnostic, DiagnosticSeverity, EmittedMessage, EventOutcome,
        FrameIdentity, InputEvent, SlipwayApp, SlipwayAppWidget, SlipwayLogic, SlipwaySsot,
        SlipwayView, SlipwayViewDefinition, SlipwayWidget, SlipwayWidgetTypes, StateObservation,
        TopologyNode, ViewDefinition, ViewDefinitionInput, WidgetId, WidgetSlotAddress,
    };
    // Geometry and layout.
    pub use slipway_core::{
        LayoutConstraints, LayoutInput, LayoutOutput, ParentLocalRect, Point, Rect, Size,
        TargetLocalRect,
    };
    // Interaction declarations (llm-contract-checklist.md "What Must Be
    // Declared") and their field types.
    pub use slipway_core::{
        CursorCapability, FocusRegionDeclaration, FocusTraversalMember, HitRegionDeclaration,
        HitRegionOrder, PaintOrderDeclaration, PaintOrderMode, PointerCaptureIntent,
        PointerEventCoordinateSpace, PresentationRegionId, ScrollRegionDeclaration,
        TextEditCommandDeclaration, TextEditKind, TextEditRegionDeclaration,
    };
    // Capability helpers: the sanctioned constructors for the declarations
    // above (the region structs are `#[non_exhaustive]`).
    pub use slipway_core::{
        focus_region_from_focus_capability, hit_region_from_pointer_capability,
        scroll_region_from_scrollable_capability,
        scroll_region_from_scrollable_capability_with_order,
        text_edit_focus_region_from_capability,
    };
    // Capability bundles (compile-time contract surface; the helper bounds).
    pub use slipway_core::{
        SlipwayCommandSurfaceCapability, SlipwayDeterministicSourceCapability,
        SlipwayPointerRegionCapability, SlipwayPopupCapability, SlipwayProviderSurfaceCapability,
        SlipwayScrollableContainerCapability, SlipwayTextInputCapability,
    };
    // Load-bearing routing, disposition, and scroll policies
    // (docs/public/api/routing-and-scroll.md).
    pub use slipway_core::{
        EventDisposition, EventPropagationEvidence, EventPropagationStage, EventPropagationStep,
        EventRoute, EventRoutePhase, EventRoutingPolicyDeclaration, ScrollAxes,
        ScrollBehaviorPolicyDeclaration, ScrollConsumptionPolicy, ScrollIndicatorMode,
        SlipwayEventDispositionPolicy, SlipwayEventRoutingPolicy, SlipwayScrollBehaviorPolicy,
        SlipwayWheelRoutingPolicy, WheelRouting, WheelRoutingPolicyDeclaration,
    };
    // Text-input policy traits and their declaration types (consumed by
    // text_edit_focus_region_from_capability).
    pub use slipway_core::{
        CaretGeometryEvidence, ImeCompositionPolicyDeclaration, SlipwayCachedTextMeasurementPolicy,
        SlipwayCaretGeometryPolicy, SlipwayFocusTraversal, SlipwayImeCompositionPolicy,
        SlipwayTextBufferPolicy, SlipwayTextEditCommandPolicy, SlipwayTextFlowPolicy,
        SlipwayTextInputTypographyPolicy, SlipwayTextInputVisualStylePolicy,
        SlipwayTextMeasurementCachePolicy, SlipwayTextMeasurementPolicy,
        SlipwayTextSelectionPolicy, SlipwayTextUndoRedoPolicy, TextBufferSnapshot, TextFlowPolicy,
        TextInputTypographyDeclaration, TextInputVisualStyleDeclaration, TextMeasurementEvidence,
        TextSelectionPolicyDeclaration, TextUndoRedoEvidence,
    };
    // Input-event payloads and their kind enums. `InputEvent` (exported
    // above) is matched inside every `SlipwayLogic::handle_event`; its
    // variant payloads are unusable without these names (e.g.
    // `PointerEventKind::Press`). Roadmap Phase 4 (LE-H2/LE-H6): gap found
    // while authoring the reference example from the prelude alone.
    pub use slipway_core::{
        CommandEvent, FocusEvent, KeyEventKind, KeyboardEvent, Modifiers, PointerButton,
        PointerButtons, PointerDetails, PointerDeviceKind, PointerEvent, PointerEventKind,
        ScrollEvent, TextEditEvent, TextInputEvent, WheelEvent,
    };
    // Shape, path, and clip declarations consumed by the `PaintOp` variants
    // exported above (`Fill`/`Stroke`/`Group`/`Layer`). Same Phase 4 gap
    // closure: a filled rectangle or a clipped scroll window cannot be
    // painted without these names.
    pub use slipway_core::{
        ClipDeclaration, PathCommand, PathDeclaration, ShapeDeclaration, ShapeKind,
    };
    // Component types required to IMPLEMENT the nine text policies and the
    // three measurement policies whose traits are exported above (their
    // declaration structs carry these field types). Same Phase 4 gap
    // closure.
    pub use slipway_core::{
        CaretSet, SlipwayTextMeasurementCache, SlipwayTextMetricProvider, TextLineMode,
        TextMeasurementCachePolicyDeclaration, TextMeasurementPolicyDeclaration,
        TextSelectionRange, TextViewport, TextWrapMode,
    };
    // Paint, layering, and explicit text style (checklist "Style Rules").
    pub use slipway_core::{
        BaselineShift, Color, FontStyle, FontWeight, PaintInputTransparency, PaintLayerKey,
        PaintOp, TextDecoration, TextStyle,
    };
    // Pre-flight admission check (docs/public/api/diagnostics.md).
    pub use slipway_core::{
        view_definition_contract_diagnostics,
        view_definition_contract_diagnostics_for_capabilities,
        view_definition_has_blocking_contract_diagnostic,
    };
    // Satisfies every RESERVED capability-bundle bound with the documented
    // empty defaults; see the macro doc for the trait list.
    pub use slipway_core::reserved_policy_defaults;
    pub use slipway_runtime::{SlipwayImePolicy, SlipwayRuntime, SlipwayRuntimeConfig};

    #[cfg(feature = "iced")]
    pub use slipway_backend_iced::{
        run_slipway_iced_runtime_app, run_slipway_iced_runtime_app_with_config,
    };

    #[cfg(feature = "egui")]
    pub use slipway_backend_egui::run_slipway_egui_runtime_app_with_default_bridge;
}
