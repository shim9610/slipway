//! App runner and assembly only (`docs/public/authoring-layout.md`).
//! Wires the authored ssot/internal_logic/communication/view modules into
//! the Slipway runtime and the selected backend launch path. No widget
//! behavior lives here.
//!
//! IMPORT BOUNDARY (docs/public/llm-contract-checklist.md "First Rule"):
//! the four authored modules import `slipway::prelude::*` and nothing
//! else. This file is the backend boundary, so it may ALSO use the
//! facade's backend-adapter surface — `slipway::backend_iced` /
//! `slipway::backend_egui` and the font-resolution contract from the
//! facade root — behind the matching feature gates. That is the
//! checklist's sanctioned classification ("backend-native wrapper need:
//! accept that the code is backend-specific"); it is NOT a license to
//! import backend types into ssot/logic/communication/view.

use slipway::prelude::*;

use crate::communication::{ShowcaseApp, apply_messages};
use crate::ssot::ShowcaseState;

/// `--iced` / `--egui` select the backend at runtime; the SAME authored
/// modules drive both (checklist "Backend Choice": backend-neutral app
/// code never mentions a backend; switching is typed repair, not
/// translation).
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    match std::env::args().nth(1).as_deref() {
        Some("--iced") => run_iced(),
        Some("--egui") | None => run_egui(),
        Some(other) => {
            eprintln!("unknown argument: {other}");
            eprintln!("usage: slipway-example-authored [--egui|--iced]");
            Ok(())
        }
    }
}

/// Iced launch (docs/public/quickstart-authoring.md §4): the app is
/// adapted through `SlipwayAppWidget`, the reducer is passed alongside,
/// and `with_platform_ime_always_allowed` keeps OS IME (Korean/Hangul)
/// usable for the text input (docs/public/api/ime.md). Debug MCP is
/// attached by `admitted_debug()`; the window title carries the port.
#[cfg(feature = "iced")]
fn run_iced() -> Result<(), Box<dyn std::error::Error>> {
    let config = SlipwayRuntimeConfig::admitted_debug().with_platform_ime_always_allowed();
    run_slipway_iced_runtime_app_with_config(
        SlipwayAppWidget::new(ShowcaseApp::new()),
        ShowcaseState::default(),
        apply_messages,
        config,
    )?;
    Ok(())
}

#[cfg(not(feature = "iced"))]
fn run_iced() -> Result<(), Box<dyn std::error::Error>> {
    Err("this binary was built without the `iced` feature".into())
}

/// Egui launch (docs/public/quickstart-authoring.md §4): the documented
/// `SlipwayRuntime::from_app(...)` path, no root wrapper. The egui root
/// gate's `SlipwayFontResolutionPolicy` bound is satisfied by the core
/// impl on `SlipwayAppWidget`, which delegates to
/// `SlipwayApp::resolve_app_font` — this app keeps that hook's default
/// (an honest refusal: no font source is declared, so the backend falls
/// back to its own fonts and records `app-font-resolution-refused`
/// evidence). Override `resolve_app_font` in `communication.rs` only when
/// the app declares a real font source (docs/public/api/ime.md).
#[cfg(feature = "egui")]
fn run_egui() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = SlipwayRuntime::from_app(ShowcaseApp::new(), ShowcaseState::default());
    run_slipway_egui_runtime_app_with_default_bridge(
        "Slipway authored showcase",
        runtime,
        apply_messages,
    )?;
    Ok(())
}

#[cfg(not(feature = "egui"))]
fn run_egui() -> Result<(), Box<dyn std::error::Error>> {
    Err("this binary was built without the `egui` feature".into())
}

// ---------------------------------------------------------------------------
// Backend adapter glue: child traversal
// ---------------------------------------------------------------------------
// Each backend lifts authored children through its own visitor contract
// (docs/public/api/backends.md). Leaf widgets have no authored children,
// so these impls are empty bodies — assembly wiring, not behavior. They
// live HERE (not in view.rs) because they name backend-specific types;
// missing them fails compilation at the backend gate
// (`SlipwayIcedBackendChildWidget` / `SlipwayEguiBackendChildWidget`),
// which is the intended typed-repair signal when a new backend is
// selected.

#[cfg(feature = "iced")]
mod iced_glue {
    use slipway::backend_iced::{SlipwayIcedAuthoredChildren, SlipwayIcedWidgetListVisitor};

    use crate::ssot::{DraftInputWidget, NestedFeedWidget, NoteListWidget, OverlayWidget};

    macro_rules! iced_leaf_widget {
        ($widget:ty) => {
            impl SlipwayIcedAuthoredChildren for $widget {
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
        };
    }

    iced_leaf_widget!(NoteListWidget);
    iced_leaf_widget!(DraftInputWidget);
    iced_leaf_widget!(OverlayWidget);
    iced_leaf_widget!(NestedFeedWidget);
}

#[cfg(feature = "egui")]
mod egui_glue {
    use slipway::backend_egui::{SlipwayEguiAuthoredChildren, SlipwayEguiWidgetListVisitor};

    use crate::ssot::{DraftInputWidget, NestedFeedWidget, NoteListWidget, OverlayWidget};

    macro_rules! egui_leaf_widget {
        ($widget:ty) => {
            impl SlipwayEguiAuthoredChildren for $widget {
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
        };
    }

    egui_leaf_widget!(NoteListWidget);
    egui_leaf_widget!(DraftInputWidget);
    egui_leaf_widget!(OverlayWidget);
    egui_leaf_widget!(NestedFeedWidget);
}

// ---------------------------------------------------------------------------
// Backend adapter glue: font resolution (egui backend contract)
// ---------------------------------------------------------------------------
// `SlipwayEguiBackendContract` requires `SlipwayFontResolutionPolicy` on
// the app widget and every child. This example declares no font source,
// so the honest evidence is an explicit refusal — never a fabricated
// "resolved" claim (docs/public/api/ime.md: apps needing reliable
// CJK text declare their font token and source explicitly; the
// Some(source) branch below is the shape that resolution takes).

#[cfg(feature = "egui")]
mod font_glue {
    use slipway::{
        Diagnostic, EvidenceSource, FontResolutionEvidence, FontResolutionRequest,
        ResourceInstallationEvidence, ResourceInstallationStatus, ResourceRefusalEvidence,
        SlipwayFontResolutionPolicy, SlipwaySsot, SourceValidityEvidence, SourceValidityKind,
        WidgetId,
    };

    use crate::ssot::{DraftInputWidget, NestedFeedWidget, NoteListWidget, OverlayWidget};

    pub(super) fn font_evidence(
        target: WidgetId,
        request: FontResolutionRequest,
    ) -> FontResolutionEvidence {
        let mut fallback_chain = Vec::with_capacity(1 + request.fallback_families.len());
        fallback_chain.push(request.family.clone());
        fallback_chain.extend(request.fallback_families.clone());

        if let Some(source) = request.source.clone() {
            // A declared font source: report it as installable evidence
            // without claiming an installation that never happened.
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
                        provider_id: Some("slipway-example-authored".to_string()),
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

        // No source declared: refuse honestly. Reuses the admission
        // example's documented refusal code, no new diagnostic literals.
        let diagnostic = Diagnostic::unsupported(
            Some(target),
            "example-font-unresolved",
            "this example does not load or verify system fonts; visible backends provide their own font evidence",
        );
        FontResolutionEvidence {
            request,
            resolved_ref: None,
            fallback_chain,
            installation: None,
            refusal: Some(ResourceRefusalEvidence {
                resource_id: "font-request".to_string(),
                source: None,
                reason: "no loadable font source was declared by the app".to_string(),
                evidence_source: EvidenceSource {
                    label: "example_authored_refusal".to_string(),
                    backend_id: None,
                    provider_id: Some("slipway-example-authored".to_string()),
                    pass_id: None,
                },
                diagnostics: vec![diagnostic.clone()],
            }),
            valid_source: None,
            diagnostics: vec![diagnostic],
        }
    }

    macro_rules! example_font_policy {
        ($widget:ty) => {
            impl SlipwayFontResolutionPolicy for $widget {
                fn resolve_font(
                    &self,
                    _external: &Self::ExternalState,
                    _local: &Self::LocalState,
                    request: FontResolutionRequest,
                ) -> FontResolutionEvidence {
                    font_evidence(self.id(), request)
                }
            }
        };
    }

    example_font_policy!(NoteListWidget);
    example_font_policy!(DraftInputWidget);
    example_font_policy!(OverlayWidget);
    example_font_policy!(NestedFeedWidget);
}

// The Step-208 KNOWN-GAP root wrapper (a ~180-line pure-delegation
// `EguiShowcaseRoot`) was deleted in Step 209: core now implements
// `SlipwayFontResolutionPolicy` for `SlipwayAppWidget<A>` by delegating
// to `SlipwayApp::resolve_app_font`, so the documented quickstart path
// compiles as written. Do not reintroduce a root wrapper for fonts.
