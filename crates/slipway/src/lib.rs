//! Public facade crate for Slipway.
//!
//! Most users should depend on this crate instead of depending on individual
//! workspace crates. Backend support is selected with Cargo features:
//!
//! ```toml
//! slipway = { git = "https://github.com/shim9610/slipway.git", features = ["iced"] }
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

    pub use slipway_core::{
        AppLayoutPlan, BackendInputEvent, Capability, ChangeEvidence, ChildLayoutPlan,
        ChildLayoutSeed, ChildPlacement, Color, Diagnostic, EmittedMessage, EventOutcome,
        EventRoute, EventRoutePhase, FrameIdentity, HitRegionDeclaration, InputEvent,
        LayoutConstraints, LayoutInput, LayoutOutput, PaintOp, ParentLocalRect, Point, Rect,
        ScrollRegionDeclaration, Size, SlipwayApp, SlipwayAppWidget, SlipwayLogic, SlipwaySsot,
        SlipwayView, SlipwayViewDefinition, SlipwayWidgetTypes, TargetLocalRect, ViewDefinition,
        ViewDefinitionInput, WidgetId, WidgetSlotAddress,
    };
    pub use slipway_runtime::{SlipwayImePolicy, SlipwayRuntime, SlipwayRuntimeConfig};

    #[cfg(feature = "iced")]
    pub use slipway_backend_iced::{
        run_slipway_iced_runtime_app, run_slipway_iced_runtime_app_with_config,
    };

    #[cfg(feature = "egui")]
    pub use slipway_backend_egui::run_slipway_egui_runtime_app_with_default_bridge;
}
