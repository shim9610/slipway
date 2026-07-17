//! Backend-neutral authoring contract for Slipway. This crate is the single
//! source of truth for the widget/app trait surface, view declarations,
//! admission validation, and dispatch evidence; backend adapters
//! (`slipway-backend-iced`, `slipway-backend-egui`) mechanically lift these
//! contracts and add no authoring surface of their own.
//!
//! The authoring model in five lines:
//!
//! 1. Implement [`SlipwaySsot`] + [`SlipwayLogic`] + [`SlipwayView`]
//!    (= [`SlipwayWidget`]) plus the LOAD-BEARING policy traits your
//!    declared capabilities need; cover RESERVED bounds with one
//!    [`reserved_policy_defaults!`] call.
//! 2. Declare everything that must react: build the [`ViewDefinition`]
//!    regions with the capability helpers
//!    ([`hit_region_from_pointer_capability`],
//!    [`focus_region_from_focus_capability`],
//!    [`text_edit_focus_region_from_capability`],
//!    [`scroll_region_from_scrollable_capability_with_order`]) — painting
//!    something clickable is not enough.
//! 3. Validate before launch:
//!    [`view_definition_contract_diagnostics_for_capabilities`] in a unit
//!    test is the same admission check the visible backends run.
//! 4. Compose N widgets through [`SlipwayApp`] and run through the facade
//!    (`use slipway::prelude::*`) and a backend runner.
//! 5. Verify behavior with debug-MCP evidence, not by eye.
//!
//! Reading path for authoring questions (docs are the contract authority):
//! `docs/public/llm-entry.md` -> `docs/public/llm-contract-checklist.md` ->
//! the per-topic pages under `docs/public/api/` (`core.md`,
//! `routing-and-scroll.md`, `diagnostics.md`, `trait-surface.md`).
//!
//! Section map of this file, in order: identity + geometry
//! ([`WidgetId`], [`Rect`], [`TargetLocalRect`]); layout and layout-policy
//! declarations; paint ([`PaintOp`], layer transparency); input events
//! ([`InputEvent`]); [`Capability`] + [`Diagnostic`]; region declarations
//! and [`ViewDefinition`] + the admission validators
//! ([`view_definition_contract_diagnostics`]); declared-dispatch resolvers
//! and the dispatch graph; render packets and paint-unit ordering;
//! [`EventOutcome`] and event-handling declarations; the authoring traits
//! (trio, policy traits, capability bundles, capability helpers,
//! [`reserved_policy_defaults!`]); app composition ([`SlipwayApp`],
//! [`SlipwayAppWidget`]); probe plumbing.
//!
//! Status convention: an item marked `RESERVED contract surface` is
//! declared ahead of consumption — real logic behind it is a silent no-op.
//! `grep RESERVED` over this file enumerates that surface;
//! `docs/public/api/trait-surface.md` is the public index.

use std::{cmp::Ordering, collections::HashMap, mem, sync::Arc};

#[cfg(test)]
thread_local! {
    static PREPARATION_PLACEMENT_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    static FROM_LAYOUT_PLACEMENT_VISITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct WidgetId(String);

impl WidgetId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for WidgetId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for WidgetId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PresentationRegionId(String);

impl PresentationRegionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for PresentationRegionId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for PresentationRegionId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetLocalRect(Rect);

impl TargetLocalRect {
    pub fn new(rect: Rect) -> Self {
        Self(rect)
    }

    pub fn into_rect(self) -> Rect {
        self.0
    }

    pub const fn as_rect(&self) -> &Rect {
        &self.0
    }
}

impl std::ops::Deref for TargetLocalRect {
    type Target = Rect;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for TargetLocalRect {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<TargetLocalRect> for Rect {
    fn from(value: TargetLocalRect) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentLocalRect(Rect);

impl ContentLocalRect {
    pub const fn new(rect: Rect) -> Self {
        Self(rect)
    }

    pub const fn as_rect(&self) -> &Rect {
        &self.0
    }

    pub const fn into_rect(self) -> Rect {
        self.0
    }

    fn to_parent(self, translation: ContentToParentTranslation) -> ParentLocalRect {
        ParentLocalRect::from_parent_local(Rect {
            origin: Point {
                x: self.0.origin.x + translation.origin.x,
                y: self.0.origin.y + translation.origin.y,
            },
            size: self.0.size,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentToParentTranslation {
    origin: Point,
}

impl ContentToParentTranslation {
    fn from_layout_input(input: &LayoutInput) -> Self {
        Self {
            origin: input.content.into_rect().origin,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParentLocalRect(Rect);

impl ParentLocalRect {
    fn from_parent_local(rect: Rect) -> Self {
        Self(rect)
    }

    pub const fn as_rect(&self) -> &Rect {
        &self.0
    }

    pub const fn into_rect(self) -> Rect {
        self.0
    }

    pub fn translated_for_presentation(self, translation: Translation) -> Self {
        Self::from_parent_local(Rect {
            origin: Point {
                x: self.0.origin.x + translation.x,
                y: self.0.origin.y + translation.y,
            },
            size: self.0.size,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl EdgeInsets {
    pub const ZERO: Self = Self::all(0.0);

    pub const fn all(value: f32) -> Self {
        Self::trbl(value, value, value, value)
    }

    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self::trbl(vertical, horizontal, vertical, horizontal)
    }

    pub const fn trbl(top: f32, right: f32, bottom: f32, left: f32) -> Self {
        Self {
            top,
            right,
            bottom,
            left,
        }
    }

    pub const fn horizontal(self) -> f32 {
        self.left + self.right
    }
    pub const fn vertical(self) -> f32 {
        self.top + self.bottom
    }
}

impl Default for EdgeInsets {
    fn default() -> Self {
        Self::ZERO
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoxSpacing {
    pub margin: EdgeInsets,
    pub padding: EdgeInsets,
}

impl BoxSpacing {
    pub const ZERO: Self = Self::new(EdgeInsets::ZERO, EdgeInsets::ZERO);
    pub const fn new(margin: EdgeInsets, padding: EdgeInsets) -> Self {
        Self { margin, padding }
    }
    pub const fn with_margin(self, margin: EdgeInsets) -> Self {
        Self { margin, ..self }
    }
    pub const fn with_padding(self, padding: EdgeInsets) -> Self {
        Self { padding, ..self }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutConstraints {
    pub min: Size,
    pub max: Size,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutInput {
    pub viewport: TargetLocalRect,
    pub content: TargetLocalRect,
    pub constraints: LayoutConstraints,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildPlacement {
    pub child: WidgetId,
    pub bounds: ParentLocalRect,
    pub local_state_slot: Option<WidgetSlotAddress>,
    pub spacing: BoxSpacing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildLayoutSeed {
    pub child: WidgetId,
    pub local_state_slot: Option<WidgetSlotAddress>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildLayoutRequest {
    pub child: WidgetId,
    pub local_state_slot: Option<WidgetSlotAddress>,
    pub geometry: ChildLayoutGeometry,
    pub outer_constraints: LayoutConstraints,
    pub spacing: BoxSpacing,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ChildLayoutGeometry {
    RequestedOuter(ContentLocalRect),
    ExplicitBorder(ContentLocalRect),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildLayoutPlan {
    pub request: ChildLayoutRequest,
}

impl ChildLayoutPlan {
    pub fn requested_outer(
        seed: ChildLayoutSeed,
        outer: ContentLocalRect,
        outer_constraints: LayoutConstraints,
        spacing: BoxSpacing,
    ) -> Self {
        Self {
            request: ChildLayoutRequest {
                child: seed.child,
                local_state_slot: seed.local_state_slot,
                geometry: ChildLayoutGeometry::RequestedOuter(outer),
                outer_constraints,
                spacing,
            },
        }
    }

    pub fn explicit_border(
        seed: ChildLayoutSeed,
        border: ContentLocalRect,
        spacing: BoxSpacing,
    ) -> Self {
        let size = border.as_rect().size;
        Self {
            request: ChildLayoutRequest {
                child: seed.child,
                local_state_slot: seed.local_state_slot,
                geometry: ChildLayoutGeometry::ExplicitBorder(border),
                outer_constraints: LayoutConstraints {
                    min: Size {
                        width: size.width + spacing.margin.horizontal(),
                        height: size.height + spacing.margin.vertical(),
                    },
                    max: Size {
                        width: size.width + spacing.margin.horizontal(),
                        height: size.height + spacing.margin.vertical(),
                    },
                },
                spacing,
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildLayoutResult {
    pub seed: ChildLayoutSeed,
    pub layout: LayoutOutput,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppLayoutPlan {
    pub bounds: TargetLocalRect,
    pub children: Vec<ChildLayoutPlan>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutOutput {
    bounds: TargetLocalRect,
    child_placements: Vec<ChildPlacement>,
    diagnostics: Vec<Diagnostic>,
}

impl LayoutOutput {
    pub const fn bounds(&self) -> &TargetLocalRect {
        &self.bounds
    }

    pub fn child_placements(&self) -> &[ChildPlacement] {
        &self.child_placements
    }

    pub fn diagnostics(&self) -> &[Diagnostic] {
        &self.diagnostics
    }

    pub fn reset_to_leaf(&mut self, bounds: TargetLocalRect) {
        self.bounds = bounds;
        self.child_placements.clear();
        self.diagnostics.clear();
    }

    fn take_diagnostics(&mut self) -> Vec<Diagnostic> {
        std::mem::take(&mut self.diagnostics)
    }
}

pub struct LayoutOutputBuilder {
    translation: ContentToParentTranslation,
    child_placements: Vec<ChildPlacement>,
    diagnostics: Vec<Diagnostic>,
}

impl LayoutOutputBuilder {
    fn for_input(input: &LayoutInput) -> Self {
        Self {
            translation: ContentToParentTranslation::from_layout_input(input),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn reserve(&mut self, additional: usize) {
        self.child_placements.reserve(additional);
    }

    fn extend_diagnostics(&mut self, diagnostics: impl IntoIterator<Item = Diagnostic>) {
        self.diagnostics.extend(diagnostics);
    }

    pub fn push_resolved(
        &mut self,
        plan: ChildLayoutPlan,
        mut result: ChildLayoutResult,
    ) -> Result<(), BoxGeometryDiagnostic> {
        if result.seed.child != plan.request.child
            || result.seed.local_state_slot != plan.request.local_state_slot
        {
            return Err(BoxGeometryDiagnostic {
                component: "child.identity",
                value: f32::NAN,
            });
        }
        validate_spacing(plan.request.spacing)?;
        let final_size = result.layout.bounds().into_rect().size;
        validate_size(final_size)?;
        if let ChildLayoutGeometry::ExplicitBorder(border) = &plan.request.geometry {
            let authored_size = border.as_rect().size;
            if final_size != authored_size {
                return Err(BoxGeometryDiagnostic {
                    component: "explicit_border.size",
                    value: final_size.width,
                });
            }
        }

        self.diagnostics.append(&mut result.diagnostics);
        let spacing = plan.request.spacing;
        let final_border = match plan.request.geometry {
            ChildLayoutGeometry::RequestedOuter(authored_outer) => {
                let parent_outer = authored_outer.to_parent(self.translation);
                ParentLocalRect::from_parent_local(Rect {
                    origin: Point {
                        x: parent_outer.as_rect().origin.x + spacing.margin.left,
                        y: parent_outer.as_rect().origin.y + spacing.margin.top,
                    },
                    size: final_size,
                })
            }
            ChildLayoutGeometry::ExplicitBorder(authored_border) => {
                let parent_border = authored_border.to_parent(self.translation);
                parent_border
            }
        };
        self.child_placements.push(ChildPlacement {
            child: plan.request.child,
            bounds: final_border,
            local_state_slot: plan.request.local_state_slot,
            spacing,
        });
        Ok(())
    }

    pub fn finish(self, bounds: TargetLocalRect) -> LayoutOutput {
        LayoutOutput {
            bounds,
            child_placements: self.child_placements,
            diagnostics: self.diagnostics,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Translation {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TargetBoxGeometry {
    pub border: TargetLocalRect,
    pub content: TargetLocalRect,
    pub content_paint_bounds: TargetLocalRect,
    pub default_clip: TargetLocalRect,
    pub default_hit_bounds: TargetLocalRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlacementBoxGeometry {
    pub outer: ParentLocalRect,
    pub border: ParentLocalRect,
    pub content: ParentLocalRect,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PresentedBoxGeometry {
    pub outer: Rect,
    pub border: Rect,
    pub content: Rect,
    pub default_clip: Rect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BoxSpacingError {
    pub component: &'static str,
    pub value: f32,
}

pub type BoxGeometryDiagnostic = BoxSpacingError;

fn validate_spacing(spacing: BoxSpacing) -> Result<(), BoxSpacingError> {
    for (component, value) in [
        ("margin.top", spacing.margin.top),
        ("margin.right", spacing.margin.right),
        ("margin.bottom", spacing.margin.bottom),
        ("margin.left", spacing.margin.left),
        ("padding.top", spacing.padding.top),
        ("padding.right", spacing.padding.right),
        ("padding.bottom", spacing.padding.bottom),
        ("padding.left", spacing.padding.left),
    ] {
        if !value.is_finite() || value < 0.0 {
            return Err(BoxSpacingError { component, value });
        }
    }
    Ok(())
}

fn validate_size(size: Size) -> Result<(), BoxSpacingError> {
    if !size.width.is_finite() || size.width < 0.0 {
        return Err(BoxSpacingError {
            component: "border.width",
            value: size.width,
        });
    }
    if !size.height.is_finite() || size.height < 0.0 {
        return Err(BoxSpacingError {
            component: "border.height",
            value: size.height,
        });
    }
    Ok(())
}

pub fn derive_target_box(
    size: Size,
    spacing: BoxSpacing,
) -> Result<TargetBoxGeometry, BoxSpacingError> {
    validate_spacing(spacing)?;
    validate_size(size)?;
    let border = TargetLocalRect::new(Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size,
    });
    let content = TargetLocalRect::new(Rect {
        origin: Point {
            x: spacing.padding.left.min(size.width),
            y: spacing.padding.top.min(size.height),
        },
        size: Size {
            width: (size.width - spacing.padding.horizontal()).max(0.0),
            height: (size.height - spacing.padding.vertical()).max(0.0),
        },
    });
    Ok(TargetBoxGeometry {
        border,
        content,
        content_paint_bounds: content,
        default_clip: border,
        default_hit_bounds: border,
    })
}

/// Reconstructs a child's target-local layout input from its already-final
/// parent-local border. Margin is parent-side geometry and is intentionally
/// not applied again here.
pub fn child_layout_input_for_placement(placement: &ChildPlacement) -> LayoutInput {
    let size = placement.bounds.as_rect().size;
    let target = derive_target_box(size, placement.spacing)
        .expect("validated child placement must have valid box geometry");
    LayoutInput {
        viewport: target.border,
        content: target.content,
        constraints: LayoutConstraints {
            min: Size {
                width: 0.0,
                height: 0.0,
            },
            max: size,
        },
    }
}

pub fn derive_placement_box(
    border: ParentLocalRect,
    spacing: BoxSpacing,
) -> Result<PlacementBoxGeometry, BoxSpacingError> {
    let border_rect = border.as_rect();
    let target = derive_target_box(border_rect.size, spacing)?;
    if !border_rect.origin.x.is_finite() {
        return Err(BoxSpacingError {
            component: "border.x",
            value: border_rect.origin.x,
        });
    }
    if !border_rect.origin.y.is_finite() {
        return Err(BoxSpacingError {
            component: "border.y",
            value: border_rect.origin.y,
        });
    }
    let m = spacing.margin;
    Ok(PlacementBoxGeometry {
        outer: ParentLocalRect::from_parent_local(Rect {
            origin: Point {
                x: border_rect.origin.x - m.left,
                y: border_rect.origin.y - m.top,
            },
            size: Size {
                width: border_rect.size.width + m.horizontal(),
                height: border_rect.size.height + m.vertical(),
            },
        }),
        border,
        content: ParentLocalRect::from_parent_local(translate_rect(
            target.content,
            border_rect.origin,
        )),
    })
}

pub fn effective_clip(
    default_clip: TargetLocalRect,
    authored_overflow: Option<TargetLocalRect>,
    ancestor_clip: Option<Rect>,
    target_to_presented: Translation,
) -> Rect {
    let translated = translate_rect(
        authored_overflow.unwrap_or(default_clip),
        Point {
            x: target_to_presented.x,
            y: target_to_presented.y,
        },
    );
    ancestor_clip
        .and_then(|ancestor| rect_intersection(translated, ancestor))
        .unwrap_or_else(|| {
            if ancestor_clip.is_some() {
                Rect {
                    origin: translated.origin,
                    size: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                }
            } else {
                translated
            }
        })
}

pub fn full_border_hit_bounds(input: &LayoutInput) -> TargetLocalRect {
    input.viewport
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedChildLayout {
    pub input: LayoutInput,
}

pub fn prepare_child_layout(
    plan: &ChildLayoutPlan,
) -> Result<PreparedChildLayout, BoxGeometryDiagnostic> {
    let request = &plan.request;
    validate_spacing(request.spacing)?;
    let m = request.spacing.margin;
    let authored = match &request.geometry {
        ChildLayoutGeometry::RequestedOuter(outer) => outer.as_rect(),
        ChildLayoutGeometry::ExplicitBorder(border) => border.as_rect(),
    };
    let outer_size = match request.geometry {
        ChildLayoutGeometry::RequestedOuter(_) => authored.size,
        ChildLayoutGeometry::ExplicitBorder(_) => Size {
            width: authored.size.width + m.horizontal(),
            height: authored.size.height + m.vertical(),
        },
    };
    let available = Size {
        width: (outer_size.width - m.horizontal()).max(0.0),
        height: (outer_size.height - m.vertical()).max(0.0),
    };
    let max = Size {
        width: (request.outer_constraints.max.width - m.horizontal())
            .max(0.0)
            .min(available.width),
        height: (request.outer_constraints.max.height - m.vertical())
            .max(0.0)
            .min(available.height),
    };
    let min = Size {
        width: (request.outer_constraints.min.width - m.horizontal())
            .max(0.0)
            .min(max.width),
        height: (request.outer_constraints.min.height - m.vertical())
            .max(0.0)
            .min(max.height),
    };
    let target = derive_target_box(available, request.spacing)?;
    Ok(PreparedChildLayout {
        input: LayoutInput {
            viewport: target.border,
            content: target.content,
            constraints: LayoutConstraints { min, max },
        },
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresentationGeometryIndex {
    root_target_rect: Rect,
    target_rects_by_address: HashMap<WidgetSlotAddress, Rect>,
    first_target_rects_by_id: HashMap<WidgetId, Rect>,
    boxes_by_address: HashMap<WidgetSlotAddress, PresentedBoxGeometry>,
    first_border_by_id: HashMap<WidgetId, Rect>,
    root: PresentedBoxGeometry,
}

impl PresentationGeometryIndex {
    pub fn from_layout(layout: &LayoutOutput) -> Self {
        let root_target_rect = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: layout.bounds.size,
        };
        let mut target_rects_by_address = HashMap::new();
        let mut first_target_rects_by_id = HashMap::new();
        let mut boxes_by_address = HashMap::new();
        let mut first_border_by_id = HashMap::new();
        let root_target =
            derive_target_box(layout.bounds.size, BoxSpacing::ZERO).expect("zero spacing is valid");
        let root = PresentedBoxGeometry {
            outer: root_target.border.into_rect(),
            border: root_target.border.into_rect(),
            content: root_target.content.into_rect(),
            default_clip: root_target.default_clip.into_rect(),
        };

        for placement in &layout.child_placements {
            #[cfg(test)]
            FROM_LAYOUT_PLACEMENT_VISITS.with(|visits| visits.set(visits.get() + 1));
            let rect = placement.bounds.into_rect();
            if let Some(address) = placement.local_state_slot.as_ref() {
                target_rects_by_address.insert(address.clone(), rect);
            }
            first_target_rects_by_id
                .entry(placement.child.clone())
                .or_insert(rect);
            if let Ok(placement_box) = derive_placement_box(placement.bounds, placement.spacing) {
                let presented = PresentedBoxGeometry {
                    outer: placement_box.outer.into_rect(),
                    border: rect,
                    content: placement_box.content.into_rect(),
                    default_clip: rect,
                };
                if let Some(address) = placement.local_state_slot.as_ref() {
                    boxes_by_address.insert(address.clone(), presented);
                }
                first_border_by_id
                    .entry(placement.child.clone())
                    .or_insert(rect);
            }
        }

        Self {
            root_target_rect,
            target_rects_by_address,
            first_target_rects_by_id,
            boxes_by_address,
            first_border_by_id,
            root,
        }
    }

    pub fn target_rect_for_region_address(
        &self,
        target: &WidgetId,
        address: Option<&WidgetSlotAddress>,
    ) -> Rect {
        if let Some(address) = address
            && let Some(rect) = self.target_rects_by_address.get(address)
        {
            return *rect;
        }

        self.first_target_rects_by_id
            .get(target)
            .copied()
            .unwrap_or(self.root_target_rect)
    }

    pub fn target_local_point_for_region_address(
        &self,
        target: &WidgetId,
        address: Option<&WidgetSlotAddress>,
        root_local_position: Point,
    ) -> Point {
        let target_rect = self.target_rect_for_region_address(target, address);
        Point {
            x: root_local_position.x - target_rect.origin.x,
            y: root_local_position.y - target_rect.origin.y,
        }
    }

    pub fn region_root_local_rect(
        &self,
        target: &WidgetId,
        address: Option<&WidgetSlotAddress>,
        target_local_rect: Rect,
    ) -> Rect {
        let target_rect = self.target_rect_for_region_address(target, address);
        Rect {
            origin: Point {
                x: target_rect.origin.x + target_local_rect.origin.x,
                y: target_rect.origin.y + target_local_rect.origin.y,
            },
            size: target_local_rect.size,
        }
    }

    pub fn region_contains_root_local_point(
        &self,
        target: &WidgetId,
        address: Option<&WidgetSlotAddress>,
        target_local_rect: Rect,
        root_local_position: Point,
    ) -> bool {
        let target_local_position =
            self.target_local_point_for_region_address(target, address, root_local_position);
        rect_contains_point(target_local_rect, target_local_position)
    }

    pub fn box_for_address(&self, address: &WidgetSlotAddress) -> Option<&PresentedBoxGeometry> {
        self.boxes_by_address.get(address)
    }
    pub fn first_border_for_id(&self, id: &WidgetId) -> Option<Rect> {
        self.first_border_by_id.get(id).copied()
    }
    pub fn root_box(&self) -> PresentedBoxGeometry {
        self.root
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedPresentationGeometry {
    pub index: PresentationGeometryIndex,
    capture: Option<PreparedGeometryCapture>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GeometryCaptureIntent {
    None,
    RenderPacketEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedGeometryCapture {
    records: Box<[PreparedGeometryRecord]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreparedGeometryRecord {
    pub target: WidgetId,
    pub address: WidgetSlotAddress,
    pub spacing: BoxSpacing,
    pub target_local: TargetBoxGeometry,
    pub parent_local: PlacementBoxGeometry,
    pub unscrolled_root: PresentedBoxGeometry,
    pub scroll_translation: Translation,
    pub overlay_translation: Translation,
    pub final_presented: PresentedBoxGeometry,
    pub authored_overflow: Option<TargetLocalRect>,
    pub ancestor_clip_final: Option<Rect>,
    pub effective_clip_final: Rect,
}

impl PreparedPresentationGeometry {
    pub fn captured_records(&self) -> Option<&[PreparedGeometryRecord]> {
        self.capture
            .as_ref()
            .map(|capture| capture.records.as_ref())
    }
}

pub const PREPARED_GEOMETRY_REQUIRED: &str =
    concat!("view_contract.", "prepared_geometry_required");

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SizePolicy {
    Fixed(f32),
    Fill { weight: f32 },
    FitContent,
    MinContent,
    MaxContent,
    Clamp { min: f32, ideal: f32, max: f32 },
    Fraction(f32),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SizePolicyDeclaration {
    pub target: WidgetId,
    pub width: SizePolicy,
    pub height: SizePolicy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct IntrinsicSize {
    pub target: WidgetId,
    pub min_content: Size,
    pub max_content: Size,
    pub preferred: Size,
    pub baseline: Option<f32>,
    pub aspect_ratio: Option<f32>,
    pub wrap_affects_size: bool,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResizePriority {
    Low,
    Normal,
    High,
    Required,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ResizeAxisPolicy {
    pub can_grow: bool,
    pub can_shrink: bool,
    pub priority: ResizePriority,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ResizePolicyDeclaration {
    pub target: WidgetId,
    pub horizontal: ResizeAxisPolicy,
    pub vertical: ResizeAxisPolicy,
    pub preserve_aspect_ratio: bool,
    pub minimum_preserved_size: Option<Size>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverflowBehavior {
    Visible,
    Hidden,
    Clip,
    Scroll,
    Wrap,
    Ellipsis,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OverflowPolicyDeclaration {
    pub target: WidgetId,
    pub x: OverflowBehavior,
    pub y: OverflowBehavior,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResponsiveVariant {
    pub target: WidgetId,
    pub key: String,
    pub active_breakpoints: Vec<String>,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextLineMode {
    SingleLine,
    MultiLine,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextWrapMode {
    NoWrap,
    Word,
    Character,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextFlowPolicy {
    pub target: WidgetId,
    pub line_mode: TextLineMode,
    pub wrap: TextWrapMode,
    pub line_clamp: Option<usize>,
    pub allow_ellipsis: bool,
    pub baseline: Option<f32>,
    pub caret_bounds: Vec<Rect>,
    pub viewport: Option<TextViewport>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayerPolicy {
    pub target: WidgetId,
    pub z_index: i32,
    pub clip_root: Option<WidgetId>,
    pub transform_root: Option<WidgetId>,
    pub paint_containment: bool,
    pub hit_test_containment: bool,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScrollAxes {
    pub horizontal: bool,
    pub vertical: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollConsumptionPolicy {
    pub wheel: bool,
    pub drag: bool,
    pub keyboard: bool,
    pub programmatic: bool,
}

impl ScrollConsumptionPolicy {
    pub fn passive() -> Self {
        Self {
            wheel: false,
            drag: false,
            keyboard: false,
            programmatic: false,
        }
    }

    pub fn exclusive_wheel() -> Self {
        Self {
            wheel: true,
            drag: false,
            keyboard: false,
            programmatic: false,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollInputKind {
    Wheel,
    Drag,
    Keyboard,
    Programmatic,
    Custom,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScrollDeltaConsumption {
    None,
    Partial,
    Complete,
}

/// Wheel-routing mode carried by [`ScrollRegionDeclaration::wheel_routing`],
/// snapshotted at declaration time from [`SlipwayWheelRoutingPolicy`].
///
/// Selection precedence (total and deterministic, ADR-0002 B2): only
/// enabled regions that contain the point AND can still consume the delta
/// are candidates — a region at its scroll limit drops out, so at-limit
/// chaining (inner -> outer -> page, reclaimed by the inner as soon as it
/// has travel again) runs per event regardless of mode and no declared
/// preference can black-hole the wheel. Full contract, recipes, and the
/// `dispatch_graph` inspection probe:
/// `docs/public/api/routing-and-scroll.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelRouting {
    /// If this region is under the cursor and can consume the delta, it
    /// wins outright — even over a fronter candidate that would win by
    /// [`HitRegionOrder`]. Displacement warning: a `SelfFirst` outer
    /// consumes wheels aimed at its inner panels and scrolls them out from
    /// under the cursor (the Step-194 authoring that the Step-200
    /// architect UX decision reverted). Author it only when the outer
    /// surface must own the wheel.
    SelfFirst,
    /// Defers to the front-most eligible ancestor candidate (one whose
    /// viewport strictly contains this region's viewport); with no
    /// eligible ancestor it consumes normally.
    ParentFirst,
    /// LOAD-BEARING default: the front-most containing consumable region
    /// by [`HitRegionOrder`] wins, so the region the user points at
    /// scrolls first and hands off outward only at its limit. Use this
    /// unless a UX decision says otherwise.
    NearestScrollable,
    /// RESERVED. Currently routes exactly like
    /// [`WheelRouting::NearestScrollable`]: the ADR-0002 B2 selector
    /// applies no override for `Custom`, so authoring it changes nothing
    /// today. Reserved for a future custom-routing contract — do not
    /// author it expecting distinct semantics.
    Custom,
}

/// Scroll-indicator (scrollbar) presentation mode carried by
/// [`ScrollRegionDeclaration::indicator`]. WHEN: set it via
/// [`ScrollRegionDeclaration::with_scroll_indicator`] on the declaration a
/// capability helper returned — leave it unset (`Auto`) unless a UX
/// decision needs an explicit indicator state. LOAD-BEARING on both
/// visible backends; the authored-controls principle (ADR-0002): explicit
/// when declared, automatic when unspecified, default byte-identical.
/// Failure mode: none at admission — an indicator mode is presentation
/// intent, not geometry, so no `view_contract.*` code guards it; pin the
/// declared mode with a declaration-level test (the reference example's
/// `nested_inner_indicator_modes_are_declared`). Full per-backend
/// semantics: `docs/public/api/routing-and-scroll.md`
/// ("Scroll Indicators").
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollIndicatorMode {
    /// Backend-automatic (the default every helper produces): each visible
    /// backend keeps its historical indicator conditions, byte-identical
    /// to the pre-control behavior. Both backends draw an indicator for an
    /// enabled, vertically overflowing region they present themselves;
    /// iced additionally leaves native-backed regions (real mounted child
    /// placements) to the native scrollable's own scrollbar.
    #[default]
    Auto,
    /// Never present an indicator for this region. On iced this also
    /// suppresses the NATIVE scrollable's scrollbar (zero-width rail) for
    /// native-backed regions; wheel routing and scrolling are unaffected —
    /// this is a visual control only.
    Hidden,
    /// Present an indicator whenever geometrically sensible (vertical axis
    /// enabled and content taller than the viewport), overriding the
    /// backend's `Auto`-only eligibility conditions. On an iced
    /// NATIVE-backed region the native scrollable's own scrollbar is the
    /// indicator — no second, synthesized one is drawn over it.
    Visible,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollSnapPoint {
    pub target: WidgetId,
    pub position: Point,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollPolicy {
    pub target: WidgetId,
    pub region_id: Option<PresentationRegionId>,
    pub address: Option<WidgetSlotAddress>,
    pub axes: ScrollAxes,
    pub extent: Size,
    pub viewport: Rect,
    pub content_bounds: Rect,
    pub offset: Point,
    pub snap_points: Vec<ScrollSnapPoint>,
    pub wheel_routing: WheelRouting,
    pub consumption: ScrollConsumptionPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ItemRange {
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VirtualizationHint {
    None,
    Preferred,
    Required,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionPolicy {
    pub target: WidgetId,
    pub item_count: usize,
    pub row_count: Option<usize>,
    pub column_count: Option<usize>,
    pub visible_rows: Option<ItemRange>,
    pub visible_columns: Option<ItemRange>,
    pub selected_items: Vec<WidgetId>,
    pub virtualization: VirtualizationHint,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InteractionState {
    Normal,
    Hover,
    Focus,
    Active,
    Disabled,
    Selected,
    Checked,
    Expanded,
    Invalid,
    Loading,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct InteractionStateStyle {
    pub target: WidgetId,
    pub state: InteractionState,
    pub style_key: String,
    pub paint: Vec<PaintOp>,
    pub size_policy: Option<SizePolicyDeclaration>,
    pub overflow_policy: Option<OverflowPolicyDeclaration>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShapeKind {
    Rectangle,
    RoundedRectangle,
    Circle,
    Line,
    Path,
    Text,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub red: f32,
    pub green: f32,
    pub blue: f32,
    pub alpha: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PathCommand {
    MoveTo(Point),
    LineTo(Point),
    QuadraticTo {
        control: Point,
        to: Point,
    },
    CubicTo {
        control_1: Point,
        control_2: Point,
        to: Point,
    },
    Close,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathDeclaration {
    pub commands: Vec<PathCommand>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipDeclaration {
    pub id: Option<String>,
    pub bounds: Rect,
    pub path: Option<PathDeclaration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShapeDeclaration {
    pub id: Option<String>,
    pub kind: ShapeKind,
    pub bounds: Rect,
    pub path: Option<PathDeclaration>,
    pub clip: Option<ClipDeclaration>,
}

pub const DEFAULT_TEXT_FONT_FAMILY: &str = "system-ui";
pub const DEFAULT_TEXT_FONT_SIZE: f32 = 14.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontWeight {
    Normal,
    Bold,
    Weight(u16),
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FontStyle {
    Normal,
    Italic,
}

impl Default for FontStyle {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TextDecoration {
    pub underline: bool,
    pub strikethrough: bool,
}

impl TextDecoration {
    pub fn none() -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BaselineShift {
    Normal,
    Superscript,
    Subscript,
}

impl Default for BaselineShift {
    fn default() -> Self {
        Self::Normal
    }
}

/// Horizontal anchoring of painted text INSIDE [`PaintOp::Text`]'s
/// `bounds`, carried by [`TextStyle::align_x`]. WHEN: set it via
/// [`TextStyle::with_align_x`] (or [`TextStyle::centered`] for the
/// button-label pattern) — leave it unset (`Start`) unless a UX decision
/// needs a non-left anchor. LOAD-BEARING on both visible backends; the
/// authored-controls principle (ADR-0002): explicit when declared,
/// automatic when unspecified, default byte-identical. Alignment moves the
/// laid-out text WITHIN the declared rect — it never grows the rect, and
/// wrapping still happens at the rect width first (word wrap is currently
/// hardcoded per backend). Failure mode: none at admission — alignment is
/// presentation intent, not geometry, so no `view_contract.*` code guards
/// it; pin the declared value with a declaration-level test (the reference
/// example's `overlay_roam_title_declares_centered_alignment`). Contract:
/// `docs/public/api/backends.md` ("Text Wrap and Alignment").
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextAlignX {
    /// Anchor the text at the left edge of `bounds` (the default):
    /// byte-identical to the historical hardcoded left/top presentation.
    #[default]
    Start,
    /// Center the laid-out text horizontally within `bounds`; wrapped
    /// lines are centered per line on both visible backends.
    Center,
    /// Anchor the text at the right edge of `bounds`; wrapped lines are
    /// right-aligned per line on both visible backends.
    End,
}

/// Vertical anchoring of painted text INSIDE [`PaintOp::Text`]'s
/// `bounds`, carried by [`TextStyle::align_y`]. WHEN: set it via
/// [`TextStyle::with_align_y`] (or [`TextStyle::centered`]) — leave it
/// unset (`Top`) unless a UX decision needs a non-top anchor.
/// LOAD-BEARING on both visible backends; default byte-identical
/// (ADR-0002 authored-controls principle). The whole laid-out text BLOCK
/// (all wrapped lines) is anchored; text taller than `bounds` is still
/// clipped to the rect exactly as before. Failure mode: none at
/// admission — presentation intent, not geometry; pin declared values
/// with a declaration-level test. Contract:
/// `docs/public/api/backends.md` ("Text Wrap and Alignment").
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextAlignY {
    /// Anchor the text block at the top edge of `bounds` (the default):
    /// byte-identical to the historical hardcoded left/top presentation.
    #[default]
    Top,
    /// Center the laid-out text block vertically within `bounds`.
    Center,
    /// Anchor the text block at the bottom edge of `bounds`.
    Bottom,
}

/// Per-op soft-wrap declaration for painted text INSIDE
/// [`PaintOp::Text`]'s `bounds`, carried by [`TextStyle::wrap`]. WHEN:
/// declare [`TextWrap::None`] via [`TextStyle::with_wrap`] (or the
/// [`TextStyle::no_wrap`] convenience) when a label must stay on ONE line
/// — CJK headers, tabs, badges — instead of word-wrapping at the rect
/// width (audit NC-4: the consumer fought forced CJK wrapping through
/// eight artifact rounds because this opt-out did not exist). Unset =
/// [`TextWrap::Word`], byte-identical to the historical hardcoded word
/// wrap. LOAD-BEARING on both visible backends and the canonical debug
/// renderer. The two modes are the honest parity-equivalent subset of
/// the backends' native wrap options (iced `Wrapping`, egui
/// `TextWrapping`); glyph-level wrapping is deliberately not exposed
/// (ADR-0004). In BOTH modes an explicit `\n` still breaks the line and
/// clipping to the rect is unchanged — a `None` line wider than `bounds`
/// clips at the rect edge around the declared [`TextAlignX`] anchor.
/// Failure mode: none at admission — presentation intent, not geometry;
/// pin declared values with a declaration-level test. Contract:
/// `docs/public/api/backends.md` ("Text Wrap and Alignment").
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum TextWrap {
    /// Word-wrap at the rect width (the default): byte-identical to the
    /// historical hardcoded `Wrapping::Word` presentation.
    #[default]
    Word,
    /// No soft wrapping: the text lays out as one line per explicit `\n`
    /// and clips at the rect edge (single-line contract on both visible
    /// backends and the debug renderer).
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub decoration: TextDecoration,
    pub baseline: BaselineShift,
    /// Horizontal anchoring within the op's `bounds`; `Start` = the
    /// historical left anchoring. See [`TextAlignX`].
    pub align_x: TextAlignX,
    /// Vertical anchoring within the op's `bounds`; `Top` = the
    /// historical top anchoring. See [`TextAlignY`].
    pub align_y: TextAlignY,
    /// Soft-wrap mode within the op's `bounds`; `Word` = the historical
    /// hardcoded word wrap at the rect width. See [`TextWrap`].
    pub wrap: TextWrap,
}

impl TextStyle {
    pub fn new(font_family: impl Into<String>, font_size: f32) -> Self {
        Self {
            font_family: font_family.into(),
            font_size,
            font_weight: FontWeight::default(),
            font_style: FontStyle::default(),
            decoration: TextDecoration::default(),
            baseline: BaselineShift::default(),
            align_x: TextAlignX::default(),
            align_y: TextAlignY::default(),
            wrap: TextWrap::default(),
        }
    }

    pub fn plain() -> Self {
        Self::new(DEFAULT_TEXT_FONT_FAMILY, DEFAULT_TEXT_FONT_SIZE)
    }

    pub fn with_font_family(mut self, font_family: impl Into<String>) -> Self {
        self.font_family = font_family.into();
        self
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    pub fn with_font_weight(mut self, font_weight: FontWeight) -> Self {
        self.font_weight = font_weight;
        self
    }

    pub fn with_font_style(mut self, font_style: FontStyle) -> Self {
        self.font_style = font_style;
        self
    }

    pub fn with_decoration(mut self, decoration: TextDecoration) -> Self {
        self.decoration = decoration;
        self
    }

    pub fn with_baseline(mut self, baseline: BaselineShift) -> Self {
        self.baseline = baseline;
        self
    }

    /// Declare the horizontal anchoring of the text within the op's
    /// `bounds`. WHEN: a label must not sit at the left edge (tabs,
    /// button captions, numeric columns). Unset = [`TextAlignX::Start`],
    /// byte-identical to the pre-alignment presentation. See
    /// [`TextAlignX`] for the per-variant contract.
    pub fn with_align_x(mut self, align_x: TextAlignX) -> Self {
        self.align_x = align_x;
        self
    }

    /// Declare the vertical anchoring of the text within the op's
    /// `bounds`. WHEN: a label must not hug the top edge (title bars,
    /// badges). Unset = [`TextAlignY::Top`], byte-identical to the
    /// pre-alignment presentation. See [`TextAlignY`] for the
    /// per-variant contract.
    pub fn with_align_y(mut self, align_y: TextAlignY) -> Self {
        self.align_y = align_y;
        self
    }

    /// The button-label pattern: center the text BOTH ways within the
    /// op's `bounds` (`align_x: Center, align_y: Center` in one call).
    /// Declare the FULL control rect as the text op's `bounds` and let
    /// the backends center inside it — do not hand-compute insets from
    /// estimated glyph widths (the NC-14 anti-pattern). Contract:
    /// `docs/public/api/backends.md` ("Text Wrap and Alignment").
    pub fn centered(self) -> Self {
        self.with_align_x(TextAlignX::Center)
            .with_align_y(TextAlignY::Center)
    }

    /// Declare the soft-wrap mode within the op's `bounds`. WHEN: a
    /// label must not word-wrap at the rect width (CJK headers, tabs,
    /// badges — pass [`TextWrap::None`]). Unset = [`TextWrap::Word`],
    /// byte-identical to the historical hardcoded word wrap. See
    /// [`TextWrap`] for the per-variant contract.
    pub fn with_wrap(mut self, wrap: TextWrap) -> Self {
        self.wrap = wrap;
        self
    }

    /// The single-line convenience: `with_wrap(TextWrap::None)`. The
    /// text lays out as one line (explicit `\n` still breaks) and clips
    /// at the rect edge around the declared alignment anchor. Contract:
    /// `docs/public/api/backends.md` ("Text Wrap and Alignment").
    pub fn no_wrap(self) -> Self {
        self.with_wrap(TextWrap::None)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AutoLayoutRequirement {
    NotNeeded,
    Optional,
    Required,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LayoutMeasurementDependency {
    AuthoredStaticFacts,
    AuthoredState,
    ChildLayoutResults,
    BackendWrappedTextMetrics,
    ExplicitManualBounds,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct AutoLayoutPolicyDeclaration {
    pub target: WidgetId,
    pub horizontal: AutoLayoutRequirement,
    pub vertical: AutoLayoutRequirement,
    pub dependencies: Vec<LayoutMeasurementDependency>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextMeasurementPurpose {
    IntrinsicSize,
    OverflowDetection,
    CaretGeometry,
    SelectionGeometry,
    LineWrapping,
    BaselineAlignment,
    Custom(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CacheRevisionToken {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextMeasurementCacheKey {
    pub namespace: String,
    pub key: String,
    pub revisions: Vec<CacheRevisionToken>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextMeasurementCacheScope {
    None,
    WidgetLocal,
    AppLocal,
    BackendSession,
    Frame,
    Custom(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextMeasurementCacheReuse {
    Never,
    SameFrame,
    UntilRevisionChange,
    AuthorManaged,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasurementCachePolicyDeclaration {
    pub target: WidgetId,
    pub request_id: String,
    pub key: TextMeasurementCacheKey,
    pub scope: TextMeasurementCacheScope,
    pub reuse: TextMeasurementCacheReuse,
    pub required: bool,
    pub invalidates_on: Vec<CacheRevisionToken>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TextMetricSourceKind {
    OfficialBackendApi,
    OfficialPlatformApi,
    UserProvidedOfficialWrapper,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextMetricSource {
    pub provider_id: String,
    pub backend_id: Option<String>,
    pub api_name: String,
    pub kind: TextMetricSourceKind,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasurementRequest {
    pub target: WidgetId,
    pub request_id: String,
    pub content: String,
    pub style: TextStyle,
    pub available_bounds: Option<Rect>,
    pub flow: Option<TextFlowPolicy>,
    pub purposes: Vec<TextMeasurementPurpose>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasurementFacts {
    pub measured_size: Size,
    pub content_bounds: Rect,
    pub baseline: Option<f32>,
    pub line_count: Option<usize>,
    pub caret_bounds: Vec<Rect>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum InvalidTextMeasurementReason {
    HeuristicEstimate,
    DebugPlaceholderRaster,
    HardCodedCharacterWidth,
    BackendNativeTypeLeaked,
    MissingOfficialSource,
    UnsupportedByBackend,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ValidTextMeasurement {
    pub request: TextMeasurementRequest,
    pub source: TextMetricSource,
    pub facts: TextMeasurementFacts,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextMeasurementReceipt {
    Valid(ValidTextMeasurement),
    Invalid {
        request: TextMeasurementRequest,
        reason: InvalidTextMeasurementReason,
        diagnostics: Vec<Diagnostic>,
    },
    Unsupported {
        request: TextMeasurementRequest,
        diagnostics: Vec<Diagnostic>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasurementPolicyDeclaration {
    pub target: WidgetId,
    pub required: bool,
    pub purposes: Vec<TextMeasurementPurpose>,
    pub requests: Vec<TextMeasurementRequest>,
    pub cache_policies: Vec<TextMeasurementCachePolicyDeclaration>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextMeasurementCacheStatus {
    Hit,
    Miss,
    Stored,
    Bypassed,
    Invalidated,
    Unsupported,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasurementCacheEvidence {
    pub target: WidgetId,
    pub request_id: String,
    pub key: Option<TextMeasurementCacheKey>,
    pub status: TextMeasurementCacheStatus,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TextMeasurementCacheLookup {
    Hit {
        receipt: TextMeasurementReceipt,
        evidence: TextMeasurementCacheEvidence,
    },
    Miss {
        evidence: TextMeasurementCacheEvidence,
    },
    Bypassed {
        evidence: TextMeasurementCacheEvidence,
    },
    Unsupported {
        evidence: TextMeasurementCacheEvidence,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextMeasurementEvidence {
    pub target: WidgetId,
    pub policy: TextMeasurementPolicyDeclaration,
    pub receipts: Vec<TextMeasurementReceipt>,
    pub cache: Vec<TextMeasurementCacheEvidence>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AxisFitStatus {
    Fits,
    Overflow { amount: f32 },
    Underflow { amount: f32 },
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AxisFitEvidence {
    pub available: f32,
    pub measured: f32,
    pub status: AxisFitStatus,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FitOverflowEvidence {
    pub target: WidgetId,
    pub bounds: Rect,
    pub measured_content: Option<Size>,
    pub horizontal: AxisFitEvidence,
    pub vertical: AxisFitEvidence,
    pub measurement_request_ids: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

pub trait SlipwayTextMetricProvider {
    fn text_metric_source(&self) -> TextMetricSource;

    fn measure_text(&mut self, request: TextMeasurementRequest) -> TextMeasurementReceipt;
}

pub trait SlipwayTextMeasurementCache {
    fn lookup_text_measurement(
        &mut self,
        policy: &TextMeasurementCachePolicyDeclaration,
    ) -> TextMeasurementCacheLookup;

    fn store_text_measurement(
        &mut self,
        policy: &TextMeasurementCachePolicyDeclaration,
        receipt: &TextMeasurementReceipt,
    ) -> TextMeasurementCacheEvidence;
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PaintLayerKey {
    pub z_index: i32,
    pub order: Option<usize>,
}

impl PaintLayerKey {
    pub fn new(z_index: i32) -> Self {
        Self {
            z_index,
            order: None,
        }
    }

    pub fn ordered(z_index: i32, order: usize) -> Self {
        Self {
            z_index,
            order: Some(order),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PaintInputTransparency {
    #[default]
    Opaque,
    PassThrough,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PaintOp {
    Fill {
        shape: ShapeDeclaration,
        color: Color,
    },
    Stroke {
        shape: ShapeDeclaration,
        color: Color,
        width: f32,
    },
    Text {
        bounds: Rect,
        content: String,
        color: Color,
        style: TextStyle,
    },
    Group {
        id: Option<String>,
        clip: Option<ClipDeclaration>,
        ops: Vec<PaintOp>,
    },
    Layer {
        id: Option<String>,
        key: PaintLayerKey,
        input_transparency: PaintInputTransparency,
        /// Explicit wheel-channel transparency for this layer.
        ///
        /// `None` = automatic: the wheel follows `input_transparency` exactly
        /// (an `Opaque` layer blocks the wheel, a `PassThrough` layer passes
        /// it), preserving the historical single-axis behavior. `Some(_)`
        /// declares the wheel channel independently of pointer occlusion, so an
        /// author can keep a layer pointer-`Opaque` while letting the wheel
        /// reach the scroll region behind it (`Some(PassThrough)`). Resolve via
        /// [`paint_layer_blocks_wheel`].
        wheel_transparency: Option<PaintInputTransparency>,
        clip: Option<ClipDeclaration>,
        ops: Vec<PaintOp>,
    },
}

/// Resolve whether an occlusion derived from a `PaintOp::Layer` blocks the
/// wheel channel, given the layer's pointer `input_transparency` and its
/// optional explicit `wheel_transparency`.
///
/// `wheel_transparency == None` means automatic: the wheel follows the pointer
/// occlusion (`Opaque` blocks, `PassThrough` passes). `Some(_)` overrides the
/// wheel channel independently, so a pointer-`Opaque` layer can still be
/// wheel-transparent (`Some(PassThrough)`). Pointer occlusion is unaffected.
pub fn paint_layer_blocks_wheel(
    input_transparency: PaintInputTransparency,
    wheel_transparency: Option<PaintInputTransparency>,
) -> bool {
    match wheel_transparency {
        Some(wheel_transparency) => wheel_transparency == PaintInputTransparency::Opaque,
        None => input_transparency == PaintInputTransparency::Opaque,
    }
}

impl PaintOp {
    pub fn styled_text(
        bounds: Rect,
        content: impl Into<String>,
        color: Color,
        style: TextStyle,
    ) -> Self {
        Self::Text {
            bounds,
            content: content.into(),
            color,
            style,
        }
    }

    pub fn keyed_layer(key: PaintLayerKey, ops: Vec<PaintOp>) -> Self {
        Self::Layer {
            id: None,
            key,
            input_transparency: PaintInputTransparency::Opaque,
            wheel_transparency: None,
            clip: None,
            ops,
        }
    }

    pub fn keyed_layer_pass_through(key: PaintLayerKey, ops: Vec<PaintOp>) -> Self {
        Self::keyed_layer(key, ops).with_input_transparency(PaintInputTransparency::PassThrough)
    }

    pub fn with_input_transparency(mut self, input_transparency: PaintInputTransparency) -> Self {
        if let Self::Layer {
            input_transparency: current,
            ..
        } = &mut self
        {
            *current = input_transparency;
        }
        self
    }

    /// Explicitly declare this layer's wheel-channel transparency, independent
    /// of its pointer `input_transparency`.
    ///
    /// Leaving it unset keeps the automatic behavior (wheel follows pointer
    /// occlusion). Setting `PassThrough` on a pointer-`Opaque` layer lets the
    /// wheel reach the scroll region behind while the body still occludes
    /// clicks; setting `Opaque` forces the wheel to be blocked even on a
    /// pointer-`PassThrough` layer. See [`paint_layer_blocks_wheel`].
    pub fn with_wheel_transparency(mut self, wheel_transparency: PaintInputTransparency) -> Self {
        if let Self::Layer {
            wheel_transparency: current,
            ..
        } = &mut self
        {
            *current = Some(wheel_transparency);
        }
        self
    }

    pub fn with_layer_id(mut self, id: impl Into<String>) -> Self {
        if let Self::Layer { id: current, .. } = &mut self {
            *current = Some(id.into());
        }
        self
    }

    pub fn with_layer_clip(mut self, clip: ClipDeclaration) -> Self {
        if let Self::Layer { clip: current, .. } = &mut self {
            *current = Some(clip);
        }
        self
    }
}

fn translate_point(point: Point, offset: Point) -> Point {
    Point {
        x: point.x + offset.x,
        y: point.y + offset.y,
    }
}

fn translate_rect(rect: impl Into<Rect>, offset: Point) -> Rect {
    let mut rect = rect.into();
    rect.origin = translate_point(rect.origin, offset);
    rect
}

fn translate_path(mut path: PathDeclaration, offset: Point) -> PathDeclaration {
    for command in &mut path.commands {
        match command {
            PathCommand::MoveTo(point) | PathCommand::LineTo(point) => {
                *point = translate_point(*point, offset);
            }
            PathCommand::QuadraticTo { control, to } => {
                *control = translate_point(*control, offset);
                *to = translate_point(*to, offset);
            }
            PathCommand::CubicTo {
                control_1,
                control_2,
                to,
            } => {
                *control_1 = translate_point(*control_1, offset);
                *control_2 = translate_point(*control_2, offset);
                *to = translate_point(*to, offset);
            }
            PathCommand::Close => {}
        }
    }
    path
}

fn translate_clip(mut clip: ClipDeclaration, offset: Point) -> ClipDeclaration {
    clip.bounds = translate_rect(clip.bounds, offset);
    clip.path = clip.path.map(|path| translate_path(path, offset));
    clip
}

fn translate_shape(mut shape: ShapeDeclaration, offset: Point) -> ShapeDeclaration {
    shape.bounds = translate_rect(shape.bounds, offset);
    shape.path = shape.path.map(|path| translate_path(path, offset));
    shape.clip = shape.clip.map(|clip| translate_clip(clip, offset));
    shape
}

fn translate_paint_op(op: PaintOp, offset: Point) -> PaintOp {
    match op {
        PaintOp::Fill { shape, color } => PaintOp::Fill {
            shape: translate_shape(shape, offset),
            color,
        },
        PaintOp::Stroke {
            shape,
            color,
            width,
        } => PaintOp::Stroke {
            shape: translate_shape(shape, offset),
            color,
            width,
        },
        PaintOp::Text {
            bounds,
            content,
            color,
            style,
        } => PaintOp::Text {
            bounds: translate_rect(bounds, offset),
            content,
            color,
            style,
        },
        PaintOp::Group { id, clip, ops } => PaintOp::Group {
            id,
            clip: clip.map(|clip| translate_clip(clip, offset)),
            ops: ops
                .into_iter()
                .map(|op| translate_paint_op(op, offset))
                .collect(),
        },
        PaintOp::Layer {
            id,
            key,
            input_transparency,
            wheel_transparency,
            clip,
            ops,
        } => PaintOp::Layer {
            id,
            key,
            input_transparency,
            wheel_transparency,
            clip: clip.map(|clip| translate_clip(clip, offset)),
            ops: ops
                .into_iter()
                .map(|op| translate_paint_op(op, offset))
                .collect(),
        },
    }
}

fn mount_child_paint_ops(ops: Vec<PaintOp>, placement: impl Into<Rect>) -> Vec<PaintOp> {
    let offset = placement.into().origin;
    ops.into_iter()
        .map(|op| translate_paint_op(op, offset))
        .collect()
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    Pointer(PointerEvent),
    Keyboard(KeyboardEvent),
    Text(TextInputEvent),
    TextEdit(TextEditEvent),
    TextComposition(TextCompositionEvent),
    Selection(SelectionEvent),
    Wheel(WheelEvent),
    Scroll(ScrollEvent),
    Focus(FocusEvent),
    Command(CommandEvent),
    Clipboard(ClipboardEvent),
    DragDrop(DragDropEvent),
    File(FileInputEvent),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointerEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    /// Pointer position in the coordinate space selected by the matched
    /// presentation region.
    pub position: Point,
    /// Bounds of the target widget in target-local coordinates. This is not the
    /// bounds of the hit/focus region that produced the event.
    pub target_bounds: Option<TargetLocalRect>,
    pub kind: PointerEventKind,
    pub button: Option<PointerButton>,
    pub details: PointerDetails,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerEventKind {
    Move,
    Press,
    Release,
    Enter,
    Leave,
    Cancel,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Auxiliary,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerDeviceKind {
    Unknown,
    Mouse,
    Touch,
    Pen,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PointerButtons {
    pub primary: bool,
    pub secondary: bool,
    pub auxiliary: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerDetails {
    pub pointer_id: Option<u64>,
    pub device: PointerDeviceKind,
    pub buttons: PointerButtons,
    pub modifiers: Modifiers,
    pub pressure: Option<f32>,
    pub tilt_x: Option<f32>,
    pub tilt_y: Option<f32>,
    pub twist: Option<f32>,
}

impl Default for PointerDetails {
    fn default() -> Self {
        Self {
            pointer_id: None,
            device: PointerDeviceKind::Unknown,
            buttons: PointerButtons::default(),
            modifiers: Modifiers::default(),
            pressure: None,
            tilt_x: None,
            tilt_y: None,
            twist: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct KeyboardEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub key: String,
    pub kind: KeyEventKind,
    pub modifiers: Modifiers,
    pub details: KeyboardDetails,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyEventKind {
    Press,
    Release,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum KeyLocation {
    Standard,
    Left,
    Right,
    Numpad,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct KeyboardDetails {
    pub logical_key: Option<String>,
    pub physical_key: Option<String>,
    pub text: Option<String>,
    pub repeat: bool,
    pub location: KeyLocation,
}

impl Default for KeyboardDetails {
    fn default() -> Self {
        Self {
            logical_key: None,
            physical_key: None,
            text: None,
            repeat: false,
            location: KeyLocation::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextInputEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextSelectionRange {
    pub anchor: usize,
    pub focus: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CaretSet {
    pub carets: Vec<usize>,
    pub primary: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextEditKind {
    InsertText,
    DeleteBackward,
    DeleteForward,
    MoveCaret,
    ReplaceSelection,
    ReplaceBuffer,
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextEditEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub kind: TextEditKind,
    pub text: Option<String>,
    pub selection_before: Option<TextSelectionRange>,
    pub selection_after: Option<TextSelectionRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TextCompositionPhase {
    Start,
    Update,
    Commit,
    End,
    Cancel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextCompositionEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub phase: TextCompositionPhase,
    pub preedit_text: String,
    pub cursor_range: Option<TextSelectionRange>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextViewport {
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub visible_range: Option<TextSelectionRange>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SelectionMode {
    None,
    Single,
    Multiple,
    Range,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionState {
    pub target: WidgetId,
    pub mode: SelectionMode,
    pub ranges: Vec<TextSelectionRange>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectionEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub state: SelectionState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WheelEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub region_id: Option<PresentationRegionId>,
    pub delta_x: f32,
    pub delta_y: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub region_id: PresentationRegionId,
    pub offset_x: f32,
    pub offset_y: f32,
    pub viewport: TargetLocalRect,
    pub content_bounds: TargetLocalRect,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FocusEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub focused: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusTraversalMember {
    pub target: WidgetId,
    pub scope: Option<WidgetId>,
    pub tab_order: Option<i32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FocusTraversalInput {
    pub current: Option<WidgetId>,
    pub scope: Option<WidgetId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub command: String,
    pub payload_ref: Option<String>,
    pub source: Option<WidgetId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ClipboardEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub kind: ClipboardEventKind,
    pub formats: Vec<String>,
    pub payload_ref: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClipboardEventKind {
    Copy,
    Cut,
    Paste,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileDescriptor {
    pub name: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub payload_ref: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileInputEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub files: Vec<FileDescriptor>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DragDropPhase {
    Start,
    Update,
    Drop,
    Cancel,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DragDropEvent {
    pub target: WidgetId,
    pub target_slot: Option<WidgetSlotAddress>,
    pub phase: DragDropPhase,
    pub position: Point,
    pub payloads: Vec<TransferDescriptor>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TransferDescriptor {
    pub format: String,
    pub payload_ref: Option<String>,
    pub size_bytes: Option<u64>,
}

impl InputEvent {
    pub fn target(&self) -> &WidgetId {
        match self {
            Self::Pointer(event) => &event.target,
            Self::Keyboard(event) => &event.target,
            Self::Text(event) => &event.target,
            Self::TextEdit(event) => &event.target,
            Self::TextComposition(event) => &event.target,
            Self::Selection(event) => &event.target,
            Self::Wheel(event) => &event.target,
            Self::Scroll(event) => &event.target,
            Self::Focus(event) => &event.target,
            Self::Command(event) => &event.target,
            Self::Clipboard(event) => &event.target,
            Self::DragDrop(event) => &event.target,
            Self::File(event) => &event.target,
        }
    }

    pub fn target_slot(&self) -> Option<&WidgetSlotAddress> {
        match self {
            Self::Pointer(event) => event.target_slot.as_ref(),
            Self::Keyboard(event) => event.target_slot.as_ref(),
            Self::Text(event) => event.target_slot.as_ref(),
            Self::TextEdit(event) => event.target_slot.as_ref(),
            Self::TextComposition(event) => event.target_slot.as_ref(),
            Self::Selection(event) => event.target_slot.as_ref(),
            Self::Wheel(event) => event.target_slot.as_ref(),
            Self::Scroll(event) => event.target_slot.as_ref(),
            Self::Focus(event) => event.target_slot.as_ref(),
            Self::Command(event) => event.target_slot.as_ref(),
            Self::Clipboard(event) => event.target_slot.as_ref(),
            Self::DragDrop(event) => event.target_slot.as_ref(),
            Self::File(event) => event.target_slot.as_ref(),
        }
    }
}

/// The capability vocabulary a widget declares from
/// [`SlipwaySsot::capabilities`]. Declaring a capability adds obligations,
/// so declare exactly what the widget presents. Two consumers act on the
/// list: (1) the capability-aware admission pre-flight
/// ([`view_definition_contract_diagnostics_for_capabilities`]) — also run
/// by both visible backends at admission — refuses a view whose declared
/// input/presentation capability has no matching enabled region (the
/// LOAD-BEARING variants below name their refusal code); (2) backend
/// parity admission compares explicitly required capabilities against the
/// backend's [`BackendCapabilityReport`] and reports anything outside it
/// as [`UnsupportedCapabilityEvidence`].
///
/// Variants marked RESERVED are not consumed by any admission validator
/// today: they are declarative evidence for probes and parity negotiation
/// only. Status index: `docs/public/api/trait-surface.md`; refusal codes:
/// `docs/public/api/diagnostics.md`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Capability {
    /// LOAD-BEARING: requires an enabled hit region
    /// ([`hit_region_from_pointer_capability`]) or admission refuses with
    /// `view_contract.pointer_capability_missing_hit_region`.
    PointerInput,
    /// LOAD-BEARING: requires an enabled focus region
    /// ([`focus_region_from_focus_capability`]) or admission refuses with
    /// `view_contract.focus_capability_missing_focus_region`. Delivery
    /// caveat (NC-10): with only PLAIN focus regions, keyboard events are
    /// undeliverable on the iced visible backend (real ingress and
    /// physical control both reach text-edit focus regions only) —
    /// admission warns with
    /// `view_contract.keyboard_capability_plain_focus_delivery_limited`;
    /// see `docs/public/api/backends.md` ("Keyboard Delivery").
    KeyboardInput,
    /// LOAD-BEARING: requires an enabled text-edit focus region
    /// ([`text_edit_focus_region_from_capability`]) or admission refuses
    /// with `view_contract.text_input_missing_text_edit_focus_region`.
    TextInput,
    /// LOAD-BEARING: requires an enabled scroll region
    /// ([`scroll_region_from_scrollable_capability_with_order`]) or
    /// admission refuses with
    /// `view_contract.scroll_capability_missing_scroll_region`.
    WheelInput,
    /// LOAD-BEARING: same focus-region gate as
    /// [`Capability::KeyboardInput`].
    FocusInput,
    /// RESERVED: declared for [`SlipwayFocusTraversal`]; no admission
    /// validator consumes it.
    FocusTraversal,
    /// RESERVED: declared for [`SlipwaySemantics`]/semantic probes.
    SemanticObservation,
    /// RESERVED: [`SlipwayHitTesting`] is itself RESERVED; dispatch
    /// hit-testing runs on declared hit regions instead.
    HitTesting,
    /// RESERVED: capture behavior is authored per hit region via
    /// [`PointerCaptureIntent`], not gated by this variant.
    PointerCapture,
    /// LOAD-BEARING: same hit-region gate as [`Capability::PointerInput`].
    HitRegionPresentation,
    /// LOAD-BEARING: same focus-region gate as
    /// [`Capability::KeyboardInput`].
    FocusRegionPresentation,
    /// LOAD-BEARING: same text-edit gate as [`Capability::TextInput`].
    TextEditRegionPresentation,
    /// LOAD-BEARING: same scroll-region gate as
    /// [`Capability::WheelInput`].
    ScrollRegionPresentation,
    /// Backend-negotiated paint capability (shape/path/clip presentation);
    /// checked at backend parity admission, not by a core validator.
    ShapePathClipPresentation,
    /// Backend-negotiated resource capability (font installation); parity
    /// admission only.
    FontResourceInstallation,
    /// Backend-negotiated evidence capability (backend-presented input
    /// evidence); parity admission only.
    BackendPresentedEvidence,
    /// RESERVED: canonical/offscreen evidence declaration; no validator
    /// consumes it.
    CanonicalOffscreenEvidence,
    /// RESERVED: input family declared ahead of a consuming validator.
    ClipboardInput,
    /// RESERVED: input family declared ahead of a consuming validator.
    DragDropInput,
    /// RESERVED: input family declared ahead of a consuming validator.
    FileInput,
    /// RESERVED: [`SlipwayOverlayContracts`] is RESERVED; overlay z-order
    /// is enforced through [`PaintOrderDeclaration`] instead.
    Overlay,
    /// RESERVED: the command contract traits are RESERVED.
    CommandSurface,
    /// Backend-negotiated provider-surface capability; parity admission
    /// only.
    RenderSurface,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    IntrinsicSizing,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    SizePolicy,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    ResizePolicy,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    OverflowPolicy,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    ResponsiveVariants,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    TextFlowPolicy,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    LayerPolicy,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    ScrollPolicy,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    CollectionPolicy,
    /// RESERVED: mirrors the layout-intent trait of the same name.
    InteractionStateStyle,
    /// Backend-negotiated: marks participation in capability admission
    /// itself; backends attach blocking view-contract evidence under it.
    CapabilityAdmission,
    /// RESERVED: text-edit contracts are gated by
    /// [`Capability::TextInput`] instead.
    TextEditingPolicy,
    /// RESERVED: routing is enforced structurally on declared regions
    /// (the `view_contract.hit_route_*` codes), not via this variant.
    EventRoutingPolicy,
    /// RESERVED: mirrors the RESERVED policy trait of the same name.
    ContainerLayoutPolicy,
    /// RESERVED: [`SlipwayScrollBehaviorPolicy`] is consumed by the scroll
    /// helpers, but no validator consumes this variant.
    ScrollBehaviorPolicy,
    /// Backend-negotiated provider capability; parity admission only.
    ProviderSurfacePolicy,
    /// RESERVED: mirrors the RESERVED resolution policy traits.
    ResourceResolutionPolicy,
    /// RESERVED: mirrors the RESERVED deterministic-source policy traits.
    DeterministicSourcePolicy,
    /// RESERVED: mirrors the RESERVED command policy traits.
    CommandPolicy,
    /// Backend-negotiated: marks participation in backend parity
    /// negotiation; parity admission only.
    BackendCapabilityNegotiation,
    /// RESERVED: input family declared ahead of a consuming validator.
    CommandInput,
    /// Baseline declaration named by the [`SlipwayApp::capabilities`]
    /// default; no core validator gates it.
    Layout,
    /// Baseline declaration named by the [`SlipwayApp::capabilities`]
    /// default; no core validator gates it.
    Paint,
    /// Baseline declaration named by the [`SlipwayApp::capabilities`]
    /// default; no core validator gates it.
    StateObservation,
    /// Baseline declaration named by the [`SlipwayApp::capabilities`]
    /// default; no core validator gates it.
    ChildTraversal,
    /// Open extension point compared by exact string at backend parity
    /// admission only.
    Named(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiagnosticSeverity {
    Info,
    Warning,
    Unsupported,
    Error,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use = "push this Diagnostic into the surrounding declaration or outcome (e.g. ViewDefinition::diagnostics, EventOutcome::diagnostics); a dropped diagnostic disappears silently"]
pub struct Diagnostic {
    pub target: Option<WidgetId>,
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub message: String,
}

impl Diagnostic {
    pub fn unsupported(
        target: Option<WidgetId>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            target,
            severity: DiagnosticSeverity::Unsupported,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn warning(
        target: Option<WidgetId>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            target,
            severity: DiagnosticSeverity::Warning,
            code: code.into(),
            message: message.into(),
        }
    }

    pub fn error(
        target: Option<WidgetId>,
        code: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            target,
            severity: DiagnosticSeverity::Error,
            code: code.into(),
            message: message.into(),
        }
    }
}

pub const EVIDENCE_SOURCE_CANONICAL_OFFSCREEN: &str = "canonical_offscreen";
pub const EVIDENCE_SOURCE_BACKEND_PRESENTED: &str = "backend_presented";
pub const EVIDENCE_SOURCE_DEBUG_MCP: &str = "debug_mcp";
/// Source label for dispatch evidence SYNTHESIZED AFTER THE FACT to explain a
/// refused/no-match input (audit finding MF-H5). Evidence under this label
/// records what the declared dispatch state WOULD have resolved — it is
/// diagnosis, never proof that a dispatch was attempted, and the runtime's
/// backend-input contract check rejects it as physical-equivalent evidence
/// by construction (the expected label is `backend_presented`).
pub const EVIDENCE_SOURCE_POST_HOC_DIAGNOSIS: &str = "post_hoc_diagnosis";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvidenceSource {
    pub label: String,
    pub backend_id: Option<String>,
    pub provider_id: Option<String>,
    pub pass_id: Option<String>,
}

impl EvidenceSource {
    pub fn canonical_offscreen(provider_id: impl Into<String>) -> Self {
        Self {
            label: EVIDENCE_SOURCE_CANONICAL_OFFSCREEN.to_string(),
            backend_id: None,
            provider_id: Some(provider_id.into()),
            pass_id: None,
        }
    }

    pub fn backend_presented(backend_id: impl Into<String>, pass_id: impl Into<String>) -> Self {
        Self {
            label: EVIDENCE_SOURCE_BACKEND_PRESENTED.to_string(),
            backend_id: Some(backend_id.into()),
            provider_id: None,
            pass_id: Some(pass_id.into()),
        }
    }

    pub fn debug_mcp(pass_id: impl Into<String>) -> Self {
        Self {
            label: EVIDENCE_SOURCE_DEBUG_MCP.to_string(),
            backend_id: None,
            provider_id: Some("slipway-debug-mcp".to_string()),
            pass_id: Some(pass_id.into()),
        }
    }

    /// Post-hoc dispatch diagnosis source (see
    /// [`EVIDENCE_SOURCE_POST_HOC_DIAGNOSIS`]): attached to no-match debug
    /// failures so an agent can see WHY an input found no consumer, under a
    /// label that can never be confused with real dispatch evidence.
    pub fn post_hoc_diagnosis(backend_id: impl Into<String>, pass_id: impl Into<String>) -> Self {
        Self {
            label: EVIDENCE_SOURCE_POST_HOC_DIAGNOSIS.to_string(),
            backend_id: Some(backend_id.into()),
            provider_id: None,
            pass_id: Some(pass_id.into()),
        }
    }

    pub fn label(&self) -> &str {
        &self.label
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TopologyNode {
    pub id: WidgetId,
    pub children: Vec<TopologyNode>,
    pub local_state_slot: Option<WidgetSlotAddress>,
}

impl TopologyNode {
    pub fn leaf(id: WidgetId) -> Self {
        Self {
            id,
            children: Vec::new(),
            local_state_slot: None,
        }
    }

    pub fn traverse_depth_first(&self) -> ChildTraversal {
        let mut ids = Vec::new();
        self.push_depth_first(&mut ids);
        ChildTraversal {
            root: self.id.clone(),
            order: ids,
        }
    }

    fn push_depth_first(&self, ids: &mut Vec<WidgetId>) {
        ids.push(self.id.clone());
        for child in &self.children {
            child.push_depth_first(ids);
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChildTraversal {
    pub root: WidgetId,
    pub order: Vec<WidgetId>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticNode {
    pub id: WidgetId,
    pub role: String,
    pub label: Option<String>,
    pub value: Option<String>,
    pub bounds: Option<Rect>,
    pub states: Vec<SemanticState>,
    pub actions: Vec<SemanticAction>,
    pub relationships: Vec<SemanticRelationship>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticState {
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticAction {
    pub id: String,
    pub label: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SemanticRelationship {
    pub name: String,
    pub target: WidgetId,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HitTestInput {
    pub target: WidgetId,
    pub point: Point,
    pub pointer: PointerDetails,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HitTestOutput {
    pub target: Option<WidgetId>,
    pub local_point: Option<Point>,
    pub route: EventRoute,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventRoute {
    /// Debug/trace label only. Physical dispatch acceptance compares address,
    /// path, and phase; route_id is not a routing identity key.
    pub route_id: Option<String>,
    pub address: Option<WidgetSlotAddress>,
    pub path: Vec<WidgetId>,
    pub phase: EventRoutePhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventRoutePhase {
    Target,
    Capture,
    Bubble,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct EventDisposition {
    pub handled: bool,
    pub propagate: bool,
    pub default_action_allowed: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointerCaptureRequest {
    pub target: WidgetId,
    pub pointer_id: Option<u64>,
    pub phase: PointerCapturePhase,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerCapturePhase {
    Capture,
    Release,
}

/// Declares when a hit region captures the pointer, i.e. keeps receiving
/// pointer events after the cursor leaves its bounds. Authored as the
/// `capture` argument of [`hit_region_from_pointer_capability`] and
/// consumed per event by [`declared_pointer_capture_for_region`] (both
/// backends and the MCP physical-control path route captured moves/releases
/// through the capturing region). Capture outcomes are recorded as
/// [`PointerCaptureEvidence`]. Contract map: `docs/public/api/core.md`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerCaptureIntent {
    /// Never captures: events route by position only. The default for
    /// plain clickable regions.
    None,
    /// Captures on `Press` only: the press is delivered to this region
    /// even if an overlapping sibling would win the following move. Use
    /// for press-ack buttons (the example's Action card uses this).
    OnPress,
    /// Captures while a button is held, plus the terminating
    /// `Release`/`Cancel`: drag interactions keep streaming to this region
    /// after the cursor exits (the example's Slider and movable Overlay
    /// use this). Without it a fast drag drops out at the region edge.
    DuringDrag,
    /// Captures every pointer event kind unconditionally; use only for
    /// modal-grab surfaces that must own the pointer.
    Explicit,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointerCaptureEvidence {
    pub request: PointerCaptureRequest,
    pub accepted: bool,
    pub source: EvidenceSource,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollConsumptionEvidence {
    pub target: WidgetId,
    pub region_id: Option<PresentationRegionId>,
    pub input_kind: ScrollInputKind,
    pub requested_delta: Point,
    pub consumed_delta: Point,
    pub remaining_delta: Point,
    pub consumption: ScrollDeltaConsumption,
    pub source: EvidenceSource,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollState {
    pub target: WidgetId,
    pub region_id: Option<PresentationRegionId>,
    pub address: Option<WidgetSlotAddress>,
    pub offset_x: f32,
    pub offset_y: f32,
    pub axes: ScrollAxes,
    pub extent: Size,
    pub viewport: Rect,
    pub content_bounds: Rect,
    pub consumption: Vec<ScrollConsumptionEvidence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualViewportRange {
    pub target: WidgetId,
    pub region_id: Option<PresentationRegionId>,
    pub address: Option<WidgetSlotAddress>,
    pub row_range: Option<(usize, usize)>,
    pub column_range: Option<(usize, usize)>,
    pub estimated_extent: Size,
}

#[derive(Clone, Debug, PartialEq)]
pub struct OverlayDeclaration {
    pub id: WidgetId,
    pub owner: WidgetId,
    pub bounds: Rect,
    pub allowed_bounds: Option<Rect>,
    pub modality: OverlayModality,
    pub focus_scope: Option<WidgetId>,
    pub dismiss_commands: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OverlayModality {
    None,
    Modal,
    Blocking,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnchoredSurfaceDeclaration {
    pub id: WidgetId,
    pub owner: WidgetId,
    pub anchor: WidgetId,
    pub bounds: Rect,
    pub placement: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PortalDeclaration {
    pub id: WidgetId,
    pub logical_owner: WidgetId,
    pub render_parent: Option<WidgetId>,
    pub surface_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandDeclaration {
    pub id: String,
    pub label: String,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub shortcuts: Vec<ShortcutDeclaration>,
    pub scope: Option<WidgetId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutDeclaration {
    pub chord: String,
    pub command_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderSurfaceDeclaration {
    pub id: WidgetId,
    pub provider_id: String,
    pub bounds: Rect,
    pub payload_ref: Option<String>,
    pub dirty_regions: Vec<Rect>,
    pub capabilities: Vec<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FrameIdentity {
    pub surface_id: String,
    pub surface_instance_id: String,
    pub revision: u64,
    pub frame_index: u64,
    pub viewport: Rect,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CapabilityProfileKind {
    TextInput,
    ScrollableContainer,
    Popup,
    ProviderSurface,
    CommandSurface,
    DeterministicSource,
    BackendAdapter,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextBufferSnapshot {
    pub target: WidgetId,
    pub text: String,
    pub revision: Vec<CacheRevisionToken>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextSelectionPolicyDeclaration {
    pub target: WidgetId,
    pub selection: Option<TextSelectionRange>,
    pub carets: CaretSet,
    pub editable: bool,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImeCompositionPolicyDeclaration {
    pub target: WidgetId,
    pub active: bool,
    pub preedit_text: Option<String>,
    pub cursor_range: Option<TextSelectionRange>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CaretGeometryEvidence {
    pub target: WidgetId,
    pub caret_bounds: Vec<Rect>,
    pub selection_bounds: Vec<Rect>,
    pub measurement_request_ids: Vec<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TextEditCommandDeclaration {
    pub command_id: String,
    pub kind: TextEditKind,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TextUndoRedoEvidence {
    pub target: WidgetId,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_depth: Option<usize>,
    pub redo_depth: Option<usize>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventRoutingPolicyDeclaration {
    pub target: WidgetId,
    pub event_target: WidgetId,
    pub route: EventRoute,
    pub capture: Vec<PointerCaptureRequest>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventPropagationStage {
    Capture,
    Target,
    Bubble,
    AppReducer,
    StateMutation,
    RepaintRequest,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventPropagationStep {
    pub stage: EventPropagationStage,
    pub node: Option<WidgetId>,
    pub disposition: EventDisposition,
    pub emitted_messages: Vec<EmittedMessageEvidence>,
    pub changes: Vec<ChangeEvidence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventPropagationEvidence {
    pub target: WidgetId,
    pub event: InputEvent,
    pub steps: Vec<EventPropagationStep>,
    pub final_disposition: EventDisposition,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventHandlingDeclaration {
    pub routing: EventRoutingPolicyDeclaration,
    pub disposition: EventPropagationEvidence,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PointerCapturePolicyDeclaration {
    pub target: WidgetId,
    pub requests: Vec<PointerCaptureRequest>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugEventTracePolicyDeclaration {
    pub target: WidgetId,
    pub request_only: bool,
    pub include_route: bool,
    pub include_messages: bool,
    pub include_state_changes: bool,
    pub include_repaint_request: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContainerLayoutKind {
    Row,
    Column,
    Stack,
    Grid,
    Flow,
    Absolute,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContainerLayoutPolicyDeclaration {
    pub target: WidgetId,
    pub kind: ContainerLayoutKind,
    pub child_order: Vec<WidgetId>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildConstraintPolicyDeclaration {
    pub parent: WidgetId,
    pub child: WidgetId,
    pub input: LayoutInput,
    pub placement: Option<ParentLocalRect>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutInvalidationPolicyDeclaration {
    pub target: WidgetId,
    pub dependencies: Vec<LayoutMeasurementDependency>,
    pub revisions: Vec<CacheRevisionToken>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutEvidence {
    pub target: WidgetId,
    pub bounds: TargetLocalRect,
    pub child_placements: Vec<ChildPlacement>,
    pub invalidated: bool,
    pub diagnostics: Vec<Diagnostic>,
}

/// The scroll geometry a widget's [`SlipwayScrollBehaviorPolicy`] returns.
/// The scroll helpers ([`scroll_region_from_scrollable_capability_with_order`])
/// copy these fields into the [`ScrollRegionDeclaration`] they build, so
/// the field contracts documented there (offset sign, travel relationship,
/// paint responsibility) apply here identically. Authoring contract:
/// `docs/public/api/routing-and-scroll.md`.
#[derive(Clone, Debug, PartialEq)]
pub struct ScrollBehaviorPolicyDeclaration {
    pub target: WidgetId,
    /// Region id the helper adopts when its own `region_id` argument is
    /// `None` (falling back to `"{widget_id}:scroll"` when both are).
    pub region_id: Option<PresentationRegionId>,
    pub address: Option<WidgetSlotAddress>,
    pub axes: ScrollAxes,
    /// RESERVED: declared but not consumed by any workspace path today
    /// (the helper copies every other field, not this one). Travel is
    /// computed from `content_bounds` vs `viewport`; keep `extent`
    /// consistent with `content_bounds.size` for forward compatibility.
    pub extent: Size,
    /// Target-local window the region presents through; see
    /// [`ScrollRegionDeclaration::viewport`].
    pub viewport: TargetLocalRect,
    /// Declared content geometry; travel requires it to exceed the
    /// viewport — see [`ScrollRegionDeclaration::content_bounds`].
    pub content_bounds: TargetLocalRect,
    /// Current scroll offset; sign/clamp contract at
    /// [`ScrollRegionDeclaration::offset`].
    pub offset: Point,
    pub consumption: ScrollConsumptionPolicy,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WheelRoutingPolicyDeclaration {
    pub target: WidgetId,
    pub routing: WheelRouting,
    pub modifiers: Option<Modifiers>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewportObservationEvidence {
    pub target: WidgetId,
    pub viewport: TargetLocalRect,
    pub visible_rect: TargetLocalRect,
    pub scroll: Option<ScrollState>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VirtualCollectionPolicyDeclaration {
    pub target: WidgetId,
    pub item_count: usize,
    pub visible_range: Option<ItemRange>,
    pub realization_hint: VirtualizationHint,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProviderSurfaceKind {
    Canvas,
    Gpu,
    Media,
    Plot,
    Map,
    Terminal,
    RasterImage,
    Custom(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderSurfaceRequest {
    pub target: WidgetId,
    pub provider_id: String,
    pub kind: ProviderSurfaceKind,
    /// Provider bounds in the target widget's local coordinate space.
    ///
    /// Backends may map this into their own screen/root spaces, but provider
    /// authors should treat this as target-local input and expose unsupported
    /// diagnostics if their renderer cannot honor that mapping.
    pub bounds: Rect,
    pub payload_ref: Option<String>,
    /// Dirty rectangles in the same target-local coordinate space as `bounds`.
    pub dirty_regions: Vec<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderHitTestEvidence {
    pub target: WidgetId,
    pub provider_id: String,
    /// Hit-test point in the target widget's local coordinate space.
    pub point: Point,
    pub hit: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderSnapshotRequest {
    pub target: WidgetId,
    pub provider_id: String,
    /// Snapshot bounds in the target widget's local coordinate space.
    pub bounds: Rect,
    pub frame: FrameIdentity,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderSnapshotEvidence {
    pub target: WidgetId,
    pub provider_id: String,
    pub snapshot_ref: Option<String>,
    pub frame: FrameIdentity,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResourceSourceKind {
    SystemFamily,
    Asset,
    Embedded,
    BackendInstalled,
    Custom(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceSourceDeclaration {
    pub source_id: String,
    pub kind: ResourceSourceKind,
    pub family: Option<String>,
    pub asset_ref: Option<String>,
    pub revision: Vec<CacheRevisionToken>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SourceValidityKind {
    Valid,
    InvalidUtf8,
    SuspectedMojibake,
    Missing,
    Unsupported,
    Unknown,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceValidityEvidence {
    pub source_id: String,
    pub validity: SourceValidityKind,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResourceInstallationStatus {
    Installed,
    AlreadyInstalled,
    Refused,
    Unsupported,
    NotRequested,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceInstallationEvidence {
    pub resource_id: String,
    pub source: Option<ResourceSourceDeclaration>,
    pub status: ResourceInstallationStatus,
    pub evidence_source: EvidenceSource,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceRefusalEvidence {
    pub resource_id: String,
    pub source: Option<ResourceSourceDeclaration>,
    pub reason: String,
    pub evidence_source: EvidenceSource,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontResolutionRequest {
    pub family: String,
    pub fallback_families: Vec<String>,
    pub weight: FontWeight,
    pub style: FontStyle,
    pub source: Option<ResourceSourceDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FontResolutionEvidence {
    pub request: FontResolutionRequest,
    pub resolved_ref: Option<String>,
    pub fallback_chain: Vec<String>,
    pub installation: Option<ResourceInstallationEvidence>,
    pub refusal: Option<ResourceRefusalEvidence>,
    pub valid_source: Option<SourceValidityEvidence>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetResolutionRequest {
    pub asset_id: String,
    pub kind: String,
    pub variant: Option<String>,
    pub source: Option<ResourceSourceDeclaration>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AssetResolutionEvidence {
    pub request: AssetResolutionRequest,
    pub resolved_ref: Option<String>,
    pub installation: Option<ResourceInstallationEvidence>,
    pub refusal: Option<ResourceRefusalEvidence>,
    pub valid_source: Option<SourceValidityEvidence>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageDecodeRequest {
    pub asset_ref: String,
    pub target_size: Option<Size>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ImageDecodeEvidence {
    pub request: ImageDecodeRequest,
    pub decoded_size: Option<Size>,
    pub pixel_ref: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StyleTokenRequest {
    pub token: String,
    pub state: Option<InteractionState>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StyleTokenEvidence {
    pub request: StyleTokenRequest,
    pub value: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TimeSourceSnapshot {
    pub source_id: String,
    pub millis: i64,
    pub revision: CacheRevisionToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RandomSourceSnapshot {
    pub source_id: String,
    pub seed: String,
    pub draw_index: u64,
    pub revision: CacheRevisionToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalDataSnapshot {
    pub source_id: String,
    pub snapshot_ref: String,
    pub revision: CacheRevisionToken,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AnimationTimelinePolicyDeclaration {
    pub target: WidgetId,
    pub timeline_id: String,
    pub time_millis: f32,
    pub paused: bool,
    pub revision: CacheRevisionToken,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandInvocationPolicyDeclaration {
    pub command_id: String,
    pub target: WidgetId,
    pub enabled: bool,
    pub expects_state_change: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandStatusEvidence {
    pub command_id: String,
    pub enabled: bool,
    pub checked: Option<bool>,
    pub label: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ShortcutRoutingPolicyDeclaration {
    pub shortcut: ShortcutDeclaration,
    pub route: EventRoute,
    pub command_id: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UndoRedoPolicyDeclaration {
    pub target: WidgetId,
    pub can_undo: bool,
    pub can_redo: bool,
    pub undo_command: Option<String>,
    pub redo_command: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendCapabilityReport {
    pub backend_id: String,
    pub capabilities: Vec<Capability>,
    pub profiles: Vec<CapabilityProfileKind>,
    pub visible_capabilities: Vec<BackendVisibleCapability>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BackendVisibleCapability {
    HitRegions,
    Cursor,
    PointerCapture,
    FocusRegions,
    TextEditRegions,
    ScrollRegions,
    ShapePathClip,
    FontInstallation,
    BackendPresentedEvidence,
    CanonicalOffscreenEvidence,
    Custom(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendVisibleCapabilityRequirement {
    pub requirement_id: String,
    pub target: Option<WidgetId>,
    pub capability: BackendVisibleCapability,
    pub required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnsupportedCapabilityEvidence {
    pub backend_id: String,
    pub target: Option<WidgetId>,
    pub capability: Capability,
    pub visible_capability: Option<BackendVisibleCapability>,
    pub requirement_id: Option<String>,
    pub reason: String,
    pub source: EvidenceSource,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BackendParityAdmission {
    pub backend_id: String,
    pub accepted: bool,
    pub required_profiles: Vec<CapabilityProfileKind>,
    pub visible_requirements: Vec<BackendVisibleCapabilityRequirement>,
    pub unsupported: Vec<UnsupportedCapabilityEvidence>,
    pub source: EvidenceSource,
    pub diagnostics: Vec<Diagnostic>,
}

pub struct ViewDefinitionInput {
    pub frame: FrameIdentity,
    pub layout_input: LayoutInput,
    output: LayoutOutputBuilder,
}

impl ViewDefinitionInput {
    pub fn new(frame: FrameIdentity, layout_input: LayoutInput) -> Self {
        let output = LayoutOutputBuilder::for_input(&layout_input);
        Self {
            frame,
            layout_input,
            output,
        }
    }

    pub fn into_layout_parts(self) -> (FrameIdentity, LayoutInput, LayoutOutputBuilder) {
        (self.frame, self.layout_input, self.output)
    }
}

/// The one total order every declared-region selector uses to pick the
/// front-most region when hit/scroll regions overlap: `z_index` first,
/// then `paint_order`, then `traversal_order` (higher wins; see
/// [`compare_hit_region_order`]). Equal orders are NOT front of each
/// other.
///
/// Assign a distinct order whenever two enabled regions of the same kind
/// can overlap: admission refuses equal-order overlaps with
/// `view_contract.ambiguous_wheel_overlap` (wheel-consuming scroll
/// regions) or `view_contract.ambiguous_hit_overlap` (hit regions),
/// naming both region ids. The plain scroll helper defaults to
/// `{0, 0, 0}` — use [`scroll_region_from_scrollable_capability_with_order`]
/// (or the `order` argument of [`hit_region_from_pointer_capability`]) to
/// set distinct values. Full ordering contract:
/// `docs/public/api/routing-and-scroll.md`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HitRegionOrder {
    /// Primary key: explicit stacking layer (overlays go above content).
    pub z_index: i32,
    /// Secondary key: order within the layer, usually the paint order.
    pub paint_order: usize,
    /// Final tie-breaker: source traversal position.
    pub traversal_order: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CursorCapability {
    Inherited,
    Default,
    Pointer,
    Text,
    Grab,
    Grabbing,
    Move,
    Crosshair,
    NotAllowed,
    ResizeHorizontal,
    ResizeVertical,
    ResizeBoth,
    Custom(String),
}

/// Declares a pointer hit region. Construct it with
/// [`hit_region_from_pointer_capability`]; the struct is
/// `#[non_exhaustive]`, so a struct literal fails outside `slipway-core`
/// (E0639) even with every field spelled out:
///
/// ```compile_fail,E0639
/// use slipway_core::*;
///
/// let region = HitRegionDeclaration {
///     id: PresentationRegionId::from("panel:hit"),
///     target: WidgetId::from("panel"),
///     address: None,
///     bounds: TargetLocalRect::new(Rect {
///         origin: Point { x: 0.0, y: 0.0 },
///         size: Size { width: 10.0, height: 10.0 },
///     }),
///     event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
///     order: HitRegionOrder::default(),
///     route: EventRoute {
///         route_id: None,
///         address: None,
///         path: vec![WidgetId::from("panel")],
///         phase: EventRoutePhase::Target,
///     },
///     cursor: CursorCapability::Default,
///     enabled: true,
///     capture: PointerCaptureIntent::None,
///     capture_evidence: Vec::new(),
/// };
/// ```
///
/// The constructor that succeeds — the helper snapshots the route from
/// the widget's [`SlipwayEventRoutingPolicy`] and fills every field:
///
/// ```text
/// let region = hit_region_from_pointer_capability(
///     &widget, &external, &local, id, None, bounds,
///     PointerEventCoordinateSpace::TargetLocal, order, None,
///     CursorCapability::Default, true, PointerCaptureIntent::None,
/// );
/// view.hit_regions.push(region);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[must_use = "push this into ViewDefinition::hit_regions; an undeclared hit region receives no pointer dispatch"]
pub struct HitRegionDeclaration {
    pub id: PresentationRegionId,
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    /// Hit geometry in target-local coordinates.
    pub bounds: TargetLocalRect,
    /// Coordinate space used for the generated `PointerEvent::position`.
    pub event_coordinate_space: PointerEventCoordinateSpace,
    pub order: HitRegionOrder,
    pub route: EventRoute,
    pub cursor: CursorCapability,
    pub enabled: bool,
    pub capture: PointerCaptureIntent,
    pub capture_evidence: Vec<PointerCaptureEvidence>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum PointerEventCoordinateSpace {
    #[default]
    /// Pointer coordinates are relative to the target widget origin.
    TargetLocal,
    /// Pointer coordinates are relative to the matched presentation region.
    RegionLocal,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TextInputVisualStyleDeclaration {
    pub target: WidgetId,
    pub value_color: Color,
    pub placeholder_color: Color,
    pub preedit_color: Color,
    pub selection_color: Color,
    pub background_color: Color,
    pub border_color: Color,
    pub border_width: f32,
    pub border_radius: f32,
    pub icon_color: Color,
    pub diagnostics: Vec<Diagnostic>,
}

impl TextInputVisualStyleDeclaration {
    pub fn explicit(
        target: WidgetId,
        value_color: Color,
        placeholder_color: Color,
        preedit_color: Color,
        selection_color: Color,
        background_color: Color,
        border_color: Color,
        border_width: f32,
        border_radius: f32,
        icon_color: Color,
    ) -> Self {
        Self {
            target,
            value_color,
            placeholder_color,
            preedit_color,
            selection_color,
            background_color,
            border_color,
            border_width,
            border_radius,
            icon_color,
            diagnostics: Vec::new(),
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }

    pub fn with_value_color(mut self, color: Color) -> Self {
        self.value_color = color;
        self
    }

    pub fn with_placeholder_color(mut self, color: Color) -> Self {
        self.placeholder_color = color;
        self
    }

    pub fn with_preedit_color(mut self, color: Color) -> Self {
        self.preedit_color = color;
        self
    }

    pub fn with_selection_color(mut self, color: Color) -> Self {
        self.selection_color = color;
        self
    }

    pub fn with_background_color(mut self, color: Color) -> Self {
        self.background_color = color;
        self
    }

    pub fn with_border(mut self, color: Color, width: f32, radius: f32) -> Self {
        self.border_color = color;
        self.border_width = width;
        self.border_radius = radius;
        self
    }

    pub fn with_icon_color(mut self, color: Color) -> Self {
        self.icon_color = color;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TextInputTypographyDeclaration {
    pub target: WidgetId,
    pub style: TextStyle,
    pub source: Option<ResourceSourceDeclaration>,
    pub diagnostics: Vec<Diagnostic>,
}

impl TextInputTypographyDeclaration {
    pub fn explicit(target: WidgetId, style: TextStyle) -> Self {
        Self {
            target,
            style,
            source: None,
            diagnostics: Vec::new(),
        }
    }

    pub fn with_source(mut self, source: ResourceSourceDeclaration) -> Self {
        self.source = Some(source);
        self
    }

    pub fn with_style(mut self, style: TextStyle) -> Self {
        self.style = style;
        self
    }

    pub fn with_font_family(mut self, font_family: impl Into<String>) -> Self {
        self.style = self.style.with_font_family(font_family);
        self
    }

    pub fn with_font_size(mut self, font_size: f32) -> Self {
        self.style = self.style.with_font_size(font_size);
        self
    }

    pub fn with_font_weight(mut self, font_weight: FontWeight) -> Self {
        self.style = self.style.with_font_weight(font_weight);
        self
    }

    pub fn with_font_style(mut self, font_style: FontStyle) -> Self {
        self.style = self.style.with_font_style(font_style);
        self
    }

    pub fn with_decoration(mut self, decoration: TextDecoration) -> Self {
        self.style = self.style.with_decoration(decoration);
        self
    }

    pub fn with_baseline(mut self, baseline: BaselineShift) -> Self {
        self.style = self.style.with_baseline(baseline);
        self
    }

    pub fn with_diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }
}

/// Text-edit payload of a [`FocusRegionDeclaration`]. Construct via
/// [`focus_region_from_focus_capability`] /
/// [`text_edit_focus_region_from_capability`]: the plain helper declares a
/// non-text focus region (`text_edit: None`), the text helper assembles
/// this payload from the widget's text policies. The struct is
/// `#[non_exhaustive]`, so a struct literal fails outside `slipway-core`
/// (E0639).
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct TextEditRegionDeclaration {
    pub buffer: TextBufferSnapshot,
    pub selection: TextSelectionPolicyDeclaration,
    pub composition: ImeCompositionPolicyDeclaration,
    pub caret: CaretGeometryEvidence,
    pub visual_style: TextInputVisualStyleDeclaration,
    pub typography: TextInputTypographyDeclaration,
    pub edit_commands: Vec<TextEditCommandDeclaration>,
    pub undo_redo: Option<TextUndoRedoEvidence>,
    pub viewport: Option<TextViewport>,
    pub line_mode: TextLineMode,
    pub diagnostics: Vec<Diagnostic>,
}

/// Declares a focus region. Construct via
/// [`focus_region_from_focus_capability`] /
/// [`text_edit_focus_region_from_capability`] (the plain helper for
/// keyboard/focus regions, the text helper for text-input regions); the
/// struct is `#[non_exhaustive]`, so a struct literal fails outside
/// `slipway-core` (E0639) even with every field spelled out:
///
/// ```compile_fail,E0639
/// use slipway_core::*;
///
/// let region = FocusRegionDeclaration {
///     id: PresentationRegionId::from("input:focus"),
///     target: WidgetId::from("input"),
///     address: None,
///     bounds: TargetLocalRect::new(Rect {
///         origin: Point { x: 0.0, y: 0.0 },
///         size: Size { width: 120.0, height: 24.0 },
///     }),
///     member: None,
///     enabled: true,
///     text_edit: None,
/// };
/// ```
///
/// The constructors that succeed — the plain helper for non-text focus
/// regions (`text_edit: None`), the text helper for text-input widgets
/// (it assembles the text-edit payload from the widget's text policies):
///
/// ```text
/// let region = focus_region_from_focus_capability(
///     &widget, &external, &local, id, None, bounds, true,
/// );
/// let text_region = text_edit_focus_region_from_capability(
///     &widget, &external, &local, id, None, bounds,
///     member, true, &layout_input, measurement.as_ref(),
/// );
/// view.focus_regions.push(region);
/// ```
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[must_use = "push this into ViewDefinition::focus_regions; an undeclared focus region receives no focus or text input"]
pub struct FocusRegionDeclaration {
    pub id: PresentationRegionId,
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    pub bounds: TargetLocalRect,
    pub member: Option<FocusTraversalMember>,
    pub enabled: bool,
    pub text_edit: Option<TextEditRegionDeclaration>,
}

/// Declares a scrollable region. Construct it with
/// [`scroll_region_from_scrollable_capability_with_order`] (or the plain
/// variant when nothing can overlap) AFTER layout, from the final
/// [`LayoutOutput`] — the struct is `#[non_exhaustive]`, so a struct
/// literal fails outside `slipway-core`. Declaring
/// [`Capability::WheelInput`] without at least one enabled scroll region
/// refuses admission with
/// `view_contract.scroll_capability_missing_scroll_region`. Full contract:
/// `docs/public/api/routing-and-scroll.md`.
///
/// PAINT RESPONSIBILITY — declared offset and painted content MUST derive
/// from the same source, and who applies the offset depends on the region
/// kind:
///
/// * **Routed (paint-only) region** — no mounted child placements: the
///   AUTHOR paints the visible window (content already shifted by the
///   offset); the runtime and backends never translate authored
///   [`PaintOp`]s by the declared offset.
/// * **Native-backed region** — contains real mounted child placements:
///   the BACKEND applies the declared offset to the child layers, which
///   stay content-local; do not shift them yourself.
///
/// Applying the offset on the wrong side is the Step-198/199 nested-demo
/// bug family (commits `81638b2eb`/`c43ec975a`): subtracting the offset in
/// paint AND declaring it slid the painted window out of the clip — panels
/// went blank while still consuming wheels; the inverse (declaring travel
/// the paint never covers) scrolled into bare bands. Admission validates
/// geometry only (`validate_scroll_regions`) — a paint/offset mismatch is
/// silent at runtime, so pin it with a paint-content test.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
#[must_use = "push this into ViewDefinition::scroll_regions; an undeclared scroll region receives no wheel routing"]
pub struct ScrollRegionDeclaration {
    pub id: PresentationRegionId,
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    /// Target-local window the content is presented through. Must stay
    /// inside layout bounds (or declared overflow bounds) —
    /// `view_contract.scroll_viewport_outside_layout` /
    /// `view_contract.scroll_viewport_outside_overflow_bounds` otherwise —
    /// and must be derived from the final presented bounds, not the wider
    /// incoming [`LayoutInput`].
    pub viewport: TargetLocalRect,
    /// Declared content geometry. Travel exists only where
    /// `content_bounds` exceeds `viewport` per enabled axis (max offset =
    /// `content - viewport`); equal sizes declare a non-travelling region
    /// that still owns the wheel when `consumption.wheel` is true.
    /// Invalid rects refuse with `view_contract.scroll_geometry_invalid`.
    pub content_bounds: TargetLocalRect,
    /// Current scroll position in content units, applied as a NEGATIVE
    /// translation of content: positive `y` means content moved up
    /// (scrolled down the document); positive `x` means content moved
    /// left. Must be finite and non-negative
    /// (`view_contract.scroll_offset_invalid`), zero on disabled axes
    /// (`view_contract.scroll_offset_on_disabled_{x,y}_axis`); backends
    /// clamp presented offsets to `[0, content - viewport]` and record the
    /// repair as `*.visible_scroll.offset_clamped` evidence.
    pub offset: Point,
    /// Enabled scroll axes; enabled regions must declare at least one
    /// (`view_contract.scroll_axes_empty`).
    pub axes: ScrollAxes,
    /// Declaration-time wheel-routing snapshot; see [`WheelRouting`].
    pub wheel_routing: WheelRouting,
    /// Declared indicator (scrollbar) presentation: `Auto` (the helper
    /// default — backend-automatic, byte-identical to the pre-control
    /// behavior), `Hidden`, or `Visible`. Set it with
    /// [`ScrollRegionDeclaration::with_scroll_indicator`]; see
    /// [`ScrollIndicatorMode`] for the per-backend semantics.
    pub indicator: ScrollIndicatorMode,
    /// Overlap resolution key; equal orders on overlapping wheel-consuming
    /// regions refuse with `view_contract.ambiguous_wheel_overlap` — see
    /// [`HitRegionOrder`].
    pub order: HitRegionOrder,
    pub virtual_viewport: Option<VirtualViewportRange>,
    /// Which input kinds this region consumes; the wheel is routed here
    /// only when `consumption.wheel` is true.
    pub consumption: ScrollConsumptionPolicy,
    pub evidence: Vec<ScrollConsumptionEvidence>,
    pub enabled: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl ScrollRegionDeclaration {
    #[allow(clippy::too_many_arguments)]
    pub fn explicit(
        id: PresentationRegionId,
        target: WidgetId,
        address: Option<WidgetSlotAddress>,
        viewport: TargetLocalRect,
        content_bounds: TargetLocalRect,
        offset: Point,
        axes: ScrollAxes,
        wheel_routing: WheelRouting,
        order: HitRegionOrder,
        consumption: ScrollConsumptionPolicy,
        enabled: bool,
    ) -> Self {
        Self {
            id,
            target,
            address,
            viewport,
            content_bounds,
            offset,
            axes,
            wheel_routing,
            indicator: ScrollIndicatorMode::Auto,
            order,
            virtual_viewport: None,
            consumption,
            evidence: Vec::new(),
            enabled,
            diagnostics: Vec::new(),
        }
    }

    /// Declares the indicator (scrollbar) presentation for this region —
    /// the per-region override idiom: build the declaration with a
    /// capability helper, then set the mode before pushing it:
    ///
    /// ```text
    /// let region = scroll_region_from_scrollable_capability_with_order(
    ///     &widget, &external, &local, &layout, id, address, true, order,
    /// )
    /// .with_scroll_indicator(ScrollIndicatorMode::Hidden);
    /// view.scroll_regions.push(region);
    /// ```
    ///
    /// Unspecified regions keep [`ScrollIndicatorMode::Auto`]
    /// (backend-automatic, byte-identical default). Semantics per mode and
    /// backend: [`ScrollIndicatorMode`] and
    /// `docs/public/api/routing-and-scroll.md` ("Scroll Indicators").
    pub fn with_scroll_indicator(mut self, indicator: ScrollIndicatorMode) -> Self {
        self.indicator = indicator;
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticSlotDeclaration {
    pub target: WidgetId,
    pub node: SemanticNode,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeMetadataDeclaration {
    pub target: WidgetId,
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaintOrderMode {
    SourceOrder,
    ExplicitLayered,
}

#[derive(Clone, Debug)]
pub struct PaintOrderDeclaration {
    pub target: WidgetId,
    pub mode: PaintOrderMode,
    pub z_index: i32,
    pub order: Option<usize>,
    /// Declares that this view's PAINT may overlap other painted content
    /// (the overlay/popup intent flag). PAINT allowance only: it does NOT
    /// accept hit ambiguity — enabled hit regions that overlap with an
    /// identical [`HitRegionOrder`] still refuse admission with
    /// `view_contract.ambiguous_hit_overlap` (NC-11 split: paint-overlap
    /// allowance and hit-ambiguity acceptance are separate declarations;
    /// see [`Self::allow_ambiguous_hits`]).
    pub allow_overlap: bool,
    /// Accepts equal-order overlapping enabled hit regions, silencing
    /// `view_contract.ambiguous_hit_overlap` for this view. Equal orders
    /// are NOT front of each other, so which region receives a pointer
    /// event in the shared area is arbitrary — set this ONLY when that
    /// genuinely does not matter. The ordinary fix is a distinct
    /// [`HitRegionOrder`] per region (the `order` argument of
    /// [`hit_region_from_pointer_capability`]). Defaults to `false` in
    /// every constructor.
    pub allow_ambiguous_hits: bool,
    pub allow_overflow_paint: bool,
    pub overflow_bounds: Option<TargetLocalRect>,
    pub overlay_anchor: Option<AddressedOverlayAnchor>,
    pub mounted_geometry: Vec<MountedGeometryDeclaration>,
    pub diagnostics: Vec<Diagnostic>,
}

impl PartialEq for PaintOrderDeclaration {
    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
            && self.mode == other.mode
            && self.z_index == other.z_index
            && self.order == other.order
            && self.allow_overlap == other.allow_overlap
            && self.allow_ambiguous_hits == other.allow_ambiguous_hits
            && self.allow_overflow_paint == other.allow_overflow_paint
            && self.overflow_bounds == other.overflow_bounds
            && self.overlay_anchor == other.overlay_anchor
            && self.diagnostics == other.diagnostics
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddressedOverlayAnchor {
    pub address: WidgetSlotAddress,
    pub point: Point,
    pub delta: Translation,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MountedGeometryDeclaration {
    pub address: WidgetSlotAddress,
    pub parent_address: Option<WidgetSlotAddress>,
    pub authored_overflow: Option<TargetLocalRect>,
    pub overlay_anchor: Option<AddressedOverlayAnchor>,
}

impl PaintOrderDeclaration {
    pub fn source_order(target: impl Into<WidgetId>) -> Self {
        Self {
            target: target.into(),
            mode: PaintOrderMode::SourceOrder,
            z_index: 0,
            order: None,
            allow_overlap: false,
            allow_ambiguous_hits: false,
            allow_overflow_paint: false,
            overflow_bounds: None,
            overlay_anchor: None,
            mounted_geometry: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn layer(target: impl Into<WidgetId>, z_index: i32) -> Self {
        Self {
            target: target.into(),
            mode: PaintOrderMode::ExplicitLayered,
            z_index,
            order: None,
            allow_overlap: false,
            allow_ambiguous_hits: false,
            allow_overflow_paint: false,
            overflow_bounds: None,
            overlay_anchor: None,
            mounted_geometry: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn layered_order(target: impl Into<WidgetId>, z_index: i32, order: usize) -> Self {
        Self {
            target: target.into(),
            mode: PaintOrderMode::ExplicitLayered,
            z_index,
            order: Some(order),
            allow_overlap: false,
            allow_ambiguous_hits: false,
            allow_overflow_paint: false,
            overflow_bounds: None,
            overlay_anchor: None,
            mounted_geometry: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    pub fn with_overflow_bounds(mut self, bounds: TargetLocalRect) -> Self {
        self.allow_overflow_paint = true;
        self.overflow_bounds = Some(bounds);
        self
    }

    pub fn with_overlay_anchor(
        mut self,
        address: WidgetSlotAddress,
        point: Point,
        delta: Translation,
    ) -> Self {
        self.overlay_anchor = Some(AddressedOverlayAnchor {
            address,
            point,
            delta,
        });
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewDefinition {
    pub target: WidgetId,
    pub frame: FrameIdentity,
    pub layout: LayoutOutput,
    pub paint: Vec<PaintOp>,
    pub paint_order: PaintOrderDeclaration,
    pub hit_regions: Vec<HitRegionDeclaration>,
    pub focus_regions: Vec<FocusRegionDeclaration>,
    pub scroll_regions: Vec<ScrollRegionDeclaration>,
    pub wheel_traversal_boundary: DeclaredWheelTraversalBoundary,
    pub semantic_slots: Vec<SemanticSlotDeclaration>,
    pub probe_metadata: Vec<ProbeMetadataDeclaration>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DeclaredWheelTraversalBoundary {
    pub terminal_region_index: Option<usize>,
}

/// Capability-independent admission validation: checks paint order,
/// frame/layout geometry (target-local, origin `0,0`), hit/focus/scroll
/// region contracts, text-edit contracts, and paint bounds, returning one
/// [`Diagnostic`] per violation under the `view_contract.*` code family.
/// The visible backends run this same validation at admission and paint a
/// refusal panel on any blocking diagnostic, so run it (usually via the
/// capability-aware variant
/// [`view_definition_contract_diagnostics_for_capabilities`]) in a unit
/// test BEFORE launching a window and assert
/// `!view_definition_has_blocking_contract_diagnostic(..)`. The full
/// code | trigger | fix catalog lives in `docs/public/api/diagnostics.md`
/// — read the diagnostic message first; it embeds the offending region
/// id, rects, and the fixing API.
pub fn view_definition_contract_diagnostics(view: &ViewDefinition) -> Vec<Diagnostic> {
    let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
    view_definition_contract_diagnostics_with_index(view, &geometry_index)
}

fn view_definition_contract_diagnostics_with_index(
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if let Some(index) = view.wheel_traversal_boundary.terminal_region_index {
        let valid = view.scroll_regions.get(index).is_some_and(|region| {
            region.enabled
                && region.consumption.wheel
                && (region.axes.horizontal || region.axes.vertical)
        });
        if !valid {
            diagnostics.push(Diagnostic::error(
                Some(view.target.clone()),
                "view_contract.wheel_traversal_boundary_invalid",
                format!(
                    "wheel traversal terminal index {index} must name an enabled wheel-consuming scroll declaration with an enabled axis (declaration count {})",
                    view.scroll_regions.len()
                ),
            ));
        }
    }

    if view.paint_order.target != view.target {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.paint_order_target_mismatch",
            "ViewDefinition paint_order target must match the view target",
        ));
    }

    if view.paint_order.allow_overflow_paint && view.paint_order.overflow_bounds.is_none() {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.overflow_bounds_missing",
            "Views allowing overflow paint must declare overflow_bounds so backend paint, hit, focus, and scroll containment are explicit",
        ));
    }

    if let Some(bounds) = view.paint_order.overflow_bounds
        && !rect_is_valid(bounds)
    {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.overflow_bounds_invalid",
            "Overflow bounds must be finite and non-negative",
        ));
    }

    if !rect_is_valid(view.frame.viewport) {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.frame_viewport_invalid",
            "ViewDefinition frame viewport must be finite and non-negative",
        ));
    }

    if !rect_is_valid(view.layout.bounds) {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.layout_bounds_invalid",
            "ViewDefinition layout bounds must be finite and non-negative",
        ));
    } else if !rect_origin_is_zero(view.layout.bounds) {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.layout_bounds_not_target_local",
            "ViewDefinition layout bounds must use target-local coordinates with origin 0,0; parent placement is represented by ChildPlacement bounds",
        ));
    } else if rect_is_valid(view.frame.viewport)
        && !rect_contains_rect(view.frame.viewport, view.layout.bounds)
    {
        diagnostics.push(Diagnostic::warning(
            Some(view.target.clone()),
            "view_contract.layout_outside_frame_viewport",
            "ViewDefinition layout bounds extend outside the frame viewport; backend/offscreen evidence may not be comparable",
        ));
    }

    validate_hit_regions(view, geometry_index, &mut diagnostics);
    validate_focus_regions(view, geometry_index, &mut diagnostics);
    validate_scroll_regions(view, geometry_index, &mut diagnostics);
    validate_paint_bounds(view, &mut diagnostics);
    validate_content_overflow_scroll_coverage(view, &mut diagnostics);
    validate_view_contract_diagnostics(view, &mut diagnostics);

    diagnostics
}

fn mounted_address_is_ancestor(
    ancestor: &WidgetSlotAddress,
    descendant: &WidgetSlotAddress,
    declarations: &[MountedGeometryDeclaration],
) -> bool {
    let mut current = descendant;
    while let Some(declaration) = declarations
        .iter()
        .find(|declaration| declaration.address == *current)
    {
        let Some(parent) = declaration.parent_address.as_ref() else {
            return false;
        };
        if parent == ancestor {
            return true;
        }
        current = parent;
    }
    false
}

fn addressed_scroll_translation(view: &ViewDefinition, address: &WidgetSlotAddress) -> Translation {
    view.scroll_regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.address.as_ref().is_some_and(|owner| {
                    mounted_address_is_ancestor(owner, address, &view.paint_order.mounted_geometry)
                })
        })
        .fold(Translation { x: 0.0, y: 0.0 }, |mut total, region| {
            total.x -= region.offset.x;
            total.y -= region.offset.y;
            total
        })
}

pub fn validate_and_index_view(
    view: &ViewDefinition,
) -> Result<Arc<PreparedPresentationGeometry>, Vec<Diagnostic>> {
    validate_and_index_view_with_capture(view, GeometryCaptureIntent::None)
}

pub fn validate_and_index_view_with_capture(
    view: &ViewDefinition,
    capture: GeometryCaptureIntent,
) -> Result<Arc<PreparedPresentationGeometry>, Vec<Diagnostic>> {
    let mut geometry_diagnostics = Vec::new();
    let root_target_rect = view.layout.bounds.into_rect();
    let root_target =
        derive_target_box(view.layout.bounds.size, BoxSpacing::ZERO).unwrap_or_else(|_| {
            derive_target_box(
                Size {
                    width: 0.0,
                    height: 0.0,
                },
                BoxSpacing::ZERO,
            )
            .expect("zero-sized zero-spacing root is valid")
        });
    let root = PresentedBoxGeometry {
        outer: root_target.border.into_rect(),
        border: root_target.border.into_rect(),
        content: root_target.content.into_rect(),
        default_clip: root_target.default_clip.into_rect(),
    };
    let mut target_rects_by_address = HashMap::new();
    let mut first_target_rects_by_id = HashMap::new();
    let mut boxes_by_address = HashMap::new();
    let mut first_border_by_id = HashMap::new();
    let mut captured = match capture {
        GeometryCaptureIntent::None => None,
        GeometryCaptureIntent::RenderPacketEvidence => {
            Some(Vec::with_capacity(view.layout.child_placements.len()))
        }
    };
    for placement in &view.layout.child_placements {
        #[cfg(test)]
        PREPARATION_PLACEMENT_VISITS.with(|visits| visits.set(visits.get() + 1));
        let address = placement.local_state_slot.as_ref();
        let rect = placement.bounds.into_rect();
        let invalid_border = [
            ("border.x", rect.origin.x),
            ("border.y", rect.origin.y),
            ("border.width", rect.size.width),
            ("border.height", rect.size.height),
        ]
        .into_iter()
        .find(|(component, value)| {
            !value.is_finite()
                || ((*component == "border.width" || *component == "border.height") && *value < 0.0)
        });
        if let Some((component, value)) = invalid_border {
            geometry_diagnostics.push(Diagnostic::error(
                Some(placement.child.clone()),
                "view_contract.box_border_invalid",
                format!(
                    "widget {} at {:?}: {} is invalid ({value})",
                    placement.child.as_str(),
                    address,
                    component
                ),
            ));
            continue;
        }
        if let Err(error) = validate_spacing(placement.spacing) {
            let code = if error.value.is_finite() {
                "view_contract.box_spacing_negative"
            } else {
                "view_contract.box_spacing_non_finite"
            };
            geometry_diagnostics.push(Diagnostic::error(
                Some(placement.child.clone()),
                code,
                format!(
                    "widget {} at {:?}: {} is invalid ({})",
                    placement.child.as_str(),
                    address,
                    error.component,
                    error.value
                ),
            ));
            continue;
        }
        if address.is_none() {
            geometry_diagnostics.push(Diagnostic::error(
                Some(placement.child.clone()),
                "view_contract.box_geometry_mismatch",
                format!(
                    "widget {} has no mounted WidgetSlotAddress",
                    placement.child.as_str()
                ),
            ));
            continue;
        }

        let address = address.expect("missing addresses continue above");
        let declaration = view
            .paint_order
            .mounted_geometry
            .iter()
            .find(|declaration| declaration.address == *address);
        let parent_origin = declaration
            .and_then(|declaration| declaration.parent_address.as_ref())
            .and_then(|parent| boxes_by_address.get(parent))
            .map_or(root.border.origin, |geometry: &PresentedBoxGeometry| {
                geometry.border.origin
            });
        let parent_border = ParentLocalRect::from_parent_local(translate_rect(
            placement.bounds.into_rect(),
            Point {
                x: -parent_origin.x,
                y: -parent_origin.y,
            },
        ));
        let parent_local = derive_placement_box(parent_border, placement.spacing)
            .expect("border and spacing were validated above");
        let target_local = derive_target_box(placement.bounds.as_rect().size, placement.spacing)
            .expect("border and spacing were validated above");
        let unscrolled_root = PresentedBoxGeometry {
            outer: translate_rect(parent_local.outer.into_rect(), parent_origin),
            border: translate_rect(parent_local.border.into_rect(), parent_origin),
            content: translate_rect(parent_local.content.into_rect(), parent_origin),
            default_clip: translate_rect(parent_local.border.into_rect(), parent_origin),
        };
        let applicable_scrolls = view.scroll_regions.iter().filter(|region| {
            region.enabled
                && region.address.as_ref().is_some_and(|scroll_address| {
                    mounted_address_is_ancestor(
                        scroll_address,
                        address,
                        &view.paint_order.mounted_geometry,
                    )
                })
        });
        let mut scroll_translation = Translation { x: 0.0, y: 0.0 };
        let mut ancestor_clip_final: Option<Rect> = None;
        for region in applicable_scrolls {
            let owner_origin = region
                .address
                .as_ref()
                .and_then(|owner| boxes_by_address.get(owner))
                .map(|geometry: &PresentedBoxGeometry| geometry.border.origin)
                .unwrap_or(root.border.origin);
            let viewport_final = translate_rect(
                region.viewport,
                Point {
                    x: owner_origin.x + scroll_translation.x,
                    y: owner_origin.y + scroll_translation.y,
                },
            );
            ancestor_clip_final = Some(match ancestor_clip_final {
                Some(clip) => rect_intersection(clip, viewport_final).unwrap_or(Rect {
                    origin: viewport_final.origin,
                    size: Size {
                        width: 0.0,
                        height: 0.0,
                    },
                }),
                None => viewport_final,
            });
            scroll_translation.x -= region.offset.x;
            scroll_translation.y -= region.offset.y;
        }
        let overlay_translation = declaration
            .and_then(|declaration| declaration.overlay_anchor.as_ref())
            .and_then(|overlay| {
                boxes_by_address.get(&overlay.address).map(|anchor| {
                    let anchor_scroll = addressed_scroll_translation(view, &overlay.address);
                    Translation {
                        x: anchor.border.origin.x
                            + anchor_scroll.x
                            + overlay.point.x
                            + overlay.delta.x,
                        y: anchor.border.origin.y
                            + anchor_scroll.y
                            + overlay.point.y
                            + overlay.delta.y,
                    }
                })
            })
            .unwrap_or(Translation { x: 0.0, y: 0.0 });
        let target_translation = if declaration.is_some_and(|d| d.overlay_anchor.is_some()) {
            Point {
                x: overlay_translation.x,
                y: overlay_translation.y,
            }
        } else {
            Point {
                x: unscrolled_root.border.origin.x + scroll_translation.x,
                y: unscrolled_root.border.origin.y + scroll_translation.y,
            }
        };
        let final_presented = PresentedBoxGeometry {
            outer: translate_rect(
                unscrolled_root.outer,
                Point {
                    x: target_translation.x - unscrolled_root.border.origin.x,
                    y: target_translation.y - unscrolled_root.border.origin.y,
                },
            ),
            border: translate_rect(target_local.border, target_translation),
            content: translate_rect(target_local.content, target_translation),
            default_clip: translate_rect(target_local.default_clip, target_translation),
        };
        let authored_overflow = declaration.and_then(|declaration| declaration.authored_overflow);
        let effective_clip_final = effective_clip(
            target_local.default_clip,
            authored_overflow,
            ancestor_clip_final,
            Translation {
                x: target_translation.x,
                y: target_translation.y,
            },
        );

        let rect = placement.bounds.into_rect();
        target_rects_by_address.insert(address.clone(), rect);
        first_target_rects_by_id
            .entry(placement.child.clone())
            .or_insert(rect);
        boxes_by_address.insert(address.clone(), unscrolled_root);
        first_border_by_id
            .entry(placement.child.clone())
            .or_insert(rect);
        if let Some(records) = captured.as_mut() {
            records.push(PreparedGeometryRecord {
                target: placement.child.clone(),
                address: address.clone(),
                spacing: placement.spacing,
                target_local,
                parent_local,
                unscrolled_root,
                scroll_translation,
                overlay_translation,
                final_presented,
                authored_overflow,
                ancestor_clip_final,
                effective_clip_final,
            });
        }
    }
    let index = PresentationGeometryIndex {
        root_target_rect,
        target_rects_by_address,
        first_target_rects_by_id,
        boxes_by_address,
        first_border_by_id,
        root,
    };
    let mut diagnostics = geometry_diagnostics;
    diagnostics.extend(view_definition_contract_diagnostics_with_index(
        view, &index,
    ));
    if view_definition_has_blocking_contract_diagnostic(&diagnostics) {
        Err(diagnostics)
    } else {
        Ok(Arc::new(PreparedPresentationGeometry {
            index,
            capture: captured.map(|records| PreparedGeometryCapture {
                records: records.into_boxed_slice(),
            }),
            diagnostics,
        }))
    }
}

/// The full admission pre-flight: everything
/// [`view_definition_contract_diagnostics`] checks, plus the
/// capability-region gates — every declared input/presentation
/// [`Capability`] must have its matching enabled region (the
/// `view_contract.*_capability_missing_*` codes). Pass
/// `&widget.capabilities()`; this is the recommended author-side check
/// (`docs/public/api/diagnostics.md`, "Validate Before Launch").
pub fn view_definition_contract_diagnostics_for_capabilities(
    view: &ViewDefinition,
    capabilities: &[Capability],
) -> Vec<Diagnostic> {
    let mut diagnostics = view_definition_contract_diagnostics(view);
    validate_view_capabilities(view, capabilities, &mut diagnostics);
    diagnostics
}

/// `true` when any diagnostic is blocking (`Error` or `Unsupported`
/// severity): a blocking admission diagnostic makes the visible backend
/// paint a refusal panel in place of the widget. `Warning`/`Info` are
/// evidence only. See `docs/public/api/diagnostics.md`.
pub fn view_definition_has_blocking_contract_diagnostic(diagnostics: &[Diagnostic]) -> bool {
    diagnostics.iter().any(|diagnostic| {
        matches!(
            diagnostic.severity,
            DiagnosticSeverity::Error | DiagnosticSeverity::Unsupported
        )
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredPointerDispatch {
    pub selected_region: PresentationRegionId,
    pub candidate_regions: Vec<PresentationRegionId>,
    pub input: InputEvent,
    pub route: EventRoute,
    pub capture_event: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredWheelDispatch {
    pub selected_region: PresentationRegionId,
    pub candidate_regions: Vec<PresentationRegionId>,
    pub input: InputEvent,
    pub route: EventRoute,
    pub capture_event: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DeclaredEventDispatchKind {
    Pointer,
    Wheel,
    Scroll,
    Focus,
    Keyboard,
    Text,
    Command,
}

/// Coordinate space of [`DeclaredEventDispatchEvidence::input_position`].
///
/// The evidence contract is RESOLVE-POINT RECORDING: `input_position` is the
/// exact root-local point the recording backend resolved the dispatch with —
/// the point at which re-running the core resolver over the declared
/// (un-scrolled) region rects reproduces the recorded consumer choice. That
/// point is branch-dependent (a mid-travel wheel resolves through a
/// content-space translation; the at-limit fallback resolves through the
/// visible-viewport point), so every recorded position carries this explicit
/// space annotation instead of being silently mixed (audit finding MF-M1 /
/// MF-M15 / MF-M19).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchPositionSpace {
    /// Root content space: the un-scrolled root-local declaration coordinate
    /// system that layout rects, dispatch-graph node bounds, and the core
    /// resolvers use. Mid-travel pointer/wheel positions map the cursor
    /// through the presented region under it, so every ancestor scroll offset
    /// is applied. Scroll-sync evidence records the selected region's declared
    /// viewport origin in this space.
    Content,
    /// Visible-viewport space: the raw cursor mapped to the view origin plus
    /// `frame.viewport.origin`, with NO per-region scroll translation. Used
    /// only by the wheel fallback branches, where an un-displaced ancestor
    /// scroll region (e.g. the native root scroll) owns the wheel through its
    /// fixed window band once the content-space point has left every declared
    /// viewport rect.
    Viewport,
}

/// Declared dispatch evidence for one backend input resolution.
///
/// `input_position` follows the resolve-point recording contract documented
/// on [`DispatchPositionSpace`]: it is the root-local point dispatch actually
/// resolved with, and `input_position_space` names the space of that point.
/// The two fields are populated together — `input_position_space` is `Some`
/// exactly when `input_position` is `Some`. Consumers comparing positions
/// across products (dispatch graph bounds, paint declarations) must check the
/// space annotation first; only `Content`-space positions are directly
/// comparable to declaration-space rects.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredEventDispatchEvidence {
    pub source: EvidenceSource,
    pub frame: FrameIdentity,
    pub kind: DeclaredEventDispatchKind,
    pub input_position: Option<Point>,
    pub input_position_space: Option<DispatchPositionSpace>,
    pub candidate_regions: Vec<PresentationRegionId>,
    pub selected_region: Option<PresentationRegionId>,
    pub refusal_reason: Option<String>,
    pub generated_event: Option<InputEvent>,
    pub route: Option<EventRoute>,
    pub capture_event: bool,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendInputEvent {
    pub event: InputEvent,
    pub dispatch_evidence: Option<DeclaredEventDispatchEvidence>,
}

impl BackendInputEvent {
    pub fn direct(event: InputEvent) -> Self {
        Self {
            event,
            dispatch_evidence: None,
        }
    }

    pub fn declared(event: InputEvent, dispatch_evidence: DeclaredEventDispatchEvidence) -> Self {
        Self {
            event,
            dispatch_evidence: Some(dispatch_evidence),
        }
    }

    pub fn into_event(self) -> InputEvent {
        self.event
    }
}

pub const BACKEND_INPUT_DISPATCH_EVIDENCE_MISSING: &str = "backend_input.dispatch_evidence_missing";
pub const BACKEND_INPUT_DISPATCH_EVIDENCE_SOURCE_MISMATCH: &str =
    "backend_input.dispatch_evidence_source_mismatch";
pub const BACKEND_INPUT_DISPATCH_EVIDENCE_FRAME_MISMATCH: &str =
    "backend_input.dispatch_evidence_frame_mismatch";
pub const BACKEND_INPUT_DISPATCH_EVIDENCE_UNRESOLVED: &str =
    "backend_input.dispatch_evidence_unresolved";
pub const BACKEND_INPUT_DISPATCH_EVIDENCE_KIND_MISMATCH: &str =
    "backend_input.dispatch_evidence_kind_mismatch";
pub const BACKEND_INPUT_DISPATCH_EVIDENCE_EVENT_MISMATCH: &str =
    "backend_input.dispatch_evidence_event_mismatch";
pub const BACKEND_INPUT_DISPATCH_EVIDENCE_REGION_MISMATCH: &str =
    "backend_input.dispatch_evidence_region_mismatch";
pub const BACKEND_INPUT_DISPATCH_EVIDENCE_ROUTE_MISMATCH: &str =
    "backend_input.dispatch_evidence_route_mismatch";
pub const BACKEND_INPUT_DISPATCH_EVIDENCE_CANDIDATES_MISMATCH: &str =
    "backend_input.dispatch_evidence_candidates_mismatch";
/// Diagnostic code for a retained no-consumer dispatch refusal (audit finding
/// MF-H3): the backend constructed complete refusal evidence (position,
/// candidates, reason) for an input that resolved to no consumer, and the
/// runtime retained it in the bounded refusal ring instead of dropping it.
pub const BACKEND_INPUT_DISPATCH_REFUSED: &str = "backend_input.dispatch_refused";
pub const EVENT_DECLARATION_DISPATCH_ROUTE_MISMATCH: &str =
    "event_declaration.dispatch_route_mismatch";
pub const EVENT_DECLARATION_HANDLER_IGNORED_DECLARED_HANDLED: &str =
    "event_declaration.handler_ignored_declared_handled";
pub const EVENT_DECLARATION_HANDLER_HANDLED_DECLARED_UNHANDLED: &str =
    "event_declaration.handler_handled_declared_unhandled";

pub fn backend_input_dispatch_evidence_missing_diagnostic(event: &InputEvent) -> Diagnostic {
    Diagnostic::error(
        Some(event.target().clone()),
        BACKEND_INPUT_DISPATCH_EVIDENCE_MISSING,
        "backend input did not carry declaration dispatch evidence and must not be treated as physical-equivalent declaration evidence",
    )
}

#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredEventDispatchIdentity {
    pub frame: FrameIdentity,
    pub kind: DeclaredEventDispatchKind,
    pub input_position: Option<Point>,
    pub candidate_regions: Vec<PresentationRegionId>,
    pub selected_region: Option<PresentationRegionId>,
    pub generated_event: Option<InputEvent>,
    pub route: Option<EventRoute>,
    pub capture_event: bool,
}

impl DeclaredEventDispatchEvidence {
    pub fn dispatch_identity(&self) -> DeclaredEventDispatchIdentity {
        DeclaredEventDispatchIdentity {
            frame: self.frame.clone(),
            kind: self.kind,
            input_position: self.input_position,
            candidate_regions: self.candidate_regions.clone(),
            selected_region: self.selected_region.clone(),
            generated_event: self.generated_event.clone(),
            route: self.route.clone(),
            capture_event: self.capture_event,
        }
    }
}

pub fn backend_input_dispatch_evidence_contract_diagnostics(
    view: &ViewDefinition,
    input: &BackendInputEvent,
    expected_source_label: Option<&str>,
    expected_backend_id: Option<&str>,
) -> Vec<Diagnostic> {
    if input.dispatch_evidence.is_none() {
        return vec![backend_input_dispatch_evidence_missing_diagnostic(
            &input.event,
        )];
    }
    let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
    backend_input_dispatch_evidence_contract_diagnostics_with_geometry_index(
        view,
        &geometry_index,
        input,
        expected_source_label,
        expected_backend_id,
    )
}

pub fn backend_input_dispatch_evidence_contract_diagnostics_with_geometry_index(
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    input: &BackendInputEvent,
    expected_source_label: Option<&str>,
    expected_backend_id: Option<&str>,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let Some(evidence) = input.dispatch_evidence.as_ref() else {
        diagnostics.push(backend_input_dispatch_evidence_missing_diagnostic(
            &input.event,
        ));
        return diagnostics;
    };

    if let Some(expected_source_label) = expected_source_label {
        if evidence.source.label.as_str() != expected_source_label {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_SOURCE_MISMATCH,
                format!(
                    "backend input dispatch evidence source `{}` did not match expected `{}`",
                    evidence.source.label, expected_source_label
                ),
            ));
        }
    }
    if let Some(expected_backend_id) = expected_backend_id {
        if evidence.source.backend_id.as_deref() != Some(expected_backend_id) {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_SOURCE_MISMATCH,
                format!(
                    "backend input dispatch evidence backend `{:?}` did not match expected `{}`",
                    evidence.source.backend_id, expected_backend_id
                ),
            ));
        }
    }
    if evidence.frame != view.frame {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_FRAME_MISMATCH,
            "backend input dispatch evidence frame did not match the current view frame",
        ));
    }
    if evidence.selected_region.is_none()
        || evidence.generated_event.is_none()
        || evidence.refusal_reason.is_some()
    {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_UNRESOLVED,
            "backend input dispatch evidence did not resolve to an enabled declaration region",
        ));
    }
    if evidence.generated_event.as_ref() != Some(&input.event) {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_EVENT_MISMATCH,
            "backend input event did not match the generated event recorded in dispatch evidence",
        ));
    }

    match evidence.kind {
        DeclaredEventDispatchKind::Pointer => {
            validate_pointer_dispatch_evidence(
                view,
                geometry_index,
                input,
                evidence,
                &mut diagnostics,
            );
        }
        DeclaredEventDispatchKind::Wheel => {
            validate_wheel_dispatch_evidence(
                view,
                geometry_index,
                input,
                evidence,
                &mut diagnostics,
            );
        }
        DeclaredEventDispatchKind::Scroll => {
            validate_scroll_dispatch_evidence(view, input, evidence, &mut diagnostics);
        }
        DeclaredEventDispatchKind::Focus
        | DeclaredEventDispatchKind::Keyboard
        | DeclaredEventDispatchKind::Text
        | DeclaredEventDispatchKind::Command => {
            validate_focus_dispatch_evidence(
                view,
                geometry_index,
                input,
                evidence,
                &mut diagnostics,
            );
        }
    }

    diagnostics
}

pub fn dispatch_evidence_event_route_contract_diagnostics(
    input: &BackendInputEvent,
    declaration: &EventHandlingDeclaration,
) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    let Some(evidence) = input.dispatch_evidence.as_ref() else {
        diagnostics.push(backend_input_dispatch_evidence_missing_diagnostic(
            &input.event,
        ));
        return diagnostics;
    };
    let Some(dispatch_route) = evidence.route.as_ref() else {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            EVENT_DECLARATION_DISPATCH_ROUTE_MISMATCH,
            "physical dispatch evidence did not record the declared event route",
        ));
        return diagnostics;
    };

    let declared_route = &declaration.routing.route;
    if dispatch_route.address != declared_route.address
        || dispatch_route.path != declared_route.path
        || dispatch_route.phase != declared_route.phase
    {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            EVENT_DECLARATION_DISPATCH_ROUTE_MISMATCH,
            "physical dispatch route did not match the widget event routing policy",
        ));
    }

    diagnostics
}

fn dispatch_contract_error(
    event: &InputEvent,
    code: impl Into<String>,
    message: impl Into<String>,
) -> Diagnostic {
    Diagnostic::error(Some(event.target().clone()), code, message)
}

fn validate_pointer_dispatch_evidence(
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    input: &BackendInputEvent,
    evidence: &DeclaredEventDispatchEvidence,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let InputEvent::Pointer(pointer) = &input.event else {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_KIND_MISMATCH,
            "pointer dispatch evidence was attached to a non-pointer event",
        ));
        return;
    };

    if evidence
        .selected_region
        .as_ref()
        .is_some_and(|selected| view.hit_regions.iter().any(|region| &region.id == selected))
    {
        let Some(position) = evidence.input_position else {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_REGION_MISMATCH,
                "hit-region pointer dispatch evidence did not record an input position",
            ));
            return;
        };
        let expected_candidates = view
            .hit_regions
            .iter()
            .filter(|region| region.enabled)
            .filter(|region| {
                declared_region_contains_root_local_point_with_geometry_index(
                    geometry_index,
                    &region.target,
                    region.address.as_ref(),
                    region.bounds.into_rect(),
                    position,
                )
            })
            .map(|region| region.id.clone())
            .collect::<Vec<_>>();
        let selected_declared = evidence.selected_region.as_ref().and_then(|selected| {
            view.hit_regions
                .iter()
                .find(|region| &region.id == selected)
        });
        let captured_selected = selected_declared.is_some_and(|region| {
            evidence.capture_event
                && declared_pointer_capture_for_region(region, pointer.kind, true)
        });
        if captured_selected {
            let selected_present = selected_declared.is_some_and(|region| {
                evidence
                    .candidate_regions
                    .iter()
                    .any(|candidate| candidate == &region.id)
            });
            let all_candidates_declared = evidence.candidate_regions.iter().all(|candidate| {
                view.hit_regions
                    .iter()
                    .any(|region| region.enabled && &region.id == candidate)
            });
            if !selected_present || !all_candidates_declared {
                diagnostics.push(dispatch_contract_error(
                    &input.event,
                    BACKEND_INPUT_DISPATCH_EVIDENCE_CANDIDATES_MISMATCH,
                    "captured pointer dispatch evidence candidates did not include a valid retained capture owner",
                ));
            }
        } else if evidence.candidate_regions != expected_candidates {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_CANDIDATES_MISMATCH,
                "pointer dispatch evidence candidates did not match current hit regions",
            ));
        }
        let current_selected = select_declared_hit_region_at_root_local_point_with_geometry_index(
            geometry_index,
            &view.hit_regions,
            position,
        );
        let selected = if captured_selected {
            selected_declared
        } else {
            current_selected
        };
        validate_selected_region_id(
            &input.event,
            evidence.selected_region.as_ref(),
            selected.map(|region| &region.id),
            if captured_selected {
                "captured pointer dispatch evidence selected region did not match the retained capture owner"
            } else {
                "pointer dispatch evidence selected region did not match current hit resolution"
            },
            diagnostics,
        );
        if let Some(region) = selected {
            let expected_event = declared_pointer_event_for_hit_region_with_geometry_index(
                geometry_index,
                region,
                position,
                pointer.kind,
                pointer.button,
                pointer.details.clone(),
            );
            if input.event != expected_event {
                diagnostics.push(dispatch_contract_error(
                    &input.event,
                    BACKEND_INPUT_DISPATCH_EVIDENCE_EVENT_MISMATCH,
                    format!(
                        "pointer dispatch event did not match the current hit-region declaration; actual={:?}; expected={:?}",
                        input.event, expected_event
                    ),
                ));
            }
            if evidence.route.as_ref() != Some(&region.route) {
                diagnostics.push(dispatch_contract_error(
                    &input.event,
                    BACKEND_INPUT_DISPATCH_EVIDENCE_ROUTE_MISMATCH,
                    "pointer dispatch route did not match the current hit-region declaration",
                ));
            }
        }
        return;
    }

    validate_focus_dispatch_evidence(view, geometry_index, input, evidence, diagnostics);
}

fn validate_wheel_dispatch_evidence(
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    input: &BackendInputEvent,
    evidence: &DeclaredEventDispatchEvidence,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let InputEvent::Wheel(wheel) = &input.event else {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_KIND_MISMATCH,
            "wheel dispatch evidence was attached to a non-wheel event",
        ));
        return;
    };
    let Some(position) = evidence.input_position else {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_REGION_MISMATCH,
            "wheel dispatch evidence did not record an input position",
        ));
        return;
    };
    let (dispatch, _) = resolve_declared_wheel_dispatch_with_evidence_geometry_index_and_boundary(
        evidence.source.clone(),
        evidence.frame.clone(),
        geometry_index,
        &view.scroll_regions,
        view.wheel_traversal_boundary,
        position,
        wheel.delta_x,
        wheel.delta_y,
    );
    let expected_candidates = view
        .scroll_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| {
            declared_region_contains_root_local_point_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.viewport.into_rect(),
                position,
            )
        })
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    if evidence.candidate_regions != expected_candidates {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_CANDIDATES_MISMATCH,
            "wheel dispatch evidence candidates did not match current scroll regions",
        ));
    }
    validate_selected_region_id(
        &input.event,
        evidence.selected_region.as_ref(),
        dispatch.as_ref().map(|dispatch| &dispatch.selected_region),
        "wheel dispatch evidence selected region did not match current scroll resolution",
        diagnostics,
    );
    if dispatch.as_ref().map(|dispatch| &dispatch.input) != Some(&input.event) {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_EVENT_MISMATCH,
            "wheel dispatch event did not match the current scroll-region declaration",
        ));
    }
    if dispatch.as_ref().map(|dispatch| &dispatch.route) != evidence.route.as_ref() {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_ROUTE_MISMATCH,
            "wheel dispatch route did not match the current scroll-region declaration",
        ));
    }
}

fn validate_scroll_dispatch_evidence(
    view: &ViewDefinition,
    input: &BackendInputEvent,
    evidence: &DeclaredEventDispatchEvidence,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let InputEvent::Scroll(scroll) = &input.event else {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_KIND_MISMATCH,
            "scroll dispatch evidence was attached to a non-scroll event",
        ));
        return;
    };
    let selected = view
        .scroll_regions
        .iter()
        .find(|region| Some(&region.id) == evidence.selected_region.as_ref());
    validate_selected_region_id(
        &input.event,
        evidence.selected_region.as_ref(),
        selected.map(|region| &region.id),
        "scroll dispatch evidence selected region did not exist in the current view",
        diagnostics,
    );
    let expected_candidates = view
        .scroll_regions
        .iter()
        .filter(|region| region.enabled)
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    if evidence.candidate_regions != expected_candidates {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_CANDIDATES_MISMATCH,
            "scroll dispatch evidence candidates did not match current scroll regions",
        ));
    }
    if let Some(region) = selected {
        if !region.enabled
            || scroll.target != region.target
            || scroll.target_slot != region.address
            || scroll.region_id != region.id
            || scroll.viewport != region.viewport
            || scroll.content_bounds != region.content_bounds
        {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_EVENT_MISMATCH,
                "scroll dispatch event did not match the current scroll-region declaration",
            ));
        }
        let expected_route = region_event_route(&region.id, &region.target, &region.address);
        if evidence.route.as_ref() != Some(&expected_route) {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_ROUTE_MISMATCH,
                "scroll dispatch route did not match the current scroll-region declaration",
            ));
        }
    }
}

fn validate_focus_dispatch_evidence(
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    input: &BackendInputEvent,
    evidence: &DeclaredEventDispatchEvidence,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !focus_dispatch_kind_matches_event(evidence.kind, &input.event) {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_KIND_MISMATCH,
            "focus/text/keyboard/command dispatch evidence was attached to the wrong event kind",
        ));
        return;
    }

    let text_edit_required = matches!(
        evidence.kind,
        DeclaredEventDispatchKind::Keyboard
            | DeclaredEventDispatchKind::Text
            | DeclaredEventDispatchKind::Command
    );
    let selected = view
        .focus_regions
        .iter()
        .find(|region| Some(&region.id) == evidence.selected_region.as_ref());
    let expected_candidates = view
        .focus_regions
        .iter()
        .filter(|region| region.enabled && (!text_edit_required || region.text_edit.is_some()))
        .filter(|region| {
            if let Some(position) = evidence.input_position {
                declared_region_contains_root_local_point_with_geometry_index(
                    geometry_index,
                    &region.target,
                    region.address.as_ref(),
                    region.bounds.into_rect(),
                    position,
                )
            } else if let Some(selected) = selected {
                region.target == selected.target
            } else {
                true
            }
        })
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    if evidence.candidate_regions != expected_candidates {
        diagnostics.push(dispatch_contract_error(
            &input.event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_CANDIDATES_MISMATCH,
            "focus dispatch evidence candidates did not match current focus regions",
        ));
    }
    validate_selected_region_id(
        &input.event,
        evidence.selected_region.as_ref(),
        selected.map(|region| &region.id),
        "focus dispatch evidence selected region did not exist in the current view",
        diagnostics,
    );
    if let Some(region) = selected {
        if !region.enabled || (text_edit_required && region.text_edit.is_none()) {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_REGION_MISMATCH,
                "focus dispatch evidence selected region did not satisfy the required focus/text capability",
            ));
        }
        if input.event.target() != &region.target
            || input.event.target_slot() != region.address.as_ref()
        {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_EVENT_MISMATCH,
                "focus dispatch event target did not match the current focus-region declaration",
            ));
        }
        if let Some(position) = evidence.input_position {
            if !declared_region_contains_root_local_point_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
                position,
            ) {
                diagnostics.push(dispatch_contract_error(
                    &input.event,
                    BACKEND_INPUT_DISPATCH_EVIDENCE_REGION_MISMATCH,
                    "focus dispatch evidence input position was outside the selected focus region",
                ));
            }
        }
        let expected_route = region_event_route(&region.id, &region.target, &region.address);
        if evidence.route.as_ref() != Some(&expected_route) {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_ROUTE_MISMATCH,
                "focus dispatch route did not match the current focus-region declaration",
            ));
        }
    }
}

fn validate_selected_region_id(
    event: &InputEvent,
    actual: Option<&PresentationRegionId>,
    expected: Option<&PresentationRegionId>,
    message: &'static str,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if actual != expected {
        diagnostics.push(dispatch_contract_error(
            event,
            BACKEND_INPUT_DISPATCH_EVIDENCE_REGION_MISMATCH,
            message,
        ));
    }
}

fn focus_dispatch_kind_matches_event(kind: DeclaredEventDispatchKind, event: &InputEvent) -> bool {
    match kind {
        DeclaredEventDispatchKind::Pointer => matches!(event, InputEvent::Pointer(_)),
        DeclaredEventDispatchKind::Focus => matches!(event, InputEvent::Focus(_)),
        DeclaredEventDispatchKind::Keyboard => matches!(event, InputEvent::Keyboard(_)),
        DeclaredEventDispatchKind::Text => matches!(
            event,
            InputEvent::Text(_)
                | InputEvent::TextEdit(_)
                | InputEvent::TextComposition(_)
                | InputEvent::Selection(_)
        ),
        DeclaredEventDispatchKind::Command => matches!(event, InputEvent::Command(_)),
        DeclaredEventDispatchKind::Wheel => matches!(event, InputEvent::Wheel(_)),
        DeclaredEventDispatchKind::Scroll => matches!(event, InputEvent::Scroll(_)),
    }
}

fn region_event_route(
    region_id: &PresentationRegionId,
    target: &WidgetId,
    address: &Option<WidgetSlotAddress>,
) -> EventRoute {
    EventRoute {
        route_id: Some(region_id.as_str().to_string()),
        address: address.clone(),
        path: route_path_for_address(target, address),
        phase: EventRoutePhase::Target,
    }
}

fn route_path_for_address(target: &WidgetId, address: &Option<WidgetSlotAddress>) -> Vec<WidgetId> {
    address
        .as_ref()
        .map(|address| address.path.clone())
        .unwrap_or_else(|| vec![target.clone()])
}

#[derive(Clone, Debug, PartialEq)]
pub struct BackendInputTrace {
    pub input: BackendInputEvent,
    pub handled: bool,
    pub revision_before: Option<u64>,
    pub revision_after: Option<u64>,
    pub emitted_messages: Vec<EmittedMessageEvidence>,
    pub local_state: Vec<StateObservation>,
    pub changes: Vec<ChangeEvidence>,
    pub diagnostics: Vec<Diagnostic>,
}

impl BackendInputTrace {
    pub fn event_probe(&self) -> EventProbe {
        EventProbe {
            routed_target: self.input.event.target().clone(),
            event: self.input.event.clone(),
            dispatch_evidence: self.input.dispatch_evidence.clone(),
            dispatch_identity: self
                .input
                .dispatch_evidence
                .as_ref()
                .map(DeclaredEventDispatchEvidence::dispatch_identity),
            result_identity: EventResultIdentity {
                handled: Some(self.handled),
                emitted_messages: self.emitted_messages.clone(),
                change_shapes: self.changes.iter().map(ChangeShapeIdentity::from).collect(),
                diagnostics: self
                    .diagnostics
                    .iter()
                    .map(DiagnosticIdentity::from)
                    .collect(),
            },
            handled: Some(self.handled),
            revision_before: self.revision_before,
            revision_after: self.revision_after,
            emitted_messages: self.emitted_messages.clone(),
            local_state: self.local_state.clone(),
            changes: self.changes.clone(),
            diagnostics: self.diagnostics.clone(),
        }
    }
}

/// The one total order every declared-region selector uses to pick a
/// front-most region: `z_index`, then `paint_order`, then `traversal_order`.
///
/// This is the resolver's order key. The derived dispatch graph
/// ([`derive_dispatch_graph_with_geometry_index`]) and the backend occlusion
/// filters compare through this same function so graph, selection, and
/// occlusion cannot drift apart.
pub fn compare_hit_region_order(left: &HitRegionOrder, right: &HitRegionOrder) -> Ordering {
    left.z_index
        .cmp(&right.z_index)
        .then(left.paint_order.cmp(&right.paint_order))
        .then(left.traversal_order.cmp(&right.traversal_order))
}

/// `true` when `front` strictly precedes (paints over / occludes) `back`
/// under [`compare_hit_region_order`]. Equal orders are NOT front-of each
/// other, matching both backends' occlusion filters.
pub fn hit_region_order_is_front_of(front: &HitRegionOrder, back: &HitRegionOrder) -> bool {
    compare_hit_region_order(front, back) == Ordering::Greater
}

/// Whether a pointer-opaque paint occluder blocks a declared hit region.
/// LOAD-BEARING pointer-channel occlusion rule (NC-2 repair).
///
/// WHEN: every pointer-channel comparison of an opaque paint layer against
/// a declared hit region — the derived dispatch graph's pointer `Occlusion`
/// edges and the iced backend's pointer occlusion filter both call this, so
/// the declared-route oracle and live dispatch cannot drift apart.
///
/// Rule: a FOREIGN occluder (different owning widget) blocks exactly when
/// its order key is strictly front of the region
/// ([`hit_region_order_is_front_of`]) — unchanged stacking semantics. A
/// SAME-OWNER occluder at the region's own `z_index` blocks only when the
/// occluding layer carries an AUTHORED within-z order
/// (`PaintLayerKey::ordered` -> `occluder_authored_z_order`) strictly
/// greater than the region's declared `paint_order` — the authored-stack
/// case (e.g. overlapping overlay cards of one widget). A same-owner
/// same-z layer WITHOUT an authored order never blocks: the hit region
/// rides the layer it accompanies (the pointer-opaque overlay recipe,
/// `docs/public/api/routing-and-scroll.md`). Same-owner occluders at a
/// DIFFERENT z compare by the full key like foreign ones (a deliberately
/// higher-z own layer still covers the widget's own lower-z regions).
///
/// Failure mode the same-owner rule repairs: occluder order keys carry
/// paint-unit tie-break fields (defaulted unit order and traversal — the
/// mounted child's SLOT ORDINAL; see [`paint_unit_sort_key`]), a DIFFERENT
/// space from the author-declared [`HitRegionOrder`] tie-break fields.
/// Without the exemption an authored opaque overlay (layer z 100 + own
/// full-bounds hit region z 100) is occluded by ITSELF whenever its slot
/// ordinal exceeds the declared `paint_order`/`traversal_order`, and every
/// press over it drops (live-reproduced: the 2026-07-12 naive-consumer
/// audit's NC-2 modal). A press blocked by an occluder that DOES front the
/// selected region is still consumed, and the iced backend constructs
/// refusal evidence naming the occluder (`refusal_reason`) instead of
/// silence. The egui backend expresses the same-owner rule in its
/// allocation-key space, where both sides of the comparison are paint-unit
/// keys (`egui_occlusion_blocks_region`).
pub fn paint_occlusion_blocks_declared_hit_region(
    occluder_target: &WidgetId,
    occluder_order: &HitRegionOrder,
    occluder_authored_z_order: Option<usize>,
    region_target: &WidgetId,
    region_order: &HitRegionOrder,
) -> bool {
    if occluder_target == region_target && occluder_order.z_index == region_order.z_index {
        return occluder_authored_z_order
            .is_some_and(|authored| authored > region_order.paint_order);
    }
    hit_region_order_is_front_of(occluder_order, region_order)
}

pub fn select_declared_hit_region_at_point<'a>(
    hit_regions: &'a [HitRegionDeclaration],
    target_local_position: Point,
) -> Option<&'a HitRegionDeclaration> {
    hit_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| rect_contains_point(region.bounds, target_local_position))
        .max_by(|a, b| compare_hit_region_order(&a.order, &b.order))
}

pub fn select_declared_focus_region_at_point<'a>(
    focus_regions: &'a [FocusRegionDeclaration],
    target_local_position: Point,
) -> Option<&'a FocusRegionDeclaration> {
    focus_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| rect_contains_point(region.bounds, target_local_position))
        .find(|region| region.text_edit.is_some())
}

pub fn declared_region_dispatch_evidence(
    source: EvidenceSource,
    frame: FrameIdentity,
    kind: DeclaredEventDispatchKind,
    input_position: Option<Point>,
    candidate_regions: Vec<PresentationRegionId>,
    selected_region: Option<PresentationRegionId>,
    generated_event: InputEvent,
    route: Option<EventRoute>,
    capture_event: bool,
) -> DeclaredEventDispatchEvidence {
    DeclaredEventDispatchEvidence {
        source,
        frame,
        kind,
        input_position,
        input_position_space: input_position.map(|_| DispatchPositionSpace::Content),
        candidate_regions,
        selected_region,
        refusal_reason: (!capture_event)
            .then(|| "no enabled declaration region matched the backend input".to_string()),
        generated_event: Some(generated_event),
        route,
        capture_event,
        diagnostics: Vec::new(),
    }
}

pub fn declared_focus_text_dispatch_evidence(
    source: EvidenceSource,
    frame: FrameIdentity,
    focus_regions: &[FocusRegionDeclaration],
    selected_region: Option<&FocusRegionDeclaration>,
    kind: DeclaredEventDispatchKind,
    input_position: Option<Point>,
    generated_event: InputEvent,
) -> DeclaredEventDispatchEvidence {
    let text_edit_required = !matches!(
        kind,
        DeclaredEventDispatchKind::Focus | DeclaredEventDispatchKind::Pointer
    );
    let candidate_regions = focus_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| !text_edit_required || region.text_edit.is_some())
        .filter(|region| {
            if let Some(position) = input_position {
                rect_contains_point(region.bounds, position)
            } else if let Some(selected_region) = selected_region {
                region.target == selected_region.target
            } else {
                true
            }
        })
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    let route = selected_region.map(|region| EventRoute {
        route_id: Some(region.id.as_str().to_string()),
        address: region.address.clone(),
        path: route_path_for_address(&region.target, &region.address),
        phase: EventRoutePhase::Target,
    });
    declared_region_dispatch_evidence(
        source,
        frame,
        kind,
        input_position,
        candidate_regions,
        selected_region.map(|region| region.id.clone()),
        generated_event,
        route,
        selected_region.is_some(),
    )
}

pub fn declared_focus_text_dispatch_evidence_for_layout(
    source: EvidenceSource,
    frame: FrameIdentity,
    layout: &LayoutOutput,
    focus_regions: &[FocusRegionDeclaration],
    selected_region: Option<&FocusRegionDeclaration>,
    kind: DeclaredEventDispatchKind,
    input_position: Option<Point>,
    generated_event: InputEvent,
) -> DeclaredEventDispatchEvidence {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    declared_focus_text_dispatch_evidence_with_geometry_index(
        source,
        frame,
        &geometry_index,
        focus_regions,
        selected_region,
        kind,
        input_position,
        generated_event,
    )
}

pub fn declared_focus_text_dispatch_evidence_with_geometry_index(
    source: EvidenceSource,
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    focus_regions: &[FocusRegionDeclaration],
    selected_region: Option<&FocusRegionDeclaration>,
    kind: DeclaredEventDispatchKind,
    input_position: Option<Point>,
    generated_event: InputEvent,
) -> DeclaredEventDispatchEvidence {
    let text_edit_required = !matches!(
        kind,
        DeclaredEventDispatchKind::Focus | DeclaredEventDispatchKind::Pointer
    );
    let candidate_regions = focus_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| !text_edit_required || region.text_edit.is_some())
        .filter(|region| {
            if let Some(position) = input_position {
                declared_region_contains_root_local_point_with_geometry_index(
                    geometry_index,
                    &region.target,
                    region.address.as_ref(),
                    region.bounds.into_rect(),
                    position,
                )
            } else if let Some(selected_region) = selected_region {
                region.target == selected_region.target
            } else {
                true
            }
        })
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    let route = selected_region.map(|region| EventRoute {
        route_id: Some(region.id.as_str().to_string()),
        address: region.address.clone(),
        path: route_path_for_address(&region.target, &region.address),
        phase: EventRoutePhase::Target,
    });
    declared_region_dispatch_evidence(
        source,
        frame,
        kind,
        input_position,
        candidate_regions,
        selected_region.map(|region| region.id.clone()),
        generated_event,
        route,
        selected_region.is_some(),
    )
}

pub fn declared_scroll_dispatch_evidence(
    source: EvidenceSource,
    frame: FrameIdentity,
    scroll_regions: &[ScrollRegionDeclaration],
    selected_region: Option<&ScrollRegionDeclaration>,
    generated_event: InputEvent,
) -> DeclaredEventDispatchEvidence {
    declared_scroll_dispatch_evidence_at_position(
        source,
        frame,
        scroll_regions,
        selected_region,
        generated_event,
        None,
    )
}

/// [`declared_scroll_dispatch_evidence`] with a recorded `input_position`.
///
/// Scroll-sync dispatch is positionless (it mirrors a native scroll offset,
/// not a cursor), so the recorded position is the selected region's declared
/// viewport origin in root content space — the [`DispatchPositionSpace::
/// Content`] anchor that lets evidence positions be cross-checked against
/// declaration rects and dispatch-graph bounds (audit finding MF-M19). Use
/// [`declared_region_root_local_rect_with_geometry_index`] over the region's
/// viewport rect to compute it.
pub fn declared_scroll_dispatch_evidence_at_position(
    source: EvidenceSource,
    frame: FrameIdentity,
    scroll_regions: &[ScrollRegionDeclaration],
    selected_region: Option<&ScrollRegionDeclaration>,
    generated_event: InputEvent,
    input_position: Option<Point>,
) -> DeclaredEventDispatchEvidence {
    let candidate_regions = scroll_regions
        .iter()
        .filter(|region| region.enabled)
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    let route = selected_region.map(|region| EventRoute {
        route_id: Some(region.id.as_str().to_string()),
        address: region.address.clone(),
        path: route_path_for_address(&region.target, &region.address),
        phase: EventRoutePhase::Target,
    });
    declared_region_dispatch_evidence(
        source,
        frame,
        DeclaredEventDispatchKind::Scroll,
        input_position,
        candidate_regions,
        selected_region.map(|region| region.id.clone()),
        generated_event,
        route,
        selected_region.is_some(),
    )
}

pub fn select_declared_scroll_region_at_point<'a>(
    scroll_regions: &'a [ScrollRegionDeclaration],
    target_local_position: Point,
) -> Option<&'a ScrollRegionDeclaration> {
    scroll_regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.consumption.wheel
                && (region.axes.horizontal || region.axes.vertical)
                && rect_contains_point(region.viewport, target_local_position)
        })
        .max_by(|a, b| compare_hit_region_order(&a.order, &b.order))
}

pub fn select_declared_hit_region_at_root_local_point<'a>(
    layout: &LayoutOutput,
    hit_regions: &'a [HitRegionDeclaration],
    root_local_position: Point,
) -> Option<&'a HitRegionDeclaration> {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    select_declared_hit_region_at_root_local_point_with_geometry_index(
        &geometry_index,
        hit_regions,
        root_local_position,
    )
}

pub fn select_declared_hit_region_at_root_local_point_with_geometry_index<'a>(
    geometry_index: &PresentationGeometryIndex,
    hit_regions: &'a [HitRegionDeclaration],
    root_local_position: Point,
) -> Option<&'a HitRegionDeclaration> {
    hit_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| {
            declared_region_contains_root_local_point_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
                root_local_position,
            )
        })
        .max_by(|a, b| compare_hit_region_order(&a.order, &b.order))
}

pub fn select_declared_focus_region_at_root_local_point<'a>(
    layout: &LayoutOutput,
    focus_regions: &'a [FocusRegionDeclaration],
    root_local_position: Point,
) -> Option<&'a FocusRegionDeclaration> {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    select_declared_focus_region_at_root_local_point_with_geometry_index(
        &geometry_index,
        focus_regions,
        root_local_position,
    )
}

pub fn select_declared_focus_region_at_root_local_point_with_geometry_index<'a>(
    geometry_index: &PresentationGeometryIndex,
    focus_regions: &'a [FocusRegionDeclaration],
    root_local_position: Point,
) -> Option<&'a FocusRegionDeclaration> {
    focus_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| {
            declared_region_contains_root_local_point_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
                root_local_position,
            )
        })
        .find(|region| region.text_edit.is_some())
}

pub fn select_declared_scroll_region_at_root_local_point<'a>(
    layout: &LayoutOutput,
    scroll_regions: &'a [ScrollRegionDeclaration],
    root_local_position: Point,
) -> Option<&'a ScrollRegionDeclaration> {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    select_declared_scroll_region_at_root_local_point_with_geometry_index(
        &geometry_index,
        scroll_regions,
        root_local_position,
    )
}

/// Delta-agnostic sibling of
/// [`select_declared_wheel_consumer_at_root_local_point_with_geometry_index`]:
/// selects the structural wheel owner at a point without an at-limit filter
/// (used by the dispatch-graph derivation for `HitOrder`/`Chaining` edges).
///
/// Eligibility is enabled + wheel-consuming + at least one scroll axis +
/// containing the point; the winner among eligible candidates is then decided
/// by the SAME declared `wheel_routing` precedence the delta-aware selector
/// applies (see [`route_declared_wheel_winner_with_geometry_index`]), so the
/// derived graph cannot drift from routed dispatch behavior.
pub fn select_declared_scroll_region_at_root_local_point_with_geometry_index<'a>(
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &'a [ScrollRegionDeclaration],
    root_local_position: Point,
) -> Option<&'a ScrollRegionDeclaration> {
    let eligible = scroll_regions
        .iter()
        .filter(|region| {
            region.enabled
                && region.consumption.wheel
                && (region.axes.horizontal || region.axes.vertical)
                && declared_region_contains_root_local_point_with_geometry_index(
                    geometry_index,
                    &region.target,
                    region.address.as_ref(),
                    region.viewport.into_rect(),
                    root_local_position,
                )
        })
        .collect::<Vec<_>>();
    route_declared_wheel_winner_with_geometry_index(geometry_index, &eligible)
}

pub fn scroll_region_can_consume_wheel_delta(
    region: &ScrollRegionDeclaration,
    delta_x: f32,
    delta_y: f32,
) -> Option<bool> {
    if !region.enabled
        || !region.consumption.wheel
        || (!region.axes.horizontal && !region.axes.vertical)
    {
        return None;
    }

    if delta_x.abs() <= f32::EPSILON && delta_y.abs() <= f32::EPSILON {
        return Some(true);
    }

    Some(
        (region.axes.horizontal
            && scroll_axis_can_consume_wheel_delta(
                region.offset.x,
                region.content_bounds.size.width,
                region.viewport.size.width,
                delta_x,
            ))
            || (region.axes.vertical
                && scroll_axis_can_consume_wheel_delta(
                    region.offset.y,
                    region.content_bounds.size.height,
                    region.viewport.size.height,
                    delta_y,
                )),
    )
}

fn scroll_axis_can_consume_wheel_delta(
    offset: f32,
    content: f32,
    viewport: f32,
    delta: f32,
) -> bool {
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

pub fn declared_pointer_capture_for_region(
    region: &HitRegionDeclaration,
    kind: PointerEventKind,
    pointer_is_pressed: bool,
) -> bool {
    match region.capture {
        PointerCaptureIntent::None => false,
        PointerCaptureIntent::OnPress => matches!(kind, PointerEventKind::Press),
        PointerCaptureIntent::DuringDrag => {
            pointer_is_pressed
                || matches!(kind, PointerEventKind::Release | PointerEventKind::Cancel)
        }
        PointerCaptureIntent::Explicit => true,
    }
}

pub fn declared_target_rect_for_region_address(
    layout: &LayoutOutput,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
) -> Rect {
    if let Some(placement) = address.and_then(|address| {
        layout
            .child_placements
            .iter()
            .find(|placement| placement.local_state_slot.as_ref() == Some(address))
    }) {
        return placement.bounds.into_rect();
    }

    if let Some(placement) = layout
        .child_placements
        .iter()
        .find(|placement| placement.child == *target)
    {
        return placement.bounds.into_rect();
    }

    Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: layout.bounds.size,
    }
}

pub fn declared_target_rect_for_region_address_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
) -> Rect {
    geometry_index.target_rect_for_region_address(target, address)
}

pub fn declared_target_local_point_for_region_address(
    layout: &LayoutOutput,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    root_local_position: Point,
) -> Point {
    let target_rect = declared_target_rect_for_region_address(layout, target, address);
    Point {
        x: root_local_position.x - target_rect.origin.x,
        y: root_local_position.y - target_rect.origin.y,
    }
}

pub fn declared_target_local_point_for_region_address_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    root_local_position: Point,
) -> Point {
    geometry_index.target_local_point_for_region_address(target, address, root_local_position)
}

pub fn declared_region_root_local_rect(
    layout: &LayoutOutput,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    target_local_rect: Rect,
) -> Rect {
    let target_rect = declared_target_rect_for_region_address(layout, target, address);
    Rect {
        origin: Point {
            x: target_rect.origin.x + target_local_rect.origin.x,
            y: target_rect.origin.y + target_local_rect.origin.y,
        },
        size: target_local_rect.size,
    }
}

pub fn declared_region_root_local_rect_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    target_local_rect: Rect,
) -> Rect {
    geometry_index.region_root_local_rect(target, address, target_local_rect)
}

pub fn declared_region_contains_root_local_point(
    layout: &LayoutOutput,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    target_local_rect: Rect,
    root_local_position: Point,
) -> bool {
    let target_local_position = declared_target_local_point_for_region_address(
        layout,
        target,
        address,
        root_local_position,
    );
    rect_contains_point(target_local_rect, target_local_position)
}

pub fn declared_region_contains_root_local_point_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    target_local_rect: Rect,
    root_local_position: Point,
) -> bool {
    geometry_index.region_contains_root_local_point(
        target,
        address,
        target_local_rect,
        root_local_position,
    )
}

pub fn declared_target_local_bounds(target_rect: Rect) -> TargetLocalRect {
    TargetLocalRect::new(Rect {
        origin: Point { x: 0.0, y: 0.0 },
        size: target_rect.size,
    })
}

pub fn declared_pointer_position_for_hit_region(
    region: &HitRegionDeclaration,
    target_local_position: Point,
) -> Point {
    match region.event_coordinate_space {
        PointerEventCoordinateSpace::TargetLocal => target_local_position,
        PointerEventCoordinateSpace::RegionLocal => Point {
            x: target_local_position.x - region.bounds.origin.x,
            y: target_local_position.y - region.bounds.origin.y,
        },
    }
}

pub fn declared_pointer_event_for_hit_region(
    layout: &LayoutOutput,
    region: &HitRegionDeclaration,
    root_local_position: Point,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
) -> InputEvent {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    declared_pointer_event_for_hit_region_with_geometry_index(
        &geometry_index,
        region,
        root_local_position,
        kind,
        button,
        details,
    )
}

pub fn declared_pointer_event_for_hit_region_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    region: &HitRegionDeclaration,
    root_local_position: Point,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
) -> InputEvent {
    let target_rect =
        geometry_index.target_rect_for_region_address(&region.target, region.address.as_ref());
    let target_local_position = geometry_index.target_local_point_for_region_address(
        &region.target,
        region.address.as_ref(),
        root_local_position,
    );
    InputEvent::Pointer(PointerEvent {
        target: region
            .route
            .path
            .last()
            .cloned()
            .unwrap_or_else(|| region.target.clone()),
        target_slot: region
            .route
            .address
            .clone()
            .or_else(|| region.address.clone()),
        position: declared_pointer_position_for_hit_region(region, target_local_position),
        target_bounds: Some(declared_target_local_bounds(target_rect)),
        kind,
        button,
        details,
    })
}

pub fn resolve_declared_pointer_dispatch(
    layout: &LayoutOutput,
    hit_regions: &[HitRegionDeclaration],
    root_local_position: Point,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
    pointer_is_pressed: bool,
) -> Option<DeclaredPointerDispatch> {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    resolve_declared_pointer_dispatch_with_geometry_index(
        &geometry_index,
        hit_regions,
        root_local_position,
        kind,
        button,
        details,
        pointer_is_pressed,
    )
}

pub fn resolve_declared_pointer_dispatch_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    hit_regions: &[HitRegionDeclaration],
    root_local_position: Point,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
    pointer_is_pressed: bool,
) -> Option<DeclaredPointerDispatch> {
    let candidate_regions = hit_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| {
            declared_region_contains_root_local_point_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
                root_local_position,
            )
        })
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    let region = select_declared_hit_region_at_root_local_point_with_geometry_index(
        geometry_index,
        hit_regions,
        root_local_position,
    )?;
    Some(DeclaredPointerDispatch {
        selected_region: region.id.clone(),
        candidate_regions,
        input: declared_pointer_event_for_hit_region_with_geometry_index(
            geometry_index,
            region,
            root_local_position,
            kind,
            button,
            details,
        ),
        route: region.route.clone(),
        capture_event: declared_pointer_capture_for_region(region, kind, pointer_is_pressed),
    })
}

pub fn resolve_declared_pointer_dispatch_with_evidence(
    source: EvidenceSource,
    frame: FrameIdentity,
    layout: &LayoutOutput,
    hit_regions: &[HitRegionDeclaration],
    root_local_position: Point,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
    pointer_is_pressed: bool,
) -> (
    Option<DeclaredPointerDispatch>,
    DeclaredEventDispatchEvidence,
) {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    resolve_declared_pointer_dispatch_with_evidence_and_geometry_index(
        source,
        frame,
        &geometry_index,
        hit_regions,
        root_local_position,
        kind,
        button,
        details,
        pointer_is_pressed,
    )
}

pub fn resolve_declared_pointer_dispatch_with_evidence_and_geometry_index(
    source: EvidenceSource,
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    hit_regions: &[HitRegionDeclaration],
    root_local_position: Point,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
    pointer_is_pressed: bool,
) -> (
    Option<DeclaredPointerDispatch>,
    DeclaredEventDispatchEvidence,
) {
    let dispatch = resolve_declared_pointer_dispatch_with_geometry_index(
        geometry_index,
        hit_regions,
        root_local_position,
        kind,
        button,
        details,
        pointer_is_pressed,
    );
    let candidate_regions = dispatch.as_ref().map_or_else(
        || {
            hit_regions
                .iter()
                .filter(|region| region.enabled)
                .filter(|region| {
                    declared_region_contains_root_local_point_with_geometry_index(
                        geometry_index,
                        &region.target,
                        region.address.as_ref(),
                        region.bounds.into_rect(),
                        root_local_position,
                    )
                })
                .map(|region| region.id.clone())
                .collect::<Vec<_>>()
        },
        |dispatch| dispatch.candidate_regions.clone(),
    );

    let evidence = DeclaredEventDispatchEvidence {
        source,
        frame,
        kind: DeclaredEventDispatchKind::Pointer,
        input_position: Some(root_local_position),
        input_position_space: Some(DispatchPositionSpace::Content),
        candidate_regions,
        selected_region: dispatch
            .as_ref()
            .map(|dispatch| dispatch.selected_region.clone()),
        refusal_reason: dispatch
            .is_none()
            .then(|| "no enabled hit region contained the physical pointer position".to_string()),
        generated_event: dispatch.as_ref().map(|dispatch| dispatch.input.clone()),
        route: dispatch.as_ref().map(|dispatch| dispatch.route.clone()),
        capture_event: dispatch
            .as_ref()
            .is_some_and(|dispatch| dispatch.capture_event),
        diagnostics: Vec::new(),
    };

    (dispatch, evidence)
}

#[allow(clippy::too_many_arguments)]
pub fn resolve_declared_captured_pointer_dispatch_with_evidence_and_geometry_index(
    source: EvidenceSource,
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    hit_regions: &[HitRegionDeclaration],
    captured_region_id: &PresentationRegionId,
    root_local_position: Point,
    kind: PointerEventKind,
    button: Option<PointerButton>,
    details: PointerDetails,
    pointer_is_pressed: bool,
) -> (
    Option<DeclaredPointerDispatch>,
    DeclaredEventDispatchEvidence,
) {
    let mut candidate_regions = hit_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| {
            declared_region_contains_root_local_point_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
                root_local_position,
            )
        })
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();

    let dispatch = hit_regions
        .iter()
        .find(|region| region.enabled && &region.id == captured_region_id)
        .and_then(|region| {
            declared_pointer_capture_for_region(region, kind, pointer_is_pressed).then(|| {
                if !candidate_regions
                    .iter()
                    .any(|candidate| candidate == &region.id)
                {
                    candidate_regions.push(region.id.clone());
                }
                let input = declared_pointer_event_for_hit_region_with_geometry_index(
                    geometry_index,
                    region,
                    root_local_position,
                    kind,
                    button,
                    details.clone(),
                );
                DeclaredPointerDispatch {
                    selected_region: region.id.clone(),
                    candidate_regions: candidate_regions.clone(),
                    input,
                    route: region.route.clone(),
                    capture_event: true,
                }
            })
        });

    let evidence = DeclaredEventDispatchEvidence {
        source,
        frame,
        kind: DeclaredEventDispatchKind::Pointer,
        input_position: Some(root_local_position),
        input_position_space: Some(DispatchPositionSpace::Content),
        candidate_regions,
        selected_region: dispatch
            .as_ref()
            .map(|dispatch| dispatch.selected_region.clone()),
        refusal_reason: dispatch.is_none().then(|| {
            "no enabled captured hit region could retain the physical pointer event".to_string()
        }),
        generated_event: dispatch.as_ref().map(|dispatch| dispatch.input.clone()),
        route: dispatch.as_ref().map(|dispatch| dispatch.route.clone()),
        capture_event: dispatch
            .as_ref()
            .is_some_and(|dispatch| dispatch.capture_event),
        diagnostics: Vec::new(),
    };

    (dispatch, evidence)
}

pub fn resolve_declared_wheel_dispatch(
    layout: &LayoutOutput,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> Option<DeclaredWheelDispatch> {
    resolve_declared_wheel_dispatch_with_boundary(
        layout,
        scroll_regions,
        DeclaredWheelTraversalBoundary::default(),
        root_local_position,
        delta_x,
        delta_y,
    )
}

pub fn resolve_declared_wheel_dispatch_with_boundary(
    layout: &LayoutOutput,
    scroll_regions: &[ScrollRegionDeclaration],
    boundary: DeclaredWheelTraversalBoundary,
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> Option<DeclaredWheelDispatch> {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    resolve_declared_wheel_dispatch_with_geometry_index_and_boundary(
        &geometry_index,
        scroll_regions,
        boundary,
        root_local_position,
        delta_x,
        delta_y,
    )
}

pub fn resolve_declared_wheel_dispatch_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> Option<DeclaredWheelDispatch> {
    resolve_declared_wheel_dispatch_with_geometry_index_and_boundary(
        geometry_index,
        scroll_regions,
        DeclaredWheelTraversalBoundary::default(),
        root_local_position,
        delta_x,
        delta_y,
    )
}

pub fn resolve_declared_wheel_dispatch_with_geometry_index_and_boundary(
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
    boundary: DeclaredWheelTraversalBoundary,
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> Option<DeclaredWheelDispatch> {
    let region =
        select_declared_wheel_consumer_at_root_local_point_with_geometry_index_and_boundary(
            geometry_index,
            scroll_regions,
            boundary,
            root_local_position,
            delta_x,
            delta_y,
        )?;
    Some(declared_wheel_dispatch_for_selected_region(
        geometry_index,
        scroll_regions,
        root_local_position,
        delta_x,
        delta_y,
        region,
    ))
}

fn declared_wheel_dispatch_for_selected_region(
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
    region: &ScrollRegionDeclaration,
) -> DeclaredWheelDispatch {
    let candidate_regions = scroll_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| {
            declared_region_contains_root_local_point_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.viewport.into_rect(),
                root_local_position,
            )
        })
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    DeclaredWheelDispatch {
        selected_region: region.id.clone(),
        candidate_regions,
        input: InputEvent::Wheel(WheelEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            region_id: Some(region.id.clone()),
            delta_x,
            delta_y,
        }),
        route: region_event_route(&region.id, &region.target, &region.address),
        capture_event: region.consumption.wheel,
    }
}

/// Selects the scroll region that consumes a wheel at `root_local_position`
/// for the given delta, honoring each region's DECLARED
/// [`ScrollRegionDeclaration::wheel_routing`] (ADR-0002 B2). Selection is
/// total and deterministic; the precedence, in order:
///
/// 1. Eligibility filter (unchanged from the pre-routing algorithm): only
///    enabled regions that contain the point (through the same
///    geometry-index translation dispatch containment uses) and can consume
///    the delta ([`scroll_region_can_consume_wheel_delta`]) are candidates.
///    A `SelfFirst` region at its consumption limit therefore drops out of
///    the pool and default selection proceeds among the others — a declared
///    routing preference never black-holes the wheel.
/// 2. `SelfFirst` priority: if any candidate declares
///    [`WheelRouting::SelfFirst`], the front-most such candidate (the
///    existing order key, [`compare_hit_region_order`]) wins outright, even
///    over a fronter candidate that would win by order alone.
/// 3. Otherwise the front-most candidate by order key is the pending winner,
///    with [`WheelRouting::ParentFirst`] deference applied before the result
///    is final: while the pending winner declares `ParentFirst` and an
///    eligible ancestor candidate exists, the front-most eligible ancestor
///    becomes the pending winner. "Ancestor" reuses the nesting notion the
///    existing at-limit chaining rule resolves to (the outer containing
///    owner): an ancestor of region R is another candidate whose declared
///    viewport rect, mapped to root-local space through the geometry index,
///    strictly contains R's mapped viewport rect (contains it fully without
///    being mutually containing, i.e. not the same rect). A `ParentFirst`
///    region with no eligible ancestor is selected itself.
/// 4. [`WheelRouting::NearestScrollable`] (the default) and
///    [`WheelRouting::Custom`] apply no override: with only such
///    declarations, steps 2-3 are no-ops and selection is exactly the
///    front-most containing consumable candidate — byte-equivalent to the
///    pre-routing algorithm (the ADR-0002 default-equivalence guarantee).
///
/// The routing mode is read from the declaration snapshot
/// (`ScrollRegionDeclaration.wheel_routing`); per-event dynamic routing
/// stays unsupported, and [`SlipwayWheelRoutingPolicy`]'s signature is
/// declaration-time-only by construction (it receives only the identity of
/// the region being declared, not a live wheel event).
pub fn select_declared_wheel_consumer_at_root_local_point_with_geometry_index<'a>(
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &'a [ScrollRegionDeclaration],
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> Option<&'a ScrollRegionDeclaration> {
    select_declared_wheel_consumer_at_root_local_point_with_geometry_index_and_boundary(
        geometry_index,
        scroll_regions,
        DeclaredWheelTraversalBoundary::default(),
        root_local_position,
        delta_x,
        delta_y,
    )
}

pub fn select_declared_wheel_consumer_at_root_local_point_with_geometry_index_and_boundary<'a>(
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &'a [ScrollRegionDeclaration],
    boundary: DeclaredWheelTraversalBoundary,
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> Option<&'a ScrollRegionDeclaration> {
    match declared_wheel_disposition_at_root_local_point_with_geometry_index(
        geometry_index,
        scroll_regions,
        boundary,
        root_local_position,
        delta_x,
        delta_y,
    ) {
        DeclaredWheelDisposition::Moved(owner) => Some(owner),
        DeclaredWheelDisposition::ConsumedNoOp(_) | DeclaredWheelDisposition::Bubble => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DeclaredWheelDisposition<'a> {
    Moved(&'a ScrollRegionDeclaration),
    ConsumedNoOp(&'a ScrollRegionDeclaration),
    Bubble,
}

/// Resolves movement normally, while allowing only an address-less declared
/// root to absorb an outward wheel at its limit. The terminal check reuses the
/// same declaration slice and creates no candidate collection.
pub fn declared_wheel_disposition_at_root_local_point_with_geometry_index<'a>(
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &'a [ScrollRegionDeclaration],
    boundary: DeclaredWheelTraversalBoundary,
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> DeclaredWheelDisposition<'a> {
    let mut eligible = Vec::new();
    let mut terminal_root = None;
    for (index, region) in scroll_regions.iter().enumerate() {
        let contains = declared_region_contains_root_local_point_with_geometry_index(
            geometry_index,
            &region.target,
            region.address.as_ref(),
            region.viewport.into_rect(),
            root_local_position,
        );
        if !contains {
            continue;
        }
        if scroll_region_can_consume_wheel_delta(region, delta_x, delta_y) == Some(true) {
            eligible.push(region);
        } else if boundary.terminal_region_index == Some(index)
            && region.enabled
            && region.consumption.wheel
            && ((region.axes.horizontal && delta_x.abs() > f32::EPSILON)
                || (region.axes.vertical && delta_y.abs() > f32::EPSILON))
            && terminal_root.is_none_or(|current: &ScrollRegionDeclaration| {
                compare_hit_region_order(&region.order, &current.order).is_gt()
            })
        {
            terminal_root = Some(region);
        }
    }
    if let Some(owner) = route_declared_wheel_winner_with_geometry_index(geometry_index, &eligible)
    {
        return DeclaredWheelDisposition::Moved(owner);
    }
    terminal_root.map_or(DeclaredWheelDisposition::Bubble, |root| {
        DeclaredWheelDisposition::ConsumedNoOp(root)
    })
}

/// Applies the declared wheel-routing precedence over an already-filtered
/// candidate pool (containing + consumable, in declaration order). See
/// [`select_declared_wheel_consumer_at_root_local_point_with_geometry_index`]
/// for the full contract. `max_by` retains the pre-routing selectors'
/// last-max tie behavior, so a pool without `SelfFirst`/`ParentFirst`
/// declarations reproduces the old `filter(..).max_by(order)` result
/// exactly.
fn route_declared_wheel_winner_with_geometry_index<'a>(
    geometry_index: &PresentationGeometryIndex,
    eligible: &[&'a ScrollRegionDeclaration],
) -> Option<&'a ScrollRegionDeclaration> {
    if let Some(self_first) = eligible
        .iter()
        .copied()
        .filter(|region| region.wheel_routing == WheelRouting::SelfFirst)
        .max_by(|a, b| compare_hit_region_order(&a.order, &b.order))
    {
        return Some(self_first);
    }
    let mut winner = eligible
        .iter()
        .copied()
        .max_by(|a, b| compare_hit_region_order(&a.order, &b.order))?;
    // ParentFirst deference: each hop moves to a strictly-containing (hence
    // strictly larger) viewport, so the walk is acyclic and terminates; the
    // hop budget is belt and braces.
    let mut hops = eligible.len();
    while winner.wheel_routing == WheelRouting::ParentFirst && hops > 0 {
        hops -= 1;
        let winner_rect =
            declared_scroll_viewport_root_local_rect_with_geometry_index(geometry_index, winner);
        let Some(ancestor) = eligible
            .iter()
            .copied()
            .filter(|candidate| {
                let candidate_rect = declared_scroll_viewport_root_local_rect_with_geometry_index(
                    geometry_index,
                    candidate,
                );
                rect_contains_rect(candidate_rect, winner_rect)
                    && !rect_contains_rect(winner_rect, candidate_rect)
            })
            .max_by(|a, b| compare_hit_region_order(&a.order, &b.order))
        else {
            break;
        };
        winner = ancestor;
    }
    Some(winner)
}

fn declared_scroll_viewport_root_local_rect_with_geometry_index(
    geometry_index: &PresentationGeometryIndex,
    region: &ScrollRegionDeclaration,
) -> Rect {
    declared_region_root_local_rect_with_geometry_index(
        geometry_index,
        &region.target,
        region.address.as_ref(),
        region.viewport.into_rect(),
    )
}

pub fn resolve_declared_wheel_dispatch_with_evidence(
    source: EvidenceSource,
    frame: FrameIdentity,
    layout: &LayoutOutput,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> (Option<DeclaredWheelDispatch>, DeclaredEventDispatchEvidence) {
    resolve_declared_wheel_dispatch_with_evidence_and_boundary(
        source,
        frame,
        layout,
        scroll_regions,
        DeclaredWheelTraversalBoundary::default(),
        root_local_position,
        delta_x,
        delta_y,
    )
}

pub fn resolve_declared_wheel_dispatch_with_evidence_and_boundary(
    source: EvidenceSource,
    frame: FrameIdentity,
    layout: &LayoutOutput,
    scroll_regions: &[ScrollRegionDeclaration],
    boundary: DeclaredWheelTraversalBoundary,
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> (Option<DeclaredWheelDispatch>, DeclaredEventDispatchEvidence) {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    resolve_declared_wheel_dispatch_with_evidence_geometry_index_and_boundary(
        source,
        frame,
        &geometry_index,
        scroll_regions,
        boundary,
        root_local_position,
        delta_x,
        delta_y,
    )
}

pub fn resolve_declared_wheel_dispatch_with_evidence_and_geometry_index(
    source: EvidenceSource,
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> (Option<DeclaredWheelDispatch>, DeclaredEventDispatchEvidence) {
    resolve_declared_wheel_dispatch_with_evidence_geometry_index_and_boundary(
        source,
        frame,
        geometry_index,
        scroll_regions,
        DeclaredWheelTraversalBoundary::default(),
        root_local_position,
        delta_x,
        delta_y,
    )
}

pub fn resolve_declared_wheel_dispatch_with_evidence_geometry_index_and_boundary(
    source: EvidenceSource,
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
    boundary: DeclaredWheelTraversalBoundary,
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> (Option<DeclaredWheelDispatch>, DeclaredEventDispatchEvidence) {
    let dispatch = resolve_declared_wheel_dispatch_with_geometry_index_and_boundary(
        geometry_index,
        scroll_regions,
        boundary,
        root_local_position,
        delta_x,
        delta_y,
    );
    declared_wheel_dispatch_evidence(
        source,
        frame,
        geometry_index,
        scroll_regions,
        root_local_position,
        dispatch,
    )
}

pub fn resolve_selected_declared_wheel_dispatch_with_evidence_and_geometry_index(
    source: EvidenceSource,
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
    selected: &ScrollRegionDeclaration,
) -> (Option<DeclaredWheelDispatch>, DeclaredEventDispatchEvidence) {
    let dispatch = Some(declared_wheel_dispatch_for_selected_region(
        geometry_index,
        scroll_regions,
        root_local_position,
        delta_x,
        delta_y,
        selected,
    ));
    declared_wheel_dispatch_evidence(
        source,
        frame,
        geometry_index,
        scroll_regions,
        root_local_position,
        dispatch,
    )
}

pub fn resolve_bubbled_declared_wheel_dispatch_with_evidence_and_geometry_index(
    source: EvidenceSource,
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
) -> (Option<DeclaredWheelDispatch>, DeclaredEventDispatchEvidence) {
    declared_wheel_dispatch_evidence(
        source,
        frame,
        geometry_index,
        scroll_regions,
        root_local_position,
        None,
    )
}

fn declared_wheel_dispatch_evidence(
    source: EvidenceSource,
    frame: FrameIdentity,
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
    dispatch: Option<DeclaredWheelDispatch>,
) -> (Option<DeclaredWheelDispatch>, DeclaredEventDispatchEvidence) {
    let candidate_regions = dispatch.as_ref().map_or_else(
        || {
            scroll_regions
                .iter()
                .filter(|region| region.enabled)
                .filter(|region| {
                    declared_region_contains_root_local_point_with_geometry_index(
                        geometry_index,
                        &region.target,
                        region.address.as_ref(),
                        region.viewport.into_rect(),
                        root_local_position,
                    )
                })
                .map(|region| region.id.clone())
                .collect::<Vec<_>>()
        },
        |dispatch| dispatch.candidate_regions.clone(),
    );

    let evidence = DeclaredEventDispatchEvidence {
        source,
        frame,
        kind: DeclaredEventDispatchKind::Wheel,
        input_position: Some(root_local_position),
        input_position_space: Some(DispatchPositionSpace::Content),
        candidate_regions,
        selected_region: dispatch
            .as_ref()
            .map(|dispatch| dispatch.selected_region.clone()),
        refusal_reason: dispatch
            .is_none()
            .then(|| "no enabled scroll region accepted the physical wheel position".to_string()),
        generated_event: dispatch.as_ref().map(|dispatch| dispatch.input.clone()),
        route: dispatch.as_ref().map(|dispatch| dispatch.route.clone()),
        capture_event: dispatch
            .as_ref()
            .is_some_and(|dispatch| dispatch.capture_event),
        diagnostics: Vec::new(),
    };

    (dispatch, evidence)
}

// ---------------------------------------------------------------------------
// Dispatch graph (ADR-0001 Phase A.1 / ADR-0002 action item B1)
//
// A derived, inspectable read-model of the input-routing structure for one
// presented frame. The builder enumerates nodes from the same declarations
// dispatch selects over and enumerates edges by CALLING the same selection
// helpers dispatch uses (`select_declared_hit_region_at_root_local_point_
// with_geometry_index`, `select_declared_scroll_region_at_root_local_point_
// with_geometry_index`, `declared_region_contains_root_local_point_with_
// geometry_index`, `compare_hit_region_order`, `paint_layer_blocks_wheel`),
// so the graph cannot drift from resolution behavior. It is read-only: no
// dispatch path consumes it.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchGraphNodeKind {
    Hit,
    Focus,
    Scroll,
    Occlusion,
}

/// One dispatch target/region of the derived dispatch graph.
///
/// `bounds` is the region's rect mapped into root-local space through the
/// same `PresentationGeometryIndex` translation dispatch containment uses
/// (for scroll nodes this is the viewport rect). The `Option` fields are
/// populated per node kind: `capture` on hit nodes, `consumes_wheel` on
/// scroll nodes, `blocks_pointer`/`blocks_wheel` on occlusion nodes.
#[derive(Clone, Debug, PartialEq)]
pub struct DispatchGraphNode {
    pub id: String,
    pub kind: DispatchGraphNodeKind,
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    pub bounds: Rect,
    pub order: HitRegionOrder,
    pub enabled: bool,
    pub capture: Option<PointerCaptureIntent>,
    pub consumes_wheel: Option<bool>,
    pub blocks_pointer: Option<bool>,
    pub blocks_wheel: Option<bool>,
}

/// The `InputEvent`-kind scope an edge applies to.
///
/// `Pointer` covers `InputEvent::Pointer`; `Wheel` covers
/// `InputEvent::Wheel` (and the `Scroll` events a consumed wheel produces);
/// `FocusRouted` covers the non-spatial kinds (`Keyboard`, `Text`,
/// `TextEdit`, `TextComposition`, `Selection`, `Focus`, `Command`), which
/// route by focus/target and never receive spatial
/// `HitOrder`/`Occlusion`/`Chaining` edges.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchGraphChannel {
    Pointer,
    Wheel,
    FocusRouted,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DispatchGraphEdgeKind {
    /// Front-to-back precedence between two overlapping regions, decided by
    /// the selector's order key. `from` precedes (wins over) `to`.
    HitOrder,
    /// `from` (an occlusion node) blocks the edge channel from reaching
    /// `to` (a hit node on the pointer channel, a scroll node on the wheel
    /// channel) where their bounds overlap and the occluder's order key is
    /// strictly front of the region's.
    Occlusion,
    /// `from` (a hit node with `PointerCaptureIntent != None`) redirects
    /// captured pointer events to the `to` widget regardless of position.
    Capture,
    /// When `from` (a scroll node) cannot consume a wheel delta (at-limit or
    /// non-consuming), the same selector that picked it chains the wheel to
    /// `to` (the next front-most containing consumable scroll node).
    Chaining,
    /// `from` (a focus node) routes focus-routed event kinds to the `to`
    /// widget.
    FocusRoute,
}

/// One typed routing edge. `from` is always a node id. `to` is a node id for
/// the spatial edge kinds (`HitOrder`, `Occlusion`, `Chaining`) and a target
/// widget id for the routing edge kinds (`Capture`, `FocusRoute`).
#[derive(Clone, Debug, PartialEq)]
pub struct DispatchGraphEdge {
    pub kind: DispatchGraphEdgeKind,
    pub channel: DispatchGraphChannel,
    pub from: String,
    pub to: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DispatchGraph {
    pub target: WidgetId,
    pub nodes: Vec<DispatchGraphNode>,
    pub edges: Vec<DispatchGraphEdge>,
}

/// Occlusion input for dispatch-graph derivation: one pointer-opaque paint
/// layer in root-local space. `order` is the paint-unit sort key of the
/// layer ([`paint_unit_sort_key`]), `authored_z_order` is the layer's
/// AUTHORED within-z order when one was declared
/// ([`paint_unit_authored_z_order`] — the only tie-break component
/// same-owner occlusion comparisons may use, NC-2), `blocks_wheel` is the
/// resolved wheel-channel opacity ([`paint_layer_blocks_wheel`]).
/// Pointer-opaque is implied: both backends only materialize occluders for
/// pointer-opaque layers.
#[derive(Clone, Debug, PartialEq)]
pub struct DispatchGraphOcclusionRegion {
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    pub order: HitRegionOrder,
    pub authored_z_order: Option<usize>,
    pub bounds: Rect,
    pub blocks_wheel: bool,
}

/// Probe product wrapper binding a derived dispatch graph to the frame
/// identity it was derived for.
#[derive(Clone, Debug, PartialEq)]
pub struct DispatchGraphProbe {
    pub target: WidgetId,
    pub frame: FrameIdentity,
    pub graph: DispatchGraph,
}

/// Collects [`DispatchGraphOcclusionRegion`]s from a view's authored paint,
/// using the same paint-unit expansion (`expand_paint_unit_layers`), the
/// same unit sort key (`paint_unit_sort_key`), the same pointer-opacity
/// eligibility, and the same wheel-channel resolution
/// (`paint_layer_blocks_wheel`) both visible backends use to materialize
/// their occlusion regions.
pub fn dispatch_graph_occlusion_regions_for_view(
    view: &ViewDefinition,
) -> Vec<DispatchGraphOcclusionRegion> {
    let units = expand_paint_unit_layers(PaintUnit::from_view_ref(view, 0));
    let mut regions = Vec::new();
    for unit in &units {
        let key = paint_unit_sort_key(unit);
        collect_dispatch_graph_occlusion_regions(
            &unit.paint,
            &unit.target,
            unit.address.as_ref(),
            HitRegionOrder {
                z_index: key.0,
                paint_order: key.1,
                traversal_order: key.2,
            },
            paint_unit_authored_z_order(unit),
            None,
            &mut regions,
        );
    }
    regions.sort_by(|a, b| {
        compare_hit_region_order(&a.order, &b.order)
            .then_with(|| a.target.as_str().cmp(b.target.as_str()))
    });
    regions
}

/// Collects [`DispatchGraphOcclusionRegion`]s for a COMPOSED root view: the
/// root view's own paint plus the paint of every authored child, built
/// through the same child view pipeline the backends run (child view at its
/// placement's layout input, paint-unit expansion, unit sort key) and
/// translated into root-local space by the child's placement origin - the
/// same translation the backends apply when they merge child occluders into
/// the root dispatch context.
///
/// Structural read-model note: live backends additionally clip merged child
/// occluders to the currently visible band of native/ancestor scrolls; the
/// derived graph keeps the un-displaced declared geometry, consistent with
/// every other node in the graph.
pub fn dispatch_graph_occlusion_regions_for_composed_view<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    view: &ViewDefinition,
) -> Vec<DispatchGraphOcclusionRegion>
where
    W: SlipwaySsot + SlipwayViewDefinition,
{
    let mut regions = dispatch_graph_occlusion_regions_for_view(view);
    let mut visitor = DispatchGraphChildOcclusionVisitor {
        placements: &view.layout.child_placements,
        frame: &view.frame,
        regions: &mut regions,
    };
    widget.visit_authored_children(external, local, &mut visitor);
    regions.sort_by(|a, b| {
        compare_hit_region_order(&a.order, &b.order)
            .then_with(|| a.target.as_str().cmp(b.target.as_str()))
    });
    regions
}

/// Derives the composed dispatch graph for a widget tree: the root view's
/// declared regions plus occlusion inputs composed across authored children
/// ([`dispatch_graph_occlusion_regions_for_composed_view`]).
pub fn derive_dispatch_graph_for_composed_view<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    view: &ViewDefinition,
) -> DispatchGraph
where
    W: SlipwaySsot + SlipwayViewDefinition,
{
    let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
    let occlusions =
        dispatch_graph_occlusion_regions_for_composed_view(widget, external, local, view);
    derive_dispatch_graph_with_geometry_index(
        &view.target,
        &geometry_index,
        &view.hit_regions,
        &view.focus_regions,
        &view.scroll_regions,
        &occlusions,
    )
}

struct DispatchGraphChildOcclusionVisitor<'a> {
    placements: &'a [ChildPlacement],
    frame: &'a FrameIdentity,
    regions: &'a mut Vec<DispatchGraphOcclusionRegion>,
}

impl<ExternalState, AppMessage> SlipwayWidgetListVisitor<ExternalState, AppMessage>
    for DispatchGraphChildOcclusionVisitor<'_>
{
    fn visit_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayWidget<ExternalState = ExternalState, AppMessage = AppMessage>
            + SlipwayViewDefinition,
    {
        let child = widget.id();
        let Some(placement) = self.placements.iter().find(|placement| {
            placement
                .local_state_slot
                .as_ref()
                .map_or(placement.child == child, |placed| placed == &slot)
        }) else {
            return;
        };
        let placement_rect = placement.bounds.into_rect();
        let child_bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: placement_rect.size,
        };
        let layout_input = LayoutInput {
            viewport: TargetLocalRect::new(child_bounds),
            content: TargetLocalRect::new(child_bounds),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: child_bounds.size,
            },
        };
        let mut frame = self.frame.clone();
        frame.viewport = child_bounds;
        let child_view = widget.visible_backend_view_definition(
            external,
            local,
            ViewDefinitionInput::new(frame, layout_input),
        );
        let units = expand_paint_unit_layers(PaintUnit::from_view_ref(&child_view, slot.ordinal));
        for unit in &units {
            let key = paint_unit_sort_key(unit);
            let start = self.regions.len();
            collect_dispatch_graph_occlusion_regions(
                &unit.paint,
                &child,
                Some(&slot),
                HitRegionOrder {
                    z_index: key.0,
                    paint_order: key.1,
                    traversal_order: key.2,
                },
                paint_unit_authored_z_order(unit),
                None,
                self.regions,
            );
            for region in &mut self.regions[start..] {
                region.bounds = translate_rect(region.bounds, placement_rect.origin);
            }
        }
    }
}

fn collect_dispatch_graph_occlusion_regions(
    ops: &[PaintOp],
    target: &WidgetId,
    address: Option<&WidgetSlotAddress>,
    order: HitRegionOrder,
    authored_z_order: Option<usize>,
    clip: Option<Rect>,
    out: &mut Vec<DispatchGraphOcclusionRegion>,
) {
    for op in ops {
        match op {
            PaintOp::Group {
                clip: group_clip,
                ops,
                ..
            } => {
                collect_dispatch_graph_occlusion_regions(
                    ops,
                    target,
                    address,
                    order.clone(),
                    authored_z_order,
                    dispatch_graph_combine_clips(clip, group_clip.as_ref().map(|clip| clip.bounds)),
                    out,
                );
            }
            PaintOp::Layer {
                input_transparency,
                wheel_transparency,
                clip: layer_clip,
                ops,
                ..
            } => {
                let active_clip =
                    dispatch_graph_combine_clips(clip, layer_clip.as_ref().map(|clip| clip.bounds));
                if *input_transparency == PaintInputTransparency::Opaque
                    && let Some(bounds) = dispatch_graph_paint_ops_bounds(ops)
                        .and_then(|bounds| dispatch_graph_clip_rect(bounds, active_clip))
                {
                    out.push(DispatchGraphOcclusionRegion {
                        target: target.clone(),
                        address: address.cloned(),
                        order: order.clone(),
                        authored_z_order,
                        bounds,
                        blocks_wheel: paint_layer_blocks_wheel(
                            *input_transparency,
                            *wheel_transparency,
                        ),
                    });
                }
                collect_dispatch_graph_occlusion_regions(
                    ops,
                    target,
                    address,
                    order.clone(),
                    authored_z_order,
                    active_clip,
                    out,
                );
            }
            PaintOp::Fill { .. } | PaintOp::Stroke { .. } | PaintOp::Text { .. } => {}
        }
    }
}

fn dispatch_graph_paint_ops_bounds(ops: &[PaintOp]) -> Option<Rect> {
    ops.iter()
        .filter_map(dispatch_graph_paint_op_bounds)
        .reduce(dispatch_graph_union_rects)
}

fn dispatch_graph_paint_op_bounds(op: &PaintOp) -> Option<Rect> {
    match op {
        PaintOp::Fill { shape, .. } | PaintOp::Stroke { shape, .. } => {
            Some(shape.clip.as_ref().map_or(shape.bounds, |clip| {
                dispatch_graph_intersect_rects(shape.bounds, clip.bounds).unwrap_or(clip.bounds)
            }))
        }
        PaintOp::Text { bounds, .. } => Some(*bounds),
        PaintOp::Group { clip, ops, .. } | PaintOp::Layer { clip, ops, .. } => {
            dispatch_graph_paint_ops_bounds(ops).and_then(|bounds| {
                dispatch_graph_clip_rect(bounds, clip.as_ref().map(|clip| clip.bounds))
            })
        }
    }
}

fn dispatch_graph_combine_clips(current: Option<Rect>, next: Option<Rect>) -> Option<Rect> {
    match (current, next) {
        (Some(left), Some(right)) => dispatch_graph_intersect_rects(left, right),
        (Some(rect), None) | (None, Some(rect)) => Some(rect),
        (None, None) => None,
    }
}

fn dispatch_graph_clip_rect(rect: Rect, clip: Option<Rect>) -> Option<Rect> {
    clip.map_or(Some(rect), |clip| {
        dispatch_graph_intersect_rects(rect, clip)
    })
}

fn dispatch_graph_union_rects(a: Rect, b: Rect) -> Rect {
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

fn dispatch_graph_intersect_rects(a: Rect, b: Rect) -> Option<Rect> {
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

fn rect_center(rect: Rect) -> Point {
    Point {
        x: rect.origin.x + rect.size.width / 2.0,
        y: rect.origin.y + rect.size.height / 2.0,
    }
}

fn dispatch_graph_occlusion_node_id(
    region: &DispatchGraphOcclusionRegion,
    duplicate_index: usize,
) -> String {
    format!(
        "occlusion:{}:{}:{}:{}:{}",
        region.target.as_str(),
        region.order.z_index,
        region.order.paint_order,
        region.order.traversal_order,
        duplicate_index
    )
}

/// Derives the dispatch graph for a backend-assembled `ViewDefinition`: the
/// geometry index from its layout (the same index dispatch containment
/// uses), occlusion inputs from its authored paint
/// ([`dispatch_graph_occlusion_regions_for_view`]), and nodes/edges from its
/// declared regions via [`derive_dispatch_graph_with_geometry_index`].
pub fn derive_dispatch_graph_for_view(view: &ViewDefinition) -> DispatchGraph {
    let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
    let occlusions = dispatch_graph_occlusion_regions_for_view(view);
    derive_dispatch_graph_with_geometry_index(
        &view.target,
        &geometry_index,
        &view.hit_regions,
        &view.focus_regions,
        &view.scroll_regions,
        &occlusions,
    )
}

/// Single-source dispatch-graph derivation (ADR-0002 B1).
///
/// Edge enumeration calls the SAME resolver helpers dispatch uses:
///
/// * `HitOrder` (pointer): for each overlapping enabled hit-region pair the
///   winner is decided by calling
///   [`select_declared_hit_region_at_root_local_point_with_geometry_index`]
///   on exactly that pair at a point inside the overlap.
/// * `HitOrder` (wheel): same, over scroll regions through
///   [`select_declared_scroll_region_at_root_local_point_with_geometry_index`].
/// * `Occlusion`: occluder bounds overlap the region's root-local rect and
///   the occluder's order key is strictly front of the region's per
///   [`hit_region_order_is_front_of`]; the wheel channel additionally
///   requires the occluder's resolved `blocks_wheel`
///   ([`paint_layer_blocks_wheel`]).
/// * `Chaining`: the wheel selector skips an at-limit region through its
///   can-consume filter; the chain target is derived by re-running
///   [`select_declared_scroll_region_at_root_local_point_with_geometry_index`]
///   at a point inside the region with that region removed from the
///   candidates - the same "next front-most containing consumable owner"
///   the at-limit skip produces. Because both wheel selectors honor the
///   declared `wheel_routing` (ADR-0002 B2), `SelfFirst`/`ParentFirst`
///   declarations reshape the wheel `HitOrder` and `Chaining` edges here
///   exactly as they reshape dispatch.
/// * `Capture` / `FocusRoute`: declared capture intents and enabled focus
///   regions, routed to the same target the generated events use.
pub fn derive_dispatch_graph_with_geometry_index(
    target: &WidgetId,
    geometry_index: &PresentationGeometryIndex,
    hit_regions: &[HitRegionDeclaration],
    focus_regions: &[FocusRegionDeclaration],
    scroll_regions: &[ScrollRegionDeclaration],
    occlusion_regions: &[DispatchGraphOcclusionRegion],
) -> DispatchGraph {
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    // Deterministic occlusion node identity regardless of the caller's
    // collection order: sort by the resolver's order key, then target.
    let mut occlusion_regions: Vec<DispatchGraphOcclusionRegion> = occlusion_regions.to_vec();
    occlusion_regions.sort_by(|a, b| {
        compare_hit_region_order(&a.order, &b.order)
            .then_with(|| a.target.as_str().cmp(b.target.as_str()))
    });
    let occlusion_regions = occlusion_regions.as_slice();

    let hit_root_rects: Vec<Rect> = hit_regions
        .iter()
        .map(|region| {
            declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
            )
        })
        .collect();
    let scroll_root_rects: Vec<Rect> = scroll_regions
        .iter()
        .map(|region| {
            declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.viewport.into_rect(),
            )
        })
        .collect();

    for (region, root_rect) in hit_regions.iter().zip(&hit_root_rects) {
        nodes.push(DispatchGraphNode {
            id: region.id.as_str().to_string(),
            kind: DispatchGraphNodeKind::Hit,
            target: region.target.clone(),
            address: region.address.clone(),
            bounds: *root_rect,
            order: region.order.clone(),
            enabled: region.enabled,
            capture: Some(region.capture),
            consumes_wheel: None,
            blocks_pointer: None,
            blocks_wheel: None,
        });
    }
    for region in focus_regions {
        nodes.push(DispatchGraphNode {
            id: region.id.as_str().to_string(),
            kind: DispatchGraphNodeKind::Focus,
            target: region.target.clone(),
            address: region.address.clone(),
            bounds: declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
            ),
            order: HitRegionOrder::default(),
            enabled: region.enabled,
            capture: None,
            consumes_wheel: None,
            blocks_pointer: None,
            blocks_wheel: None,
        });
    }
    for (region, root_rect) in scroll_regions.iter().zip(&scroll_root_rects) {
        nodes.push(DispatchGraphNode {
            id: region.id.as_str().to_string(),
            kind: DispatchGraphNodeKind::Scroll,
            target: region.target.clone(),
            address: region.address.clone(),
            bounds: *root_rect,
            order: region.order.clone(),
            enabled: region.enabled,
            capture: None,
            consumes_wheel: Some(region.consumption.wheel),
            blocks_pointer: None,
            blocks_wheel: None,
        });
    }
    let mut occlusion_node_ids = Vec::with_capacity(occlusion_regions.len());
    for (index, region) in occlusion_regions.iter().enumerate() {
        let duplicate_index = occlusion_regions[..index]
            .iter()
            .filter(|earlier| earlier.target == region.target && earlier.order == region.order)
            .count();
        let id = dispatch_graph_occlusion_node_id(region, duplicate_index);
        occlusion_node_ids.push(id.clone());
        nodes.push(DispatchGraphNode {
            id,
            kind: DispatchGraphNodeKind::Occlusion,
            target: region.target.clone(),
            address: region.address.clone(),
            bounds: region.bounds,
            order: region.order.clone(),
            enabled: true,
            capture: None,
            consumes_wheel: None,
            blocks_pointer: Some(true),
            blocks_wheel: Some(region.blocks_wheel),
        });
    }

    // HitOrder (pointer): overlapping enabled hit-region pairs, winner via
    // the pair-restricted hit selector at a point inside the overlap.
    for left in 0..hit_regions.len() {
        if !hit_regions[left].enabled {
            continue;
        }
        for right in (left + 1)..hit_regions.len() {
            if !hit_regions[right].enabled {
                continue;
            }
            let Some(overlap) =
                dispatch_graph_intersect_rects(hit_root_rects[left], hit_root_rects[right])
            else {
                continue;
            };
            let probe = rect_center(overlap);
            let pair = [hit_regions[left].clone(), hit_regions[right].clone()];
            let both_contain = pair.iter().all(|region| {
                declared_region_contains_root_local_point_with_geometry_index(
                    geometry_index,
                    &region.target,
                    region.address.as_ref(),
                    region.bounds.into_rect(),
                    probe,
                )
            });
            if !both_contain {
                continue;
            }
            let Some(winner) = select_declared_hit_region_at_root_local_point_with_geometry_index(
                geometry_index,
                &pair,
                probe,
            ) else {
                continue;
            };
            let (from, to) = if winner.id == hit_regions[left].id {
                (&hit_regions[left].id, &hit_regions[right].id)
            } else {
                (&hit_regions[right].id, &hit_regions[left].id)
            };
            edges.push(DispatchGraphEdge {
                kind: DispatchGraphEdgeKind::HitOrder,
                channel: DispatchGraphChannel::Pointer,
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            });
        }
    }

    // HitOrder (wheel): overlapping wheel-eligible scroll-region pairs,
    // winner via the pair-restricted scroll selector. Eligibility of each
    // region is decided by the selector itself on a single-region slice.
    for left in 0..scroll_regions.len() {
        for right in (left + 1)..scroll_regions.len() {
            let Some(overlap) =
                dispatch_graph_intersect_rects(scroll_root_rects[left], scroll_root_rects[right])
            else {
                continue;
            };
            let probe = rect_center(overlap);
            let both_eligible = [left, right].into_iter().all(|index| {
                select_declared_scroll_region_at_root_local_point_with_geometry_index(
                    geometry_index,
                    std::slice::from_ref(&scroll_regions[index]),
                    probe,
                )
                .is_some()
            });
            if !both_eligible {
                continue;
            }
            let pair = [scroll_regions[left].clone(), scroll_regions[right].clone()];
            let Some(winner) =
                select_declared_scroll_region_at_root_local_point_with_geometry_index(
                    geometry_index,
                    &pair,
                    probe,
                )
            else {
                continue;
            };
            let (from, to) = if winner.id == scroll_regions[left].id {
                (&scroll_regions[left].id, &scroll_regions[right].id)
            } else {
                (&scroll_regions[right].id, &scroll_regions[left].id)
            };
            edges.push(DispatchGraphEdge {
                kind: DispatchGraphEdgeKind::HitOrder,
                channel: DispatchGraphChannel::Wheel,
                from: from.as_str().to_string(),
                to: to.as_str().to_string(),
            });
        }
    }

    // Occlusion: occluder overlaps the region's root-local rect and is
    // strictly front of it. Pointer channel targets hit regions through
    // [`paint_occlusion_blocks_declared_hit_region`] (same-owner same-z
    // exemption, NC-2) — the same predicate the iced pointer occlusion
    // filter dispatches through; wheel channel targets wheel-consuming
    // scroll regions and additionally requires the occluder's resolved
    // wheel opacity.
    for (occlusion, occlusion_id) in occlusion_regions.iter().zip(&occlusion_node_ids) {
        for (region, root_rect) in hit_regions.iter().zip(&hit_root_rects) {
            if region.enabled
                && dispatch_graph_intersect_rects(occlusion.bounds, *root_rect).is_some()
                && paint_occlusion_blocks_declared_hit_region(
                    &occlusion.target,
                    &occlusion.order,
                    occlusion.authored_z_order,
                    &region.target,
                    &region.order,
                )
            {
                edges.push(DispatchGraphEdge {
                    kind: DispatchGraphEdgeKind::Occlusion,
                    channel: DispatchGraphChannel::Pointer,
                    from: occlusion_id.clone(),
                    to: region.id.as_str().to_string(),
                });
            }
        }
        if !occlusion.blocks_wheel {
            continue;
        }
        for (region, root_rect) in scroll_regions.iter().zip(&scroll_root_rects) {
            if region.enabled
                && region.consumption.wheel
                && dispatch_graph_intersect_rects(occlusion.bounds, *root_rect).is_some()
                && hit_region_order_is_front_of(&occlusion.order, &region.order)
            {
                edges.push(DispatchGraphEdge {
                    kind: DispatchGraphEdgeKind::Occlusion,
                    channel: DispatchGraphChannel::Wheel,
                    from: occlusion_id.clone(),
                    to: region.id.as_str().to_string(),
                });
            }
        }
    }

    // Capture: declared capture intents redirect captured pointer events to
    // the same target the generated pointer event routes to.
    for region in hit_regions {
        if region.enabled && region.capture != PointerCaptureIntent::None {
            let capture_target = region
                .route
                .path
                .last()
                .cloned()
                .unwrap_or_else(|| region.target.clone());
            edges.push(DispatchGraphEdge {
                kind: DispatchGraphEdgeKind::Capture,
                channel: DispatchGraphChannel::Pointer,
                from: region.id.as_str().to_string(),
                to: capture_target.as_str().to_string(),
            });
        }
    }

    // Chaining: for each wheel-eligible scroll region, the owner that
    // receives the wheel when this region cannot consume is the selector's
    // choice over the remaining regions at a point inside this region.
    for (index, region) in scroll_regions.iter().enumerate() {
        let probe = rect_center(scroll_root_rects[index]);
        let self_eligible = select_declared_scroll_region_at_root_local_point_with_geometry_index(
            geometry_index,
            std::slice::from_ref(region),
            probe,
        )
        .is_some();
        if !self_eligible {
            continue;
        }
        let others: Vec<ScrollRegionDeclaration> = scroll_regions
            .iter()
            .enumerate()
            .filter(|(other, _)| *other != index)
            .map(|(_, other)| other.clone())
            .collect();
        if let Some(next) = select_declared_scroll_region_at_root_local_point_with_geometry_index(
            geometry_index,
            &others,
            probe,
        ) {
            edges.push(DispatchGraphEdge {
                kind: DispatchGraphEdgeKind::Chaining,
                channel: DispatchGraphChannel::Wheel,
                from: region.id.as_str().to_string(),
                to: next.id.as_str().to_string(),
            });
        }
    }

    // FocusRoute: enabled focus regions route focus-routed event kinds to
    // the widget instance at the end of their route path.
    for region in focus_regions {
        if region.enabled {
            let route_target = route_path_for_address(&region.target, &region.address)
                .last()
                .cloned()
                .unwrap_or_else(|| region.target.clone());
            edges.push(DispatchGraphEdge {
                kind: DispatchGraphEdgeKind::FocusRoute,
                channel: DispatchGraphChannel::FocusRouted,
                from: region.id.as_str().to_string(),
                to: route_target.as_str().to_string(),
            });
        }
    }

    DispatchGraph {
        target: target.clone(),
        nodes,
        edges,
    }
}

/// Shared trailing hint for the geometry-family admission diagnostics: the
/// most common cause of an out-of-bounds declaration is authoring window or
/// parent coordinates where the contract requires target-local ones.
const TARGET_LOCAL_BOUNDS_HINT: &str =
    "bounds are target-local (origin 0,0); window/parent placement belongs in ChildPlacement";

fn contract_rect_display(rect: impl Into<Rect>) -> String {
    let rect = rect.into();
    format!(
        "({}, {}, {}, {})",
        rect.origin.x, rect.origin.y, rect.size.width, rect.size.height
    )
}

fn validate_hit_regions(
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for region in &view.hit_regions {
        if !rect_is_valid(region.bounds) {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.hit_bounds_invalid",
                "Hit region bounds must be finite and non-negative",
            ));
        } else if !view.paint_order.allow_overflow_paint {
            let permitted = declared_target_local_bounds(
                declared_target_rect_for_region_address_with_geometry_index(
                    geometry_index,
                    &region.target,
                    region.address.as_ref(),
                ),
            );
            if !rect_contains_rect(permitted, region.bounds) {
                diagnostics.push(Diagnostic::error(
                    Some(region.target.clone()),
                    "view_contract.hit_bounds_outside_layout",
                    format!(
                        "Enabled hit regions must stay inside layout bounds unless overflow paint is explicitly allowed: region `{}` declared bounds {} vs layout {}; {TARGET_LOCAL_BOUNDS_HINT}",
                        region.id.as_str(),
                        contract_rect_display(region.bounds),
                        contract_rect_display(permitted),
                    ),
                ));
            }
        } else if let Some(overflow_bounds) = view.paint_order.overflow_bounds {
            let root_bounds = declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
            );
            if !rect_contains_rect(overflow_bounds, root_bounds) {
                diagnostics.push(Diagnostic::error(
                    Some(region.target.clone()),
                    "view_contract.hit_bounds_outside_overflow_bounds",
                    format!(
                        "Enabled hit regions must stay inside declared overflow bounds: region `{}` declared bounds {} (root-local {}) vs overflow bounds {}; {TARGET_LOCAL_BOUNDS_HINT}",
                        region.id.as_str(),
                        contract_rect_display(region.bounds),
                        contract_rect_display(root_bounds),
                        contract_rect_display(overflow_bounds),
                    ),
                ));
            }
        }

        if region.enabled && region.route.path.is_empty() {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.hit_route_empty",
                "Enabled hit regions must declare a non-empty event route path",
            ));
        } else if region.enabled
            && !region
                .route
                .path
                .iter()
                .any(|target| target == &region.target)
        {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.hit_route_target_missing",
                "Enabled hit region event route path must contain the hit region target",
            ));
        }

        if region.enabled && region.address != region.route.address {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.hit_route_address_mismatch",
                "Hit region address and event route address must match so backend dispatch can reach the same widget instance",
            ));
        }
    }

    // NC-11 split (roadmap Phase 6 item 5): paint-overlap allowance
    // (`allow_overlap`) is a PAINT declaration and must not disarm the
    // hit-ambiguity guard — the naive-consumer modal set it for paint
    // reasons and a genuine hit tie on such a view shipped unseen. Only
    // the explicit `allow_ambiguous_hits` acceptance silences this check
    // (the wheel twin `ambiguous_wheel_overlap` never had a disarm).
    if view.paint_order.allow_ambiguous_hits {
        return;
    }

    for left_index in 0..view.hit_regions.len() {
        let left = &view.hit_regions[left_index];
        if !left.enabled || !rect_is_valid(left.bounds) {
            continue;
        }

        for right in view.hit_regions.iter().skip(left_index + 1) {
            if !right.enabled || !rect_is_valid(right.bounds) {
                continue;
            }

            let left_bounds = declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &left.target,
                left.address.as_ref(),
                left.bounds.into_rect(),
            );
            let right_bounds = declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &right.target,
                right.address.as_ref(),
                right.bounds.into_rect(),
            );
            if left.order == right.order && rects_intersect(left_bounds, right_bounds) {
                diagnostics.push(Diagnostic::error(
                    Some(view.target.clone()),
                    "view_contract.ambiguous_hit_overlap",
                    format!(
                        "Enabled hit regions overlap with identical ordering: `{}` and `{}` share HitRegionOrder {{ z_index: {}, paint_order: {}, traversal_order: {} }}; give each region a distinct HitRegionOrder (the `order` argument of hit_region_from_pointer_capability) or make the hit geometry disjoint. Paint-overlap allowance (allow_overlap) does not accept hit ambiguity; only PaintOrderDeclaration::allow_ambiguous_hits does, and it makes pointer selection in the shared area arbitrary",
                        left.id.as_str(),
                        right.id.as_str(),
                        left.order.z_index,
                        left.order.paint_order,
                        left.order.traversal_order,
                    ),
                ));
            }
        }
    }
}

fn validate_focus_regions(
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for region in &view.focus_regions {
        if !rect_is_valid(region.bounds) {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.focus_bounds_invalid",
                "Focus region bounds must be finite and non-negative",
            ));
        } else if region.enabled && !view.paint_order.allow_overflow_paint {
            let permitted = declared_target_local_bounds(
                declared_target_rect_for_region_address_with_geometry_index(
                    geometry_index,
                    &region.target,
                    region.address.as_ref(),
                ),
            );
            if !rect_contains_rect(permitted, region.bounds) {
                diagnostics.push(Diagnostic::error(
                    Some(region.target.clone()),
                    "view_contract.focus_bounds_outside_layout",
                    format!(
                        "Enabled focus regions must stay inside layout bounds unless overflow paint is explicitly allowed: region `{}` declared bounds {} vs layout {}; {TARGET_LOCAL_BOUNDS_HINT}",
                        region.id.as_str(),
                        contract_rect_display(region.bounds),
                        contract_rect_display(permitted),
                    ),
                ));
            }
        } else if region.enabled
            && let Some(overflow_bounds) = view.paint_order.overflow_bounds
        {
            let root_bounds = declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.bounds.into_rect(),
            );
            if !rect_contains_rect(overflow_bounds, root_bounds) {
                diagnostics.push(Diagnostic::error(
                    Some(region.target.clone()),
                    "view_contract.focus_bounds_outside_overflow_bounds",
                    format!(
                        "Enabled focus regions must stay inside declared overflow bounds: region `{}` declared bounds {} (root-local {}) vs overflow bounds {}; {TARGET_LOCAL_BOUNDS_HINT}",
                        region.id.as_str(),
                        contract_rect_display(region.bounds),
                        contract_rect_display(root_bounds),
                        contract_rect_display(overflow_bounds),
                    ),
                ));
            }
        }

        if let Some(text_edit) = &region.text_edit {
            validate_text_edit_region(region, text_edit, diagnostics);
        }
    }
}

fn validate_view_capabilities(
    view: &ViewDefinition,
    capabilities: &[Capability],
    diagnostics: &mut Vec<Diagnostic>,
) {
    if (capabilities.contains(&Capability::PointerInput)
        || capabilities.contains(&Capability::HitRegionPresentation))
        && !view.hit_regions.iter().any(|region| region.enabled)
    {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.pointer_capability_missing_hit_region",
            "Widgets declaring pointer input or hit-region presentation must expose at least one enabled hit region; use hit_region_from_pointer_capability or remove the capability",
        ));
    }

    if (capabilities.contains(&Capability::FocusInput)
        || capabilities.contains(&Capability::KeyboardInput)
        || capabilities.contains(&Capability::FocusRegionPresentation))
        && !view.focus_regions.iter().any(|region| region.enabled)
    {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.focus_capability_missing_focus_region",
            "Widgets declaring focus, keyboard input, or focus-region presentation must expose at least one enabled focus region; use focus_region_from_focus_capability (text_edit_focus_region_from_capability for text-input widgets) or remove the capability",
        ));
    }

    if (capabilities.contains(&Capability::TextInput)
        || capabilities.contains(&Capability::TextEditRegionPresentation))
        && !view
            .focus_regions
            .iter()
            .any(|region| region.enabled && region.text_edit.is_some())
    {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.text_input_missing_text_edit_focus_region",
            "Widgets declaring Capability::TextInput must expose at least one enabled focus region with a TextEditRegionDeclaration; use text_edit_focus_region_from_capability or remove TextInput",
        ));
    }

    // NC-10 deliverability advisory (roadmap Phase 6 item 5): declaring
    // KeyboardInput on plain (non-text-edit) focus regions admits, but the
    // iced visible backend delivers keyboard input ONLY to text-edit focus
    // regions — real ingress routes exclusively through text-edit regions
    // and physical control refuses with
    // `native-physical-control-text-focus-widget-unavailable` — so the
    // widget's keyboard handlers are unreachable there (egui delivers once
    // the region is focused). Warning, not blocking: the capability IS
    // deliverable on egui. Suppressed when any enabled text-edit focus
    // region exists (keyboard has a deliverable target on both backends)
    // and when there is no enabled focus region at all (the blocking
    // `focus_capability_missing_focus_region` error already owns that
    // shape).
    if capabilities.contains(&Capability::KeyboardInput)
        && view.focus_regions.iter().any(|region| region.enabled)
        && !view
            .focus_regions
            .iter()
            .any(|region| region.enabled && region.text_edit.is_some())
    {
        diagnostics.push(Diagnostic::warning(
            Some(view.target.clone()),
            "view_contract.keyboard_capability_plain_focus_delivery_limited",
            "Capability::KeyboardInput is declared with only plain (non-text-edit) focus regions: the iced backend delivers keyboard events exclusively to text-edit focus regions — real keyboard input never reaches a plain focus region there, and physical control refuses with native-physical-control-text-focus-widget-unavailable, so keyboard handlers on this widget cannot run or be test-exercised on iced (egui delivers after the region gains focus). Declare a text-edit focus region (text_edit_focus_region_from_capability) if keyboard delivery must work on every visible backend, or treat the handler as egui-only (docs/public/api/backends.md, Keyboard Delivery)",
        ));
    }

    if (capabilities.contains(&Capability::WheelInput)
        || capabilities.contains(&Capability::ScrollRegionPresentation))
        && !view.scroll_regions.iter().any(|region| region.enabled)
    {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.scroll_capability_missing_scroll_region",
            "Widgets declaring wheel input or scroll-region presentation must expose at least one enabled scroll region; use scroll_region_from_scrollable_capability or remove the capability",
        ));
    }
}

fn validate_text_edit_region(
    focus: &FocusRegionDeclaration,
    text_edit: &TextEditRegionDeclaration,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let target = Some(focus.target.clone());

    if text_edit.buffer.target != focus.target {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_buffer_target_mismatch",
            "Text edit buffer target must match the focus region target",
        ));
    }

    if text_edit.selection.target != focus.target {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_selection_target_mismatch",
            "Text edit selection target must match the focus region target",
        ));
    }

    if text_edit.composition.target != focus.target {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_composition_target_mismatch",
            "Text edit IME composition target must match the focus region target",
        ));
    }

    if text_edit.caret.target != focus.target {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_caret_target_mismatch",
            "Text edit caret geometry target must match the focus region target",
        ));
    }

    if text_edit.visual_style.target != focus.target {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_visual_style_target_mismatch",
            "Text input visual style target must match the focus region target",
        ));
    }

    if text_edit.typography.target != focus.target {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_typography_target_mismatch",
            "Text input typography target must match the focus region target",
        ));
    }

    if !text_edit.typography.style.font_size.is_finite()
        || text_edit.typography.style.font_size <= 0.0
    {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_typography_invalid_font_size",
            "Text input typography font size must be finite and positive",
        ));
    }

    if !text_edit.visual_style.border_width.is_finite()
        || text_edit.visual_style.border_width < 0.0
        || !text_edit.visual_style.border_radius.is_finite()
        || text_edit.visual_style.border_radius < 0.0
    {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_visual_style_invalid_metric",
            "Text input visual style border metrics must be finite and non-negative",
        ));
    }

    if let Some(undo_redo) = &text_edit.undo_redo
        && undo_redo.target != focus.target
    {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_undo_target_mismatch",
            "Text edit undo/redo target must match the focus region target",
        ));
    }

    let char_len = text_edit.buffer.text.chars().count();
    validate_selection_range(
        &target,
        "view_contract.text_edit_selection_out_of_bounds",
        text_edit.selection.selection.as_ref(),
        char_len,
        diagnostics,
    );
    validate_selection_range(
        &target,
        "view_contract.text_edit_composition_cursor_out_of_bounds",
        text_edit.composition.cursor_range.as_ref(),
        char_len,
        diagnostics,
    );
    validate_selection_range(
        &target,
        "view_contract.text_edit_viewport_range_out_of_bounds",
        text_edit
            .viewport
            .as_ref()
            .and_then(|viewport| viewport.visible_range.as_ref()),
        char_len,
        diagnostics,
    );

    if matches!(text_edit.line_mode, TextLineMode::SingleLine)
        && (text_edit.buffer.text.contains('\n') || text_edit.buffer.text.contains('\r'))
    {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.text_edit_single_line_contains_newline",
            "Single-line text edit buffers must not contain newline characters",
        ));
    }

    if text_edit.selection.editable {
        let has_insert = text_edit
            .edit_commands
            .iter()
            .any(|command| command.enabled && command.kind == TextEditKind::InsertText);
        let has_delete = text_edit.edit_commands.iter().any(|command| {
            matches!(
                command.kind,
                TextEditKind::DeleteBackward | TextEditKind::DeleteForward
            )
        });
        let has_replace_buffer = text_edit
            .edit_commands
            .iter()
            .any(|command| command.enabled && command.kind == TextEditKind::ReplaceBuffer);

        if !has_insert {
            diagnostics.push(Diagnostic::error(
                target.clone(),
                "view_contract.text_edit_missing_insert_command",
                "Editable text edit regions must declare an enabled InsertText command",
            ));
        }

        if !has_delete {
            diagnostics.push(Diagnostic::error(
                target.clone(),
                "view_contract.text_edit_missing_delete_command",
                "Editable text edit regions must declare at least one delete command",
            ));
        }

        if !has_replace_buffer {
            diagnostics.push(Diagnostic::error(
                target,
                "view_contract.text_edit_missing_replace_buffer_command",
                "Editable text edit regions must declare an enabled ReplaceBuffer command for native backend text widgets",
            ));
        }
    }
}

fn validate_selection_range(
    target: &Option<WidgetId>,
    code: &'static str,
    range: Option<&TextSelectionRange>,
    char_len: usize,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(range) = range else {
        return;
    };

    if range.anchor > char_len || range.focus > char_len {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            code,
            "Text selection/caret ranges must stay inside the current text buffer",
        ));
    }
}

fn validate_scroll_regions(
    view: &ViewDefinition,
    geometry_index: &PresentationGeometryIndex,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for region in &view.scroll_regions {
        if !rect_is_valid(region.viewport) || !rect_is_valid(region.content_bounds) {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.scroll_geometry_invalid",
                "Scroll region viewport and content bounds must be finite and non-negative",
            ));
        } else if region.enabled && !view.paint_order.allow_overflow_paint {
            let permitted = declared_target_local_bounds(
                declared_target_rect_for_region_address_with_geometry_index(
                    geometry_index,
                    &region.target,
                    region.address.as_ref(),
                ),
            );
            if !rect_contains_rect(permitted, region.viewport) {
                diagnostics.push(Diagnostic::error(
                    Some(region.target.clone()),
                    "view_contract.scroll_viewport_outside_layout",
                    format!(
                        "Enabled scroll viewport must stay inside layout bounds unless overflow paint is explicitly allowed: region `{}` declared viewport {} vs layout {}; {TARGET_LOCAL_BOUNDS_HINT}",
                        region.id.as_str(),
                        contract_rect_display(region.viewport),
                        contract_rect_display(permitted),
                    ),
                ));
            }
        } else if region.enabled
            && let Some(overflow_bounds) = view.paint_order.overflow_bounds
        {
            let root_bounds = declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &region.target,
                region.address.as_ref(),
                region.viewport.into_rect(),
            );
            if !rect_contains_rect(overflow_bounds, root_bounds) {
                diagnostics.push(Diagnostic::error(
                    Some(region.target.clone()),
                    "view_contract.scroll_viewport_outside_overflow_bounds",
                    format!(
                        "Enabled scroll viewports must stay inside declared overflow bounds: region `{}` declared viewport {} (root-local {}) vs overflow bounds {}; {TARGET_LOCAL_BOUNDS_HINT}",
                        region.id.as_str(),
                        contract_rect_display(region.viewport),
                        contract_rect_display(root_bounds),
                        contract_rect_display(overflow_bounds),
                    ),
                ));
            }
        }

        validate_scroll_region_contract(region, diagnostics);
    }

    for left_index in 0..view.scroll_regions.len() {
        let left = &view.scroll_regions[left_index];
        if !left.enabled || !left.consumption.wheel || !rect_is_valid(left.viewport) {
            continue;
        }
        for right in view.scroll_regions.iter().skip(left_index + 1) {
            if !right.enabled
                || !right.consumption.wheel
                || !rect_is_valid(right.viewport)
                || left.order != right.order
            {
                continue;
            }
            let left_bounds = declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &left.target,
                left.address.as_ref(),
                left.viewport.into_rect(),
            );
            let right_bounds = declared_region_root_local_rect_with_geometry_index(
                geometry_index,
                &right.target,
                right.address.as_ref(),
                right.viewport.into_rect(),
            );
            if rects_intersect(left_bounds, right_bounds) {
                diagnostics.push(Diagnostic::error(
                    Some(view.target.clone()),
                    "view_contract.ambiguous_wheel_overlap",
                    format!(
                        "Enabled wheel-consuming scroll regions overlap with identical ordering: `{}` and `{}` share HitRegionOrder {{ z_index: {}, paint_order: {}, traversal_order: {} }}; declare a distinct order per region via scroll_region_from_scrollable_capability_with_order (or assign ScrollRegionDeclaration::order) so exactly one region consumes the wheel event",
                        left.id.as_str(),
                        right.id.as_str(),
                        left.order.z_index,
                        left.order.paint_order,
                        left.order.traversal_order,
                    ),
                ));
            }
        }
    }
}

fn validate_scroll_region_contract(
    region: &ScrollRegionDeclaration,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let target = Some(region.target.clone());

    if region.enabled && !region.axes.horizontal && !region.axes.vertical {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.scroll_axes_empty",
            "Enabled scroll regions must declare at least one scroll axis",
        ));
    }

    if !region.offset.x.is_finite()
        || !region.offset.y.is_finite()
        || region.offset.x < 0.0
        || region.offset.y < 0.0
    {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.scroll_offset_invalid",
            "Scroll offsets must be finite and non-negative",
        ));
    }

    if !region.axes.horizontal && region.offset.x.abs() > f32::EPSILON {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.scroll_offset_on_disabled_x_axis",
            "Horizontal scroll offset must be zero when horizontal scrolling is disabled",
        ));
    }

    if !region.axes.vertical && region.offset.y.abs() > f32::EPSILON {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.scroll_offset_on_disabled_y_axis",
            "Vertical scroll offset must be zero when vertical scrolling is disabled",
        ));
    }

    if rect_is_valid(region.viewport)
        && rect_is_valid(region.content_bounds)
        && !rect_contains_rect(region.content_bounds, region.viewport)
    {
        diagnostics.push(Diagnostic::error(
            target.clone(),
            "view_contract.scroll_content_does_not_cover_viewport",
            "Scroll content bounds must contain the declared viewport",
        ));
    }

    if rect_is_valid(region.viewport) && rect_is_valid(region.content_bounds) {
        let max_x = (region.content_bounds.size.width - region.viewport.size.width).max(0.0);
        let max_y = (region.content_bounds.size.height - region.viewport.size.height).max(0.0);
        if region.offset.x > max_x + 0.01 || region.offset.y > max_y + 0.01 {
            diagnostics.push(Diagnostic::error(
                target,
                "view_contract.scroll_offset_out_of_range",
                format!(
                    "Scroll offsets must fit inside content bounds for the declared viewport: region `{}` declared offset ({}, {}) vs maximum ({}, {}) from viewport {} within content bounds {}; {TARGET_LOCAL_BOUNDS_HINT}",
                    region.id.as_str(),
                    region.offset.x,
                    region.offset.y,
                    max_x,
                    max_y,
                    contract_rect_display(region.viewport),
                    contract_rect_display(region.content_bounds),
                ),
            ));
        }
    }
}

fn validate_paint_bounds(view: &ViewDefinition, diagnostics: &mut Vec<Diagnostic>) {
    if !rect_is_valid(view.layout.bounds) {
        return;
    }

    let mut paint_bounds = Vec::new();
    for op in &view.paint {
        collect_paint_bounds(op, &mut paint_bounds);
    }

    if let Some(overflow_bounds) = view.paint_order.overflow_bounds {
        for bounds in paint_bounds {
            if rect_is_valid(bounds) && !rect_contains_rect(overflow_bounds, bounds) {
                diagnostics.push(Diagnostic::error(
                    Some(view.target.clone()),
                    "view_contract.paint_bounds_outside_overflow_bounds",
                    format!(
                        "Paint bounds extend outside declared overflow bounds: painted {} vs overflow bounds {}; {TARGET_LOCAL_BOUNDS_HINT}",
                        contract_rect_display(bounds),
                        contract_rect_display(overflow_bounds),
                    ),
                ));
            }
        }
        return;
    }

    if view.paint_order.allow_overflow_paint {
        return;
    }

    for bounds in paint_bounds {
        if rect_is_valid(bounds) && !rect_contains_rect(view.layout.bounds, bounds) {
            diagnostics.push(Diagnostic::warning(
                Some(view.target.clone()),
                "view_contract.paint_bounds_outside_layout",
                format!(
                    "Paint bounds extend outside layout bounds without explicit overflow paint allowance: painted {} vs layout {}; {TARGET_LOCAL_BOUNDS_HINT}",
                    contract_rect_display(bounds),
                    contract_rect_display(view.layout.bounds),
                ),
            ));
        }
    }
}

/// NC-13 inducement advisory (LLM-ergonomics roadmap Phase 6 item 2):
/// painted content that extends beyond the view's own container — the layout
/// bounds intersected with the frame viewport — needs a covering enabled
/// scroll region or an intentional overflow declaration, or the user simply
/// cannot reach it. Emits ONE
/// `view_contract.content_overflow_without_scroll_region` warning
/// (advisory, NON-blocking: admission still passes) when the CLIP-AWARE
/// painted extent leaves the container and no enabled scroll region's
/// `content_bounds` covers the overflowing ops.
///
/// Deliberately silent for the intentional-overflow patterns:
///
/// * a declared `PaintOrderDeclaration::overflow_bounds` (the Step-210
///   roaming-overlay pattern legitimately paints outside layout; paint
///   outside the declared rect is already the
///   `view_contract.paint_bounds_outside_overflow_bounds` error);
/// * overflow a group/layer/shape clip already clips — the effective extent
///   is the clip intersection ([`collect_effective_paint_extents`]), so
///   clipped content never counts as reachable overflow;
/// * overflow covered by an enabled scroll region's `content_bounds` —
///   scrolling is exactly how the user reaches it.
fn validate_content_overflow_scroll_coverage(
    view: &ViewDefinition,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if !rect_is_valid(view.layout.bounds) {
        return;
    }
    // Intentional-overflow declarations (Step 210): `overflow_bounds` is the
    // author's explicit allowance; `allow_overflow_paint` without bounds is
    // already the `view_contract.overflow_bounds_missing` error.
    if view.paint_order.allow_overflow_paint || view.paint_order.overflow_bounds.is_some() {
        return;
    }

    let mut container: Rect = view.layout.bounds.into_rect();
    if rect_is_valid(view.frame.viewport)
        && view.frame.viewport.size.width > 0.0
        && view.frame.viewport.size.height > 0.0
        && let Some(visible) = rect_intersection(container, view.frame.viewport)
    {
        container = visible;
    }

    let mut extents = Vec::new();
    for op in &view.paint {
        collect_effective_paint_extents(op, &mut extents);
    }

    let mut painted_extent: Option<Rect> = None;
    let mut any_uncovered = false;
    for bounds in extents {
        painted_extent = Some(match painted_extent {
            Some(existing) => bounding_union_rect(existing, bounds),
            None => bounds,
        });
        if rect_contains_rect(container, bounds) {
            continue;
        }
        let covered = view.scroll_regions.iter().any(|region| {
            region.enabled
                && rect_is_valid(region.content_bounds)
                && rect_contains_rect(region.content_bounds, bounds)
        });
        if !covered {
            any_uncovered = true;
        }
    }

    let Some(extent) = painted_extent else {
        return;
    };
    if !any_uncovered {
        return;
    }
    let overflow = (rect_max_x(extent) - rect_max_x(container))
        .max(rect_max_y(extent) - rect_max_y(container))
        .max(container.origin.x - extent.origin.x)
        .max(container.origin.y - extent.origin.y)
        .max(0.0);
    diagnostics.push(Diagnostic::warning(
        Some(view.target.clone()),
        "view_contract.content_overflow_without_scroll_region",
        format!(
            "content extends {overflow} px beyond the view's bounds {} (painted extent {}) and no enabled scroll region covers the overflow; declare a scroll region (`scroll_region_from_scrollable_capability`, `_with_order` when regions can overlap, app-level page pattern `SlipwayApp::app_scroll_regions` — docs/public/api/routing-and-scroll.md) or clip intentionally (group/layer clip, or `PaintOrderDeclaration::with_overflow_bounds` for deliberate overflow)",
            contract_rect_display(container),
            contract_rect_display(extent),
        ),
    ));
}

fn validate_view_contract_diagnostics(view: &ViewDefinition, diagnostics: &mut Vec<Diagnostic>) {
    for diagnostic in &view.diagnostics {
        if diagnostic.code.starts_with("view_contract.")
            && matches!(
                diagnostic.severity,
                DiagnosticSeverity::Error | DiagnosticSeverity::Unsupported
            )
        {
            diagnostics.push(diagnostic.clone());
        }
    }
}

fn collect_paint_bounds(op: &PaintOp, bounds: &mut Vec<Rect>) {
    match op {
        PaintOp::Fill { shape, .. } | PaintOp::Stroke { shape, .. } => {
            bounds.push(shape.bounds);
            if let Some(clip) = &shape.clip {
                bounds.push(clip.bounds);
            }
        }
        PaintOp::Text {
            bounds: text_bounds,
            ..
        } => bounds.push(*text_bounds),
        PaintOp::Group { clip, ops, .. } => {
            if let Some(clip) = clip {
                bounds.push(clip.bounds);
            }
            for op in ops {
                collect_paint_bounds(op, bounds);
            }
        }
        PaintOp::Layer { clip, ops, .. } => {
            if let Some(clip) = clip {
                bounds.push(clip.bounds);
            }
            for op in ops {
                collect_paint_bounds(op, bounds);
            }
        }
    }
}

/// Clip-aware painted extents for the NC-13 overflow advisory
/// ([`validate_content_overflow_scroll_coverage`]): a clipped shape
/// contributes only the shape-clip intersection, and ops under a clipped
/// group/layer contribute only their intersection with that clip — clipped
/// overflow never reaches pixels, so it never counts as unreachable content.
/// Unlike [`collect_paint_bounds`], clip rects themselves are NOT extents.
fn collect_effective_paint_extents(op: &PaintOp, extents: &mut Vec<Rect>) {
    match op {
        PaintOp::Fill { shape, .. } | PaintOp::Stroke { shape, .. } => {
            if !rect_is_valid(shape.bounds) {
                return;
            }
            match &shape.clip {
                Some(clip) if rect_is_valid(clip.bounds) => {
                    if let Some(visible) = rect_intersection(shape.bounds, clip.bounds) {
                        extents.push(visible);
                    }
                }
                _ => extents.push(shape.bounds),
            }
        }
        PaintOp::Text { bounds, .. } => {
            if rect_is_valid(*bounds) {
                extents.push(*bounds);
            }
        }
        PaintOp::Group { clip, ops, .. } | PaintOp::Layer { clip, ops, .. } => {
            let mut inner = Vec::new();
            for op in ops {
                collect_effective_paint_extents(op, &mut inner);
            }
            match clip {
                Some(clip) if rect_is_valid(clip.bounds) => {
                    for bounds in inner {
                        if let Some(visible) = rect_intersection(bounds, clip.bounds) {
                            extents.push(visible);
                        }
                    }
                }
                _ => extents.extend(inner),
            }
        }
    }
}

/// The overlapping region of two rects, `None` when they are disjoint.
fn rect_intersection(left: impl Into<Rect>, right: impl Into<Rect>) -> Option<Rect> {
    let left = left.into();
    let right = right.into();
    let x0 = left.origin.x.max(right.origin.x);
    let y0 = left.origin.y.max(right.origin.y);
    let x1 = rect_max_x(left).min(rect_max_x(right));
    let y1 = rect_max_y(left).min(rect_max_y(right));
    if x1 < x0 || y1 < y0 {
        return None;
    }
    Some(Rect {
        origin: Point { x: x0, y: y0 },
        size: Size {
            width: x1 - x0,
            height: y1 - y0,
        },
    })
}

fn rect_is_valid(rect: impl Into<Rect>) -> bool {
    let rect = rect.into();
    rect.origin.x.is_finite()
        && rect.origin.y.is_finite()
        && rect.size.width.is_finite()
        && rect.size.height.is_finite()
        && rect.size.width >= 0.0
        && rect.size.height >= 0.0
}

fn rect_contains_rect(outer: impl Into<Rect>, inner: impl Into<Rect>) -> bool {
    const EPSILON: f32 = 0.01;
    let outer = outer.into();
    let inner = inner.into();

    inner.origin.x + EPSILON >= outer.origin.x
        && inner.origin.y + EPSILON >= outer.origin.y
        && rect_max_x(inner) <= rect_max_x(outer) + EPSILON
        && rect_max_y(inner) <= rect_max_y(outer) + EPSILON
}

fn rect_contains_point(rect: impl Into<Rect>, point: Point) -> bool {
    const EPSILON: f32 = 0.01;
    let rect = rect.into();

    point.x + EPSILON >= rect.origin.x
        && point.y + EPSILON >= rect.origin.y
        && point.x <= rect_max_x(rect) + EPSILON
        && point.y <= rect_max_y(rect) + EPSILON
}

fn rects_intersect(left: impl Into<Rect>, right: impl Into<Rect>) -> bool {
    const EPSILON: f32 = 0.01;
    let left = left.into();
    let right = right.into();

    left.origin.x < rect_max_x(right) - EPSILON
        && rect_max_x(left) > right.origin.x + EPSILON
        && left.origin.y < rect_max_y(right) - EPSILON
        && rect_max_y(left) > right.origin.y + EPSILON
}

fn rect_max_x(rect: Rect) -> f32 {
    rect.origin.x + rect.size.width
}

fn rect_max_y(rect: Rect) -> f32 {
    rect.origin.y + rect.size.height
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderPacket {
    pub target: WidgetId,
    pub frame: FrameIdentity,
    pub layout: LayoutOutput,
    pub paint: Vec<PaintOp>,
    pub surfaces: Vec<RenderSurfaceDeclaration>,
    pub diagnostics: Vec<Diagnostic>,
    pub prepared_geometry: Option<Arc<PreparedPresentationGeometry>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderEvidence {
    pub target: WidgetId,
    pub frame: FrameIdentity,
    pub source: EvidenceSource,
    pub provider_id: String,
    pub artifact_ref: Option<String>,
    pub artifact_path: Option<String>,
    pub pixel_hash: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderRefusal {
    pub target: Option<WidgetId>,
    pub frame: FrameIdentity,
    pub source: Option<EvidenceSource>,
    pub provider_id: Option<String>,
    pub reason: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WidgetSlotAddress {
    pub widget: WidgetId,
    pub path: Vec<WidgetId>,
    pub ordinal: usize,
}

impl WidgetSlotAddress {
    pub fn new(widget: WidgetId, ordinal: usize) -> Self {
        Self {
            widget: widget.clone(),
            path: vec![widget],
            ordinal,
        }
    }

    pub fn child(&self, widget: WidgetId, ordinal: usize) -> Self {
        let mut path = self.path.clone();
        path.push(widget.clone());
        Self {
            widget,
            path,
            ordinal,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaintUnit {
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    pub order: PaintOrderDeclaration,
    pub traversal_order: usize,
    pub paint: Vec<PaintOp>,
}

impl PaintUnit {
    pub fn source_order(
        target: impl Into<WidgetId>,
        address: Option<WidgetSlotAddress>,
        traversal_order: usize,
        paint: Vec<PaintOp>,
    ) -> Self {
        let target = target.into();
        Self {
            target: target.clone(),
            address,
            order: PaintOrderDeclaration::source_order(target),
            traversal_order,
            paint,
        }
    }

    pub fn from_view(view: ViewDefinition, traversal_order: usize) -> Self {
        Self {
            target: view.target,
            address: None,
            order: view.paint_order,
            traversal_order,
            paint: view.paint,
        }
    }

    pub fn from_view_ref(view: &ViewDefinition, traversal_order: usize) -> Self {
        Self {
            target: view.target.clone(),
            address: None,
            order: view.paint_order.clone(),
            traversal_order,
            paint: view.paint.clone(),
        }
    }
}

#[derive(Clone)]
struct PaintGroupContext {
    id: Option<String>,
    clip: Option<ClipDeclaration>,
}

fn wrap_paint_with_group_context(
    mut paint: Vec<PaintOp>,
    context: &[PaintGroupContext],
) -> Vec<PaintOp> {
    for group in context.iter().rev() {
        paint = vec![PaintOp::Group {
            id: group.id.clone(),
            clip: group.clip.clone(),
            ops: paint,
        }];
    }
    paint
}

fn paint_layer_order(target: &WidgetId, key: PaintLayerKey) -> PaintOrderDeclaration {
    let mut order = PaintOrderDeclaration::layer(target.clone(), key.z_index);
    order.order = key.order;
    order
}

fn expand_paint_ops_into_units(
    ops: Vec<PaintOp>,
    unit: &PaintUnit,
    context: &mut Vec<PaintGroupContext>,
    expanded: &mut Vec<PaintUnit>,
) -> Vec<PaintOp> {
    let mut default_paint = Vec::new();

    for op in ops {
        match op {
            PaintOp::Group { id, clip, ops } => {
                context.push(PaintGroupContext {
                    id: id.clone(),
                    clip: clip.clone(),
                });
                let group_default = expand_paint_ops_into_units(ops, unit, context, expanded);
                context.pop();

                if !group_default.is_empty() {
                    default_paint.push(PaintOp::Group {
                        id,
                        clip,
                        ops: group_default,
                    });
                }
            }
            PaintOp::Layer {
                id,
                key,
                input_transparency,
                wheel_transparency,
                clip,
                ops,
            } => {
                context.push(PaintGroupContext {
                    id: id.clone(),
                    clip: clip.clone(),
                });
                let layer_default = expand_paint_ops_into_units(ops, unit, context, expanded);
                context.pop();

                if !layer_default.is_empty() {
                    let layer = PaintOp::Layer {
                        id,
                        key,
                        input_transparency,
                        wheel_transparency,
                        clip,
                        ops: layer_default,
                    };
                    expanded.push(PaintUnit {
                        target: unit.target.clone(),
                        address: unit.address.clone(),
                        order: paint_layer_order(&unit.target, key),
                        traversal_order: unit.traversal_order,
                        paint: wrap_paint_with_group_context(vec![layer], context),
                    });
                }
            }
            op => default_paint.push(op),
        }
    }

    default_paint
}

pub fn expand_paint_unit_layers(unit: PaintUnit) -> Vec<PaintUnit> {
    let PaintUnit {
        target,
        address,
        order,
        traversal_order,
        paint,
    } = unit;
    let template = PaintUnit {
        target,
        address,
        order,
        traversal_order,
        paint: Vec::new(),
    };
    let mut expanded = Vec::new();
    let mut context = Vec::new();
    let default_paint = expand_paint_ops_into_units(paint, &template, &mut context, &mut expanded);
    let mut units = Vec::new();

    if !default_paint.is_empty() || expanded.is_empty() {
        units.push(PaintUnit {
            paint: default_paint,
            ..template
        });
    }

    units.extend(expanded);
    units
}

pub fn expand_paint_unit_layers_in_units(units: Vec<PaintUnit>) -> Vec<PaintUnit> {
    units
        .into_iter()
        .flat_map(expand_paint_unit_layers)
        .collect()
}

pub fn paint_unit_sort_key(unit: &PaintUnit) -> (i32, usize, usize) {
    match unit.order.mode {
        PaintOrderMode::SourceOrder => (0, unit.traversal_order, unit.traversal_order),
        PaintOrderMode::ExplicitLayered => (
            unit.order.z_index,
            unit.order.order.unwrap_or(unit.traversal_order),
            unit.traversal_order,
        ),
    }
}

/// The AUTHORED within-z order of a paint unit's layer, when the author
/// declared one (`PaintLayerKey::ordered` / an explicit
/// `PaintOrderDeclaration.order`). `None` for source-order units and for
/// explicit layers keyed without an order (`PaintLayerKey::new`), whose
/// sort-key tie-break fields are DEFAULTED from the unit traversal (the
/// mounted slot ordinal) and therefore carry no authored meaning. Same-owner
/// occlusion comparisons ([`paint_occlusion_blocks_declared_hit_region`])
/// may only consult this authored component (NC-2).
pub fn paint_unit_authored_z_order(unit: &PaintUnit) -> Option<usize> {
    match unit.order.mode {
        PaintOrderMode::SourceOrder => None,
        PaintOrderMode::ExplicitLayered => unit.order.order,
    }
}

pub fn ordered_paint_units(mut units: Vec<PaintUnit>) -> Vec<PaintUnit> {
    units.sort_by_key(paint_unit_sort_key);
    units
}

pub fn flatten_ordered_paint_units(units: Vec<PaintUnit>) -> Vec<PaintOp> {
    ordered_paint_units(expand_paint_unit_layers_in_units(units))
        .into_iter()
        .flat_map(|unit| unit.paint)
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct StateObservation {
    pub target: WidgetId,
    pub slot: Option<WidgetSlotAddress>,
    pub name: String,
    pub value: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeEvidence {
    pub target: WidgetId,
    pub slot: Option<WidgetSlotAddress>,
    pub field: String,
    pub before: Option<String>,
    pub after: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmittedMessageEvidence {
    pub target: WidgetId,
    pub name: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeShapeIdentity {
    pub target: WidgetId,
    pub slot: Option<WidgetSlotAddress>,
    pub field: String,
    pub before_present: bool,
    pub after_present: bool,
}

impl From<&ChangeEvidence> for ChangeShapeIdentity {
    fn from(change: &ChangeEvidence) -> Self {
        Self {
            target: change.target.clone(),
            slot: change.slot.clone(),
            field: change.field.clone(),
            before_present: change.before.is_some(),
            after_present: change.after.is_some(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticIdentity {
    pub target: Option<WidgetId>,
    pub severity: DiagnosticSeverity,
    pub code: String,
}

impl From<&Diagnostic> for DiagnosticIdentity {
    fn from(diagnostic: &Diagnostic) -> Self {
        Self {
            target: diagnostic.target.clone(),
            severity: diagnostic.severity.clone(),
            code: diagnostic.code.clone(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EventResultIdentity {
    pub handled: Option<bool>,
    pub emitted_messages: Vec<EmittedMessageEvidence>,
    pub change_shapes: Vec<ChangeShapeIdentity>,
    pub diagnostics: Vec<DiagnosticIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProbeRequest {
    pub target: Option<WidgetId>,
    pub kinds: Vec<ProbeKind>,
    pub event_trace_limit: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ProbeKind {
    Topology,
    State,
    Event,
    Change,
    Diagnostics,
    Paint,
    Semantics,
    Surface,
    LayoutIntent,
    ViewDefinition,
    RenderPacket,
    RenderEvidence,
    DispatchGraph,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ProbeProduct {
    Topology(TopologyProbe),
    State(StateProbe),
    Event(EventProbe),
    Change(ChangeProbe),
    Semantics(SemanticsProbe),
    Paint(PaintProbe),
    Surface(SurfaceProbe),
    LayoutIntent(LayoutIntentProbe),
    ViewDefinition(ViewDefinition),
    RenderPacket(RenderPacket),
    RenderEvidence(RenderEvidence),
    DispatchGraph(DispatchGraphProbe),
    Diagnostic(Diagnostic),
}

#[derive(Clone, Debug, PartialEq)]
pub struct TopologyProbe {
    pub root: TopologyNode,
    pub traversal: ChildTraversal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StateProbe {
    pub target: WidgetId,
    pub observations: Vec<StateObservation>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EventProbe {
    pub routed_target: WidgetId,
    pub event: InputEvent,
    pub dispatch_evidence: Option<DeclaredEventDispatchEvidence>,
    pub dispatch_identity: Option<DeclaredEventDispatchIdentity>,
    pub result_identity: EventResultIdentity,
    pub handled: Option<bool>,
    pub revision_before: Option<u64>,
    pub revision_after: Option<u64>,
    pub emitted_messages: Vec<EmittedMessageEvidence>,
    pub local_state: Vec<StateObservation>,
    pub changes: Vec<ChangeEvidence>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChangeProbe {
    pub target: WidgetId,
    pub changes: Vec<ChangeEvidence>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SemanticsProbe {
    pub root: WidgetId,
    pub nodes: Vec<SemanticNode>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaintProbe {
    pub target: WidgetId,
    pub ops: Vec<PaintOp>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SurfaceProbe {
    pub target: WidgetId,
    pub surfaces: Vec<RenderSurfaceDeclaration>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutIntentProbe {
    pub target: WidgetId,
    pub intrinsic_size: Option<IntrinsicSize>,
    pub size_policy: Option<SizePolicyDeclaration>,
    pub resize_policy: Option<ResizePolicyDeclaration>,
    pub overflow_policy: Option<OverflowPolicyDeclaration>,
    pub auto_layout: Option<AutoLayoutPolicyDeclaration>,
    pub responsive_variant: Option<ResponsiveVariant>,
    pub text_flow: Option<TextFlowPolicy>,
    pub text_measurement_cache: Vec<TextMeasurementCachePolicyDeclaration>,
    pub text_measurement: Option<TextMeasurementEvidence>,
    pub fit_overflow: Vec<FitOverflowEvidence>,
    pub layer: Option<LayerPolicy>,
    pub scroll: Option<ScrollPolicy>,
    pub collection: Option<CollectionPolicy>,
    pub interaction_styles: Vec<InteractionStateStyle>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct EmittedMessage<M> {
    pub target: WidgetId,
    pub name: String,
    pub message: M,
}

/// What [`SlipwayLogic::handle_event`] returns for every routed event:
/// the handled/propagate decision plus everything the event produced
/// (typed messages upward, change/observation evidence, probe products,
/// diagnostics). Build it with the constructors below; struct literals
/// are for merging paths. Contract map: `docs/public/api/core.md`
/// ("Logic").
///
/// The declared disposition ([`SlipwayEventDispositionPolicy`]) never
/// overrides these fields directly — declaration and actual outcome are
/// reconciled by [`apply_event_handling_declaration`], which warns (and
/// the physical path errors) on mismatch and applies the declared
/// propagation only when `handled` is true.
#[derive(Clone, Debug, PartialEq)]
#[must_use = "return this from SlipwayLogic::handle_event (or merge it via merge_event_outcomes); a dropped outcome discards its emitted messages, changes, and diagnostics"]
pub struct EventOutcome<M> {
    /// `true` when the widget consumed the event. Handling implies intent:
    /// a handler that mutates local state and then reports
    /// `handled: false` violates the declaration contract
    /// (`event_declaration.handler_ignored_declared_handled`).
    pub handled: bool,
    /// Whether the original event continues to the parent/app reducer.
    /// `handled: true, propagate: true` bubbles after local handling;
    /// unhandled outcomes always propagate.
    pub propagate: bool,
    pub emitted_messages: Vec<EmittedMessage<M>>,
    pub changes: Vec<ChangeEvidence>,
    pub observations: Vec<StateObservation>,
    pub probes: Vec<ProbeProduct>,
    pub diagnostics: Vec<Diagnostic>,
}

impl<M> EventOutcome<M> {
    /// Not-my-event: `handled: false, propagate: true`, nothing produced.
    /// This is also the refusal convention on validation failure: when an
    /// event fails a contract check (wrong target, unresolved region,
    /// invalid dispatch evidence), return `ignored()` — with a
    /// [`Diagnostic`] pushed onto `diagnostics` where evidence matters —
    /// instead of fabricating a handled result. The runtime input-refusal
    /// paths follow the same convention.
    pub fn ignored() -> Self {
        Self {
            handled: false,
            propagate: true,
            emitted_messages: Vec::new(),
            changes: Vec::new(),
            observations: Vec::new(),
            probes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Consumed locally: `handled: true, propagate: false`, no messages.
    /// Use for interactions that change only widget-local state.
    pub fn handled() -> Self {
        Self {
            handled: true,
            propagate: false,
            emitted_messages: Vec::new(),
            changes: Vec::new(),
            observations: Vec::new(),
            probes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Consumed, emitting one typed message upward (`handled: true,
    /// propagate: false`) — the ordinary way a widget talks to the app
    /// reducer; siblings are never mutated directly.
    pub fn message(target: WidgetId, name: impl Into<String>, message: M) -> Self {
        Self {
            handled: true,
            propagate: false,
            emitted_messages: vec![EmittedMessage {
                target,
                name: name.into(),
                message,
            }],
            changes: Vec::new(),
            observations: Vec::new(),
            probes: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    /// Overrides `propagate` (e.g. handle locally AND bubble to the app
    /// reducer). Note [`apply_event_handling_declaration`] re-applies the
    /// declared propagation for handled outcomes on the framework path.
    pub fn with_propagation(mut self, propagate: bool) -> Self {
        self.propagate = propagate;
        self
    }
}

pub fn declared_event_handling<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    event: &InputEvent,
) -> EventHandlingDeclaration
where
    W: SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy,
{
    let routing = widget.event_routing_policy(external, local, event);
    let disposition = widget.event_disposition(external, local, event, &routing.route);
    EventHandlingDeclaration {
        routing,
        disposition,
    }
}

pub fn apply_event_handling_declaration<M>(
    declaration: EventHandlingDeclaration,
    mut outcome: EventOutcome<M>,
) -> EventOutcome<M> {
    let target = declaration.routing.target.clone();
    outcome.diagnostics.extend(declaration.routing.diagnostics);
    outcome
        .diagnostics
        .extend(declaration.disposition.diagnostics);
    if declaration.disposition.final_disposition.handled && !outcome.handled {
        outcome.diagnostics.push(Diagnostic::warning(
            Some(target.clone()),
            EVENT_DECLARATION_HANDLER_IGNORED_DECLARED_HANDLED,
            "Event disposition policy declared this event handled, but the widget handler returned ignored; author both from one table via event_handling_table! (or align the hand-written disposition)",
        ));
    } else if !declaration.disposition.final_disposition.handled && outcome.handled {
        outcome.diagnostics.push(Diagnostic::warning(
            Some(target),
            EVENT_DECLARATION_HANDLER_HANDLED_DECLARED_UNHANDLED,
            "Widget handler handled an event that its disposition policy did not declare as handled; author both from one table via event_handling_table! (or align the hand-written disposition)",
        ));
    }
    if outcome.handled {
        outcome.propagate = declaration.disposition.final_disposition.propagate;
    } else {
        outcome.propagate = true;
    }
    outcome
}

pub fn apply_physical_event_handling_declaration<M>(
    declaration: EventHandlingDeclaration,
    outcome: EventOutcome<M>,
) -> EventOutcome<M> {
    let mut outcome = apply_event_handling_declaration(declaration, outcome);
    let mut mismatch = false;
    for diagnostic in &mut outcome.diagnostics {
        if diagnostic.code == EVENT_DECLARATION_HANDLER_IGNORED_DECLARED_HANDLED
            || diagnostic.code == EVENT_DECLARATION_HANDLER_HANDLED_DECLARED_UNHANDLED
        {
            diagnostic.severity = DiagnosticSeverity::Error;
            mismatch = true;
        }
    }
    if mismatch {
        outcome.handled = false;
        outcome.propagate = true;
        outcome.emitted_messages.clear();
        outcome.changes.clear();
        outcome.probes.clear();
        outcome.observations.clear();
    }
    outcome
}

pub fn event_outcome_has_physical_declaration_mismatch<M>(outcome: &EventOutcome<M>) -> bool {
    outcome.diagnostics.iter().any(|diagnostic| {
        diagnostic.severity == DiagnosticSeverity::Error
            && (diagnostic.code == EVENT_DECLARATION_HANDLER_IGNORED_DECLARED_HANDLED
                || diagnostic.code == EVENT_DECLARATION_HANDLER_HANDLED_DECLARED_UNHANDLED)
    })
}

pub fn refuse_event_declared_unhandled<M>(
    declaration: EventHandlingDeclaration,
) -> EventOutcome<M> {
    let target = declaration.routing.target.clone();
    let mut diagnostics = declaration.routing.diagnostics;
    diagnostics.extend(declaration.disposition.diagnostics);
    diagnostics.push(Diagnostic::error(
        Some(target),
        EVENT_DECLARATION_HANDLER_HANDLED_DECLARED_UNHANDLED,
        "Event disposition policy declared this event unhandled; backend physical-equivalent dispatch must not call the widget handler",
    ));
    EventOutcome {
        handled: false,
        propagate: true,
        emitted_messages: Vec::new(),
        changes: Vec::new(),
        observations: Vec::new(),
        probes: Vec::new(),
        diagnostics,
    }
}

/// Builds the standard single-step, target-phase
/// [`EventPropagationEvidence`] from one `handled` decision:
/// `propagate: !handled`, `default_action_allowed: true`, one `Target`
/// stage step whose node is the route's last path entry.
///
/// WHEN: inside a hand-written [`SlipwayEventDispositionPolicy`] impl
/// for the ordinary consume-or-ignore widget (the same shape
/// [`event_handling_table!`] generates — prefer the table, which also
/// derives `handled` itself). Failure mode of hand-building the evidence
/// instead: a `final_disposition` that disagrees with the steps, or a
/// `propagate` that silently swallows unhandled events; misdeclared
/// handledness surfaces as `event_declaration.handler_ignored_declared_handled`
/// / `event_declaration.handler_handled_declared_unhandled`.
/// LOAD-BEARING: the physical dispatch path refuses declared-unhandled
/// events before the handler runs. Docs: `docs/public/api/core.md`
/// ("Interaction Declarations").
pub fn target_event_disposition(
    target: WidgetId,
    event: &InputEvent,
    route: &EventRoute,
    handled: bool,
) -> EventPropagationEvidence {
    let disposition = EventDisposition {
        handled,
        propagate: !handled,
        default_action_allowed: true,
    };
    EventPropagationEvidence {
        target,
        event: event.clone(),
        steps: vec![EventPropagationStep {
            stage: EventPropagationStage::Target,
            node: route.path.last().cloned(),
            disposition,
            emitted_messages: Vec::new(),
            changes: Vec::new(),
        }],
        final_disposition: disposition,
        diagnostics: Vec::new(),
    }
}

/// Generates [`SlipwayLogic::handle_event`] AND
/// [`SlipwayEventDispositionPolicy::event_disposition`] from ONE match
/// table, so declared handledness and actual handler behavior are
/// synchronized BY CONSTRUCTION (audit NC-8: hand-duplicating the
/// predicate arm-for-arm is a guaranteed-drift design; ADR-0003).
///
/// WHEN: every ordinary consume-or-ignore widget — this is the taught
/// authoring form (`crates/slipway-example-authored/src/internal_logic.rs`
/// models it). Hand-write the two impls only for capture/bubble-phase
/// declarations, custom propagation (`handled: true, propagate: true`
/// bubbling), or a [`SlipwayLogic::project_frame_viewport`] override.
///
/// Table contract:
///
/// * `|widget, external, local| match event { .. }` — four author-chosen
///   names: `widget` binds the receiver (`self` cannot cross the macro
///   boundary), `external`/`local` the state parameters, and `event` the
///   matched `&InputEvent`.
/// * Each arm's PATTERN + GUARD is the declared disposition: the arm
///   body runs exactly when the event is declared handled. Guards see
///   `local` as `&LocalState` in BOTH expansions; bodies see it as
///   `&mut LocalState`. Patterns bind by reference (the scrutinee is
///   `&InputEvent` in both expansions), so payload fields are read via
///   `Copy`/`&` access.
/// * Events that match no arm — or whose target is not
///   `SlipwaySsot::id(self)` — are ignored AND declared unhandled; do
///   NOT write a `_` catch-all arm (it would declare every event kind
///   handled, and the runtime reconciliation errors on the physical
///   path: `event_declaration.handler_ignored_declared_handled`).
/// * Bodies must return a HANDLED [`EventOutcome`] (`handled()` /
///   `message(..)` / merged variants) — returning `ignored()` from a
///   matched arm contradicts the arm's own declaration and is diagnosed
///   by the unchanged runtime reconciliation.
///
/// Requires [`SlipwayWidgetTypes`] and [`SlipwaySsot`] on the widget
/// (the declaration target is `SlipwaySsot::id`); the generated
/// disposition is the single-step target-phase evidence of
/// [`target_event_disposition`]. LOAD-BEARING. Docs:
/// `docs/public/api/core.md` ("Interaction Declarations"),
/// `docs/public/api/diagnostics.md` ("event_declaration").
///
/// ```
/// use slipway_core::*;
///
/// struct Toggle;
///
/// impl SlipwayWidgetTypes for Toggle {
///     type ExternalState = ();
///     type LocalState = bool;
///     type AppMessage = ();
/// }
///
/// impl SlipwaySsot for Toggle {
///     fn id(&self) -> WidgetId {
///         WidgetId::from("toggle")
///     }
///     fn capabilities(&self) -> Vec<Capability> {
///         vec![Capability::PointerInput]
///     }
///     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
///         TopologyNode::leaf(self.id())
///     }
///     fn unsupported(&self) -> Vec<Diagnostic> {
///         Vec::new()
///     }
/// }
///
/// event_handling_table! {
///     impl Toggle {
///         |widget, external, local| match event {
///             InputEvent::Pointer(pointer)
///                 if pointer.kind == PointerEventKind::Press && !*local =>
///             {
///                 *local = true;
///                 EventOutcome::handled()
///             },
///         }
///     }
/// }
///
/// let toggle = Toggle;
/// let press = InputEvent::Pointer(PointerEvent {
///     target: toggle.id(),
///     target_slot: None,
///     position: Point { x: 1.0, y: 1.0 },
///     target_bounds: None,
///     kind: PointerEventKind::Press,
///     button: Some(PointerButton::Primary),
///     details: PointerDetails::default(),
/// });
/// let route = EventRoute {
///     route_id: None,
///     address: None,
///     path: vec![toggle.id()],
///     phase: EventRoutePhase::Target,
/// };
///
/// // The declared disposition and the handler agree by construction:
/// // the same pattern+guard tokens decide both.
/// let declared = toggle.event_disposition(&(), &false, &press, &route);
/// let mut local = false;
/// let outcome = toggle.handle_event(&(), &mut local, press.clone());
/// assert!(declared.final_disposition.handled);
/// assert!(outcome.handled && local);
///
/// // Guard false (already toggled): ignored AND declared unhandled.
/// let declared = toggle.event_disposition(&(), &true, &press, &route);
/// let mut local = true;
/// let outcome = toggle.handle_event(&(), &mut local, press);
/// assert!(!declared.final_disposition.handled);
/// assert!(!outcome.handled);
/// ```
#[macro_export]
macro_rules! event_handling_table {
    (
        impl $widget:ty {
            |$this:ident, $external:ident, $local:ident| match $event:ident {
                $( $pattern:pat $( if $guard:expr )? => $body:expr ),+ $(,)?
            }
        }
    ) => {
        impl $crate::SlipwayLogic for $widget {
            #[allow(unused_variables)]
            fn handle_event(
                &self,
                $external: &Self::ExternalState,
                $local: &mut Self::LocalState,
                event: $crate::InputEvent,
            ) -> $crate::EventOutcome<Self::AppMessage> {
                if event.target() != &<Self as $crate::SlipwaySsot>::id(self) {
                    return $crate::EventOutcome::ignored();
                }
                let $this = self;
                let $event: &$crate::InputEvent = &event;
                match $event {
                    $( $pattern $( if { let $local = &*$local; $guard } )? => $body, )+
                    #[allow(unreachable_patterns)]
                    _ => $crate::EventOutcome::ignored(),
                }
            }
        }

        impl $crate::SlipwayEventDispositionPolicy for $widget {
            #[allow(unused_variables)]
            fn event_disposition(
                &self,
                $external: &Self::ExternalState,
                $local: &Self::LocalState,
                event: &$crate::InputEvent,
                route: &$crate::EventRoute,
            ) -> $crate::EventPropagationEvidence {
                let $this = self;
                let $event: &$crate::InputEvent = event;
                let handled = event.target() == &<Self as $crate::SlipwaySsot>::id(self)
                    && match $event {
                        $( $pattern $( if { let $local = &*$local; $guard } )? => true, )+
                        #[allow(unreachable_patterns)]
                        _ => false,
                    };
                $crate::target_event_disposition(
                    <Self as $crate::SlipwaySsot>::id(self),
                    event,
                    route,
                    handled,
                )
            }
        }
    };
}

pub fn merge_event_outcomes<M>(
    mut first: EventOutcome<M>,
    mut second: EventOutcome<M>,
) -> EventOutcome<M> {
    first.handled = first.handled || second.handled;
    first.propagate = second.propagate;
    first.emitted_messages.append(&mut second.emitted_messages);
    first.changes.append(&mut second.changes);
    first.observations.append(&mut second.observations);
    first.probes.append(&mut second.probes);
    first.diagnostics.append(&mut second.diagnostics);
    first
}

/// The associated types every other authoring trait hangs off — implement
/// this FIRST on every widget/app type. `ExternalState` is the app-state
/// projection the widget reads (never mutates); `LocalState` is the
/// widget-private state slot the runtime owns per instance; `AppMessage`
/// is the typed message enum emitted upward to the parent/app reducer.
/// LOAD-BEARING. Model: `docs/public/api/core.md`; pattern:
/// `docs/public/quickstart-authoring.md`.
pub trait SlipwayWidgetTypes {
    type ExternalState;
    type LocalState;
    type AppMessage;
}

/// The widget's single source of truth: stable identity, declared
/// capabilities, topology, and unsupported evidence. One of the three
/// mandatory traits (with [`SlipwayLogic`] and [`SlipwayView`] it forms
/// [`SlipwayWidget`]) — every authored widget implements it by hand.
/// LOAD-BEARING: consulted at admission, mounting, probe assembly, and
/// dispatch. Model: `docs/public/api/core.md` ("State And Identity").
///
/// `capabilities()` feeds backend admission directly: every declared
/// input/presentation [`Capability`] must be matched by an enabled region
/// in the view or admission refuses (the
/// `view_contract.*_capability_missing_*` family — see the variant docs
/// on [`Capability`]). `unsupported()` declares known gaps as honest
/// evidence instead of silent absence. `visit_authored_children` has a
/// correct empty default for leaf widgets; containers override it to
/// expose their authored children (a child NOT visited is invisible to
/// layout, dispatch, and probes).
pub trait SlipwaySsot: SlipwayWidgetTypes {
    fn id(&self) -> WidgetId;
    fn capabilities(&self) -> Vec<Capability>;
    fn topology(&self, external: &Self::ExternalState) -> TopologyNode;
    fn unsupported(&self) -> Vec<Diagnostic>;

    fn visit_authored_children<V>(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _visitor: &mut V,
    ) where
        V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
    }
}

/// The widget-internal event logic: turns one routed [`InputEvent`] into
/// an [`EventOutcome`] (handled/propagate, typed messages upward, change
/// evidence, diagnostics). One of the three mandatory traits forming
/// [`SlipwayWidget`]. LOAD-BEARING: runs on every dispatch, after the
/// declaration contract admits the event. Model:
/// `docs/public/api/core.md` ("Logic").
///
/// Contract: mutate ONLY `local`; app-level effects travel as emitted
/// messages to the parent reducer (never mutate siblings). Events reach
/// this handler only through declared regions/routes — if an interaction
/// works only because state was mutated directly, the interaction
/// contract is not satisfied. Return [`EventOutcome::ignored`] for events
/// this widget does not consume; a handler that mutates state and then
/// returns ignored is a declaration violation
/// (`event_declaration.handler_ignored_declared_handled`, an ERROR on the
/// backend physical path — see [`apply_event_handling_declaration`]).
pub trait SlipwayLogic: SlipwayWidgetTypes {
    fn handle_event(
        &self,
        external: &Self::ExternalState,
        local: &mut Self::LocalState,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage>;

    /// Platform-truth projection: the runtime calls this whenever the
    /// presenting backend records the live frame viewport
    /// (`SlipwayRuntime::record_presented_viewport`), BEFORE the next
    /// frame's declarations are built.
    ///
    /// WHEN: override it only when authored geometry must track the real
    /// window size everywhere state is read — e.g. a per-frame clamp that
    /// paint, hit declarations, AND event handlers must all reproduce
    /// (`ViewDefinitionInput.frame.viewport` reaches only
    /// `view_definition`, never `paint` or `handle_event`; this hook is
    /// the sanctioned channel that makes the viewport ordinary external
    /// state). Pattern: `SlipwayApp::project_frame_viewport` +
    /// `crates/slipway-example-authored` (`ShowcaseState::viewport` feeds
    /// the roaming-overlay allowance). Failure mode of hand-rolling it:
    /// a widget that derives window geometry in `view_definition` but not
    /// in `paint`/`handle_event` silently splits the MUST-agree
    /// paint/hit/clamp chain — no admission diagnostic can catch it.
    /// LOAD-BEARING when overridden; the default is a no-op and
    /// byte-identical for every widget that ignores it. Unlike
    /// `handle_event`, this is invoked by the RUNTIME (not by routed
    /// input) and — with [`SlipwayLogic::project_text_metrics`] — is a
    /// sanctioned external-state writer besides the app reducer. Docs:
    /// `docs/public/api/routing-and-scroll.md` ("Overlay Drag Patterns").
    fn project_frame_viewport(&self, _external: &mut Self::ExternalState, _viewport: Rect) {}

    /// Platform-truth projection, measurement flavor: the runtime calls
    /// this with the presenting backend's REAL text-metric provider
    /// (`SlipwayRuntime::project_text_metrics`), on the same cadence as
    /// [`SlipwayLogic::project_frame_viewport`].
    ///
    /// WHEN: override it when authored geometry must track laid-out text
    /// size (a badge sized to its label, a center-ellipsized header) —
    /// build a [`TextMeasurementRequest`] with the SAME [`TextStyle`] the
    /// paint op declares, call `metrics.measure_text(..)`, and write the
    /// [`TextMeasurementFacts`] into external state; `paint`,
    /// `view_definition`, and `handle_event` then read the measured size
    /// like any other state (the MUST-agree chain). Pattern:
    /// `SlipwayApp::project_text_metrics` +
    /// `crates/slipway-example-authored` (`ShowcaseState::window_badge`
    /// sizes the input card's badge to its measured label). Failure mode
    /// of hand-rolling it: estimated character-width ratios drift off
    /// wherever the guess is wrong (audit NC-4/NC-14 — the anti-pattern
    /// this hook replaces); honor the receipt honestly — an
    /// `Invalid`/`Unsupported` [`TextMeasurementReceipt`] means "no
    /// measurement", never a fabricated size. LOAD-BEARING when
    /// overridden; the default is a no-op and byte-identical for every
    /// widget that ignores it. This runtime-invoked hook,
    /// `project_frame_viewport`, and the app reducer are the ONLY
    /// sanctioned writers of external state. Docs:
    /// `docs/public/api/backends.md` ("Text Wrap and Alignment").
    fn project_text_metrics(
        &self,
        _external: &mut Self::ExternalState,
        _metrics: &mut dyn SlipwayTextMetricProvider,
    ) {
    }
}

/// Focus-traversal policy. `focus_member` is consulted at declaration time
/// by [`focus_region_from_focus_capability`] (it becomes the declared
/// traversal member of a plain focus region; `None` declares no explicit
/// tab order). `next_focus`/`previous_focus` remain RESERVED contract
/// surface: no dispatch path consults them today, so real logic there is a
/// silent no-op.
pub trait SlipwayFocusTraversal: SlipwayWidgetTypes {
    fn focus_member(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Option<FocusTraversalMember>;

    fn next_focus(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: FocusTraversalInput,
    ) -> Option<WidgetId>;

    fn previous_focus(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: FocusTraversalInput,
    ) -> Option<WidgetId>;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently calls `semantics`; implementors may
/// return empty/default values ([`reserved_policy_defaults!`] covers it).
/// Consuming it is future work — do not delete the trait or its bounds.
/// Status index: `docs/public/api/trait-surface.md`.
pub trait SlipwaySemantics: SlipwayWidgetTypes {
    fn semantics(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<SemanticNode>;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core dispatch path currently consults `hit_test` (dispatch
/// hit-testing runs on declared hit regions, not this trait); implementors
/// may return empty/default values. Consuming it is future work — do not
/// delete the trait or its bounds.
pub trait SlipwayHitTesting: SlipwayWidgetTypes {
    fn hit_test(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: HitTestInput,
    ) -> HitTestOutput;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core dispatch path currently consults `scroll_state` or
/// `virtual_viewports`; implementors may return empty/default values.
/// Consuming it is future work — do not delete the trait or its bounds.
pub trait SlipwayViewportContracts: SlipwayWidgetTypes {
    fn scroll_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<ScrollState>;

    fn virtual_viewports(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<VirtualViewportRange>;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core dispatch path currently consults `overlays`,
/// `anchored_surfaces`, or `portals`; implementors may return
/// empty/default values. Consuming it is future work — do not delete the
/// trait or its bounds.
pub trait SlipwayOverlayContracts: SlipwayWidgetTypes {
    fn overlays(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<OverlayDeclaration>;

    fn anchored_surfaces(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<AnchoredSurfaceDeclaration>;

    fn portals(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<PortalDeclaration>;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core dispatch path currently consults `commands`;
/// implementors may return empty/default values
/// ([`reserved_policy_defaults!`] covers it). Consuming it is future work
/// — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayCommandContracts: SlipwayWidgetTypes {
    fn commands(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<CommandDeclaration>;
}

/// RESERVED contract surface: declared ahead of consumption. Provider
/// surface declarations reach backends through the native wrapper specs
/// and the view definition, not this trait; no path outside capability
/// tests consults `render_surfaces` ([`reserved_policy_defaults!`] covers
/// it). Consuming it is future work — do not delete the trait or its
/// bounds. Status index: `docs/public/api/trait-surface.md`.
pub trait SlipwayRenderSurfaces: SlipwayWidgetTypes {
    fn render_surfaces(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<RenderSurfaceDeclaration>;
}

/// Text policy 1/9 for text-input widgets: the current buffer contents.
/// LOAD-BEARING: snapshotted at declaration time by
/// [`text_edit_focus_region_from_capability`] into the region's
/// [`TextEditRegionDeclaration`]. The snapshot's `target` must be the
/// focus-region owner or admission refuses with
/// `view_contract.text_edit_buffer_target_mismatch`. IME contract:
/// `docs/public/api/ime.md`.
pub trait SlipwayTextBufferPolicy: SlipwayWidgetTypes {
    fn text_buffer(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextBufferSnapshot;
}

/// Text policy 2/9: selection mode and current selection range.
/// LOAD-BEARING via [`text_edit_focus_region_from_capability`]. Target
/// must match the region owner
/// (`view_contract.text_edit_selection_target_mismatch`); ranges must stay
/// inside the buffer (`view_contract.text_edit_selection_out_of_bounds`).
pub trait SlipwayTextSelectionPolicy: SlipwayWidgetTypes {
    fn text_selection(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextSelectionPolicyDeclaration;
}

/// Text policy 3/9: IME (preedit/composition) support declaration —
/// required for CJK input. LOAD-BEARING via
/// [`text_edit_focus_region_from_capability`]. Target must match the
/// region owner (`view_contract.text_edit_composition_target_mismatch`);
/// composition cursors must stay inside the buffer
/// (`view_contract.text_edit_composition_cursor_out_of_bounds`).
/// Platform IME contract and Hangul checklist: `docs/public/api/ime.md`.
pub trait SlipwayImeCompositionPolicy: SlipwayWidgetTypes {
    fn ime_composition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> ImeCompositionPolicyDeclaration;
}

/// Text policy 4/9: caret placement evidence. LOAD-BEARING via
/// [`text_edit_focus_region_from_capability`]; `measurement` carries real
/// text-measurement evidence when the widget opted into the measurement
/// policies (`None` otherwise — do not fabricate metrics). Target must
/// match the region owner
/// (`view_contract.text_edit_caret_target_mismatch`).
pub trait SlipwayCaretGeometryPolicy: SlipwayWidgetTypes {
    fn caret_geometry(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        measurement: Option<&TextMeasurementEvidence>,
    ) -> CaretGeometryEvidence;
}

/// Text policy 5/9: which edit commands the region supports. An editable
/// region MUST declare enabled `InsertText`, a delete command, and
/// `ReplaceBuffer` or admission refuses
/// (`view_contract.text_edit_missing_insert_command` /
/// `..._missing_delete_command` / `..._missing_replace_buffer_command` —
/// native backend text widgets replace the buffer). LOAD-BEARING via
/// [`text_edit_focus_region_from_capability`].
pub trait SlipwayTextEditCommandPolicy: SlipwayWidgetTypes {
    fn text_edit_commands(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<TextEditCommandDeclaration>;
}

/// Text policy 6/9: explicit colors/borders for the input surface —
/// Slipway never reads backend theme defaults as style authority.
/// LOAD-BEARING via [`text_edit_focus_region_from_capability`]. Target
/// must match the region owner
/// (`view_contract.text_edit_visual_style_target_mismatch`); metrics must
/// be finite and non-negative
/// (`view_contract.text_edit_visual_style_invalid_metric`). Token
/// contract: `docs/public/api/ime.md`.
pub trait SlipwayTextInputVisualStylePolicy: SlipwayWidgetTypes {
    fn text_input_visual_style(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextInputVisualStyleDeclaration;
}

/// Text policy 7/9: explicit typography (family/size/weight) for the
/// input surface. LOAD-BEARING via
/// [`text_edit_focus_region_from_capability`]. Target must match the
/// region owner (`view_contract.text_edit_typography_target_mismatch`);
/// font size must be finite and positive
/// (`view_contract.text_edit_typography_invalid_font_size`).
pub trait SlipwayTextInputTypographyPolicy: SlipwayWidgetTypes {
    fn text_input_typography(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextInputTypographyDeclaration;
}

/// Text policy 8/9: undo/redo depth evidence. LOAD-BEARING via
/// [`text_edit_focus_region_from_capability`]. Target must match the
/// region owner (`view_contract.text_edit_undo_target_mismatch`).
pub trait SlipwayTextUndoRedoPolicy: SlipwayWidgetTypes {
    fn text_undo_redo(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextUndoRedoEvidence;
}

/// Declares how an event routes through the widget tree. LOAD-BEARING
/// twice: per event on live dispatch (via [`declared_event_handling`])
/// and ONCE at declaration time by
/// [`hit_region_from_pointer_capability`], which freezes the returned
/// route into the hit region (see that helper's doc for the snapshot
/// caveat). The route must be non-empty and contain the region target
/// with a matching address, or admission refuses with
/// `view_contract.hit_route_empty` /
/// `view_contract.hit_route_target_missing` /
/// `view_contract.hit_route_address_mismatch`. `route_id` is a
/// human/debug label, never the identity key. Model:
/// `docs/public/api/core.md` ("Interaction Declarations").
pub trait SlipwayEventRoutingPolicy: SlipwayWidgetTypes {
    fn event_routing_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        event: &InputEvent,
    ) -> EventRoutingPolicyDeclaration;
}

/// Declares the intended disposition (handled/propagate) of an event along
/// a route, consulted BEFORE the handler runs: the backend physical path
/// refuses a declared-unhandled event without calling
/// [`SlipwayLogic::handle_event`] ([`refuse_event_declared_unhandled`]),
/// and the declared propagation is applied only when the handler actually
/// handled the event. The declared disposition never overrides the widget
/// handler's actual outcome: the two are reconciled only by runtime
/// diagnostics ([`apply_event_handling_declaration`] warns — and the
/// physical path errors — on mismatch).
///
/// WHEN: author this together with `handle_event` from ONE table via
/// [`event_handling_table!`] — that is the taught pattern
/// (`crates/slipway-example-authored/src/internal_logic.rs`). A
/// hand-written impl duplicates handledness the handler already encodes,
/// which is the audited guaranteed-drift design (NC-8); hand-write it
/// only for capture/bubble-phase declarations or custom propagation, and
/// build the evidence with [`target_event_disposition`]. Failure mode of
/// drift: `event_declaration.handler_ignored_declared_handled` /
/// `event_declaration.handler_handled_declared_unhandled` (Error on the
/// physical path, which also strips the outcome). LOAD-BEARING. Docs:
/// `docs/public/api/core.md` ("Interaction Declarations"),
/// `docs/public/api/diagnostics.md` ("event_declaration").
pub trait SlipwayEventDispositionPolicy: SlipwayWidgetTypes {
    fn event_disposition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        event: &InputEvent,
        route: &EventRoute,
    ) -> EventPropagationEvidence;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core dispatch path currently consults
/// `pointer_capture_policy` — live capture behavior is authored per hit
/// region via [`PointerCaptureIntent`] instead; implementors may return
/// empty/default values ([`reserved_policy_defaults!`] covers it).
/// Consuming it is future work — do not delete the trait or its bounds.
/// Status index: `docs/public/api/trait-surface.md`.
pub trait SlipwayPointerCapturePolicy: SlipwayWidgetTypes {
    fn pointer_capture_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        pointer: PointerDetails,
    ) -> PointerCapturePolicyDeclaration;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `debug_event_trace_policy` —
/// the runtime trace ring is bounded by a fixed runtime capacity, not
/// this declaration; implementors may return empty/default values
/// ([`reserved_policy_defaults!`] covers it). Consuming it is future work
/// — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayDebugEventTracePolicy: SlipwayWidgetTypes {
    fn debug_event_trace_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> DebugEventTracePolicyDeclaration;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core layout path currently consults
/// `container_layout_policy` (it is exercised only by the core
/// API-surface tests); implementors may return empty/default values.
/// Consuming it is future work — do not delete the trait or its bounds.
pub trait SlipwayContainerLayoutPolicy: SlipwayWidgetTypes {
    fn container_layout_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ContainerLayoutPolicyDeclaration;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core layout path currently consults `child_constraints`;
/// implementors may return empty/default values. Consuming it is future
/// work — do not delete the trait or its bounds.
pub trait SlipwayChildConstraintPolicy: SlipwayWidgetTypes {
    fn child_constraints(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> Vec<ChildConstraintPolicyDeclaration>;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core layout path currently consults
/// `layout_invalidation_policy`; implementors may return empty/default
/// values. Consuming it is future work — do not delete the trait or its
/// bounds.
pub trait SlipwayLayoutInvalidationPolicy: SlipwayWidgetTypes {
    fn layout_invalidation_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> LayoutInvalidationPolicyDeclaration;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core layout path currently consults `layout_evidence`;
/// implementors may return empty/default values. Consuming it is future
/// work — do not delete the trait or its bounds.
pub trait SlipwayLayoutEvidencePolicy: SlipwayWidgetTypes {
    fn layout_evidence(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        output: &LayoutOutput,
    ) -> LayoutEvidence;
}

/// Declares a widget's scroll geometry (viewport, content bounds, offset,
/// axes, consumption). LOAD-BEARING: consulted at declaration time by
/// [`scroll_region_from_scrollable_capability_with_order`] (and the plain
/// variant), which passes a [`LayoutInput`] derived from the final
/// `layout.bounds` and copies the returned fields into the
/// [`ScrollRegionDeclaration`] it builds — so the geometry contract
/// documented on that struct (offset sign/clamp, travel relationship,
/// paint-responsibility split) is authored HERE. Implement it by hand for
/// every scrollable container; the geometry-family refusal codes and the
/// helper flow are in `docs/public/api/routing-and-scroll.md` and
/// `docs/public/api/diagnostics.md`.
///
/// A widget declaring several scroll regions with distinct geometry can
/// return one region's geometry here and override the others per region
/// after the helper call — the declaration is admitted per region, so the
/// override stays inside the contract (the example's
/// `nested_scroll_region` documents this idiom).
pub trait SlipwayScrollBehaviorPolicy: SlipwayWidgetTypes {
    fn scroll_behavior_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ScrollBehaviorPolicyDeclaration;
}

/// Declares the wheel-routing mode a widget's scroll regions author.
///
/// The declared [`WheelRoutingPolicyDeclaration::routing`] is snapshotted
/// into [`ScrollRegionDeclaration::wheel_routing`] at declaration time (see
/// [`scroll_region_from_scrollable_capability_with_order`]), and that frozen
/// declaration-time value is what wheel selection routes on (ADR-0002 B2).
/// The signature is declaration-time-only BY CONSTRUCTION: implementations
/// receive `region`, the resolved identity of the scroll region being
/// declared — the only input the committed routing semantics may depend
/// on — so a widget that declares several scroll regions can author a
/// different mode per region while the value stays frozen per region per
/// frame (ADR-0002 B3). Per-event dynamic wheel routing stays unsupported;
/// earlier revisions passed a synthetic zero-delta [`WheelEvent`] here,
/// which invited delta-dependent logic that could never take effect, and
/// the signature was made honest in the post-B3 trait-surface cleanup (the
/// ADR-0002 recorded follow-up).
pub trait SlipwayWheelRoutingPolicy: SlipwayWidgetTypes {
    fn wheel_routing_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        region: &PresentationRegionId,
    ) -> WheelRoutingPolicyDeclaration;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core dispatch path currently consults
/// `viewport_observation`; implementors may return empty/default values.
/// Consuming it is future work — do not delete the trait or its bounds.
pub trait SlipwayViewportObservationPolicy: SlipwayWidgetTypes {
    fn viewport_observation(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> ViewportObservationEvidence;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core dispatch path currently consults
/// `virtual_collection_policy`; implementors may return empty/default
/// values. Consuming it is future work — do not delete the trait or its
/// bounds.
pub trait SlipwayVirtualCollectionPolicy: SlipwayWidgetTypes {
    fn virtual_collection_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> VirtualCollectionPolicyDeclaration;
}

pub trait SlipwayCanvasProvider {
    fn canvas_surfaces(&self) -> Vec<ProviderSurfaceRequest>;
}

pub trait SlipwayGpuSurfaceProvider {
    fn gpu_surfaces(&self) -> Vec<ProviderSurfaceRequest>;
}

pub trait SlipwayMediaProvider {
    fn media_surfaces(&self) -> Vec<ProviderSurfaceRequest>;
}

pub trait SlipwayPlotProvider {
    fn plot_surfaces(&self) -> Vec<ProviderSurfaceRequest>;
}

pub trait SlipwayProviderHitTestPolicy {
    fn provider_hit_test(&self, request: HitTestInput) -> ProviderHitTestEvidence;
}

pub trait SlipwayProviderSnapshotPolicy {
    fn provider_snapshot(&mut self, request: ProviderSnapshotRequest) -> ProviderSnapshotEvidence;
}

/// Resolves a declared font source into installation/refusal evidence.
/// WHEN: implement on widgets that declare text with a font source
/// (`docs/public/api/ime.md`); LOAD-BEARING on egui (text-edit font
/// installation and the `SlipwayEguiBackendContract` root gate). For the
/// root of a plain facade app, [`SlipwayAppWidget`] provides this by
/// delegating to [`SlipwayApp::resolve_app_font`] (default: honest
/// refusal with `app-font-resolution-refused`,
/// `docs/public/api/diagnostics.md`). Never report a resolved/installed
/// font that was not actually validated; refusal evidence is the honest
/// default. Pattern: `crates/slipway-example-authored/src/app_runner.rs`
/// font glue.
pub trait SlipwayFontResolutionPolicy: SlipwayWidgetTypes {
    fn resolve_font(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        request: FontResolutionRequest,
    ) -> FontResolutionEvidence;
}

pub trait SlipwayAssetResolutionPolicy: SlipwayWidgetTypes {
    fn resolve_asset(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        request: AssetResolutionRequest,
    ) -> AssetResolutionEvidence;
}

pub trait SlipwayImageDecodeProvider {
    fn decode_image(&mut self, request: ImageDecodeRequest) -> ImageDecodeEvidence;
}

pub trait SlipwayStyleTokenPolicy: SlipwayWidgetTypes {
    fn resolve_style_token(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        request: StyleTokenRequest,
    ) -> StyleTokenEvidence;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `time_source` outside the
/// API-surface tests; implementors may return empty/default values
/// ([`reserved_policy_defaults!`] covers it). Consuming it is future work
/// — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayTimeSourcePolicy: SlipwayWidgetTypes {
    fn time_source(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TimeSourceSnapshot;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `random_source`; implementors
/// may return empty/default values ([`reserved_policy_defaults!`] covers
/// it). Consuming it is future work — do not delete the trait or its
/// bounds. Status index: `docs/public/api/trait-surface.md`.
pub trait SlipwayRandomSourcePolicy: SlipwayWidgetTypes {
    fn random_source(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> RandomSourceSnapshot;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `external_data_snapshot`;
/// implementors may return empty/default values
/// ([`reserved_policy_defaults!`] covers it). Consuming it is future work
/// — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayExternalDataSnapshotPolicy: SlipwayWidgetTypes {
    fn external_data_snapshot(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> ExternalDataSnapshot;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `animation_timeline_policy`;
/// implementors may return empty/default values
/// ([`reserved_policy_defaults!`] covers it). Consuming it is future work
/// — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayAnimationTimelinePolicy: SlipwayWidgetTypes {
    fn animation_timeline_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> AnimationTimelinePolicyDeclaration;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `command_invocation_policy`
/// outside the API-surface tests; implementors may return empty/default
/// values ([`reserved_policy_defaults!`] covers it). Consuming it is
/// future work — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayCommandInvocationPolicy: SlipwayWidgetTypes {
    fn command_invocation_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        command: &CommandEvent,
    ) -> CommandInvocationPolicyDeclaration;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `command_status`;
/// implementors may return empty/default values
/// ([`reserved_policy_defaults!`] covers it). Consuming it is future work
/// — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayCommandStatusPolicy: SlipwayWidgetTypes {
    fn command_status(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        command_id: &str,
    ) -> CommandStatusEvidence;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `shortcut_routing_policy`;
/// implementors may return empty/default values
/// ([`reserved_policy_defaults!`] covers it). Consuming it is future work
/// — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayShortcutRoutingPolicy: SlipwayWidgetTypes {
    fn shortcut_routing_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        shortcut: &ShortcutDeclaration,
    ) -> ShortcutRoutingPolicyDeclaration;
}

/// RESERVED contract surface: declared ahead of consumption. No backend,
/// runtime, or core path currently consults `undo_redo_policy` (text
/// undo/redo evidence travels through [`SlipwayTextUndoRedoPolicy`]
/// instead); implementors may return empty/default values
/// ([`reserved_policy_defaults!`] covers it). Consuming it is future work
/// — do not delete the trait or its bounds. Status index:
/// `docs/public/api/trait-surface.md`.
pub trait SlipwayUndoRedoPolicy: SlipwayWidgetTypes {
    fn undo_redo_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> UndoRedoPolicyDeclaration;
}

pub trait SlipwayBackendCapabilityProbe {
    fn backend_capabilities(&self) -> BackendCapabilityReport;
}

pub trait SlipwayUnsupportedCapabilityEvidence {
    fn unsupported_capabilities(
        &self,
        required: &[Capability],
    ) -> Vec<UnsupportedCapabilityEvidence>;
}

pub trait SlipwayBackendParityAdmission {
    fn backend_parity_admission(
        &self,
        required_profiles: &[CapabilityProfileKind],
    ) -> BackendParityAdmission;
}

pub trait SlipwayViewDefinition: SlipwayWidgetTypes {
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition;

    fn visible_backend_view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        self.view_definition(external, local, input)
    }
}

pub trait SlipwayOffscreenRenderer {
    fn render_offscreen(&mut self, packet: RenderPacket) -> Result<RenderEvidence, RenderRefusal>;
}

pub trait SlipwayIntrinsicSizing: SlipwayWidgetTypes {
    fn intrinsic_size(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> IntrinsicSize;
}

pub trait SlipwaySizePolicy: SlipwayWidgetTypes {
    fn size_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> SizePolicyDeclaration;
}

pub trait SlipwayResizePolicy: SlipwayWidgetTypes {
    fn resize_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ResizePolicyDeclaration;
}

pub trait SlipwayOverflowPolicy: SlipwayWidgetTypes {
    fn overflow_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> OverflowPolicyDeclaration;
}

pub trait SlipwayAutoLayoutPolicy: SlipwayWidgetTypes {
    fn auto_layout_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> AutoLayoutPolicyDeclaration;
}

pub trait SlipwayResponsiveVariants: SlipwayWidgetTypes {
    fn responsive_variant(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ResponsiveVariant;
}

/// Text policy 9/9: line mode (single/multi), wrap mode, and the text
/// viewport. LOAD-BEARING via [`text_edit_focus_region_from_capability`],
/// which copies `viewport` and `line_mode` into the text-edit region.
/// A `SingleLine` buffer must not contain newlines
/// (`view_contract.text_edit_single_line_contains_newline`); the visible
/// range must stay inside the buffer
/// (`view_contract.text_edit_viewport_range_out_of_bounds`).
pub trait SlipwayTextFlowPolicy: SlipwayWidgetTypes {
    fn text_flow_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> TextFlowPolicy;
}

pub trait SlipwayTextMeasurementPolicy: SlipwayWidgetTypes {
    fn text_measurement_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> TextMeasurementPolicyDeclaration;

    fn text_measurement_evidence<P>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
        provider: &mut P,
    ) -> TextMeasurementEvidence
    where
        P: SlipwayTextMetricProvider;
}

pub trait SlipwayTextMeasurementCachePolicy: SlipwayWidgetTypes {
    fn text_measurement_cache_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> Vec<TextMeasurementCachePolicyDeclaration>;
}

pub trait SlipwayCachedTextMeasurementPolicy: SlipwayWidgetTypes {
    fn cached_text_measurement_evidence<P, C>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
        provider: &mut P,
        cache: &mut C,
    ) -> TextMeasurementEvidence
    where
        P: SlipwayTextMetricProvider,
        C: SlipwayTextMeasurementCache;
}

pub trait SlipwayFitOverflowEvidence: SlipwayWidgetTypes {
    fn fit_overflow_evidence(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
        text_measurement: Option<&TextMeasurementEvidence>,
    ) -> Vec<FitOverflowEvidence>;
}

pub trait SlipwayLayerPolicy: SlipwayWidgetTypes {
    fn layer_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> LayerPolicy;
}

pub trait SlipwayScrollPolicy: SlipwayWidgetTypes {
    fn scroll_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ScrollPolicy;
}

pub trait SlipwayCollectionPolicy: SlipwayWidgetTypes {
    fn collection_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> CollectionPolicy;
}

pub trait SlipwayInteractionStateStyle: SlipwayWidgetTypes {
    fn interaction_state_styles(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> Vec<InteractionStateStyle>;
}

pub trait SlipwayLayoutIntent: SlipwayWidgetTypes {
    fn layout_intent(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> LayoutIntentProbe;
}

/// The widget's view mechanics: local-state initialization, layout,
/// paint, and state observation. One of the three mandatory traits
/// forming [`SlipwayWidget`]; LOAD-BEARING every frame. Model:
/// `docs/public/api/core.md` ("View And Layout").
///
/// `layout` must return TARGET-LOCAL bounds with origin `0,0` — parent
/// placement is represented only by [`ChildPlacement`] — or admission
/// refuses with `view_contract.layout_bounds_not_target_local`
/// (`view_contract.layout_bounds_invalid` for non-finite bounds). `paint`
/// declares visuals only: nothing painted reacts to input until the
/// matching hit/focus/scroll region is declared, and text paint must
/// carry an explicit [`TextStyle`] ([`PaintOp::styled_text`] is the only
/// text constructor). For scrollable content, painted geometry and
/// declared scroll offset MUST derive from the same source — see
/// [`ScrollRegionDeclaration`]. `observe_state` is the explicit
/// debug/probe observation hook, not a render path.
pub trait SlipwayView: SlipwayWidgetTypes {
    fn initial_local_state(&self) -> Self::LocalState;

    fn layout(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: LayoutInput,
        output: LayoutOutputBuilder,
    ) -> LayoutOutput;

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp>;

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation>;
}

pub fn layout_view<W: SlipwayView>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    input: LayoutInput,
) -> LayoutOutput {
    let output = LayoutOutputBuilder::for_input(&input);
    widget.layout(external, local, input, output)
}

pub fn layout_view_definition<W: SlipwayView>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    input: ViewDefinitionInput,
) -> (FrameIdentity, LayoutOutput) {
    let (frame, layout_input, output) = input.into_layout_parts();
    let layout = widget.layout(external, local, layout_input, output);
    (frame, layout)
}

pub fn prepare_leaf_layout(output: LayoutOutputBuilder, bounds: TargetLocalRect) -> LayoutOutput {
    output.finish(bounds)
}

pub fn prepare_resolved_layout(
    mut output: LayoutOutputBuilder,
    bounds: TargetLocalRect,
    resolved: impl IntoIterator<Item = (ChildLayoutPlan, ChildLayoutResult)>,
) -> Result<LayoutOutput, BoxGeometryDiagnostic> {
    for (plan, result) in resolved {
        output.push_resolved(plan, result)?;
    }
    Ok(output.finish(bounds))
}

fn prepare_child_paint_layout(input: &LayoutInput, bounds: TargetLocalRect) -> LayoutOutput {
    prepare_leaf_layout(LayoutOutputBuilder::for_input(input), bounds)
}

/// The composite every authored widget must satisfy: shorthand for
/// [`SlipwaySsot`] + [`SlipwayLogic`] + [`SlipwayView`] (blanket-implemented,
/// never implemented directly). LOAD-BEARING: this is the bound the
/// capability bundles, helpers, and app composition require. Pattern:
/// `docs/public/quickstart-authoring.md`.
pub trait SlipwayWidget: SlipwaySsot + SlipwayLogic + SlipwayView {}

impl<W> SlipwayWidget for W where W: SlipwaySsot + SlipwayLogic + SlipwayView {}

/// Capability bundle for text-input widgets. No bound may be removed: the
/// bundle is contract surface declared ahead of full consumption.
///
/// Load-bearing bounds today: [`SlipwayWidget`]; the text policy bounds
/// consumed by [`text_edit_focus_region_from_capability`] (buffer,
/// selection, IME composition, caret geometry, edit commands, visual
/// style, typography, undo/redo, text flow); the measurement bounds
/// ([`SlipwayTextMeasurementPolicy`], [`SlipwayTextMeasurementCachePolicy`],
/// [`SlipwayCachedTextMeasurementPolicy`]) consumed by the measurement
/// evidence and layout-intent paths; and [`SlipwayEventRoutingPolicy`] +
/// [`SlipwayEventDispositionPolicy`] (live dispatch via
/// [`declared_event_handling`]).
///
/// RESERVED bounds (no consumer on this bundle's paths):
/// [`SlipwayFocusTraversal`] ([`text_edit_focus_region_from_capability`]
/// takes the traversal member as a parameter; only the plain-focus helper
/// [`focus_region_from_focus_capability`] consults `focus_member`),
/// [`SlipwaySemantics`], and [`SlipwayDebugEventTracePolicy`].
///
/// A widget missing load-bearing bounds fails at the bundle with a
/// triaged error, even after [`reserved_policy_defaults!`] covers every
/// RESERVED bound:
///
/// ```compile_fail,E0277
/// use slipway_core::*;
///
/// struct Incomplete;
/// impl SlipwayWidgetTypes for Incomplete {
///     type ExternalState = ();
///     type LocalState = ();
///     type AppMessage = ();
/// }
/// # impl SlipwaySsot for Incomplete {
/// #     fn id(&self) -> WidgetId { WidgetId::from("incomplete") }
/// #     fn capabilities(&self) -> Vec<Capability> { Vec::new() }
/// #     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode { TopologyNode::leaf(self.id()) }
/// #     fn unsupported(&self) -> Vec<Diagnostic> { Vec::new() }
/// # }
/// reserved_policy_defaults!(Incomplete);
///
/// // LOAD-BEARING bounds (the widget trio and the text policies) are
/// // still missing, so the bundle refuses:
/// fn requires_bundle<T: SlipwayTextInputCapability>() {}
/// requires_bundle::<Incomplete>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not satisfy the `SlipwayTextInputCapability` bundle (text-input widgets)",
    label = "missing one or more supertraits of the text-input capability bundle",
    note = "each missing supertrait is named in its own `help:` line of this error; implement the LOAD-BEARING ones by hand: SlipwaySsot + SlipwayLogic + SlipwayView (= SlipwayWidget), the nine text policies consumed by `text_edit_focus_region_from_capability` (SlipwayTextBufferPolicy, SlipwayTextSelectionPolicy, SlipwayImeCompositionPolicy, SlipwayCaretGeometryPolicy, SlipwayTextEditCommandPolicy, SlipwayTextInputVisualStylePolicy, SlipwayTextInputTypographyPolicy, SlipwayTextUndoRedoPolicy, SlipwayTextFlowPolicy), the measurement policies (SlipwayTextMeasurementPolicy, SlipwayTextMeasurementCachePolicy, SlipwayCachedTextMeasurementPolicy), and SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy",
    note = "the remaining bounds are RESERVED (no runtime consumer; real logic in them is a silent no-op, except SlipwayFocusTraversal::focus_member which `focus_region_from_focus_capability` snapshots at declaration time): SlipwayFocusTraversal, SlipwaySemantics, SlipwayDebugEventTracePolicy — satisfy every RESERVED bound at once with `slipway_core::reserved_policy_defaults!(YourWidget);`"
)]
pub trait SlipwayTextInputCapability:
    SlipwayWidget
    + SlipwayTextBufferPolicy
    + SlipwayTextSelectionPolicy
    + SlipwayImeCompositionPolicy
    + SlipwayCaretGeometryPolicy
    + SlipwayTextEditCommandPolicy
    + SlipwayTextInputVisualStylePolicy
    + SlipwayTextInputTypographyPolicy
    + SlipwayTextUndoRedoPolicy
    + SlipwayTextFlowPolicy
    + SlipwayTextMeasurementPolicy
    + SlipwayTextMeasurementCachePolicy
    + SlipwayCachedTextMeasurementPolicy
    + SlipwayFocusTraversal
    + SlipwaySemantics
    + SlipwayEventRoutingPolicy
    + SlipwayEventDispositionPolicy
    + SlipwayDebugEventTracePolicy
{
}

impl<W> SlipwayTextInputCapability for W where
    W: SlipwayWidget
        + SlipwayTextBufferPolicy
        + SlipwayTextSelectionPolicy
        + SlipwayImeCompositionPolicy
        + SlipwayCaretGeometryPolicy
        + SlipwayTextEditCommandPolicy
        + SlipwayTextInputVisualStylePolicy
        + SlipwayTextInputTypographyPolicy
        + SlipwayTextUndoRedoPolicy
        + SlipwayTextFlowPolicy
        + SlipwayTextMeasurementPolicy
        + SlipwayTextMeasurementCachePolicy
        + SlipwayCachedTextMeasurementPolicy
        + SlipwayFocusTraversal
        + SlipwaySemantics
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy
        + SlipwayDebugEventTracePolicy
{
}

/// Capability bundle for pointer-interactive widgets. All bounds are
/// load-bearing today: [`SlipwayEventRoutingPolicy`] is consulted at
/// declaration time by [`hit_region_from_pointer_capability`] and per
/// event by [`declared_event_handling`]; [`SlipwayEventDispositionPolicy`]
/// per event by [`declared_event_handling`].
///
/// A widget missing load-bearing bounds fails at the bundle with a
/// triaged error:
///
/// ```compile_fail,E0277
/// use slipway_core::*;
///
/// struct Incomplete;
/// impl SlipwayWidgetTypes for Incomplete {
///     type ExternalState = ();
///     type LocalState = ();
///     type AppMessage = ();
/// }
///
/// // ALL bounds of this bundle are load-bearing; the widget trio and the
/// // routing/disposition policies are missing, so the bundle refuses:
/// fn requires_bundle<T: SlipwayPointerRegionCapability>() {}
/// requires_bundle::<Incomplete>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not satisfy the `SlipwayPointerRegionCapability` bundle (pointer-interactive widgets)",
    label = "missing one or more supertraits of the pointer-region capability bundle",
    note = "each missing supertrait is named in its own `help:` line of this error; ALL bounds of this bundle are LOAD-BEARING (none are RESERVED): implement SlipwaySsot + SlipwayLogic + SlipwayView (= SlipwayWidget), SlipwayEventRoutingPolicy, and SlipwayEventDispositionPolicy by hand, then declare the region with `hit_region_from_pointer_capability`"
)]
pub trait SlipwayPointerRegionCapability:
    SlipwayWidget + SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy
{
}

impl<W> SlipwayPointerRegionCapability for W where
    W: SlipwayWidget + SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy
{
}

/// Builds a [`HitRegionDeclaration`] whose route is snapshotted from the
/// widget's [`SlipwayEventRoutingPolicy`] at declaration time. Use it for
/// every widget declaring [`Capability::PointerInput`] (the sanctioned
/// constructor — the struct is `#[non_exhaustive]`); the bound is
/// [`SlipwayPointerRegionCapability`]. Contract map:
/// `docs/public/api/core.md`; ordering:
/// `docs/public/api/routing-and-scroll.md`.
///
/// `bounds` MUST match the painted geometry of the control — derive both
/// from the same named constants, since admission validates bounds
/// against layout (`view_contract.hit_bounds_outside_layout`) but cannot
/// know what the paint looks like: a hit/paint drift routes clicks to
/// rows the user does not see. `order` resolves overlaps (equal-order
/// overlaps refuse with `view_contract.ambiguous_hit_overlap`); a
/// `route_id` of `Some(..)` overrides the snapshotted route's debug label
/// ONLY — route identity (address/path/phase) still comes from the
/// routing policy.
///
/// The helper invokes `event_routing_policy` ONCE with a synthetic
/// primary-button `Press` at the region origin; the returned route is
/// frozen into the declaration for the frame. The synthetic event does NOT
/// make the declared route per-event dynamic: its kind, button, position,
/// and pointer details are placeholders, so an implementation consulted
/// through this helper must derive the route from widget/region identity
/// and state only. Unlike [`SlipwayWheelRoutingPolicy`] (whose signature
/// was made declaration-time-only by construction in the trait-surface
/// cleanup), `SlipwayEventRoutingPolicy` keeps its `&InputEvent` parameter
/// because the live dispatch path ([`declared_event_handling`])
/// legitimately consults it per event; narrowing this helper's snapshot
/// input is a recorded follow-up decision, not silently done here.
#[allow(clippy::too_many_arguments)]
#[must_use = "push the returned declaration into ViewDefinition::hit_regions; building it without declaring it routes nothing"]
pub fn hit_region_from_pointer_capability<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    id: PresentationRegionId,
    address: Option<WidgetSlotAddress>,
    bounds: TargetLocalRect,
    event_coordinate_space: PointerEventCoordinateSpace,
    order: HitRegionOrder,
    route_id: Option<String>,
    cursor: CursorCapability,
    enabled: bool,
    capture: PointerCaptureIntent,
) -> HitRegionDeclaration
where
    W: SlipwayPointerRegionCapability,
{
    let target = widget.id();
    let mut route = widget
        .event_routing_policy(
            external,
            local,
            &InputEvent::Pointer(PointerEvent {
                target: target.clone(),
                target_slot: address.clone(),
                position: bounds.origin,
                target_bounds: Some(bounds),
                kind: PointerEventKind::Press,
                button: Some(PointerButton::Primary),
                details: PointerDetails::default(),
            }),
        )
        .route;
    if route_id.is_some() {
        route.route_id = route_id;
    }

    HitRegionDeclaration {
        id,
        target,
        address,
        bounds,
        event_coordinate_space,
        order,
        route,
        cursor,
        enabled,
        capture,
        capture_evidence: Vec::new(),
    }
}

/// Builds the text-edit [`FocusRegionDeclaration`] for widgets declaring
/// [`Capability::TextInput`] — the sanctioned constructor (the structs are
/// `#[non_exhaustive]`), bound by [`SlipwayTextInputCapability`]. It
/// snapshots all nine text policies at declaration time: buffer,
/// selection, IME composition, caret geometry (fed `measurement`), edit
/// commands, visual style, typography, undo/redo, and text flow (whose
/// `viewport`/`line_mode` become the region's). Declaring `TextInput`
/// without an enabled region built here refuses admission with
/// `view_contract.text_input_missing_text_edit_focus_region`; per-policy
/// refusal codes are on each `SlipwayText*Policy` trait. IME contract:
/// `docs/public/api/ime.md`.
///
/// Parameters beyond the policy snapshot: `id` names the region for
/// evidence and MCP control; `bounds` is the target-local editable area
/// and must match the painted input surface; `member` is the traversal
/// member (passed in rather than read from
/// [`SlipwayFocusTraversal::focus_member`] — pass
/// `SlipwayFocusTraversal::focus_member(widget, external, local)` to reuse
/// it, or `None` for no explicit tab order); `layout_input` feeds the
/// text-flow policy; `measurement` carries real text-measurement evidence
/// or `None` (never fabricate metrics). For focusable but non-editable
/// widgets use [`focus_region_from_focus_capability`] instead.
#[allow(clippy::too_many_arguments)]
#[must_use = "push the returned declaration into ViewDefinition::focus_regions; building it without declaring it wires no text input"]
pub fn text_edit_focus_region_from_capability<W>(
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
    W: SlipwayTextInputCapability,
{
    let text_flow = widget.text_flow_policy(external, local, layout_input);
    FocusRegionDeclaration {
        id,
        target: widget.id(),
        address,
        bounds,
        member,
        enabled,
        text_edit: Some(TextEditRegionDeclaration {
            buffer: widget.text_buffer(external, local),
            selection: widget.text_selection(external, local),
            composition: widget.ime_composition(external, local),
            caret: widget.caret_geometry(external, local, measurement),
            visual_style: widget.text_input_visual_style(external, local),
            typography: widget.text_input_typography(external, local),
            edit_commands: widget.text_edit_commands(external, local),
            undo_redo: Some(widget.text_undo_redo(external, local)),
            viewport: text_flow.viewport,
            line_mode: text_flow.line_mode,
            diagnostics: Vec::new(),
        }),
    }
}

/// Builds a plain (non-text) [`FocusRegionDeclaration`] for widgets that
/// declare `Capability::FocusInput`, `Capability::KeyboardInput`, or
/// `Capability::FocusRegionPresentation` without text editing: `text_edit`
/// is always `None`. The traversal member is snapshotted from the widget's
/// [`SlipwayFocusTraversal::focus_member`] at declaration time (a `None`
/// member declares no explicit tab order).
///
/// Declaring one of those capabilities without at least one enabled focus
/// region refuses admission with
/// `view_contract.focus_capability_missing_focus_region`. For text-input
/// widgets use [`text_edit_focus_region_from_capability`] instead — a
/// plain region cannot satisfy
/// `view_contract.text_input_missing_text_edit_focus_region`.
#[must_use = "push the returned declaration into ViewDefinition::focus_regions; building it without declaring it receives no focus or keyboard input"]
pub fn focus_region_from_focus_capability<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    id: PresentationRegionId,
    address: Option<WidgetSlotAddress>,
    bounds: TargetLocalRect,
    enabled: bool,
) -> FocusRegionDeclaration
where
    W: SlipwayWidget + SlipwayFocusTraversal,
{
    FocusRegionDeclaration {
        id,
        target: widget.id(),
        address,
        bounds,
        member: widget.focus_member(external, local),
        enabled,
        text_edit: None,
    }
}

/// Capability bundle for scrollable containers. No bound may be removed:
/// the bundle is contract surface declared ahead of full consumption.
///
/// Load-bearing bounds today: [`SlipwayWidget`],
/// [`SlipwayScrollBehaviorPolicy`] and [`SlipwayWheelRoutingPolicy`]
/// (consulted by [`scroll_region_from_scrollable_capability_with_order`]),
/// and [`SlipwayEventRoutingPolicy`] + [`SlipwayEventDispositionPolicy`]
/// (consulted per event by [`declared_event_handling`] and at declaration
/// time by [`hit_region_from_pointer_capability`]).
///
/// RESERVED bounds (no current consumer; see each trait's doc):
/// [`SlipwayContainerLayoutPolicy`], [`SlipwayChildConstraintPolicy`],
/// [`SlipwayLayoutInvalidationPolicy`], [`SlipwayLayoutEvidencePolicy`],
/// [`SlipwayViewportObservationPolicy`],
/// [`SlipwayVirtualCollectionPolicy`], [`SlipwayHitTesting`], and
/// [`SlipwaySemantics`] (no harness path calls `semantics` today).
///
/// A widget missing load-bearing bounds fails at the bundle with a
/// triaged error, even after [`reserved_policy_defaults!`] covers every
/// RESERVED bound:
///
/// ```compile_fail,E0277
/// use slipway_core::*;
///
/// struct Incomplete;
/// impl SlipwayWidgetTypes for Incomplete {
///     type ExternalState = ();
///     type LocalState = ();
///     type AppMessage = ();
/// }
/// # impl SlipwaySsot for Incomplete {
/// #     fn id(&self) -> WidgetId { WidgetId::from("incomplete") }
/// #     fn capabilities(&self) -> Vec<Capability> { Vec::new() }
/// #     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode { TopologyNode::leaf(self.id()) }
/// #     fn unsupported(&self) -> Vec<Diagnostic> { Vec::new() }
/// # }
/// reserved_policy_defaults!(Incomplete);
///
/// // LOAD-BEARING bounds (the rest of the widget trio, the scroll
/// // policies, and the routing/disposition policies) are still missing,
/// // so the bundle refuses:
/// fn requires_bundle<T: SlipwayScrollableContainerCapability>() {}
/// requires_bundle::<Incomplete>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not satisfy the `SlipwayScrollableContainerCapability` bundle (scrollable containers)",
    label = "missing one or more supertraits of the scrollable-container capability bundle",
    note = "each missing supertrait is named in its own `help:` line of this error; implement the LOAD-BEARING ones by hand: SlipwaySsot + SlipwayLogic + SlipwayView (= SlipwayWidget), SlipwayScrollBehaviorPolicy, SlipwayWheelRoutingPolicy, SlipwayEventRoutingPolicy, and SlipwayEventDispositionPolicy",
    note = "the remaining bounds are RESERVED (no runtime consumer; real logic in them is a silent no-op): SlipwayContainerLayoutPolicy, SlipwayChildConstraintPolicy, SlipwayLayoutInvalidationPolicy, SlipwayLayoutEvidencePolicy, SlipwayViewportObservationPolicy, SlipwayVirtualCollectionPolicy, SlipwayHitTesting, SlipwaySemantics — satisfy every RESERVED bound at once with `slipway_core::reserved_policy_defaults!(YourWidget);`"
)]
pub trait SlipwayScrollableContainerCapability:
    SlipwayWidget
    + SlipwayContainerLayoutPolicy
    + SlipwayChildConstraintPolicy
    + SlipwayLayoutInvalidationPolicy
    + SlipwayLayoutEvidencePolicy
    + SlipwayScrollBehaviorPolicy
    + SlipwayWheelRoutingPolicy
    + SlipwayViewportObservationPolicy
    + SlipwayVirtualCollectionPolicy
    + SlipwayHitTesting
    + SlipwayEventRoutingPolicy
    + SlipwayEventDispositionPolicy
    + SlipwaySemantics
{
}

impl<W> SlipwayScrollableContainerCapability for W where
    W: SlipwayWidget
        + SlipwayContainerLayoutPolicy
        + SlipwayChildConstraintPolicy
        + SlipwayLayoutInvalidationPolicy
        + SlipwayLayoutEvidencePolicy
        + SlipwayScrollBehaviorPolicy
        + SlipwayWheelRoutingPolicy
        + SlipwayViewportObservationPolicy
        + SlipwayVirtualCollectionPolicy
        + SlipwayHitTesting
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy
        + SlipwaySemantics
{
}

/// Builds a [`ScrollRegionDeclaration`] from the widget's
/// [`SlipwayScrollBehaviorPolicy`] and [`SlipwayWheelRoutingPolicy`] —
/// the sanctioned constructor for widgets declaring
/// [`Capability::WheelInput`] (bound:
/// [`SlipwayScrollableContainerCapability`]). Call it AFTER layout with
/// the final [`LayoutOutput`], never the incoming [`LayoutInput`]: the
/// declared viewport derives from `layout.bounds`. Full contract:
/// `docs/public/api/routing-and-scroll.md`.
///
/// WARNING: this variant defaults [`ScrollRegionDeclaration::order`] to
/// `{0, 0, 0}`. If this region can overlap another enabled
/// wheel-consuming region (nested scroll areas, scrollable content under
/// an overlay), admission refuses with
/// `view_contract.ambiguous_wheel_overlap` — give each region a distinct
/// order via [`scroll_region_from_scrollable_capability_with_order`] (or
/// by assigning `declaration.order` before pushing). Use the plain
/// variant only when nothing can overlap.
#[must_use = "push the returned declaration into ViewDefinition::scroll_regions; building it without declaring it routes no wheel input"]
pub fn scroll_region_from_scrollable_capability<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    layout: &LayoutOutput,
    region_id: Option<PresentationRegionId>,
    address: Option<WidgetSlotAddress>,
    enabled: bool,
) -> ScrollRegionDeclaration
where
    W: SlipwayScrollableContainerCapability,
{
    scroll_region_from_scrollable_capability_with_order(
        widget,
        external,
        local,
        layout,
        region_id,
        address,
        enabled,
        HitRegionOrder {
            z_index: 0,
            paint_order: 0,
            traversal_order: 0,
        },
    )
}

/// [`scroll_region_from_scrollable_capability`] with an explicit
/// [`HitRegionOrder`] — the variant to use whenever declared scroll
/// regions can overlap (it is the fix API named by the
/// `view_contract.ambiguous_wheel_overlap` refusal). The scroll geometry
/// comes from [`SlipwayScrollBehaviorPolicy`] (fed a [`LayoutInput`]
/// derived from `layout.bounds`); the wheel-routing mode is snapshotted
/// per region from [`SlipwayWheelRoutingPolicy`] with the resolved region
/// id, so one widget can author distinct modes for several regions. When
/// to use which order values: `docs/public/api/routing-and-scroll.md`.
#[allow(clippy::too_many_arguments)]
#[must_use = "push the returned declaration into ViewDefinition::scroll_regions; building it without declaring it routes no wheel input"]
pub fn scroll_region_from_scrollable_capability_with_order<W>(
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
    W: SlipwayScrollableContainerCapability,
{
    let viewport = layout.bounds;
    let policy_input = LayoutInput {
        viewport,
        content: viewport,
        constraints: LayoutConstraints {
            min: Size {
                width: 0.0,
                height: 0.0,
            },
            max: viewport.size,
        },
    };
    let policy = widget.scroll_behavior_policy(external, local, &policy_input);
    let id = region_id
        .or(policy.region_id)
        .unwrap_or_else(|| PresentationRegionId::from(format!("{}:scroll", widget.id().as_str())));
    // The declaration-time snapshot call: the policy receives exactly the
    // resolved identity of the region being declared, so a widget that
    // declares several scroll regions can author per-region routing modes
    // (ADR-0002 B3).
    let wheel_routing = widget.wheel_routing_policy(external, local, &id).routing;
    ScrollRegionDeclaration {
        id,
        target: policy.target,
        address: address.or(policy.address),
        viewport: policy.viewport,
        content_bounds: policy.content_bounds,
        offset: policy.offset,
        axes: policy.axes,
        wheel_routing,
        indicator: ScrollIndicatorMode::Auto,
        order,
        virtual_viewport: None,
        consumption: policy.consumption,
        evidence: Vec::new(),
        enabled,
        diagnostics: policy.diagnostics,
    }
}

/// Capability bundle for popup/overlay widgets. No bound may be removed:
/// the bundle is contract surface declared ahead of full consumption.
///
/// Load-bearing bounds today: [`SlipwayWidget`],
/// [`SlipwayEventRoutingPolicy`], and [`SlipwayEventDispositionPolicy`]
/// (live dispatch via [`declared_event_handling`]; declaration-time route
/// snapshot via [`hit_region_from_pointer_capability`]).
///
/// RESERVED bounds (no current consumer): [`SlipwayOverlayContracts`],
/// [`SlipwayFocusTraversal`] (except `focus_member`, which
/// [`focus_region_from_focus_capability`] snapshots at declaration time),
/// [`SlipwaySemantics`], [`SlipwayHitTesting`],
/// [`SlipwayPointerCapturePolicy`], and [`SlipwayCommandContracts`].
///
/// A widget missing load-bearing bounds fails at the bundle with a
/// triaged error, even after [`reserved_policy_defaults!`] covers every
/// RESERVED bound:
///
/// ```compile_fail,E0277
/// use slipway_core::*;
///
/// struct Incomplete;
/// impl SlipwayWidgetTypes for Incomplete {
///     type ExternalState = ();
///     type LocalState = ();
///     type AppMessage = ();
/// }
/// # impl SlipwaySsot for Incomplete {
/// #     fn id(&self) -> WidgetId { WidgetId::from("incomplete") }
/// #     fn capabilities(&self) -> Vec<Capability> { Vec::new() }
/// #     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode { TopologyNode::leaf(self.id()) }
/// #     fn unsupported(&self) -> Vec<Diagnostic> { Vec::new() }
/// # }
/// reserved_policy_defaults!(Incomplete);
///
/// // LOAD-BEARING bounds (the rest of the widget trio and the
/// // routing/disposition policies) are still missing, so the bundle
/// // refuses:
/// fn requires_bundle<T: SlipwayPopupCapability>() {}
/// requires_bundle::<Incomplete>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not satisfy the `SlipwayPopupCapability` bundle (popup/overlay widgets)",
    label = "missing one or more supertraits of the popup capability bundle",
    note = "each missing supertrait is named in its own `help:` line of this error; implement the LOAD-BEARING ones by hand: SlipwaySsot + SlipwayLogic + SlipwayView (= SlipwayWidget), SlipwayEventRoutingPolicy, and SlipwayEventDispositionPolicy",
    note = "the remaining bounds are RESERVED (no runtime consumer; real logic in them is a silent no-op, except SlipwayFocusTraversal::focus_member which `focus_region_from_focus_capability` snapshots at declaration time): SlipwayOverlayContracts, SlipwayFocusTraversal, SlipwaySemantics, SlipwayHitTesting, SlipwayPointerCapturePolicy, SlipwayCommandContracts — satisfy every RESERVED bound at once with `slipway_core::reserved_policy_defaults!(YourWidget);`"
)]
pub trait SlipwayPopupCapability:
    SlipwayWidget
    + SlipwayOverlayContracts
    + SlipwayFocusTraversal
    + SlipwaySemantics
    + SlipwayHitTesting
    + SlipwayEventRoutingPolicy
    + SlipwayEventDispositionPolicy
    + SlipwayPointerCapturePolicy
    + SlipwayCommandContracts
{
}

impl<W> SlipwayPopupCapability for W where
    W: SlipwayWidget
        + SlipwayOverlayContracts
        + SlipwayFocusTraversal
        + SlipwaySemantics
        + SlipwayHitTesting
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy
        + SlipwayPointerCapturePolicy
        + SlipwayCommandContracts
{
}

/// Capability bundle for provider-surface widgets. No bound may be
/// removed: the bundle is contract surface declared ahead of full
/// consumption.
///
/// Load-bearing bounds today: the provider enumeration and
/// hit-test/snapshot bounds ([`SlipwayCanvasProvider`],
/// [`SlipwayGpuSurfaceProvider`], [`SlipwayMediaProvider`],
/// [`SlipwayPlotProvider`], [`SlipwayProviderHitTestPolicy`],
/// [`SlipwayProviderSnapshotPolicy`]) are consulted through the backends'
/// native provider adapters and the provider capability suites. RESERVED
/// bound (no current consumer outside capability tests):
/// [`SlipwayRenderSurfaces`] (surface declarations reach backends through
/// the view definition, not this trait, today).
///
/// A widget missing load-bearing bounds fails at the bundle with a
/// triaged error, even after [`reserved_policy_defaults!`] covers the
/// RESERVED [`SlipwayRenderSurfaces`] bound:
///
/// ```compile_fail,E0277
/// use slipway_core::*;
///
/// struct Incomplete;
/// impl SlipwayWidgetTypes for Incomplete {
///     type ExternalState = ();
///     type LocalState = ();
///     type AppMessage = ();
/// }
/// # impl SlipwaySsot for Incomplete {
/// #     fn id(&self) -> WidgetId { WidgetId::from("incomplete") }
/// #     fn capabilities(&self) -> Vec<Capability> { Vec::new() }
/// #     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode { TopologyNode::leaf(self.id()) }
/// #     fn unsupported(&self) -> Vec<Diagnostic> { Vec::new() }
/// # }
/// reserved_policy_defaults!(Incomplete);
///
/// // LOAD-BEARING bounds (the provider enumeration and hit-test/snapshot
/// // traits) are still missing, so the bundle refuses:
/// fn requires_bundle<T: SlipwayProviderSurfaceCapability>() {}
/// requires_bundle::<Incomplete>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not satisfy the `SlipwayProviderSurfaceCapability` bundle (provider-surface widgets)",
    label = "missing one or more supertraits of the provider-surface capability bundle",
    note = "each missing supertrait is named in its own `help:` line of this error; implement the LOAD-BEARING ones by hand: SlipwayCanvasProvider, SlipwayGpuSurfaceProvider, SlipwayMediaProvider, SlipwayPlotProvider, SlipwayProviderHitTestPolicy, and SlipwayProviderSnapshotPolicy (consulted through the backends' native provider adapters)",
    note = "the remaining bound is RESERVED (no runtime consumer; real logic in it is a silent no-op): SlipwayRenderSurfaces — it is covered by `slipway_core::reserved_policy_defaults!(YourWidget);` (requires the widget trio)"
)]
pub trait SlipwayProviderSurfaceCapability:
    SlipwayRenderSurfaces
    + SlipwayCanvasProvider
    + SlipwayGpuSurfaceProvider
    + SlipwayMediaProvider
    + SlipwayPlotProvider
    + SlipwayProviderHitTestPolicy
    + SlipwayProviderSnapshotPolicy
{
}

impl<W> SlipwayProviderSurfaceCapability for W where
    W: SlipwayRenderSurfaces
        + SlipwayCanvasProvider
        + SlipwayGpuSurfaceProvider
        + SlipwayMediaProvider
        + SlipwayPlotProvider
        + SlipwayProviderHitTestPolicy
        + SlipwayProviderSnapshotPolicy
{
}

/// Capability bundle for command-surface widgets. No bound may be removed:
/// the bundle is contract surface declared ahead of full consumption.
///
/// Load-bearing bounds today: [`SlipwayWidget`] and
/// [`SlipwayEventRoutingPolicy`] (live dispatch via
/// [`declared_event_handling`]).
///
/// RESERVED bounds (no current consumer): [`SlipwayCommandContracts`],
/// [`SlipwayCommandInvocationPolicy`], [`SlipwayCommandStatusPolicy`],
/// [`SlipwayShortcutRoutingPolicy`], and [`SlipwayUndoRedoPolicy`].
///
/// A widget missing load-bearing bounds fails at the bundle with a
/// triaged error, even after [`reserved_policy_defaults!`] covers every
/// RESERVED bound:
///
/// ```compile_fail,E0277
/// use slipway_core::*;
///
/// struct Incomplete;
/// impl SlipwayWidgetTypes for Incomplete {
///     type ExternalState = ();
///     type LocalState = ();
///     type AppMessage = ();
/// }
/// # impl SlipwaySsot for Incomplete {
/// #     fn id(&self) -> WidgetId { WidgetId::from("incomplete") }
/// #     fn capabilities(&self) -> Vec<Capability> { Vec::new() }
/// #     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode { TopologyNode::leaf(self.id()) }
/// #     fn unsupported(&self) -> Vec<Diagnostic> { Vec::new() }
/// # }
/// reserved_policy_defaults!(Incomplete);
///
/// // LOAD-BEARING bounds (the rest of the widget trio and the routing
/// // policy) are still missing, so the bundle refuses:
/// fn requires_bundle<T: SlipwayCommandSurfaceCapability>() {}
/// requires_bundle::<Incomplete>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not satisfy the `SlipwayCommandSurfaceCapability` bundle (command-surface widgets)",
    label = "missing one or more supertraits of the command-surface capability bundle",
    note = "each missing supertrait is named in its own `help:` line of this error; implement the LOAD-BEARING ones by hand: SlipwaySsot + SlipwayLogic + SlipwayView (= SlipwayWidget) and SlipwayEventRoutingPolicy",
    note = "the remaining bounds are RESERVED (no runtime consumer; real logic in them is a silent no-op): SlipwayCommandContracts, SlipwayCommandInvocationPolicy, SlipwayCommandStatusPolicy, SlipwayShortcutRoutingPolicy, SlipwayUndoRedoPolicy — satisfy every RESERVED bound at once with `slipway_core::reserved_policy_defaults!(YourWidget);`"
)]
pub trait SlipwayCommandSurfaceCapability:
    SlipwayWidget
    + SlipwayCommandContracts
    + SlipwayCommandInvocationPolicy
    + SlipwayCommandStatusPolicy
    + SlipwayShortcutRoutingPolicy
    + SlipwayUndoRedoPolicy
    + SlipwayEventRoutingPolicy
{
}

impl<W> SlipwayCommandSurfaceCapability for W where
    W: SlipwayWidget
        + SlipwayCommandContracts
        + SlipwayCommandInvocationPolicy
        + SlipwayCommandStatusPolicy
        + SlipwayShortcutRoutingPolicy
        + SlipwayUndoRedoPolicy
        + SlipwayEventRoutingPolicy
{
}

/// Capability bundle for deterministic-source widgets. No bound may be
/// removed: the bundle is contract surface declared ahead of full
/// consumption.
///
/// Load-bearing bound today: [`SlipwayWidget`]. RESERVED bounds (no
/// current consumer beyond the core API-surface tests):
/// [`SlipwayTimeSourcePolicy`], [`SlipwayRandomSourcePolicy`],
/// [`SlipwayExternalDataSnapshotPolicy`], and
/// [`SlipwayAnimationTimelinePolicy`].
///
/// A widget missing load-bearing bounds fails at the bundle with a
/// triaged error, even after [`reserved_policy_defaults!`] covers every
/// RESERVED bound:
///
/// ```compile_fail,E0277
/// use slipway_core::*;
///
/// struct Incomplete;
/// impl SlipwayWidgetTypes for Incomplete {
///     type ExternalState = ();
///     type LocalState = ();
///     type AppMessage = ();
/// }
/// # impl SlipwaySsot for Incomplete {
/// #     fn id(&self) -> WidgetId { WidgetId::from("incomplete") }
/// #     fn capabilities(&self) -> Vec<Capability> { Vec::new() }
/// #     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode { TopologyNode::leaf(self.id()) }
/// #     fn unsupported(&self) -> Vec<Diagnostic> { Vec::new() }
/// # }
/// reserved_policy_defaults!(Incomplete);
///
/// // The LOAD-BEARING widget trio is still incomplete (SlipwayLogic and
/// // SlipwayView are missing), so the bundle refuses:
/// fn requires_bundle<T: SlipwayDeterministicSourceCapability>() {}
/// requires_bundle::<Incomplete>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not satisfy the `SlipwayDeterministicSourceCapability` bundle (deterministic-source widgets)",
    label = "missing one or more supertraits of the deterministic-source capability bundle",
    note = "each missing supertrait is named in its own `help:` line of this error; the only LOAD-BEARING bound is SlipwaySsot + SlipwayLogic + SlipwayView (= SlipwayWidget) — implement the trio by hand",
    note = "the remaining bounds are RESERVED (no runtime consumer; real logic in them is a silent no-op): SlipwayTimeSourcePolicy, SlipwayRandomSourcePolicy, SlipwayExternalDataSnapshotPolicy, SlipwayAnimationTimelinePolicy — satisfy every RESERVED bound at once with `slipway_core::reserved_policy_defaults!(YourWidget);`"
)]
pub trait SlipwayDeterministicSourceCapability:
    SlipwayWidget
    + SlipwayTimeSourcePolicy
    + SlipwayRandomSourcePolicy
    + SlipwayExternalDataSnapshotPolicy
    + SlipwayAnimationTimelinePolicy
{
}

impl<W> SlipwayDeterministicSourceCapability for W where
    W: SlipwayWidget
        + SlipwayTimeSourcePolicy
        + SlipwayRandomSourcePolicy
        + SlipwayExternalDataSnapshotPolicy
        + SlipwayAnimationTimelinePolicy
{
}

/// Capability bundle for backend admission. All bounds are load-bearing
/// today: both backends consult `backend_capabilities`,
/// `unsupported_capabilities`, and `backend_parity_admission` through
/// their admission gates.
///
/// A type missing load-bearing bounds fails at the bundle with a triaged
/// error:
///
/// ```compile_fail,E0277
/// use slipway_core::*;
///
/// struct IncompleteGate;
///
/// // ALL bounds of this bundle are load-bearing; the probe, evidence,
/// // and parity traits are missing, so the bundle refuses:
/// fn requires_bundle<T: SlipwayBackendAdmissionCapability>() {}
/// requires_bundle::<IncompleteGate>();
/// ```
#[diagnostic::on_unimplemented(
    message = "`{Self}` does not satisfy the `SlipwayBackendAdmissionCapability` bundle (backend admission gates)",
    label = "missing one or more supertraits of the backend-admission capability bundle",
    note = "each missing supertrait is named in its own `help:` line of this error; ALL bounds of this bundle are LOAD-BEARING (none are RESERVED): implement SlipwayBackendCapabilityProbe, SlipwayUnsupportedCapabilityEvidence, and SlipwayBackendParityAdmission by hand — both backends consult them through their admission gates"
)]
pub trait SlipwayBackendAdmissionCapability:
    SlipwayBackendCapabilityProbe + SlipwayUnsupportedCapabilityEvidence + SlipwayBackendParityAdmission
{
}

impl<W> SlipwayBackendAdmissionCapability for W where
    W: SlipwayBackendCapabilityProbe
        + SlipwayUnsupportedCapabilityEvidence
        + SlipwayBackendParityAdmission
{
}

/// Implements every RESERVED policy trait for one or more widget types
/// with the documented empty/default values, so the RESERVED bounds of the
/// capability bundles are satisfied without hand-written boilerplate.
///
/// WHEN to use: after implementing a bundle's LOAD-BEARING bounds by hand
/// (each bundle trait's doc and its `on_unimplemented` notes carry the
/// triage), invoke this macro once per widget type to cover the rest.
/// Do NOT hand-write real logic in these traits instead: no backend,
/// runtime, or core dispatch path consults them today, so real logic in a
/// RESERVED trait method is a verified total silent no-op (audit
/// 2026-07-11, finding LE-M20). One exception:
/// [`focus_region_from_focus_capability`] snapshots
/// [`SlipwayFocusTraversal::focus_member`] at declaration time — implement
/// that trait by hand instead of using this macro if the widget's plain
/// focus regions need an explicit traversal member (the macro's default
/// returns `None`, declaring no tab order).
///
/// Traits implemented (the Step-197 RESERVED set — trait-level
/// "RESERVED contract surface" markers plus every bound the capability
/// bundles triage as RESERVED): [`SlipwayContainerLayoutPolicy`],
/// [`SlipwayChildConstraintPolicy`], [`SlipwayLayoutInvalidationPolicy`],
/// [`SlipwayLayoutEvidencePolicy`], [`SlipwayViewportObservationPolicy`],
/// [`SlipwayVirtualCollectionPolicy`], [`SlipwayHitTesting`],
/// [`SlipwayViewportContracts`], [`SlipwayOverlayContracts`],
/// [`SlipwaySemantics`], [`SlipwayFocusTraversal`],
/// [`SlipwayDebugEventTracePolicy`], [`SlipwayPointerCapturePolicy`],
/// [`SlipwayCommandContracts`], [`SlipwayCommandInvocationPolicy`],
/// [`SlipwayCommandStatusPolicy`], [`SlipwayShortcutRoutingPolicy`],
/// [`SlipwayUndoRedoPolicy`], [`SlipwayTimeSourcePolicy`],
/// [`SlipwayRandomSourcePolicy`], [`SlipwayExternalDataSnapshotPolicy`],
/// [`SlipwayAnimationTimelinePolicy`], and [`SlipwayRenderSurfaces`].
///
/// The widget type must implement [`SlipwayWidgetTypes`] and
/// [`SlipwaySsot`] (declaration `target` fields are filled from
/// `SlipwaySsot::id`). Everything else is the neutral empty value:
/// empty `Vec`s, `None`, `false`, zero counts, empty strings, a no-hit
/// [`HitTestOutput`], and `ContainerLayoutKind::Custom("reserved-default")`.
///
/// ```
/// use slipway_core::*;
///
/// struct MyList;
///
/// impl SlipwayWidgetTypes for MyList {
///     type ExternalState = ();
///     type LocalState = ();
///     type AppMessage = ();
/// }
///
/// impl SlipwaySsot for MyList {
///     fn id(&self) -> WidgetId {
///         WidgetId::from("my.list")
///     }
///     fn capabilities(&self) -> Vec<Capability> {
///         Vec::new()
///     }
///     fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
///         TopologyNode::leaf(self.id())
///     }
///     fn unsupported(&self) -> Vec<Diagnostic> {
///         Vec::new()
///     }
/// }
///
/// reserved_policy_defaults!(MyList);
///
/// // The RESERVED bounds of the bundles are now satisfied; only the
/// // LOAD-BEARING bounds (the widget trio and the live policies) remain
/// // for the author to implement by hand.
/// fn assert_reserved_covered<T: SlipwayHitTesting + SlipwaySemantics + SlipwayFocusTraversal>() {}
/// assert_reserved_covered::<MyList>();
/// ```
#[macro_export]
macro_rules! reserved_policy_defaults {
    ($($widget:ty),+ $(,)?) => {
        $( $crate::reserved_policy_defaults!(@single $widget); )+
    };
    (@single $widget:ty) => {
        impl $crate::SlipwayContainerLayoutPolicy for $widget {
            fn container_layout_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                _input: &$crate::LayoutInput,
            ) -> $crate::ContainerLayoutPolicyDeclaration {
                $crate::ContainerLayoutPolicyDeclaration {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    kind: $crate::ContainerLayoutKind::Custom(::std::string::String::from(
                        "reserved-default",
                    )),
                    child_order: ::std::vec::Vec::new(),
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayChildConstraintPolicy for $widget {
            fn child_constraints(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                _input: &$crate::LayoutInput,
            ) -> ::std::vec::Vec<$crate::ChildConstraintPolicyDeclaration> {
                ::std::vec::Vec::new()
            }
        }

        impl $crate::SlipwayLayoutInvalidationPolicy for $widget {
            fn layout_invalidation_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::LayoutInvalidationPolicyDeclaration {
                $crate::LayoutInvalidationPolicyDeclaration {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    dependencies: ::std::vec::Vec::new(),
                    revisions: ::std::vec::Vec::new(),
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayLayoutEvidencePolicy for $widget {
            fn layout_evidence(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                output: &$crate::LayoutOutput,
            ) -> $crate::LayoutEvidence {
                $crate::LayoutEvidence {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    bounds: *output.bounds(),
                    child_placements: output.child_placements().to_vec(),
                    invalidated: false,
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayViewportObservationPolicy for $widget {
            fn viewport_observation(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::ViewportObservationEvidence {
                let empty = $crate::TargetLocalRect::new($crate::Rect {
                    origin: $crate::Point { x: 0.0, y: 0.0 },
                    size: $crate::Size {
                        width: 0.0,
                        height: 0.0,
                    },
                });
                $crate::ViewportObservationEvidence {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    viewport: empty,
                    visible_rect: empty,
                    scroll: ::std::option::Option::None,
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayVirtualCollectionPolicy for $widget {
            fn virtual_collection_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::VirtualCollectionPolicyDeclaration {
                $crate::VirtualCollectionPolicyDeclaration {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    item_count: 0,
                    visible_range: ::std::option::Option::None,
                    realization_hint: $crate::VirtualizationHint::None,
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayHitTesting for $widget {
            fn hit_test(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                _input: $crate::HitTestInput,
            ) -> $crate::HitTestOutput {
                $crate::HitTestOutput {
                    target: ::std::option::Option::None,
                    local_point: ::std::option::Option::None,
                    route: $crate::EventRoute {
                        route_id: ::std::option::Option::None,
                        address: ::std::option::Option::None,
                        path: ::std::vec::Vec::new(),
                        phase: $crate::EventRoutePhase::Target,
                    },
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayViewportContracts for $widget {
            fn scroll_state(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::vec::Vec<$crate::ScrollState> {
                ::std::vec::Vec::new()
            }

            fn virtual_viewports(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::vec::Vec<$crate::VirtualViewportRange> {
                ::std::vec::Vec::new()
            }
        }

        impl $crate::SlipwayOverlayContracts for $widget {
            fn overlays(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::vec::Vec<$crate::OverlayDeclaration> {
                ::std::vec::Vec::new()
            }

            fn anchored_surfaces(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::vec::Vec<$crate::AnchoredSurfaceDeclaration> {
                ::std::vec::Vec::new()
            }

            fn portals(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::vec::Vec<$crate::PortalDeclaration> {
                ::std::vec::Vec::new()
            }
        }

        impl $crate::SlipwaySemantics for $widget {
            fn semantics(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::vec::Vec<$crate::SemanticNode> {
                ::std::vec::Vec::new()
            }
        }

        impl $crate::SlipwayFocusTraversal for $widget {
            fn focus_member(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::option::Option<$crate::FocusTraversalMember> {
                ::std::option::Option::None
            }

            fn next_focus(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                _input: $crate::FocusTraversalInput,
            ) -> ::std::option::Option<$crate::WidgetId> {
                ::std::option::Option::None
            }

            fn previous_focus(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                _input: $crate::FocusTraversalInput,
            ) -> ::std::option::Option<$crate::WidgetId> {
                ::std::option::Option::None
            }
        }

        impl $crate::SlipwayDebugEventTracePolicy for $widget {
            fn debug_event_trace_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::DebugEventTracePolicyDeclaration {
                $crate::DebugEventTracePolicyDeclaration {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    request_only: true,
                    include_route: false,
                    include_messages: false,
                    include_state_changes: false,
                    include_repaint_request: false,
                }
            }
        }

        impl $crate::SlipwayPointerCapturePolicy for $widget {
            fn pointer_capture_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                _pointer: $crate::PointerDetails,
            ) -> $crate::PointerCapturePolicyDeclaration {
                $crate::PointerCapturePolicyDeclaration {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    requests: ::std::vec::Vec::new(),
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayCommandContracts for $widget {
            fn commands(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::vec::Vec<$crate::CommandDeclaration> {
                ::std::vec::Vec::new()
            }
        }

        impl $crate::SlipwayCommandInvocationPolicy for $widget {
            fn command_invocation_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                command: &$crate::CommandEvent,
            ) -> $crate::CommandInvocationPolicyDeclaration {
                $crate::CommandInvocationPolicyDeclaration {
                    command_id: command.command.clone(),
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    enabled: false,
                    expects_state_change: false,
                }
            }
        }

        impl $crate::SlipwayCommandStatusPolicy for $widget {
            fn command_status(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                command_id: &str,
            ) -> $crate::CommandStatusEvidence {
                $crate::CommandStatusEvidence {
                    command_id: ::std::string::String::from(command_id),
                    enabled: false,
                    checked: ::std::option::Option::None,
                    label: ::std::option::Option::None,
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayShortcutRoutingPolicy for $widget {
            fn shortcut_routing_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                shortcut: &$crate::ShortcutDeclaration,
            ) -> $crate::ShortcutRoutingPolicyDeclaration {
                $crate::ShortcutRoutingPolicyDeclaration {
                    shortcut: shortcut.clone(),
                    route: $crate::EventRoute {
                        route_id: ::std::option::Option::None,
                        address: ::std::option::Option::None,
                        path: ::std::vec::Vec::new(),
                        phase: $crate::EventRoutePhase::Target,
                    },
                    command_id: shortcut.command_id.clone(),
                }
            }
        }

        impl $crate::SlipwayUndoRedoPolicy for $widget {
            fn undo_redo_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::UndoRedoPolicyDeclaration {
                $crate::UndoRedoPolicyDeclaration {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    can_undo: false,
                    can_redo: false,
                    undo_command: ::std::option::Option::None,
                    redo_command: ::std::option::Option::None,
                    diagnostics: ::std::vec::Vec::new(),
                }
            }
        }

        impl $crate::SlipwayTimeSourcePolicy for $widget {
            fn time_source(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::TimeSourceSnapshot {
                $crate::TimeSourceSnapshot {
                    source_id: ::std::string::String::from("reserved-default"),
                    millis: 0,
                    revision: $crate::CacheRevisionToken {
                        name: ::std::string::String::from("reserved-default"),
                        value: ::std::string::String::new(),
                    },
                }
            }
        }

        impl $crate::SlipwayRandomSourcePolicy for $widget {
            fn random_source(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::RandomSourceSnapshot {
                $crate::RandomSourceSnapshot {
                    source_id: ::std::string::String::from("reserved-default"),
                    seed: ::std::string::String::new(),
                    draw_index: 0,
                    revision: $crate::CacheRevisionToken {
                        name: ::std::string::String::from("reserved-default"),
                        value: ::std::string::String::new(),
                    },
                }
            }
        }

        impl $crate::SlipwayExternalDataSnapshotPolicy for $widget {
            fn external_data_snapshot(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::ExternalDataSnapshot {
                $crate::ExternalDataSnapshot {
                    source_id: ::std::string::String::from("reserved-default"),
                    snapshot_ref: ::std::string::String::new(),
                    revision: $crate::CacheRevisionToken {
                        name: ::std::string::String::from("reserved-default"),
                        value: ::std::string::String::new(),
                    },
                }
            }
        }

        impl $crate::SlipwayAnimationTimelinePolicy for $widget {
            fn animation_timeline_policy(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> $crate::AnimationTimelinePolicyDeclaration {
                $crate::AnimationTimelinePolicyDeclaration {
                    target: <Self as $crate::SlipwaySsot>::id(self),
                    timeline_id: ::std::string::String::from("reserved-default"),
                    time_millis: 0.0,
                    paused: true,
                    revision: $crate::CacheRevisionToken {
                        name: ::std::string::String::from("reserved-default"),
                        value: ::std::string::String::new(),
                    },
                }
            }
        }

        impl $crate::SlipwayRenderSurfaces for $widget {
            fn render_surfaces(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
            ) -> ::std::vec::Vec<$crate::RenderSurfaceDeclaration> {
                ::std::vec::Vec::new()
            }
        }
    };
}

pub trait SlipwayAuthoredWidget: SlipwayWidget {}

impl<W> SlipwayAuthoredWidget for W where W: SlipwayWidget {}

#[derive(Clone, Debug, PartialEq)]
pub struct WidgetSlot<W: SlipwayAuthoredWidget> {
    pub widget: W,
    pub local_state: W::LocalState,
}

impl<W: SlipwayAuthoredWidget> WidgetSlot<W> {
    pub fn new(widget: W) -> Self {
        let local_state = widget.initial_local_state();
        Self {
            widget,
            local_state,
        }
    }

    pub fn address(&self, ordinal: usize) -> WidgetSlotAddress {
        WidgetSlotAddress::new(self.widget.id(), ordinal)
    }
}

pub trait SlipwayWidgetListVisitor<ExternalState, AppMessage> {
    fn visit_child<W>(
        &mut self,
        widget: &W,
        external: &ExternalState,
        local: &W::LocalState,
        slot: WidgetSlotAddress,
    ) where
        W: SlipwayWidget<ExternalState = ExternalState, AppMessage = AppMessage>
            + SlipwayViewDefinition;
}

pub trait SlipwayWidgetList {
    type ExternalState;
    type LocalState;
    type AppMessage;

    fn initial_child_local_state(&self) -> Self::LocalState;
    fn widget_count(&self) -> usize;

    fn visit_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        visitor: &mut V,
    ) where
        V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>;

    fn child_topology(
        &self,
        external: &Self::ExternalState,
        parent_slot: &WidgetSlotAddress,
    ) -> Vec<TopologyNode>;

    fn child_layout_seeds(&self, parent_slot: &WidgetSlotAddress) -> Vec<ChildLayoutSeed>;

    fn layout_children(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        plans: &[ChildLayoutPlan],
    ) -> Vec<ChildLayoutResult>;

    fn route_event(
        &self,
        external: &Self::ExternalState,
        local: &mut Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage>;

    fn child_paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp>;

    fn child_paint_units(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        frame: &FrameIdentity,
        layout: &LayoutOutput,
    ) -> Vec<PaintUnit>;

    fn observe_child_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
    ) -> Vec<StateObservation>;
}

pub trait SlipwayWidgetListViewDefinition: SlipwayWidgetList {
    fn child_view_definitions(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        parent_slot: &WidgetSlotAddress,
        frame: &FrameIdentity,
        layout: &LayoutOutput,
    ) -> Vec<ViewDefinition>;
}

/// Combines N authored widgets into one desktop app — the public path for
/// multi-widget apps. Implement it on your app type, declare the children
/// as `Widgets` (tuples of [`WidgetSlot`] are supported up to 16
/// children), and run it through a backend runner, which adapts it via
/// [`SlipwayAppWidget`]. LOAD-BEARING: the runtime drives every method.
/// Model: `docs/public/api/core.md` ("App Composition"); pattern:
/// `docs/public/quickstart-authoring.md`.
///
/// N widgets must remain N addressable children: do not fake children by
/// painting them inside one root view. Child-to-app effects travel as
/// typed messages into `handle_event` (the app reducer); app-to-child
/// state is projected through `ExternalState`. The provided defaults are
/// load-bearing and honest for a plain app: `capabilities()` declares the
/// baseline (layout/paint/observation/traversal — extend it when the app
/// itself consumes input), `layout_plan` gives every child the full
/// viewport (override it to place children; child layout inputs stay
/// target-local origin `0,0` or admission refuses with
/// `view_contract.child_input_viewport_not_target_local`),
/// `handle_event` ignores app-level events, and `resolve_app_font`
/// refuses honestly (override it only to declare a real font source). A
/// scroll container is an authored app/container whose `layout_plan`
/// positions children from its scroll offset — not a built-in widget.
pub trait SlipwayApp {
    type ExternalState;
    type LocalState;
    type AppMessage;
    type Widgets: SlipwayWidgetList<ExternalState = Self::ExternalState, AppMessage = Self::AppMessage>
        + SlipwayWidgetListViewDefinition<
            ExternalState = Self::ExternalState,
            AppMessage = Self::AppMessage,
        >;

    fn id(&self) -> WidgetId;
    fn widgets(&self) -> &Self::Widgets;

    fn capabilities(&self) -> Vec<Capability> {
        vec![
            Capability::ChildTraversal,
            Capability::Layout,
            Capability::Paint,
            Capability::StateObservation,
        ]
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

    /// App-level platform-truth projection (see
    /// [`SlipwayLogic::project_frame_viewport`] for the full contract —
    /// [`SlipwayAppWidget<A>`] forwards to this hook). WHEN: override it
    /// when app or widget code must reproduce window-derived geometry in
    /// `paint`/`handle_event`, where `ViewDefinitionInput` never reaches;
    /// write the viewport into a field of your external state and read it
    /// wherever the geometry is consumed. Pattern:
    /// `crates/slipway-example-authored` (`ShowcaseState::viewport` — the
    /// roaming overlay's allowance and drag clamp share it). Failure mode
    /// of skipping it: window-true geometry computed in `view_definition`
    /// only silently diverges from paint and handlers (no diagnostic).
    /// LOAD-BEARING when overridden; the no-op default is byte-identical.
    /// This runtime-invoked hook, [`SlipwayApp::project_text_metrics`],
    /// and the app reducer are the ONLY sanctioned writers of external
    /// state. Docs: `docs/public/api/routing-and-scroll.md` ("Overlay
    /// Drag Patterns").
    fn project_frame_viewport(&self, _external: &mut Self::ExternalState, _viewport: Rect) {}

    /// App-level text-measurement projection (see
    /// [`SlipwayLogic::project_text_metrics`] for the full contract —
    /// [`SlipwayAppWidget<A>`] forwards to this hook). WHEN: override it
    /// when an element must size itself to laid-out text (a badge hugging
    /// its label, a center-ellipsized header): build a
    /// [`TextMeasurementRequest`] with the SAME [`TextStyle`] the paint
    /// op declares, call `metrics.measure_text(..)`, and write the valid
    /// receipt's [`TextMeasurementFacts`] into external state for
    /// `paint`/`view_definition`/`handle_event` to read. The provider is
    /// the presenting backend's REAL text layout (never an estimate), so
    /// the measured size equals what the backend presents. Pattern:
    /// `crates/slipway-example-authored/src/communication.rs`
    /// (`ShowcaseState::window_badge`). Failure mode of skipping it:
    /// hand-computed character-width ratios drift wherever the guess is
    /// wrong (audit NC-4/NC-14 anti-pattern); never fabricate a size from
    /// an `Invalid`/`Unsupported` receipt. LOAD-BEARING when overridden;
    /// the no-op default is byte-identical. Docs:
    /// `docs/public/api/backends.md` ("Text Wrap and Alignment").
    fn project_text_metrics(
        &self,
        _external: &mut Self::ExternalState,
        _metrics: &mut dyn SlipwayTextMetricProvider,
    ) {
    }

    /// App-level scroll regions appended to the COMPOSED view — the
    /// wrapper-less path to a page/root scroll region. WHEN: the whole
    /// card column must scroll when it exceeds the window (the admission
    /// stress harness proves the pattern with a wrapper; this hook is the
    /// documented `SlipwayApp` equivalent). Declare the region with
    /// `ScrollRegionDeclaration::explicit`: viewport = the frame
    /// viewport, content = the full column (`layout.bounds`), offset from
    /// app-local state (this app's `handle_event` receives the region's
    /// Wheel/Scroll events because the region targets the app id), a
    /// BACK-most order (negative `z_index`) so every widget region fronts
    /// it, and gate the declaration on content actually exceeding the
    /// viewport. Chaining is automatic: at-limit widget regions hand the
    /// wheel to this region as the outermost containing candidate
    /// (`docs/public/api/routing-and-scroll.md`, "Chaining"). Pattern:
    /// `crates/slipway-example-authored/src/communication.rs`
    /// (`page_scroll_region`). Failure modes: an offset not clamped to
    /// `content - viewport` refuses with
    /// `view_contract.scroll_offset_invalid`; an always-declared region
    /// with zero travel refuses with
    /// `view_contract.scroll_geometry_invalid`. LOAD-BEARING when
    /// overridden; the empty default is byte-identical.
    fn app_scroll_regions(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _frame: &FrameIdentity,
        _input: &LayoutInput,
        _layout: &LayoutOutput,
    ) -> Vec<ScrollRegionDeclaration> {
        Vec::new()
    }

    /// App-level font-resolution hook. WHEN: override it only when the
    /// app declares a loadable font source (custom/CJK fonts —
    /// `docs/public/api/ime.md`); ordinary apps keep the default. The
    /// default is an HONEST refusal: it resolves nothing, claims no
    /// installation, and carries the `app-font-resolution-refused`
    /// diagnostic (`docs/public/api/diagnostics.md`) — backends fall back
    /// to their own fonts and record the refusal as evidence. Never
    /// fabricate a "resolved" claim for a source that was not validated.
    ///
    /// LOAD-BEARING on egui: the core [`SlipwayFontResolutionPolicy`]
    /// impl for [`SlipwayAppWidget<A>`] delegates here, which is what
    /// satisfies the egui root gate for a plain facade app (the orphan
    /// rule forbids the app crate implementing that policy on
    /// `SlipwayAppWidget<A>` itself — Step 208 finding). Pattern for a
    /// real source: `crates/slipway-example-authored/src/app_runner.rs`
    /// font glue.
    fn resolve_app_font(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        request: FontResolutionRequest,
    ) -> FontResolutionEvidence {
        let mut fallback_chain = Vec::with_capacity(1 + request.fallback_families.len());
        fallback_chain.push(request.family.clone());
        fallback_chain.extend(request.fallback_families.clone());
        let diagnostic = Diagnostic::unsupported(
            Some(self.id()),
            "app-font-resolution-refused",
            "SlipwayApp::resolve_app_font default: this app declares no loadable font source \
             and performs no font resolution; override resolve_app_font to declare one \
             (docs/public/api/ime.md)",
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
                    label: "app_font_resolution_default".to_string(),
                    backend_id: None,
                    provider_id: None,
                    pass_id: None,
                },
                diagnostics: vec![diagnostic.clone()],
            }),
            valid_source: None,
            diagnostics: vec![diagnostic],
        }
    }

    fn layout_plan(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        children: Vec<ChildLayoutSeed>,
    ) -> AppLayoutPlan {
        AppLayoutPlan {
            bounds: input.viewport,
            children: children
                .into_iter()
                .map(|seed| {
                    ChildLayoutPlan::requested_outer(
                        seed,
                        ContentLocalRect::new(input.viewport.into_rect()),
                        input.constraints,
                        BoxSpacing::ZERO,
                    )
                })
                .collect(),
            diagnostics: Vec::new(),
        }
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        output: LayoutOutputBuilder,
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
        _local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        Vec::new()
    }
}

pub struct SlipwayAppLocalState<A: SlipwayApp> {
    pub app: A::LocalState,
    pub widgets: <A::Widgets as SlipwayWidgetList>::LocalState,
}

impl<A> Clone for SlipwayAppLocalState<A>
where
    A: SlipwayApp,
    A::LocalState: Clone,
    <A::Widgets as SlipwayWidgetList>::LocalState: Clone,
{
    fn clone(&self) -> Self {
        Self {
            app: self.app.clone(),
            widgets: self.widgets.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlipwayAppWidget<A: SlipwayApp> {
    pub app: A,
}

impl<A: SlipwayApp> SlipwayAppWidget<A> {
    pub fn new(app: A) -> Self {
        Self { app }
    }

    pub fn into_inner(self) -> A {
        self.app
    }
}

impl<A: SlipwayApp> SlipwayWidgetTypes for SlipwayAppWidget<A> {
    type ExternalState = A::ExternalState;
    type LocalState = SlipwayAppLocalState<A>;
    type AppMessage = A::AppMessage;
}

impl<A: SlipwayApp> SlipwaySsot for SlipwayAppWidget<A> {
    fn id(&self) -> WidgetId {
        self.app.id()
    }

    fn capabilities(&self) -> Vec<Capability> {
        self.app.capabilities()
    }

    fn topology(&self, external: &Self::ExternalState) -> TopologyNode {
        let root = self.app.id();
        let root_slot = WidgetSlotAddress::new(root.clone(), 0);
        TopologyNode {
            id: root,
            children: self.app.widgets().child_topology(external, &root_slot),
            local_state_slot: Some(root_slot),
        }
    }

    fn unsupported(&self) -> Vec<Diagnostic> {
        self.app.unsupported()
    }

    fn visit_authored_children<V>(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        visitor: &mut V,
    ) where
        V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        self.app
            .widgets()
            .visit_children(external, &local.widgets, &root_slot, visitor);
    }
}

impl<A: SlipwayApp> SlipwayLogic for SlipwayAppWidget<A> {
    fn handle_event(
        &self,
        external: &Self::ExternalState,
        local: &mut Self::LocalState,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        if event.target() == &self.app.id() {
            return self.app.handle_event(external, &mut local.app, event);
        }

        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        let outcome =
            self.app
                .widgets()
                .route_event(external, &mut local.widgets, &root_slot, event.clone());
        if outcome.handled && !outcome.propagate {
            outcome
        } else {
            merge_event_outcomes(
                outcome,
                self.app.handle_event(external, &mut local.app, event),
            )
        }
    }

    fn project_frame_viewport(&self, external: &mut Self::ExternalState, viewport: Rect) {
        self.app.project_frame_viewport(external, viewport);
    }

    fn project_text_metrics(
        &self,
        external: &mut Self::ExternalState,
        metrics: &mut dyn SlipwayTextMetricProvider,
    ) {
        self.app.project_text_metrics(external, metrics);
    }
}

// `do_not_recommend`: without it, a widget missing its own
// `SlipwayEventRoutingPolicy` impl gets the misleading rustc suggestion
// "the trait is implemented for `SlipwayAppWidget<A>`", steering authors
// toward wrapping the widget instead of implementing the missing policy
// (audit 2026-07-11, LE-H7).
#[diagnostic::do_not_recommend]
impl<A: SlipwayApp> SlipwayEventRoutingPolicy for SlipwayAppWidget<A> {
    fn event_routing_policy(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        event: &InputEvent,
    ) -> EventRoutingPolicyDeclaration {
        let target = self.app.id();
        let event_target = event.target().clone();
        let mut path = vec![target.clone()];
        if event_target != target {
            path.push(event_target.clone());
        }
        EventRoutingPolicyDeclaration {
            target,
            event_target,
            route: EventRoute {
                route_id: Some("app.runtime.route".to_string()),
                address: event.target_slot().cloned(),
                path,
                phase: EventRoutePhase::Target,
            },
            capture: Vec::new(),
            diagnostics: Vec::new(),
        }
    }
}

// `do_not_recommend`: same misleading-wrapper suppression as the
// `SlipwayEventRoutingPolicy` impl above (audit 2026-07-11, LE-H7).
#[diagnostic::do_not_recommend]
impl<A: SlipwayApp> SlipwayEventDispositionPolicy for SlipwayAppWidget<A> {
    fn event_disposition(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        event: &InputEvent,
        route: &EventRoute,
    ) -> EventPropagationEvidence {
        let target = self.app.id();
        let handled =
            event.target() == &target || route.path.iter().any(|node| node == event.target());
        let disposition = EventDisposition {
            handled,
            propagate: !handled,
            default_action_allowed: true,
        };
        EventPropagationEvidence {
            target: target.clone(),
            event: event.clone(),
            steps: vec![EventPropagationStep {
                stage: EventPropagationStage::Target,
                node: route.path.last().cloned().or(Some(target)),
                disposition,
                emitted_messages: Vec::new(),
                changes: Vec::new(),
            }],
            final_disposition: disposition,
            diagnostics: Vec::new(),
        }
    }
}

// `do_not_recommend`: same misleading-wrapper suppression as the
// `SlipwayEventRoutingPolicy` impl above (audit 2026-07-11, LE-H7).
//
// This impl is what makes the documented egui quickstart path compile for
// a plain facade app (Step 208 finding): the egui root gate
// (`SlipwayEguiBackendContract`) requires `SlipwayFontResolutionPolicy`
// on the root widget, and the orphan rule forbids an app crate from
// implementing that policy on `SlipwayAppWidget<A>`. Core provides it by
// delegating to [`SlipwayApp::resolve_app_font`], whose default is an
// honest refusal.
#[diagnostic::do_not_recommend]
impl<A: SlipwayApp> SlipwayFontResolutionPolicy for SlipwayAppWidget<A> {
    fn resolve_font(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        request: FontResolutionRequest,
    ) -> FontResolutionEvidence {
        self.app.resolve_app_font(external, &local.app, request)
    }
}

impl<A: SlipwayApp> SlipwayView for SlipwayAppWidget<A> {
    fn initial_local_state(&self) -> Self::LocalState {
        SlipwayAppLocalState {
            app: self.app.initial_local_state(),
            widgets: self.app.widgets().initial_child_local_state(),
        }
    }

    fn layout(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: LayoutInput,
        mut output: LayoutOutputBuilder,
    ) -> LayoutOutput {
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        let seeds = self.app.widgets().child_layout_seeds(&root_slot);
        let plan = self
            .app
            .layout_plan(external, &local.app, input.clone(), seeds);
        let planned_bounds = plan.bounds;
        let mut child_results = self.app.widgets().layout_children(
            external,
            &local.widgets,
            &root_slot,
            &plan.children,
        );
        output.extend_diagnostics(plan.diagnostics);
        output.reserve(plan.children.len());

        for child_plan in plan.children {
            if let Some(result_index) = child_results
                .iter()
                .position(|result| child_layout_result_matches_plan(result, &child_plan))
            {
                let result = child_results.remove(result_index);
                output
                    .push_resolved(child_plan, result)
                    .expect("validated child geometry resolves after execution");
            } else {
                output.extend_diagnostics([Diagnostic {
                    target: Some(child_plan.request.child.clone()),
                    severity: DiagnosticSeverity::Warning,
                    code: "missing-child-layout".to_string(),
                    message: "app layout plan requested a child that is not in the widget list"
                        .to_string(),
                }]);
            }
        }

        let mut finish_input = input;
        finish_input.viewport = planned_bounds;
        self.app.layout(external, &local.app, finish_input, output)
    }

    fn paint(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        let root_id = self.id();
        let synthetic_frame = FrameIdentity {
            surface_id: format!("{}:paint", root_id.as_str()),
            surface_instance_id: root_id.as_str().to_string(),
            revision: 0,
            frame_index: 0,
            viewport: layout.bounds.into_rect(),
        };
        let mut units = vec![PaintUnit::source_order(
            root_id,
            Some(root_slot.clone()),
            0,
            self.app.paint(external, &local.app, layout),
        )];

        units.extend(self.app.widgets().child_paint_units(
            external,
            &local.widgets,
            &root_slot,
            &synthetic_frame,
            layout,
        ));

        flatten_ordered_paint_units(units)
    }

    fn observe_state(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<StateObservation> {
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        let mut observations = self.app.observe_state(external, &local.app);
        observations.extend(self.app.widgets().observe_child_state(
            external,
            &local.widgets,
            &root_slot,
        ));
        observations
    }
}

impl<A: SlipwayApp> SlipwayViewDefinition for SlipwayAppWidget<A> {
    fn view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        let (frame, layout_input, output) = input.into_layout_parts();
        let mut layout = self.layout(external, local, layout_input.clone(), output);
        let root_paint = self.app.paint(external, &local.app, &layout);
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        let child_views = self.app.widgets().child_view_definitions(
            external,
            &local.widgets,
            &root_slot,
            &frame,
            &layout,
        );

        let mut hit_regions = Vec::new();
        let mut focus_regions = Vec::new();
        let mut scroll_regions = Vec::new();
        let mut semantic_slots = Vec::new();
        let mut probe_metadata = Vec::new();
        let mut diagnostics = layout.diagnostics.clone();
        let mut child_overflow_bounds: Option<Rect> = None;
        let mut paint_order_mounts = Vec::new();

        for child in child_views {
            // A child that declared overflow bounds (the roaming-overlay
            // pattern: `PaintOrderDeclaration::with_overflow_bounds`) keeps
            // that allowance in the COMPOSED view too, or the composed-level
            // re-validation would refuse the very regions the child's own
            // admission accepted. The child's overflow rect is child-local;
            // the mounted child layout is root-local, so its origin is the
            // placement origin the rect must translate by. Apps with no
            // overflow-declaring child keep the exact pre-existing
            // paint_order (byte-identical default).
            if child.paint_order.allow_overflow_paint {
                if let Some(bounds) = child.paint_order.overflow_bounds {
                    let origin = child.layout.bounds.into_rect().origin;
                    let translated = Rect {
                        origin: Point {
                            x: bounds.into_rect().origin.x + origin.x,
                            y: bounds.into_rect().origin.y + origin.y,
                        },
                        size: bounds.into_rect().size,
                    };
                    child_overflow_bounds = Some(match child_overflow_bounds {
                        Some(existing) => bounding_union_rect(existing, translated),
                        None => translated,
                    });
                }
            }
            paint_order_mounts.extend(child.paint_order.mounted_geometry.iter().cloned());
            hit_regions.extend(child.hit_regions);
            focus_regions.extend(child.focus_regions);
            scroll_regions.extend(child.scroll_regions);
            semantic_slots.extend(child.semantic_slots);
            probe_metadata.extend(child.probe_metadata);
            layout
                .child_placements
                .extend(child.layout.child_placements.clone());
            diagnostics.extend(child.layout.diagnostics);
            diagnostics.extend(child.diagnostics);
        }

        let mut paint_order = PaintOrderDeclaration::source_order(self.id());
        paint_order.mounted_geometry = paint_order_mounts;
        if let Some(overflow) = child_overflow_bounds {
            // The composed allowance must still contain the root's own
            // layout (paint validation checks EVERY op against the overflow
            // rect once one is declared).
            paint_order = paint_order.with_overflow_bounds(TargetLocalRect::new(
                bounding_union_rect(overflow, layout.bounds.into_rect()),
            ));
        }

        // App-declared regions (the page/root-scroll pattern): appended
        // AFTER the child regions so the app can see the final composed
        // layout; the empty default keeps the composed view byte-identical
        // for apps that do not override the hook.
        let app_scroll_regions =
            self.app
                .app_scroll_regions(external, &local.app, &frame, &layout_input, &layout);
        let wheel_traversal_boundary = DeclaredWheelTraversalBoundary {
            terminal_region_index: (!app_scroll_regions.is_empty()).then_some(scroll_regions.len()),
        };
        scroll_regions.extend(app_scroll_regions);

        ViewDefinition {
            target: self.id(),
            frame,
            layout,
            paint: root_paint,
            paint_order,
            hit_regions,
            focus_regions,
            scroll_regions,
            wheel_traversal_boundary,
            semantic_slots,
            probe_metadata,
            diagnostics,
        }
    }

    fn visible_backend_view_definition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: ViewDefinitionInput,
    ) -> ViewDefinition {
        self.view_definition(external, local, input)
    }
}

impl SlipwayWidgetList for () {
    type ExternalState = ();
    type LocalState = ();
    type AppMessage = ();

    fn initial_child_local_state(&self) -> Self::LocalState {}

    fn widget_count(&self) -> usize {
        0
    }

    fn visit_children<V>(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _visitor: &mut V,
    ) where
        V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
    {
    }

    fn child_topology(
        &self,
        _external: &Self::ExternalState,
        _parent_slot: &WidgetSlotAddress,
    ) -> Vec<TopologyNode> {
        Vec::new()
    }

    fn child_layout_seeds(&self, _parent_slot: &WidgetSlotAddress) -> Vec<ChildLayoutSeed> {
        Vec::new()
    }

    fn layout_children(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _plans: &[ChildLayoutPlan],
    ) -> Vec<ChildLayoutResult> {
        Vec::new()
    }

    fn route_event(
        &self,
        _external: &Self::ExternalState,
        _local: &mut Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _event: InputEvent,
    ) -> EventOutcome<Self::AppMessage> {
        EventOutcome::ignored()
    }

    fn child_paint(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _layout: &LayoutOutput,
    ) -> Vec<PaintOp> {
        Vec::new()
    }

    fn child_paint_units(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _frame: &FrameIdentity,
        _layout: &LayoutOutput,
    ) -> Vec<PaintUnit> {
        Vec::new()
    }

    fn observe_child_state(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
    ) -> Vec<StateObservation> {
        Vec::new()
    }
}

impl SlipwayWidgetListViewDefinition for () {
    fn child_view_definitions(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        _parent_slot: &WidgetSlotAddress,
        _frame: &FrameIdentity,
        _layout: &LayoutOutput,
    ) -> Vec<ViewDefinition> {
        Vec::new()
    }
}

macro_rules! impl_widget_list_tuple {
    ($($widget:ident $index:tt),+) => {
        impl<ExternalState, AppMessage, $($widget),+> SlipwayWidgetList for ($($widget,)+)
        where
            $($widget: SlipwayWidget<ExternalState = ExternalState, AppMessage = AppMessage>
                + SlipwayEventRoutingPolicy
                + SlipwayEventDispositionPolicy
                + SlipwayViewDefinition,)+
        {
            type ExternalState = ExternalState;
            type LocalState = ($($widget::LocalState,)+);
            type AppMessage = AppMessage;

            fn initial_child_local_state(&self) -> Self::LocalState {
                ($(self.$index.initial_local_state(),)+)
            }

            fn widget_count(&self) -> usize {
                0 $(+ {
                    let _ = &self.$index;
                    1
                })+
            }

            fn visit_children<V>(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                visitor: &mut V,
            ) where
                V: SlipwayWidgetListVisitor<Self::ExternalState, Self::AppMessage>,
            {
                $(
                    visitor.visit_child(
                        &self.$index,
                        external,
                        &local.$index,
                        parent_slot.child(self.$index.id(), $index),
                    );
                )+
            }

            fn child_topology(
                &self,
                external: &Self::ExternalState,
                parent_slot: &WidgetSlotAddress,
            ) -> Vec<TopologyNode> {
                let mut children = Vec::new();
                $(
                    let child_slot = parent_slot.child(self.$index.id(), $index);
                    children.push(mount_child_topology(
                        self.$index.topology(external),
                        child_slot,
                    ));
                )+
                children
            }

            fn child_layout_seeds(&self, parent_slot: &WidgetSlotAddress) -> Vec<ChildLayoutSeed> {
                let mut seeds = Vec::new();
                $(
                    let child = self.$index.id();
                    seeds.push(ChildLayoutSeed {
                        child: child.clone(),
                        local_state_slot: Some(parent_slot.child(child, $index)),
                    });
                )+
                seeds
            }

            fn layout_children(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                plans: &[ChildLayoutPlan],
            ) -> Vec<ChildLayoutResult> {
                let mut results = Vec::new();
                for plan in plans {
                    let request = &plan.request;
                    $(
                        let child = self.$index.id();
                        let child_slot = parent_slot.child(child.clone(), $index);
                        if child_layout_request_matches_slot(request, &child, &child_slot) {
                            let prepared = prepare_child_layout(plan);
                            let mut child_layout = prepared.as_ref().map(|prepared| {
                                let output = LayoutOutputBuilder::for_input(&prepared.input);
                                self.$index.layout(
                                    external,
                                    &local.$index,
                                    prepared.input.clone(),
                                    output,
                                )
                            }).unwrap_or_else(|_| {
                                let input = LayoutInput {
                                    viewport: TargetLocalRect::new(Rect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 0.0, height: 0.0 } }),
                                    content: TargetLocalRect::new(Rect { origin: Point { x: 0.0, y: 0.0 }, size: Size { width: 0.0, height: 0.0 } }),
                                    constraints: LayoutConstraints { min: Size { width: 0.0, height: 0.0 }, max: Size { width: 0.0, height: 0.0 } },
                                };
                                LayoutOutputBuilder::for_input(&input).finish(input.viewport)
                            });
                            let diagnostics = child_layout.take_diagnostics();
                            results.push(ChildLayoutResult {
                                seed: ChildLayoutSeed {
                                    child: child.clone(),
                                    local_state_slot: Some(child_slot),
                                },
                                layout: child_layout,
                                diagnostics,
                            });
                            continue;
                        }
                    )+
                }
                results
            }

            fn route_event(
                &self,
                external: &Self::ExternalState,
                local: &mut Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                event: InputEvent,
            ) -> EventOutcome<Self::AppMessage> {
                let target = event.target().clone();
                let target_slot = event.target_slot().cloned();
                let addressed_matches = ($(
                    target_slot.as_ref().is_some_and(|target_slot| {
                        target_slot_matches_direct_child(
                            target_slot,
                            parent_slot,
                            &self.$index.id(),
                            $index,
                        )
                    }),
                )+);
                let addressed_match_count = 0 $(+ usize::from(addressed_matches.$index))+;
                $(
                    let child_slot = parent_slot.child(self.$index.id(), $index);
                    let event_matches_child = if let Some(target_slot) = &target_slot {
                        let _ = target_slot;
                        addressed_match_count == 1 && addressed_matches.$index
                    } else {
                        widget_contains_target(&self.$index, external, &target)
                    };
                    if event_matches_child {
                        let declaration = declared_event_handling(
                            &self.$index,
                            external,
                            &local.$index,
                            &event,
                        );
                        if !declaration.disposition.final_disposition.handled {
                            return mount_event_outcome(
                                refuse_event_declared_unhandled(declaration),
                                &child_slot,
                            );
                        }
                        let outcome = self.$index.handle_event(
                            external,
                            &mut local.$index,
                            event.clone(),
                        );
                        let outcome = apply_physical_event_handling_declaration(declaration, outcome);
                        return mount_event_outcome(outcome, &child_slot);
                    }
                )+
                EventOutcome::ignored()
            }

            fn child_paint(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                layout: &LayoutOutput,
            ) -> Vec<PaintOp> {
                let mut ops = Vec::new();
                $(
                    let child = self.$index.id();
                    let child_slot = parent_slot.child(child.clone(), $index);
                    if let Some(placement) = layout.child_placements.iter().find(|placement| {
                        child_placement_matches_slot(placement, &child, &child_slot)
                    }) {
                        let child_input = child_view_definition_input(placement.bounds);
                        let child_layout = prepare_child_paint_layout(&child_input, TargetLocalRect::new(Rect {
                                origin: Point { x: 0.0, y: 0.0 },
                                size: placement.bounds.as_rect().size,
                            }));
                        ops.extend(mount_child_paint_ops(
                            self.$index.paint(external, &local.$index, &child_layout),
                            placement.bounds.into_rect(),
                        ));
                    }
                )+
                ops
            }

            fn child_paint_units(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                frame: &FrameIdentity,
                layout: &LayoutOutput,
            ) -> Vec<PaintUnit> {
                let mut units = Vec::new();
                $(
                    let child = self.$index.id();
                    let child_slot = parent_slot.child(child.clone(), $index);
                    if let Some(placement) = layout.child_placements.iter().find(|placement| {
                        child_placement_matches_slot(placement, &child, &child_slot)
                    }) {
                        let child_input = child_view_definition_input(placement.bounds);
                        let child_layout = prepare_child_paint_layout(&child_input, TargetLocalRect::new(Rect {
                                origin: Point { x: 0.0, y: 0.0 },
                                size: placement.bounds.as_rect().size,
                            }));
                        let view = self.$index.view_definition(
                            external,
                            &local.$index,
                            ViewDefinitionInput::new(frame.clone(), child_view_definition_input(placement.bounds)),
                        );
                        let mut unit = PaintUnit::from_view(view, $index + 1);
                        unit.address = placement.local_state_slot.clone().or(Some(child_slot));
                        unit.paint = mount_child_paint_ops(
                            self.$index.paint(external, &local.$index, &child_layout),
                            placement.bounds.into_rect(),
                        );
                        units.push(unit);
                    }
                )+
                units
            }

            fn observe_child_state(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
            ) -> Vec<StateObservation> {
                let mut observations = Vec::new();
                $(
                    let child_slot = parent_slot.child(self.$index.id(), $index);
                    observations.extend(mount_state_observations(
                        self.$index.observe_state(external, &local.$index),
                        &child_slot,
                    ));
                )+
                observations
            }
        }

        impl<ExternalState, AppMessage, $($widget),+> SlipwayWidgetListViewDefinition for ($($widget,)+)
        where
            $($widget: SlipwayWidget<ExternalState = ExternalState, AppMessage = AppMessage>
                + SlipwayEventRoutingPolicy
                + SlipwayEventDispositionPolicy
                + SlipwayViewDefinition,)+
        {
            fn child_view_definitions(
                &self,
                external: &Self::ExternalState,
                local: &Self::LocalState,
                parent_slot: &WidgetSlotAddress,
                frame: &FrameIdentity,
                layout: &LayoutOutput,
            ) -> Vec<ViewDefinition> {
                let mut views = Vec::new();
                $(
                    let child = self.$index.id();
                    let child_slot = parent_slot.child(child.clone(), $index);
                    if let Some(placement) = layout.child_placements.iter().find(|placement| {
                        child_placement_matches_slot(placement, &child, &child_slot)
                    }) {
                        let view = self.$index.view_definition(
                            external,
                            &local.$index,
                            ViewDefinitionInput::new(frame.clone(), child_view_definition_input(placement.bounds)),
                        );
                        views.push(mount_child_view_definition(
                            view,
                            parent_slot,
                            placement.local_state_slot.as_ref(),
                            placement.bounds,
                        ));
                    }
                )+
                views
            }
        }
    };
}

impl_widget_list_tuple!(A 0);
impl_widget_list_tuple!(A 0, B 1);
impl_widget_list_tuple!(A 0, B 1, C 2);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13, O 14);
impl_widget_list_tuple!(A 0, B 1, C 2, D 3, E 4, F 5, G 6, H 7, I 8, J 9, K 10, L 11, M 12, N 13, O 14, P 15);

fn widget_contains_target<W>(widget: &W, external: &W::ExternalState, target: &WidgetId) -> bool
where
    W: SlipwayWidget,
{
    widget
        .topology(external)
        .traverse_depth_first()
        .order
        .iter()
        .any(|id| id == target)
}

fn child_layout_request_matches_slot(
    request: &ChildLayoutRequest,
    child: &WidgetId,
    child_slot: &WidgetSlotAddress,
) -> bool {
    if let Some(request_slot) = &request.local_state_slot {
        request_slot == child_slot
    } else {
        request.child == *child
    }
}

fn child_layout_result_matches_plan(result: &ChildLayoutResult, plan: &ChildLayoutPlan) -> bool {
    if let Some(plan_slot) = &plan.request.local_state_slot {
        result.seed.local_state_slot.as_ref() == Some(plan_slot)
    } else {
        result.seed.child == plan.request.child
    }
}

fn child_placement_matches_slot(
    placement: &ChildPlacement,
    child: &WidgetId,
    child_slot: &WidgetSlotAddress,
) -> bool {
    if let Some(placement_slot) = &placement.local_state_slot {
        placement_slot == child_slot
    } else {
        placement.child == *child
    }
}

fn child_view_definition_input(bounds: ParentLocalRect) -> LayoutInput {
    let size = bounds.as_rect().size;
    LayoutInput {
        viewport: TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size,
        }),
        content: TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size,
        }),
        constraints: LayoutConstraints {
            min: Size {
                width: 0.0,
                height: 0.0,
            },
            max: size,
        },
    }
}

/// Smallest rect containing both inputs (used to compose child-declared
/// overflow allowances into the app root's paint order).
fn bounding_union_rect(a: Rect, b: Rect) -> Rect {
    let min_x = a.origin.x.min(b.origin.x);
    let min_y = a.origin.y.min(b.origin.y);
    let max_x = (a.origin.x + a.size.width.max(0.0)).max(b.origin.x + b.size.width.max(0.0));
    let max_y = (a.origin.y + a.size.height.max(0.0)).max(b.origin.y + b.size.height.max(0.0));
    Rect {
        origin: Point { x: min_x, y: min_y },
        size: Size {
            width: max_x - min_x,
            height: max_y - min_y,
        },
    }
}

fn mount_child_view_definition(
    mut view: ViewDefinition,
    parent_slot: &WidgetSlotAddress,
    slot: Option<&WidgetSlotAddress>,
    placement: ParentLocalRect,
) -> ViewDefinition {
    if let Some(slot) = slot {
        for declaration in &mut view.paint_order.mounted_geometry {
            declaration.address = mount_widget_slot_address(declaration.address.clone(), slot);
            if let Some(parent) = declaration.parent_address.take() {
                declaration.parent_address = Some(mount_widget_slot_address(parent, slot));
            }
            if let Some(anchor) = &mut declaration.overlay_anchor {
                anchor.address = mount_widget_slot_address(anchor.address.clone(), slot);
            }
        }
        if let Some(anchor) = &mut view.paint_order.overlay_anchor {
            anchor.address = mount_widget_slot_address(anchor.address.clone(), parent_slot);
        }
        view.paint_order.mounted_geometry.insert(
            0,
            MountedGeometryDeclaration {
                address: slot.clone(),
                parent_address: Some(parent_slot.clone()),
                authored_overflow: (view.paint_order.allow_overflow_paint)
                    .then_some(view.paint_order.overflow_bounds)
                    .flatten(),
                overlay_anchor: view.paint_order.overlay_anchor.clone(),
            },
        );
    }
    mount_child_view_definition_geometry(&mut view, placement);

    if let Some(slot) = slot {
        for placement in &mut view.layout.child_placements {
            mount_existing_optional_slot_address(&mut placement.local_state_slot, slot);
        }

        for region in &mut view.hit_regions {
            mount_optional_slot_address(&mut region.address, slot);
            mount_optional_slot_address(&mut region.route.address, slot);
            region.route.path = region
                .route
                .address
                .as_ref()
                .map(|address| address.path.clone())
                .unwrap_or_default();
        }

        for region in &mut view.focus_regions {
            mount_optional_slot_address(&mut region.address, slot);
        }

        for region in &mut view.scroll_regions {
            mount_optional_slot_address(&mut region.address, slot);
        }
    }

    view
}

fn mount_child_view_definition_geometry(view: &mut ViewDefinition, placement: ParentLocalRect) {
    let offset = placement.as_rect().origin;

    view.layout = translate_layout_output(view.layout.clone(), offset);
    view.paint = view
        .paint
        .drain(..)
        .map(|op| translate_paint_op(op, offset))
        .collect();
}

fn translate_layout_output(mut layout: LayoutOutput, offset: Point) -> LayoutOutput {
    layout.bounds = TargetLocalRect::new(translate_rect(layout.bounds, offset));
    for placement in &mut layout.child_placements {
        placement.bounds = ParentLocalRect::from_parent_local(translate_rect(
            placement.bounds.into_rect(),
            offset,
        ));
    }
    layout
}

fn rect_origin_is_zero(rect: impl Into<Rect>) -> bool {
    let rect = rect.into();
    rect.origin.x == 0.0 && rect.origin.y == 0.0
}

fn mount_optional_slot_address(
    address: &mut Option<WidgetSlotAddress>,
    parent_slot: &WidgetSlotAddress,
) {
    *address = Some(
        address
            .take()
            .map(|slot| mount_widget_slot_address(slot, parent_slot))
            .unwrap_or_else(|| parent_slot.clone()),
    );
}

fn mount_existing_optional_slot_address(
    address: &mut Option<WidgetSlotAddress>,
    parent_slot: &WidgetSlotAddress,
) {
    if let Some(slot) = address.take() {
        *address = Some(mount_widget_slot_address(slot, parent_slot));
    }
}

fn mount_child_topology(mut node: TopologyNode, child_slot: WidgetSlotAddress) -> TopologyNode {
    node.local_state_slot = Some(child_slot.clone());
    for child in &mut node.children {
        mount_descendant_topology_slots(child, &child_slot);
    }
    node
}

fn mount_descendant_topology_slots(node: &mut TopologyNode, parent_slot: &WidgetSlotAddress) {
    if let Some(slot) = node.local_state_slot.clone() {
        node.local_state_slot = Some(mount_widget_slot_address(slot, parent_slot));
    }
    for child in &mut node.children {
        mount_descendant_topology_slots(child, parent_slot);
    }
}

fn mount_state_observations(
    observations: Vec<StateObservation>,
    child_slot: &WidgetSlotAddress,
) -> Vec<StateObservation> {
    observations
        .into_iter()
        .map(|mut observation| {
            observation.slot = observation
                .slot
                .map(|slot| mount_widget_slot_address(slot, child_slot))
                .or_else(|| Some(child_slot.clone()));
            observation
        })
        .collect()
}

fn mount_event_outcome<M>(
    mut outcome: EventOutcome<M>,
    child_slot: &WidgetSlotAddress,
) -> EventOutcome<M> {
    outcome.observations = mount_state_observations(outcome.observations, child_slot);
    outcome.changes = outcome
        .changes
        .into_iter()
        .map(|change| mount_change_evidence(change, child_slot))
        .collect();
    for probe in &mut outcome.probes {
        mount_probe_slots(probe, child_slot);
    }
    outcome
}

fn mount_change_evidence(
    mut change: ChangeEvidence,
    child_slot: &WidgetSlotAddress,
) -> ChangeEvidence {
    change.slot = change
        .slot
        .map(|slot| mount_widget_slot_address(slot, child_slot))
        .or_else(|| Some(child_slot.clone()));
    change
}

fn mount_probe_slots(probe: &mut ProbeProduct, child_slot: &WidgetSlotAddress) {
    match probe {
        ProbeProduct::Topology(topology) => {
            topology.root = mount_child_topology(topology.root.clone(), child_slot.clone());
            topology.traversal = topology.root.traverse_depth_first();
        }
        ProbeProduct::State(state) => {
            state.observations =
                mount_state_observations(mem::take(&mut state.observations), child_slot);
        }
        ProbeProduct::Event(event) => {
            event.local_state =
                mount_state_observations(mem::take(&mut event.local_state), child_slot);
            event.changes = mem::take(&mut event.changes)
                .into_iter()
                .map(|change| mount_change_evidence(change, child_slot))
                .collect();
        }
        ProbeProduct::Change(change) => {
            change.changes = mem::take(&mut change.changes)
                .into_iter()
                .map(|change| mount_change_evidence(change, child_slot))
                .collect();
        }
        _ => {}
    }
}

/// Mounts a self-rooted child address below an already-mounted parent address.
///
/// Already-mounted inputs are returned unchanged, making the operation
/// idempotent across nested presentation passes.
pub fn mount_widget_slot_address(
    slot: WidgetSlotAddress,
    parent_slot: &WidgetSlotAddress,
) -> WidgetSlotAddress {
    if slot.path.starts_with(&parent_slot.path) {
        return slot;
    }

    if slot.widget == parent_slot.widget {
        return parent_slot.clone();
    }

    let WidgetSlotAddress {
        widget,
        path: child_path,
        ordinal,
    } = slot;
    let skip_boundary = usize::from(child_path.first() == Some(&parent_slot.widget));
    let mut path = Vec::with_capacity(parent_slot.path.len() + child_path.len() - skip_boundary);
    path.extend(parent_slot.path.iter().cloned());
    path.extend(child_path.into_iter().skip(skip_boundary));

    WidgetSlotAddress {
        widget,
        path,
        ordinal,
    }
}

fn target_slot_matches_direct_child(
    target_slot: &WidgetSlotAddress,
    parent_slot: &WidgetSlotAddress,
    child: &WidgetId,
    child_ordinal: usize,
) -> bool {
    if target_slot.widget == *child
        && target_slot.ordinal == child_ordinal
        && target_slot.path.len() == parent_slot.path.len() + 1
        && target_slot.path.starts_with(&parent_slot.path)
        && target_slot.path.last() == Some(child)
    {
        return true;
    }

    let app = &parent_slot.widget;
    let mut matched_child_index = None;
    for (index, window) in target_slot.path.windows(2).enumerate() {
        if &window[0] == app && &window[1] == child {
            if matched_child_index.is_some() {
                return false;
            }
            matched_child_index = Some(index + 1);
        }
    }

    let Some(child_index) = matched_child_index else {
        return false;
    };
    if child_index + 1 == target_slot.path.len() {
        target_slot.widget == *child && target_slot.ordinal == child_ordinal
    } else {
        true
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeCollector {
    products: Vec<ProbeProduct>,
}

impl ProbeCollector {
    pub fn new() -> Self {
        Self {
            products: Vec::new(),
        }
    }

    pub fn push(&mut self, product: ProbeProduct) {
        self.products.push(product);
    }

    pub fn extend(&mut self, products: impl IntoIterator<Item = ProbeProduct>) {
        self.products.extend(products);
    }

    pub fn is_empty(&self) -> bool {
        self.products.is_empty()
    }

    pub fn len(&self) -> usize {
        self.products.len()
    }

    pub fn take(&mut self) -> Vec<ProbeProduct> {
        mem::take(&mut self.products)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_rgb(red: u8, green: u8, blue: u8) -> Color {
        Color {
            red: f32::from(red) / 255.0,
            green: f32::from(green) / 255.0,
            blue: f32::from(blue) / 255.0,
            alpha: 1.0,
        }
    }

    fn test_hit_region_with_capture(capture: PointerCaptureIntent) -> HitRegionDeclaration {
        let target = WidgetId::from("capture-target");
        HitRegionDeclaration {
            id: PresentationRegionId::from("capture-region"),
            target: target.clone(),
            address: None,
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 40.0,
                },
            }),
            event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
            order: HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
            route: EventRoute {
                route_id: Some("capture-test".to_string()),
                address: None,
                path: vec![target],
                phase: EventRoutePhase::Target,
            },
            cursor: CursorCapability::Pointer,
            enabled: true,
            capture,
            capture_evidence: Vec::new(),
        }
    }

    #[test]
    fn during_drag_capture_does_not_claim_hover_moves() {
        let region = test_hit_region_with_capture(PointerCaptureIntent::DuringDrag);

        assert!(!declared_pointer_capture_for_region(
            &region,
            PointerEventKind::Move,
            false,
        ));
        assert!(declared_pointer_capture_for_region(
            &region,
            PointerEventKind::Move,
            true,
        ));
        assert!(declared_pointer_capture_for_region(
            &region,
            PointerEventKind::Release,
            false,
        ));
        assert!(declared_pointer_capture_for_region(
            &region,
            PointerEventKind::Cancel,
            false,
        ));
    }

    #[derive(Clone, Debug, PartialEq)]
    struct FakeWidget {
        id: WidgetId,
    }

    impl FakeWidget {
        fn text_measurement_request(&self, input: &LayoutInput) -> TextMeasurementRequest {
            TextMeasurementRequest {
                target: self.id.clone(),
                request_id: "title".to_string(),
                content: "Official metric wrapper".to_string(),
                style: TextStyle::plain(),
                available_bounds: Some(input.viewport.into_rect()),
                flow: Some(TextFlowPolicy {
                    target: self.id.clone(),
                    line_mode: TextLineMode::SingleLine,
                    wrap: TextWrapMode::NoWrap,
                    line_clamp: Some(1),
                    allow_ellipsis: true,
                    baseline: None,
                    caret_bounds: Vec::new(),
                    viewport: None,
                }),
                purposes: vec![
                    TextMeasurementPurpose::IntrinsicSize,
                    TextMeasurementPurpose::OverflowDetection,
                ],
            }
        }

        fn text_measurement_cache_policy_for_title(&self) -> TextMeasurementCachePolicyDeclaration {
            TextMeasurementCachePolicyDeclaration {
                target: self.id.clone(),
                request_id: "title".to_string(),
                key: TextMeasurementCacheKey {
                    namespace: "fake-widget-text".to_string(),
                    key: "title/default-style/320x240".to_string(),
                    revisions: vec![CacheRevisionToken {
                        name: "content".to_string(),
                        value: "rev-1".to_string(),
                    }],
                },
                scope: TextMeasurementCacheScope::WidgetLocal,
                reuse: TextMeasurementCacheReuse::UntilRevisionChange,
                required: true,
                invalidates_on: vec![CacheRevisionToken {
                    name: "content".to_string(),
                    value: "rev-1".to_string(),
                }],
                diagnostics: Vec::new(),
            }
        }
    }

    struct FakeOfficialMetricProvider;

    impl SlipwayTextMetricProvider for FakeOfficialMetricProvider {
        fn text_metric_source(&self) -> TextMetricSource {
            TextMetricSource {
                provider_id: "contract-test-provider".to_string(),
                backend_id: Some("test-backend-wrapper".to_string()),
                api_name: "official_text_measure".to_string(),
                kind: TextMetricSourceKind::OfficialBackendApi,
            }
        }

        fn measure_text(&mut self, request: TextMeasurementRequest) -> TextMeasurementReceipt {
            TextMeasurementReceipt::Valid(ValidTextMeasurement {
                facts: TextMeasurementFacts {
                    measured_size: Size {
                        width: 96.0,
                        height: 18.0,
                    },
                    content_bounds: request.available_bounds.unwrap_or(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 96.0,
                            height: 18.0,
                        },
                    }),
                    baseline: Some(14.0),
                    line_count: Some(1),
                    caret_bounds: Vec::new(),
                },
                source: self.text_metric_source(),
                request,
            })
        }
    }

    #[derive(Default)]
    struct FakeTextMeasurementCache {
        stored: Option<(TextMeasurementCacheKey, TextMeasurementReceipt)>,
    }

    struct FakeBackend;

    impl SlipwayTextMeasurementCache for FakeTextMeasurementCache {
        fn lookup_text_measurement(
            &mut self,
            policy: &TextMeasurementCachePolicyDeclaration,
        ) -> TextMeasurementCacheLookup {
            let evidence = |status| TextMeasurementCacheEvidence {
                target: policy.target.clone(),
                request_id: policy.request_id.clone(),
                key: Some(policy.key.clone()),
                status,
                diagnostics: Vec::new(),
            };

            match &self.stored {
                Some((key, receipt)) if key == &policy.key => TextMeasurementCacheLookup::Hit {
                    receipt: receipt.clone(),
                    evidence: evidence(TextMeasurementCacheStatus::Hit),
                },
                _ => TextMeasurementCacheLookup::Miss {
                    evidence: evidence(TextMeasurementCacheStatus::Miss),
                },
            }
        }

        fn store_text_measurement(
            &mut self,
            policy: &TextMeasurementCachePolicyDeclaration,
            receipt: &TextMeasurementReceipt,
        ) -> TextMeasurementCacheEvidence {
            self.stored = Some((policy.key.clone(), receipt.clone()));
            TextMeasurementCacheEvidence {
                target: policy.target.clone(),
                request_id: policy.request_id.clone(),
                key: Some(policy.key.clone()),
                status: TextMeasurementCacheStatus::Stored,
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct External {
        child: WidgetId,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Local {
        count: u32,
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Counted,
    }

    #[derive(Default)]
    struct FakeChildAccessVisitor {
        visited: usize,
    }

    impl SlipwayWidgetListVisitor<External, Message> for FakeChildAccessVisitor {
        fn visit_child<W>(
            &mut self,
            _widget: &W,
            _external: &External,
            _local: &W::LocalState,
            _slot: WidgetSlotAddress,
        ) where
            W: SlipwayWidget<ExternalState = External, AppMessage = Message>
                + SlipwayViewDefinition,
        {
            self.visited += 1;
        }
    }

    #[test]
    fn text_paint_carries_default_and_declared_style() {
        let bounds = Rect {
            origin: Point { x: 1.0, y: 2.0 },
            size: Size {
                width: 80.0,
                height: 20.0,
            },
        };
        let color = Color {
            red: 0.1,
            green: 0.2,
            blue: 0.3,
            alpha: 1.0,
        };

        let plain = PaintOp::styled_text(bounds, "plain", color, TextStyle::plain());
        let PaintOp::Text { style, .. } = plain else {
            panic!("text constructor returns text paint op");
        };
        assert_eq!(style, TextStyle::plain());
        assert_eq!(style.font_family, DEFAULT_TEXT_FONT_FAMILY);
        assert_eq!(style.font_size, DEFAULT_TEXT_FONT_SIZE);
        assert_eq!(style.font_weight, FontWeight::Normal);
        assert_eq!(style.font_style, FontStyle::Normal);
        assert_eq!(style.decoration, TextDecoration::none());
        assert_eq!(style.baseline, BaselineShift::Normal);
        // Unspecified alignment is the historical left/top anchoring —
        // the byte-identical default the backends key off (NC-14).
        assert_eq!(style.align_x, TextAlignX::Start);
        assert_eq!(style.align_y, TextAlignY::Top);
        assert_eq!(TextAlignX::default(), TextAlignX::Start);
        assert_eq!(TextAlignY::default(), TextAlignY::Top);
        // Unspecified wrap is the historical hardcoded word wrap — the
        // byte-identical default the backends key off (NC-4).
        assert_eq!(style.wrap, TextWrap::Word);
        assert_eq!(TextWrap::default(), TextWrap::Word);

        let overridden = TextStyle::plain()
            .with_font_family("system-ui")
            .with_font_size(18.0)
            .with_font_weight(FontWeight::Bold);
        assert_eq!(overridden.font_family, "system-ui");
        assert_eq!(overridden.font_size, 18.0);
        assert_eq!(overridden.font_weight, FontWeight::Bold);
        assert_eq!(overridden.font_style, FontStyle::Normal);

        let aligned = TextStyle::plain()
            .with_align_x(TextAlignX::End)
            .with_align_y(TextAlignY::Bottom);
        assert_eq!(aligned.align_x, TextAlignX::End);
        assert_eq!(aligned.align_y, TextAlignY::Bottom);
        let centered = TextStyle::plain().centered();
        assert_eq!(centered.align_x, TextAlignX::Center);
        assert_eq!(centered.align_y, TextAlignY::Center);
        let unwrapped = TextStyle::plain().no_wrap();
        assert_eq!(unwrapped.wrap, TextWrap::None);
        assert_eq!(
            TextStyle::plain().with_wrap(TextWrap::Word),
            TextStyle::plain()
        );

        let declared = TextStyle {
            font_family: "Inter, system-ui".to_string(),
            font_size: 18.0,
            font_weight: FontWeight::Bold,
            font_style: FontStyle::Italic,
            decoration: TextDecoration {
                underline: true,
                strikethrough: true,
            },
            baseline: BaselineShift::Superscript,
            align_x: TextAlignX::Center,
            align_y: TextAlignY::Bottom,
            wrap: TextWrap::None,
        };
        let styled = PaintOp::styled_text(bounds, "styled", color, declared.clone());
        let PaintOp::Text { style, .. } = styled else {
            panic!("styled text constructor returns text paint op");
        };
        assert_eq!(style, declared);
    }

    impl SlipwayWidgetTypes for FakeWidget {
        type ExternalState = External;
        type LocalState = Local;
        type AppMessage = Message;
    }

    impl SlipwaySsot for FakeWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::PointerInput, Capability::StateObservation]
        }

        fn topology(&self, external: &Self::ExternalState) -> TopologyNode {
            TopologyNode {
                id: self.id.clone(),
                children: vec![TopologyNode::leaf(external.child.clone())],
                local_state_slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            vec![Diagnostic::unsupported(
                Some(self.id.clone()),
                "not-supported",
                "feature is not declared",
            )]
        }
    }

    impl SlipwayLogic for FakeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            local.count += 1;
            let observation = StateObservation {
                target: self.id.clone(),
                slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                name: "count".to_string(),
                value: local.count.to_string(),
            };
            let change = ChangeEvidence {
                target: self.id.clone(),
                slot: observation.slot.clone(),
                field: "count".to_string(),
                before: Some((local.count - 1).to_string()),
                after: Some(local.count.to_string()),
            };
            let message = EmittedMessage {
                target: self.id.clone(),
                name: "counted".to_string(),
                message: Message::Counted,
            };
            EventOutcome {
                handled: true,
                propagate: false,
                emitted_messages: vec![message],
                changes: vec![change.clone()],
                observations: vec![observation.clone()],
                probes: vec![ProbeProduct::Event(EventProbe {
                    routed_target: event.target().clone(),
                    event,
                    dispatch_evidence: None,
                    dispatch_identity: None,
                    result_identity: EventResultIdentity {
                        handled: Some(true),
                        emitted_messages: vec![EmittedMessageEvidence {
                            target: self.id.clone(),
                            name: "counted".to_string(),
                        }],
                        change_shapes: vec![ChangeShapeIdentity::from(&change)],
                        diagnostics: Vec::new(),
                    },
                    handled: Some(true),
                    revision_before: None,
                    revision_after: None,
                    emitted_messages: vec![EmittedMessageEvidence {
                        target: self.id.clone(),
                        name: "counted".to_string(),
                    }],
                    local_state: vec![observation],
                    changes: vec![change],
                    diagnostics: Vec::new(),
                })],
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayView for FakeWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            Local { count: 0 }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: LayoutOutputBuilder,
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
                target: self.id.clone(),
                slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                name: "count".to_string(),
                value: local.count.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for FakeWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = layout_view(self, external, local, input.layout_input);
            let address = Some(WidgetSlotAddress::new(self.id.clone(), 0));
            let scroll_content = Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: layout.bounds.size.width,
                    height: 1200.0,
                },
            };
            ViewDefinition {
                target: self.id.clone(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: PaintOrderDeclaration::source_order(self.id.clone()),
                hit_regions: vec![HitRegionDeclaration {
                    id: PresentationRegionId::from("root-hit"),
                    target: self.id.clone(),
                    address: address.clone(),
                    bounds: layout.bounds,
                    event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
                    order: HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    route: EventRoute {
                        route_id: Some("root-route".to_string()),
                        address: address.clone(),
                        path: vec![self.id.clone()],
                        phase: EventRoutePhase::Target,
                    },
                    cursor: CursorCapability::Pointer,
                    enabled: true,
                    capture: PointerCaptureIntent::None,
                    capture_evidence: Vec::new(),
                }],
                focus_regions: vec![FocusRegionDeclaration {
                    id: PresentationRegionId::from("root-focus"),
                    target: self.id.clone(),
                    address: address.clone(),
                    bounds: layout.bounds,
                    member: None,
                    enabled: true,
                    text_edit: Some(TextEditRegionDeclaration {
                        buffer: self.text_buffer(external, local),
                        selection: self.text_selection(external, local),
                        composition: self.ime_composition(external, local),
                        caret: self.caret_geometry(external, local, None),
                        visual_style: self.text_input_visual_style(external, local),
                        typography: self.text_input_typography(external, local),
                        edit_commands: self.text_edit_commands(external, local),
                        undo_redo: Some(self.text_undo_redo(external, local)),
                        viewport: None,
                        line_mode: TextLineMode::SingleLine,
                        diagnostics: Vec::new(),
                    }),
                }],
                scroll_regions: vec![ScrollRegionDeclaration {
                    id: PresentationRegionId::from("root-scroll"),
                    target: self.id.clone(),
                    address,
                    viewport: layout.bounds,
                    content_bounds: TargetLocalRect::new(scroll_content),
                    offset: Point { x: 0.0, y: 24.0 },
                    axes: ScrollAxes {
                        horizontal: false,
                        vertical: true,
                    },
                    wheel_routing: WheelRouting::SelfFirst,
                    indicator: ScrollIndicatorMode::Auto,
                    order: HitRegionOrder::default(),
                    virtual_viewport: None,
                    consumption: ScrollConsumptionPolicy {
                        wheel: true,
                        drag: false,
                        keyboard: true,
                        programmatic: true,
                    },
                    evidence: Vec::new(),
                    enabled: true,
                    diagnostics: Vec::new(),
                }],
                wheel_traversal_boundary: Default::default(),
                semantic_slots: vec![SemanticSlotDeclaration {
                    target: self.id.clone(),
                    node: SemanticNode {
                        id: self.id.clone(),
                        role: "test".to_string(),
                        label: Some("fake".to_string()),
                        value: None,
                        bounds: Some(layout.bounds.into_rect()),
                        states: Vec::new(),
                        actions: Vec::new(),
                        relationships: Vec::new(),
                    },
                }],
                probe_metadata: vec![ProbeMetadataDeclaration {
                    target: self.id.clone(),
                    name: "count".to_string(),
                    value: local.count.to_string(),
                }],
                diagnostics: self.unsupported(),
                layout,
            }
        }
    }

    fn assert_authored<W: SlipwayAuthoredWidget>(_widget: &W) {}

    fn assert_focus_traversal<W: SlipwayFocusTraversal>(_widget: &W) {}

    fn assert_layout_intent_contracts<W>(_widget: &W)
    where
        W: SlipwayIntrinsicSizing
            + SlipwaySizePolicy
            + SlipwayResizePolicy
            + SlipwayOverflowPolicy
            + SlipwayAutoLayoutPolicy
            + SlipwayResponsiveVariants
            + SlipwayTextFlowPolicy
            + SlipwayTextMeasurementPolicy
            + SlipwayTextMeasurementCachePolicy
            + SlipwayCachedTextMeasurementPolicy
            + SlipwayFitOverflowEvidence
            + SlipwayLayerPolicy
            + SlipwayScrollPolicy
            + SlipwayCollectionPolicy
            + SlipwayInteractionStateStyle,
    {
    }

    fn assert_layout_intent_aggregate<W: SlipwayLayoutIntent>(_widget: &W) {}

    fn assert_view_definition<W: SlipwayViewDefinition>(_widget: &W) {}

    fn assert_text_input_capability<W: SlipwayTextInputCapability>(_widget: &W) {}

    fn assert_scrollable_container_capability<W: SlipwayScrollableContainerCapability>(
        _widget: &W,
    ) {
    }

    fn assert_popup_capability<W: SlipwayPopupCapability>(_widget: &W) {}

    fn assert_provider_surface_capability<W: SlipwayProviderSurfaceCapability>(_widget: &W) {}

    fn assert_command_surface_capability<W: SlipwayCommandSurfaceCapability>(_widget: &W) {}

    fn assert_deterministic_source_capability<W: SlipwayDeterministicSourceCapability>(
        _widget: &W,
    ) {
    }

    fn assert_backend_admission_capability<W: SlipwayBackendAdmissionCapability>(_backend: &W) {}

    fn frame_identity() -> FrameIdentity {
        FrameIdentity {
            surface_id: "surface".to_string(),
            surface_instance_id: "instance".to_string(),
            revision: 3,
            frame_index: 7,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 320.0,
                    height: 240.0,
                },
            },
        }
    }

    #[test]
    fn declared_pointer_dispatch_uses_route_slot_coordinates_and_capture() {
        let root = WidgetId::from("root");
        let child = WidgetId::from("child");
        let child_slot = WidgetSlotAddress::new(root.clone(), 0).child(child.clone(), 0);
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 100.0,
                },
            }),
            child_placements: vec![ChildPlacement {
                child: child.clone(),
                bounds: ParentLocalRect::from_parent_local(Rect {
                    origin: Point { x: 20.0, y: 10.0 },
                    size: Size {
                        width: 50.0,
                        height: 40.0,
                    },
                }),
                local_state_slot: Some(child_slot.clone()),
                spacing: BoxSpacing::ZERO,
            }],
            diagnostics: Vec::new(),
        };
        let region = HitRegionDeclaration {
            id: PresentationRegionId::from("button-hit"),
            target: child.clone(),
            address: Some(child_slot.clone()),
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 50.0,
                    height: 40.0,
                },
            }),
            event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
            order: HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
            route: EventRoute {
                route_id: Some("activate".to_string()),
                address: Some(child_slot.clone()),
                path: vec![root, child.clone()],
                phase: EventRoutePhase::Target,
            },
            cursor: CursorCapability::Pointer,
            enabled: true,
            capture: PointerCaptureIntent::OnPress,
            capture_evidence: Vec::new(),
        };

        let dispatch = resolve_declared_pointer_dispatch(
            &layout,
            &[region],
            Point { x: 25.0, y: 15.0 },
            PointerEventKind::Press,
            Some(PointerButton::Primary),
            PointerDetails::default(),
            true,
        )
        .expect("declared hit dispatch");

        assert_eq!(
            dispatch.selected_region,
            PresentationRegionId::from("button-hit")
        );
        assert_eq!(
            dispatch.candidate_regions,
            vec![PresentationRegionId::from("button-hit")]
        );
        assert!(dispatch.capture_event);
        let InputEvent::Pointer(pointer) = dispatch.input else {
            panic!("expected pointer input");
        };
        assert_eq!(pointer.target, child);
        assert_eq!(pointer.target_slot, Some(child_slot));
        assert_eq!(pointer.position, Point { x: 5.0, y: 5.0 });
        assert_eq!(
            pointer.target_bounds,
            Some(TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 50.0,
                    height: 40.0,
                },
            }))
        );
    }

    #[test]
    fn declared_target_rect_prefers_duplicate_child_exact_slot_address() {
        let root = WidgetId::from("root");
        let child = WidgetId::from("child");
        let first_slot = WidgetSlotAddress::new(root.clone(), 0).child(child.clone(), 0);
        let second_slot = WidgetSlotAddress::new(root, 0).child(child.clone(), 1);
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 240.0,
                    height: 120.0,
                },
            }),
            child_placements: vec![
                ChildPlacement {
                    child: child.clone(),
                    bounds: ParentLocalRect::from_parent_local(Rect {
                        origin: Point { x: 10.0, y: 5.0 },
                        size: Size {
                            width: 40.0,
                            height: 30.0,
                        },
                    }),
                    local_state_slot: Some(first_slot),
                    spacing: BoxSpacing::ZERO,
                },
                ChildPlacement {
                    child: child.clone(),
                    bounds: ParentLocalRect::from_parent_local(Rect {
                        origin: Point { x: 80.0, y: 45.0 },
                        size: Size {
                            width: 70.0,
                            height: 50.0,
                        },
                    }),
                    local_state_slot: Some(second_slot.clone()),
                    spacing: BoxSpacing::ZERO,
                },
            ],
            diagnostics: Vec::new(),
        };
        let region_address = Some(second_slot);
        let region = HitRegionDeclaration {
            id: PresentationRegionId::from("second-child-hit"),
            target: child.clone(),
            address: region_address.clone(),
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 70.0,
                    height: 50.0,
                },
            }),
            event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
            order: HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
            route: EventRoute {
                route_id: Some("second".to_string()),
                address: region_address,
                path: vec![child.clone()],
                phase: EventRoutePhase::Target,
            },
            cursor: CursorCapability::Pointer,
            enabled: true,
            capture: PointerCaptureIntent::None,
            capture_evidence: Vec::new(),
        };

        let resolved = declared_target_rect_for_region_address(
            &layout,
            &region.target,
            region.address.as_ref(),
        );

        assert_eq!(
            resolved,
            Rect {
                origin: Point { x: 80.0, y: 45.0 },
                size: Size {
                    width: 70.0,
                    height: 50.0,
                },
            }
        );
    }

    #[test]
    fn geometry_index_resolves_duplicate_child_exact_slot_address() {
        let root = WidgetId::from("root");
        let child = WidgetId::from("child");
        let first_slot = WidgetSlotAddress::new(root.clone(), 0).child(child.clone(), 0);
        let second_slot = WidgetSlotAddress::new(root, 0).child(child.clone(), 1);
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 300.0,
                    height: 160.0,
                },
            }),
            child_placements: vec![
                ChildPlacement {
                    child: child.clone(),
                    bounds: ParentLocalRect::from_parent_local(Rect {
                        origin: Point { x: 12.0, y: 6.0 },
                        size: Size {
                            width: 40.0,
                            height: 30.0,
                        },
                    }),
                    local_state_slot: Some(first_slot),
                    spacing: BoxSpacing::ZERO,
                },
                ChildPlacement {
                    child: child.clone(),
                    bounds: ParentLocalRect::from_parent_local(Rect {
                        origin: Point { x: 120.0, y: 70.0 },
                        size: Size {
                            width: 80.0,
                            height: 44.0,
                        },
                    }),
                    local_state_slot: Some(second_slot.clone()),
                    spacing: BoxSpacing::ZERO,
                },
            ],
            diagnostics: Vec::new(),
        };
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);

        assert_eq!(
            geometry_index.target_rect_for_region_address(&child, Some(&second_slot)),
            Rect {
                origin: Point { x: 120.0, y: 70.0 },
                size: Size {
                    width: 80.0,
                    height: 44.0,
                },
            }
        );
        assert_eq!(
            geometry_index.target_local_point_for_region_address(
                &child,
                Some(&second_slot),
                Point { x: 125.0, y: 75.0 },
            ),
            Point { x: 5.0, y: 5.0 }
        );
    }

    #[test]
    fn indexed_pointer_dispatch_uses_mounted_child_geometry() {
        let root = WidgetId::from("root");
        let child = WidgetId::from("child");
        let slot = WidgetSlotAddress::new(root, 0).child(child.clone(), 0);
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 240.0,
                    height: 120.0,
                },
            }),
            child_placements: vec![ChildPlacement {
                child: child.clone(),
                bounds: ParentLocalRect::from_parent_local(Rect {
                    origin: Point { x: 60.0, y: 40.0 },
                    size: Size {
                        width: 90.0,
                        height: 32.0,
                    },
                }),
                local_state_slot: Some(slot.clone()),
                spacing: BoxSpacing::ZERO,
            }],
            diagnostics: Vec::new(),
        };
        let region = HitRegionDeclaration {
            id: PresentationRegionId::from("child-hit"),
            target: child.clone(),
            address: Some(slot.clone()),
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 10.0, y: 4.0 },
                size: Size {
                    width: 40.0,
                    height: 20.0,
                },
            }),
            event_coordinate_space: PointerEventCoordinateSpace::RegionLocal,
            order: HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
            route: EventRoute {
                route_id: Some("child-hit".to_string()),
                address: Some(slot.clone()),
                path: vec![child.clone()],
                phase: EventRoutePhase::Target,
            },
            cursor: CursorCapability::Pointer,
            enabled: true,
            capture: PointerCaptureIntent::None,
            capture_evidence: Vec::new(),
        };
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);

        let dispatch = resolve_declared_pointer_dispatch_with_geometry_index(
            &geometry_index,
            &[region],
            Point { x: 75.0, y: 49.0 },
            PointerEventKind::Press,
            Some(PointerButton::Primary),
            PointerDetails::default(),
            false,
        )
        .expect("indexed dispatch hits mounted child region");

        let InputEvent::Pointer(pointer) = dispatch.input else {
            panic!("expected pointer input");
        };
        assert_eq!(pointer.target, child);
        assert_eq!(pointer.target_slot, Some(slot));
        assert_eq!(pointer.position, Point { x: 5.0, y: 5.0 });
        assert_eq!(
            pointer.target_bounds,
            Some(TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 90.0,
                    height: 32.0,
                },
            }))
        );
    }

    #[test]
    fn declared_wheel_dispatch_selects_topmost_enabled_scroll_region() {
        let target = WidgetId::from("scroll");
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            }),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let scroll_a = ScrollRegionDeclaration {
            id: PresentationRegionId::from("outer"),
            target: target.clone(),
            address: None,
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            }),
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 300.0,
                },
            }),
            offset: Point { x: 0.0, y: 0.0 },
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            wheel_routing: WheelRouting::NearestScrollable,
            indicator: ScrollIndicatorMode::Auto,
            order: HitRegionOrder::default(),
            virtual_viewport: None,
            consumption: ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
            evidence: Vec::new(),
            enabled: true,
            diagnostics: Vec::new(),
        };
        let mut scroll_b = scroll_a.clone();
        scroll_b.id = PresentationRegionId::from("inner");
        scroll_b.order = HitRegionOrder {
            z_index: 1,
            paint_order: 0,
            traversal_order: 0,
        };

        let dispatch = resolve_declared_wheel_dispatch(
            &layout,
            &[scroll_a, scroll_b],
            Point { x: 20.0, y: 20.0 },
            0.0,
            -4.0,
        )
        .expect("declared wheel dispatch");

        assert_eq!(
            dispatch.selected_region,
            PresentationRegionId::from("inner")
        );
        assert_eq!(
            dispatch
                .route
                .route_id
                .as_deref()
                .expect("wheel dispatch records route id"),
            "inner"
        );
        assert!(dispatch.capture_event);
        let InputEvent::Wheel(wheel) = dispatch.input else {
            panic!("expected wheel input");
        };
        assert_eq!(wheel.target, target);
        assert_eq!(wheel.region_id, Some(PresentationRegionId::from("inner")));
        assert_eq!(wheel.delta_y, -4.0);
    }

    #[test]
    fn declared_wheel_dispatch_skips_scroll_region_at_directional_boundary() {
        let target = WidgetId::from("scroll");
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            }),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let outer = ScrollRegionDeclaration {
            id: PresentationRegionId::from("outer"),
            target: target.clone(),
            address: None,
            viewport: layout.bounds,
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 300.0,
                },
            }),
            offset: Point { x: 0.0, y: 0.0 },
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            wheel_routing: WheelRouting::NearestScrollable,
            indicator: ScrollIndicatorMode::Auto,
            order: HitRegionOrder::default(),
            virtual_viewport: None,
            consumption: ScrollConsumptionPolicy::exclusive_wheel(),
            evidence: Vec::new(),
            enabled: true,
            diagnostics: Vec::new(),
        };
        let mut inner = outer.clone();
        inner.id = PresentationRegionId::from("inner");
        inner.order = HitRegionOrder {
            z_index: 1,
            paint_order: 0,
            traversal_order: 0,
        };

        let mut movable_inner = inner.clone();
        movable_inner.offset.y = 100.0;
        let dispatch = resolve_declared_wheel_dispatch(
            &layout,
            &[outer.clone(), movable_inner],
            Point { x: 20.0, y: 20.0 },
            0.0,
            -4.0,
        )
        .expect("movable top scroll consumes");
        assert_eq!(
            dispatch.selected_region,
            PresentationRegionId::from("inner")
        );

        let mut bottom_inner = inner.clone();
        bottom_inner.offset.y = 200.0;
        let dispatch = resolve_declared_wheel_dispatch(
            &layout,
            &[outer.clone(), bottom_inner],
            Point { x: 20.0, y: 20.0 },
            0.0,
            -4.0,
        )
        .expect("boundary inner bubbles to outer scroll owner");
        assert_eq!(
            dispatch.selected_region,
            PresentationRegionId::from("outer")
        );

        let mut bottom_outer = outer;
        bottom_outer.offset.y = 200.0;
        let mut bottom_inner = inner;
        bottom_inner.offset.y = 200.0;
        assert!(
            resolve_declared_wheel_dispatch(
                &layout,
                &[bottom_outer, bottom_inner],
                Point { x: 20.0, y: 20.0 },
                0.0,
                -4.0,
            )
            .is_none(),
            "wheel has no owner when every containing scroll region is at the boundary"
        );
    }

    fn routed_wheel_test_layout() -> LayoutOutput {
        LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 200.0,
                },
            }),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn routed_wheel_test_scroll_region(
        id: &str,
        viewport: Rect,
        content_height: f32,
        offset_y: f32,
        z_index: i32,
        wheel_routing: WheelRouting,
    ) -> ScrollRegionDeclaration {
        ScrollRegionDeclaration::explicit(
            PresentationRegionId::from(id),
            WidgetId::from("scroll"),
            None,
            TargetLocalRect::new(viewport),
            TargetLocalRect::new(Rect {
                origin: viewport.origin,
                size: Size {
                    width: viewport.size.width,
                    height: content_height,
                },
            }),
            Point {
                x: 0.0,
                y: offset_y,
            },
            ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            wheel_routing,
            HitRegionOrder {
                z_index,
                paint_order: 0,
                traversal_order: 0,
            },
            ScrollConsumptionPolicy::exclusive_wheel(),
            true,
        )
    }

    fn routed_wheel_outer_viewport() -> Rect {
        Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 200.0,
                height: 200.0,
            },
        }
    }

    fn routed_wheel_inner_viewport() -> Rect {
        Rect {
            origin: Point { x: 20.0, y: 20.0 },
            size: Size {
                width: 100.0,
                height: 100.0,
            },
        }
    }

    /// The pre-routing selection algorithm, encoded literally: front-most
    /// containing consumable region by the order key. New routing tests
    /// compare against this to prove default-equivalence (identical output)
    /// and to document exactly where `SelfFirst`/`ParentFirst` diverge.
    fn pre_routing_wheel_winner<'a>(
        geometry_index: &PresentationGeometryIndex,
        scroll_regions: &'a [ScrollRegionDeclaration],
        root_local_position: Point,
        delta_x: f32,
        delta_y: f32,
    ) -> Option<&'a ScrollRegionDeclaration> {
        scroll_regions
            .iter()
            .filter(|region| {
                scroll_region_can_consume_wheel_delta(region, delta_x, delta_y) == Some(true)
                    && declared_region_contains_root_local_point_with_geometry_index(
                        geometry_index,
                        &region.target,
                        region.address.as_ref(),
                        region.viewport.into_rect(),
                        root_local_position,
                    )
            })
            .max_by(|a, b| compare_hit_region_order(&a.order, &b.order))
    }

    #[test]
    fn mounted_terminal_boundary_uses_index_not_address() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let mut outer = routed_wheel_test_scroll_region(
            "outer",
            routed_wheel_outer_viewport(),
            400.0,
            200.0,
            -1,
            WheelRouting::NearestScrollable,
        );
        let mut inner = routed_wheel_test_scroll_region(
            "inner",
            routed_wheel_inner_viewport(),
            300.0,
            200.0,
            1,
            WheelRouting::NearestScrollable,
        );
        inner.address = Some(WidgetSlotAddress::new(inner.target.clone(), 0));
        let point = Point { x: 50.0, y: 50.0 };
        outer.address = Some(WidgetSlotAddress::new(outer.target.clone(), 0));

        outer.offset.y = 0.0;
        let regions = [outer.clone(), inner.clone()];
        assert!(matches!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &regions,
                DeclaredWheelTraversalBoundary { terminal_region_index: Some(0) },
                point,
                0.0,
                -48.0,
            ),
            DeclaredWheelDisposition::Moved(region) if region.id == outer.id
        ));

        outer.offset.y = 200.0;
        let regions = [outer.clone(), inner];
        assert!(matches!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &regions,
                DeclaredWheelTraversalBoundary { terminal_region_index: Some(0) },
                point,
                0.0,
                -48.0,
            ),
            DeclaredWheelDisposition::ConsumedNoOp(region) if region.id == outer.id
        ));
        assert_eq!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &regions,
                DeclaredWheelTraversalBoundary {
                    terminal_region_index: Some(0)
                },
                Point { x: 500.0, y: 500.0 },
                0.0,
                -48.0,
            ),
            DeclaredWheelDisposition::Bubble
        );

        let nested_only = regions[1].clone();
        assert_eq!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &[nested_only],
                DeclaredWheelTraversalBoundary::default(),
                point,
                0.0,
                -48.0,
            ),
            DeclaredWheelDisposition::Bubble,
            "an address-bearing nested declaration is never a terminal root"
        );
    }

    #[test]
    fn terminal_boundary_diagonal_preserves_movable_axis() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let mut root = routed_wheel_test_scroll_region(
            "root",
            routed_wheel_outer_viewport(),
            300.0,
            0.0,
            0,
            WheelRouting::NearestScrollable,
        );
        root.axes.horizontal = true;
        root.content_bounds = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 300.0,
                height: 300.0,
            },
        });
        root.offset.x = 100.0;
        let boundary = DeclaredWheelTraversalBoundary {
            terminal_region_index: Some(0),
        };
        assert!(matches!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &[root.clone()],
                boundary,
                Point { x: 50.0, y: 50.0 },
                -48.0,
                -48.0,
            ),
            DeclaredWheelDisposition::Moved(_)
        ));
        root.offset.y = 100.0;
        assert!(matches!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &[root.clone()],
                boundary,
                Point { x: 50.0, y: 50.0 },
                -48.0,
                -48.0,
            ),
            DeclaredWheelDisposition::ConsumedNoOp(_)
        ));
        assert!(matches!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &[root],
                boundary,
                Point { x: 50.0, y: 50.0 },
                0.0,
                48.0,
            ),
            DeclaredWheelDisposition::Moved(_)
        ));
    }

    #[test]
    fn terminal_boundary_rejects_stale_index() {
        let mut view = dashboard_card_column_view();
        view.wheel_traversal_boundary.terminal_region_index = Some(view.scroll_regions.len());
        assert!(
            view_definition_contract_diagnostics(&view)
                .iter()
                .any(|diagnostic| {
                    diagnostic.code == "view_contract.wheel_traversal_boundary_invalid"
                        && diagnostic.severity == DiagnosticSeverity::Error
                })
        );

        let mut disabled = routed_wheel_test_scroll_region(
            "disabled",
            routed_wheel_outer_viewport(),
            400.0,
            0.0,
            0,
            WheelRouting::NearestScrollable,
        );
        disabled.enabled = false;
        view.scroll_regions = vec![disabled];
        view.wheel_traversal_boundary.terminal_region_index = Some(0);
        assert!(
            view_definition_contract_diagnostics(&view)
                .iter()
                .any(|diagnostic| {
                    diagnostic.code == "view_contract.wheel_traversal_boundary_invalid"
                        && diagnostic.severity == DiagnosticSeverity::Error
                })
        );
    }

    #[test]
    fn terminal_root_disposition_scales_through_sixty_four_declarations() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let mut regions = Vec::with_capacity(64);
        for index in 0..63 {
            let mut nested = routed_wheel_test_scroll_region(
                &format!("nested-{index}"),
                routed_wheel_inner_viewport(),
                300.0,
                200.0,
                index,
                WheelRouting::NearestScrollable,
            );
            nested.address = Some(WidgetSlotAddress::new(
                nested.target.clone(),
                index as usize,
            ));
            regions.push(nested);
        }
        let root = routed_wheel_test_scroll_region(
            "root",
            routed_wheel_outer_viewport(),
            400.0,
            200.0,
            -1,
            WheelRouting::NearestScrollable,
        );
        regions.push(root.clone());

        assert!(matches!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &regions,
                DeclaredWheelTraversalBoundary { terminal_region_index: Some(63) },
                Point { x: 50.0, y: 50.0 },
                0.0,
                -48.0,
            ),
            DeclaredWheelDisposition::ConsumedNoOp(owner) if owner.id == root.id
        ));
        regions.pop();
        assert_eq!(
            declared_wheel_disposition_at_root_local_point_with_geometry_index(
                &geometry_index,
                &regions,
                DeclaredWheelTraversalBoundary::default(),
                Point { x: 50.0, y: 50.0 },
                0.0,
                -48.0,
            ),
            DeclaredWheelDisposition::Bubble
        );
    }

    #[test]
    fn wheel_routing_nearest_scrollable_only_matches_pre_routing_selection() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let regions = vec![
            routed_wheel_test_scroll_region(
                "outer",
                routed_wheel_outer_viewport(),
                400.0,
                0.0,
                -1,
                WheelRouting::NearestScrollable,
            ),
            routed_wheel_test_scroll_region(
                "inner",
                routed_wheel_inner_viewport(),
                300.0,
                0.0,
                1,
                WheelRouting::NearestScrollable,
            ),
        ];

        for (point, delta_y) in [
            (Point { x: 50.0, y: 50.0 }, -4.0),
            (Point { x: 50.0, y: 50.0 }, 4.0),
            (Point { x: 150.0, y: 150.0 }, -4.0),
            (Point { x: 10.0, y: 10.0 }, -4.0),
            (Point { x: 500.0, y: 500.0 }, -4.0),
        ] {
            let selected = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
                &geometry_index,
                &regions,
                point,
                0.0,
                delta_y,
            );
            let expected = pre_routing_wheel_winner(&geometry_index, &regions, point, 0.0, delta_y);
            assert_eq!(
                selected.map(|region| &region.id),
                expected.map(|region| &region.id),
                "NearestScrollable-only selection must be identical to the pre-routing algorithm at {point:?} delta_y={delta_y}"
            );
        }

        // Literal expectations of the encoded pre-routing behavior.
        let inner = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
            &geometry_index,
            &regions,
            Point { x: 50.0, y: 50.0 },
            0.0,
            -4.0,
        )
        .expect("nested point selects front-most consumable region");
        assert_eq!(inner.id, PresentationRegionId::from("inner"));
        let outer = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
            &geometry_index,
            &regions,
            Point { x: 150.0, y: 150.0 },
            0.0,
            -4.0,
        )
        .expect("point outside the inner viewport selects the outer region");
        assert_eq!(outer.id, PresentationRegionId::from("outer"));
    }

    #[test]
    fn wheel_routing_self_first_back_region_wins_over_fronter_order() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let regions = vec![
            routed_wheel_test_scroll_region(
                "outer",
                routed_wheel_outer_viewport(),
                400.0,
                0.0,
                -1,
                WheelRouting::SelfFirst,
            ),
            routed_wheel_test_scroll_region(
                "inner",
                routed_wheel_inner_viewport(),
                300.0,
                0.0,
                1,
                WheelRouting::NearestScrollable,
            ),
        ];
        let point = Point { x: 50.0, y: 50.0 };

        // The pre-routing algorithm would pick the fronter inner region.
        let pre_routing = pre_routing_wheel_winner(&geometry_index, &regions, point, 0.0, -4.0)
            .expect("both regions are containing and consumable");
        assert_eq!(pre_routing.id, PresentationRegionId::from("inner"));

        // SelfFirst on the back outer region overrides the order key.
        let selected = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
            &geometry_index,
            &regions,
            point,
            0.0,
            -4.0,
        )
        .expect("SelfFirst region consumes");
        assert_eq!(selected.id, PresentationRegionId::from("outer"));
    }

    #[test]
    fn wheel_routing_self_first_at_limit_falls_back_to_default_selection() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        // The SelfFirst outer region sits at its downward consumption limit
        // (offset == content - viewport), so it cannot consume delta_y < 0.
        let regions = vec![
            routed_wheel_test_scroll_region(
                "outer",
                routed_wheel_outer_viewport(),
                400.0,
                200.0,
                -1,
                WheelRouting::SelfFirst,
            ),
            routed_wheel_test_scroll_region(
                "inner",
                routed_wheel_inner_viewport(),
                300.0,
                0.0,
                1,
                WheelRouting::NearestScrollable,
            ),
        ];

        let selected = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
            &geometry_index,
            &regions,
            Point { x: 50.0, y: 50.0 },
            0.0,
            -4.0,
        )
        .expect("an at-limit SelfFirst declaration must not black-hole the wheel");
        assert_eq!(selected.id, PresentationRegionId::from("inner"));
    }

    #[test]
    fn wheel_routing_parent_first_defers_to_containing_ancestor() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let regions = vec![
            routed_wheel_test_scroll_region(
                "outer",
                routed_wheel_outer_viewport(),
                400.0,
                0.0,
                -1,
                WheelRouting::NearestScrollable,
            ),
            routed_wheel_test_scroll_region(
                "inner",
                routed_wheel_inner_viewport(),
                300.0,
                0.0,
                1,
                WheelRouting::ParentFirst,
            ),
        ];
        let point = Point { x: 50.0, y: 50.0 };

        // The pre-routing algorithm would pick the fronter inner region.
        let pre_routing = pre_routing_wheel_winner(&geometry_index, &regions, point, 0.0, -4.0)
            .expect("both regions are containing and consumable");
        assert_eq!(pre_routing.id, PresentationRegionId::from("inner"));

        // ParentFirst defers to the eligible strictly-containing ancestor.
        let selected = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
            &geometry_index,
            &regions,
            point,
            0.0,
            -4.0,
        )
        .expect("the deferred-to ancestor consumes");
        assert_eq!(selected.id, PresentationRegionId::from("outer"));
    }

    #[test]
    fn wheel_routing_parent_first_without_eligible_ancestor_selects_itself() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let point = Point { x: 50.0, y: 50.0 };

        // Ancestor at its consumption limit: not eligible, so the
        // ParentFirst region is selected itself.
        let regions = vec![
            routed_wheel_test_scroll_region(
                "outer",
                routed_wheel_outer_viewport(),
                400.0,
                200.0,
                -1,
                WheelRouting::NearestScrollable,
            ),
            routed_wheel_test_scroll_region(
                "inner",
                routed_wheel_inner_viewport(),
                300.0,
                0.0,
                1,
                WheelRouting::ParentFirst,
            ),
        ];
        let selected = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
            &geometry_index,
            &regions,
            point,
            0.0,
            -4.0,
        )
        .expect("ParentFirst with an at-limit ancestor keeps the wheel");
        assert_eq!(selected.id, PresentationRegionId::from("inner"));

        // No ancestor at all: the ParentFirst region is selected itself.
        let alone = vec![routed_wheel_test_scroll_region(
            "inner",
            routed_wheel_inner_viewport(),
            300.0,
            0.0,
            1,
            WheelRouting::ParentFirst,
        )];
        let selected = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
            &geometry_index,
            &alone,
            point,
            0.0,
            -4.0,
        )
        .expect("ParentFirst without any ancestor keeps the wheel");
        assert_eq!(selected.id, PresentationRegionId::from("inner"));
    }

    #[test]
    fn wheel_routing_overlapping_self_first_regions_select_front_most() {
        let layout = routed_wheel_test_layout();
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let back = routed_wheel_test_scroll_region(
            "self-back",
            Rect {
                origin: Point { x: 20.0, y: 20.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            },
            300.0,
            0.0,
            1,
            WheelRouting::SelfFirst,
        );
        let front = routed_wheel_test_scroll_region(
            "self-front",
            Rect {
                origin: Point { x: 40.0, y: 40.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            },
            300.0,
            0.0,
            2,
            WheelRouting::SelfFirst,
        );
        let point = Point { x: 60.0, y: 60.0 };

        // Two overlapping SelfFirst candidates: the front-most by the
        // existing order key wins, independent of declaration order.
        for regions in [
            vec![back.clone(), front.clone()],
            vec![front.clone(), back.clone()],
        ] {
            let selected = select_declared_wheel_consumer_at_root_local_point_with_geometry_index(
                &geometry_index,
                &regions,
                point,
                0.0,
                -4.0,
            )
            .expect("overlapping SelfFirst candidates still select deterministically");
            assert_eq!(selected.id, PresentationRegionId::from("self-front"));
        }
    }

    #[test]
    fn overlapping_wheel_consumers_with_same_order_are_contract_errors() {
        let target = WidgetId::from("scroll");
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 100.0,
                },
            }),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let scroll = ScrollRegionDeclaration {
            id: PresentationRegionId::from("outer"),
            target: target.clone(),
            address: None,
            viewport: layout.bounds,
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 300.0,
                },
            }),
            offset: Point { x: 0.0, y: 0.0 },
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            wheel_routing: WheelRouting::NearestScrollable,
            indicator: ScrollIndicatorMode::Auto,
            order: HitRegionOrder::default(),
            virtual_viewport: None,
            consumption: ScrollConsumptionPolicy::exclusive_wheel(),
            evidence: Vec::new(),
            enabled: true,
            diagnostics: Vec::new(),
        };
        let mut inner = scroll.clone();
        inner.id = PresentationRegionId::from("inner");

        let view = ViewDefinition {
            target: target.clone(),
            frame: frame_identity(),
            layout,
            paint: Vec::new(),
            paint_order: PaintOrderDeclaration::source_order(target),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: vec![scroll, inner],
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let diagnostics = view_definition_contract_diagnostics(&view);

        let overlap = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "view_contract.ambiguous_wheel_overlap")
            .expect("identically ordered overlapping wheel consumers are refused");
        // The refusal names both offending regions and the fix API (LE-M17).
        assert!(overlap.message.contains("`outer`"), "{}", overlap.message);
        assert!(overlap.message.contains("`inner`"), "{}", overlap.message);
        assert!(
            overlap
                .message
                .contains("scroll_region_from_scrollable_capability_with_order"),
            "{}",
            overlap.message
        );
    }

    #[test]
    fn declared_wheel_dispatch_resolves_child_target_local_viewport() {
        let root = WidgetId::from("root");
        let child = WidgetId::from("child-scroll");
        let child_slot = WidgetSlotAddress::new(root, 0).child(child.clone(), 0);
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 160.0,
                },
            }),
            child_placements: vec![ChildPlacement {
                child: child.clone(),
                bounds: ParentLocalRect::from_parent_local(Rect {
                    origin: Point { x: 60.0, y: 30.0 },
                    size: Size {
                        width: 80.0,
                        height: 50.0,
                    },
                }),
                local_state_slot: Some(child_slot.clone()),
                spacing: BoxSpacing::ZERO,
            }],
            diagnostics: Vec::new(),
        };
        let region = ScrollRegionDeclaration {
            id: PresentationRegionId::from("child-scroll-region"),
            target: child.clone(),
            address: Some(child_slot.clone()),
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 50.0,
                },
            }),
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 80.0,
                    height: 200.0,
                },
            }),
            offset: Point { x: 0.0, y: 0.0 },
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            wheel_routing: WheelRouting::NearestScrollable,
            indicator: ScrollIndicatorMode::Auto,
            order: HitRegionOrder::default(),
            virtual_viewport: None,
            consumption: ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
            evidence: Vec::new(),
            enabled: true,
            diagnostics: Vec::new(),
        };

        let (dispatch, evidence) = resolve_declared_wheel_dispatch_with_evidence(
            EvidenceSource::backend_presented("test-backend", "wheel"),
            frame_identity(),
            &layout,
            std::slice::from_ref(&region),
            Point { x: 65.0, y: 35.0 },
            0.0,
            -12.0,
        );
        let dispatch = dispatch.expect("child target-local wheel dispatch");

        assert_eq!(dispatch.selected_region, region.id.clone());
        assert_eq!(dispatch.candidate_regions, vec![region.id.clone()]);
        assert_eq!(evidence.input_position, Some(Point { x: 65.0, y: 35.0 }));
        assert_eq!(
            evidence.input_position_space,
            Some(DispatchPositionSpace::Content),
            "core wheel resolution records the content-space resolve point"
        );
        assert_eq!(evidence.selected_region, Some(region.id.clone()));
        let InputEvent::Wheel(wheel) = dispatch.input else {
            panic!("expected wheel input");
        };
        assert_eq!(wheel.target, child);
        assert_eq!(wheel.target_slot, Some(child_slot));
        assert_eq!(
            wheel.region_id,
            Some(PresentationRegionId::from("child-scroll-region"))
        );
        assert_eq!(wheel.delta_y, -12.0);
    }

    #[test]
    fn declared_scroll_dispatch_evidence_records_selected_region_and_route() {
        let target = WidgetId::from("scroll");
        let region = ScrollRegionDeclaration {
            id: PresentationRegionId::from("main-scroll"),
            target: target.clone(),
            address: Some(WidgetSlotAddress::new(target.clone(), 0)),
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 80.0,
                },
            }),
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 240.0,
                },
            }),
            offset: Point { x: 0.0, y: 0.0 },
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            wheel_routing: WheelRouting::NearestScrollable,
            indicator: ScrollIndicatorMode::Auto,
            order: HitRegionOrder::default(),
            virtual_viewport: None,
            consumption: ScrollConsumptionPolicy {
                wheel: true,
                drag: true,
                keyboard: true,
                programmatic: false,
            },
            evidence: Vec::new(),
            enabled: true,
            diagnostics: Vec::new(),
        };
        let event = InputEvent::Scroll(ScrollEvent {
            target: target.clone(),
            target_slot: region.address.clone(),
            region_id: region.id.clone(),
            offset_x: 0.0,
            offset_y: 42.0,
            viewport: region.viewport,
            content_bounds: region.content_bounds,
        });

        let evidence = declared_scroll_dispatch_evidence(
            EvidenceSource::backend_presented("test-backend", "native-scroll"),
            frame_identity(),
            std::slice::from_ref(&region),
            Some(&region),
            event.clone(),
        );

        assert_eq!(evidence.kind, DeclaredEventDispatchKind::Scroll);
        assert_eq!(evidence.selected_region, Some(region.id.clone()));
        assert_eq!(evidence.candidate_regions, vec![region.id.clone()]);
        assert_eq!(evidence.generated_event, Some(event.clone()));
        assert!(evidence.capture_event);
        assert_eq!(
            evidence
                .route
                .as_ref()
                .and_then(|route| route.route_id.as_deref()),
            Some("main-scroll")
        );
        assert_eq!(evidence.input_position, None);
        assert_eq!(
            evidence.input_position_space, None,
            "positionless evidence carries no space annotation"
        );

        let anchored = declared_scroll_dispatch_evidence_at_position(
            EvidenceSource::backend_presented("test-backend", "native-scroll"),
            frame_identity(),
            std::slice::from_ref(&region),
            Some(&region),
            event,
            Some(Point { x: 7.0, y: 11.0 }),
        );
        assert_eq!(anchored.input_position, Some(Point { x: 7.0, y: 11.0 }));
        assert_eq!(
            anchored.input_position_space,
            Some(DispatchPositionSpace::Content),
            "the scroll-sync anchor is recorded in content space"
        );
    }

    #[test]
    fn declared_wheel_refusal_evidence_carries_position_space_and_candidates() {
        let root = WidgetId::from("root");
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 160.0,
                },
            }),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let region = ScrollRegionDeclaration {
            id: PresentationRegionId::from("root-scroll"),
            target: root.clone(),
            address: Some(WidgetSlotAddress::new(root, 0)),
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 160.0,
                },
            }),
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 400.0,
                },
            }),
            offset: Point { x: 0.0, y: 0.0 },
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            wheel_routing: WheelRouting::NearestScrollable,
            indicator: ScrollIndicatorMode::Auto,
            order: HitRegionOrder::default(),
            virtual_viewport: None,
            consumption: ScrollConsumptionPolicy {
                wheel: true,
                drag: false,
                keyboard: false,
                programmatic: false,
            },
            evidence: Vec::new(),
            enabled: true,
            diagnostics: Vec::new(),
        };

        // Wheel UP at offset 0: the only containing region cannot consume the
        // delta, so dispatch resolves to None and the refusal evidence must
        // carry the position (with space), the candidates, and the reason.
        let (dispatch, evidence) = resolve_declared_wheel_dispatch_with_evidence(
            EvidenceSource::backend_presented("test-backend", "wheel"),
            frame_identity(),
            &layout,
            std::slice::from_ref(&region),
            Point { x: 20.0, y: 20.0 },
            0.0,
            12.0,
        );

        assert!(dispatch.is_none(), "wheel up at offset 0 has no consumer");
        assert_eq!(evidence.input_position, Some(Point { x: 20.0, y: 20.0 }));
        assert_eq!(
            evidence.input_position_space,
            Some(DispatchPositionSpace::Content)
        );
        assert_eq!(
            evidence.candidate_regions,
            vec![PresentationRegionId::from("root-scroll")],
            "containing-but-unconsumable regions stay listed as candidates"
        );
        assert_eq!(evidence.selected_region, None);
        assert_eq!(
            evidence.refusal_reason.as_deref(),
            Some("no enabled scroll region accepted the physical wheel position")
        );
    }

    fn test_text_edit_region(target: WidgetId) -> TextEditRegionDeclaration {
        TextEditRegionDeclaration {
            buffer: TextBufferSnapshot {
                target: target.clone(),
                text: String::new(),
                revision: Vec::new(),
                diagnostics: Vec::new(),
            },
            selection: TextSelectionPolicyDeclaration {
                target: target.clone(),
                selection: None,
                carets: CaretSet {
                    carets: vec![0],
                    primary: Some(0),
                },
                editable: true,
                diagnostics: Vec::new(),
            },
            composition: ImeCompositionPolicyDeclaration {
                target: target.clone(),
                active: false,
                preedit_text: None,
                cursor_range: None,
                diagnostics: Vec::new(),
            },
            caret: CaretGeometryEvidence {
                target: target.clone(),
                caret_bounds: Vec::new(),
                selection_bounds: Vec::new(),
                measurement_request_ids: Vec::new(),
                diagnostics: Vec::new(),
            },
            visual_style: TextInputVisualStyleDeclaration::explicit(
                target.clone(),
                test_rgb(15, 23, 42),
                test_rgb(100, 116, 139),
                test_rgb(15, 23, 42),
                test_rgb(191, 219, 254),
                test_rgb(255, 255, 255),
                test_rgb(203, 213, 225),
                1.0,
                4.0,
                test_rgb(15, 23, 42),
            ),
            typography: TextInputTypographyDeclaration::explicit(target, TextStyle::plain()),
            edit_commands: vec![TextEditCommandDeclaration {
                command_id: "insert".to_string(),
                kind: TextEditKind::InsertText,
                enabled: true,
            }],
            undo_redo: None,
            viewport: None,
            line_mode: TextLineMode::SingleLine,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn declared_focus_text_dispatch_evidence_limits_positionless_candidates_to_selected_target() {
        let selected_target = WidgetId::from("selected-text");
        let other_target = WidgetId::from("other-text");
        let selected_region = FocusRegionDeclaration {
            id: PresentationRegionId::from("selected-focus"),
            target: selected_target.clone(),
            address: None,
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 100.0,
                    height: 24.0,
                },
            }),
            member: None,
            enabled: true,
            text_edit: Some(test_text_edit_region(selected_target.clone())),
        };
        let same_target_region = FocusRegionDeclaration {
            id: PresentationRegionId::from("selected-secondary-focus"),
            target: selected_target.clone(),
            address: Some(WidgetSlotAddress::new(selected_target.clone(), 1)),
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 28.0 },
                size: Size {
                    width: 100.0,
                    height: 24.0,
                },
            }),
            member: None,
            enabled: true,
            text_edit: Some(test_text_edit_region(selected_target.clone())),
        };
        let other_region = FocusRegionDeclaration {
            id: PresentationRegionId::from("other-focus"),
            target: other_target.clone(),
            address: None,
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 56.0 },
                size: Size {
                    width: 100.0,
                    height: 24.0,
                },
            }),
            member: None,
            enabled: true,
            text_edit: Some(test_text_edit_region(other_target)),
        };
        let focus_regions = vec![
            other_region,
            selected_region.clone(),
            same_target_region.clone(),
        ];
        let event = InputEvent::Keyboard(KeyboardEvent {
            target: selected_target,
            target_slot: None,
            key: "Enter".to_string(),
            kind: KeyEventKind::Press,
            modifiers: Modifiers::default(),
            details: KeyboardDetails::default(),
        });

        let evidence = declared_focus_text_dispatch_evidence(
            EvidenceSource::backend_presented("test-backend", "focused-input"),
            frame_identity(),
            &focus_regions,
            Some(&selected_region),
            DeclaredEventDispatchKind::Keyboard,
            None,
            event,
        );

        assert_eq!(
            evidence.candidate_regions,
            vec![
                PresentationRegionId::from("selected-focus"),
                PresentationRegionId::from("selected-secondary-focus")
            ]
        );
        assert_eq!(
            evidence.selected_region,
            Some(PresentationRegionId::from("selected-focus"))
        );
    }

    #[test]
    fn backend_input_dispatch_evidence_contract_accepts_current_declared_hit() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let frame = frame_identity();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame.clone(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    content: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            ),
        );
        let (dispatch, evidence) = resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.layout,
            &view.hit_regions,
            Point { x: 2.0, y: 2.0 },
            PointerEventKind::Press,
            Some(PointerButton::Primary),
            PointerDetails::default(),
            true,
        );
        let dispatch = dispatch.expect("declared hit dispatch");
        let input = BackendInputEvent::declared(dispatch.input, evidence);

        let diagnostics = backend_input_dispatch_evidence_contract_diagnostics(
            &view,
            &input,
            Some(EVIDENCE_SOURCE_BACKEND_PRESENTED),
            Some("test-backend"),
        );

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn backend_input_dispatch_evidence_contract_accepts_retained_capture_candidate() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let frame = frame_identity();
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame.clone(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    content: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            ),
        );
        let mut lower = view.hit_regions[0].clone();
        lower.id = PresentationRegionId::from("lower-hit");
        lower.capture = PointerCaptureIntent::None;
        lower.order = HitRegionOrder {
            z_index: 0,
            paint_order: 0,
            traversal_order: 0,
        };
        let mut captured = view.hit_regions[0].clone();
        captured.id = PresentationRegionId::from("captured-hit");
        captured.capture = PointerCaptureIntent::DuringDrag;
        captured.order = HitRegionOrder {
            z_index: 1,
            paint_order: 0,
            traversal_order: 1,
        };
        captured.route.route_id = Some("captured-route".to_string());
        view.hit_regions = vec![lower, captured.clone()];

        let position = Point { x: 8.0, y: 8.0 };
        let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
        let details = PointerDetails {
            device: PointerDeviceKind::Mouse,
            buttons: PointerButtons {
                primary: true,
                secondary: false,
                auxiliary: false,
            },
            ..PointerDetails::default()
        };
        let event = declared_pointer_event_for_hit_region_with_geometry_index(
            &geometry_index,
            &captured,
            position,
            PointerEventKind::Move,
            None,
            details,
        );
        let evidence = DeclaredEventDispatchEvidence {
            source: EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            kind: DeclaredEventDispatchKind::Pointer,
            input_position: Some(position),
            input_position_space: Some(DispatchPositionSpace::Content),
            candidate_regions: vec![captured.id.clone()],
            selected_region: Some(captured.id.clone()),
            refusal_reason: None,
            generated_event: Some(event.clone()),
            route: Some(captured.route.clone()),
            capture_event: true,
            diagnostics: Vec::new(),
        };
        let input = BackendInputEvent::declared(event, evidence);

        let diagnostics = backend_input_dispatch_evidence_contract_diagnostics(
            &view,
            &input,
            Some(EVIDENCE_SOURCE_BACKEND_PRESENTED),
            Some("test-backend"),
        );

        assert_eq!(diagnostics, Vec::<Diagnostic>::new());
    }

    #[test]
    fn backend_input_dispatch_evidence_contract_rejects_forged_region() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let frame = frame_identity();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame.clone(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    content: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            ),
        );
        let (dispatch, mut evidence) = resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.layout,
            &view.hit_regions,
            Point { x: 2.0, y: 2.0 },
            PointerEventKind::Press,
            Some(PointerButton::Primary),
            PointerDetails::default(),
            true,
        );
        let dispatch = dispatch.expect("declared hit dispatch");
        evidence.selected_region = Some(PresentationRegionId::from("forged-hit"));
        let input = BackendInputEvent::declared(dispatch.input, evidence);

        let diagnostics = backend_input_dispatch_evidence_contract_diagnostics(
            &view,
            &input,
            Some(EVIDENCE_SOURCE_BACKEND_PRESENTED),
            Some("test-backend"),
        );

        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == BACKEND_INPUT_DISPATCH_EVIDENCE_REGION_MISMATCH
        }));
    }

    #[test]
    fn declared_dispatch_identity_ignores_evidence_source() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let frame = frame_identity();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame.clone(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame.viewport),
                    content: TargetLocalRect::new(frame.viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame.viewport.size,
                    },
                },
            ),
        );
        let (_, mcp_evidence) = resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::debug_mcp("physical-control"),
            frame.clone(),
            &view.layout,
            &view.hit_regions,
            Point { x: 2.0, y: 2.0 },
            PointerEventKind::Press,
            Some(PointerButton::Primary),
            PointerDetails::default(),
            true,
        );
        let (_, backend_evidence) = resolve_declared_pointer_dispatch_with_evidence(
            EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.layout,
            &view.hit_regions,
            Point { x: 2.0, y: 2.0 },
            PointerEventKind::Press,
            Some(PointerButton::Primary),
            PointerDetails::default(),
            true,
        );

        assert_ne!(mcp_evidence.source, backend_evidence.source);
        assert_eq!(
            mcp_evidence.dispatch_identity(),
            backend_evidence.dispatch_identity()
        );
    }

    #[test]
    fn event_result_identity_compares_result_shape_not_state_values() {
        let target = WidgetId::from("counter");
        let slot = Some(WidgetSlotAddress::new(target.clone(), 0));
        let first_change = ChangeEvidence {
            target: target.clone(),
            slot: slot.clone(),
            field: "count".to_string(),
            before: Some("0".to_string()),
            after: Some("1".to_string()),
        };
        let second_change = ChangeEvidence {
            target: target.clone(),
            slot,
            field: "count".to_string(),
            before: Some("1".to_string()),
            after: Some("2".to_string()),
        };
        let message = EmittedMessageEvidence {
            target,
            name: "press".to_string(),
        };

        let first = EventResultIdentity {
            handled: Some(true),
            emitted_messages: vec![message.clone()],
            change_shapes: vec![ChangeShapeIdentity::from(&first_change)],
            diagnostics: Vec::new(),
        };
        let second = EventResultIdentity {
            handled: Some(true),
            emitted_messages: vec![message],
            change_shapes: vec![ChangeShapeIdentity::from(&second_change)],
            diagnostics: Vec::new(),
        };

        assert_eq!(first, second);
    }

    impl SlipwayFocusTraversal for FakeWidget {
        fn focus_member(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Option<FocusTraversalMember> {
            Some(FocusTraversalMember {
                target: self.id.clone(),
                scope: None,
                tab_order: Some(10),
            })
        }

        fn next_focus(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: FocusTraversalInput,
        ) -> Option<WidgetId> {
            Some(external.child.clone())
        }

        fn previous_focus(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: FocusTraversalInput,
        ) -> Option<WidgetId> {
            None
        }
    }

    impl SlipwaySemantics for FakeWidget {
        fn semantics(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<SemanticNode> {
            vec![SemanticNode {
                id: self.id.clone(),
                role: "textbox".to_string(),
                label: Some("fake".to_string()),
                value: Some("Official metric wrapper".to_string()),
                bounds: None,
                states: Vec::new(),
                actions: Vec::new(),
                relationships: Vec::new(),
            }]
        }
    }

    impl SlipwayHitTesting for FakeWidget {
        fn hit_test(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: HitTestInput,
        ) -> HitTestOutput {
            HitTestOutput {
                target: Some(self.id.clone()),
                local_point: Some(input.point),
                route: EventRoute {
                    route_id: Some("hit-test-route".to_string()),
                    address: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                    path: vec![self.id.clone()],
                    phase: EventRoutePhase::Target,
                },
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayViewportContracts for FakeWidget {
        fn scroll_state(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<ScrollState> {
            vec![ScrollState {
                target: self.id.clone(),
                region_id: Some(PresentationRegionId::from("root-scroll")),
                address: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                offset_x: 0.0,
                offset_y: 24.0,
                axes: ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                extent: Size {
                    width: 320.0,
                    height: 1200.0,
                },
                viewport: frame_identity().viewport,
                content_bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 320.0,
                        height: 1200.0,
                    },
                },
                consumption: Vec::new(),
            }]
        }

        fn virtual_viewports(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<VirtualViewportRange> {
            vec![VirtualViewportRange {
                target: self.id.clone(),
                region_id: Some(PresentationRegionId::from("root-scroll")),
                address: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                row_range: Some((2, 12)),
                column_range: Some((0, 3)),
                estimated_extent: Size {
                    width: 320.0,
                    height: 1200.0,
                },
            }]
        }
    }

    impl SlipwayOverlayContracts for FakeWidget {
        fn overlays(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<OverlayDeclaration> {
            vec![OverlayDeclaration {
                id: WidgetId::from("popup"),
                owner: self.id.clone(),
                bounds: frame_identity().viewport,
                allowed_bounds: Some(frame_identity().viewport),
                modality: OverlayModality::Modal,
                focus_scope: Some(self.id.clone()),
                dismiss_commands: vec!["escape".to_string()],
            }]
        }

        fn anchored_surfaces(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<AnchoredSurfaceDeclaration> {
            vec![AnchoredSurfaceDeclaration {
                id: WidgetId::from("anchor"),
                owner: self.id.clone(),
                anchor: self.id.clone(),
                bounds: frame_identity().viewport,
                placement: "bottom-start".to_string(),
            }]
        }

        fn portals(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<PortalDeclaration> {
            vec![PortalDeclaration {
                id: WidgetId::from("portal"),
                logical_owner: self.id.clone(),
                render_parent: None,
                surface_id: "surface".to_string(),
            }]
        }
    }

    impl SlipwayCommandContracts for FakeWidget {
        fn commands(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<CommandDeclaration> {
            vec![CommandDeclaration {
                id: "copy".to_string(),
                label: "Copy".to_string(),
                enabled: true,
                checked: None,
                shortcuts: vec![ShortcutDeclaration {
                    chord: "Ctrl+C".to_string(),
                    command_id: "copy".to_string(),
                }],
                scope: Some(self.id.clone()),
            }]
        }
    }

    impl SlipwayRenderSurfaces for FakeWidget {
        fn render_surfaces(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<RenderSurfaceDeclaration> {
            vec![RenderSurfaceDeclaration {
                id: WidgetId::from("canvas"),
                provider_id: "fake-provider".to_string(),
                bounds: frame_identity().viewport,
                payload_ref: Some("payload".to_string()),
                dirty_regions: Vec::new(),
                capabilities: vec!["canvas".to_string()],
            }]
        }
    }

    impl SlipwayTextBufferPolicy for FakeWidget {
        fn text_buffer(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TextBufferSnapshot {
            TextBufferSnapshot {
                target: self.id.clone(),
                text: "Official metric wrapper".to_string(),
                revision: vec![CacheRevisionToken {
                    name: "text".to_string(),
                    value: "rev-1".to_string(),
                }],
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayTextSelectionPolicy for FakeWidget {
        fn text_selection(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TextSelectionPolicyDeclaration {
            TextSelectionPolicyDeclaration {
                target: self.id.clone(),
                selection: Some(TextSelectionRange {
                    anchor: 0,
                    focus: 8,
                }),
                carets: CaretSet {
                    carets: vec![8],
                    primary: Some(8),
                },
                editable: true,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayImeCompositionPolicy for FakeWidget {
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

    impl SlipwayCaretGeometryPolicy for FakeWidget {
        fn caret_geometry(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _measurement: Option<&TextMeasurementEvidence>,
        ) -> CaretGeometryEvidence {
            CaretGeometryEvidence {
                target: self.id.clone(),
                caret_bounds: vec![Rect {
                    origin: Point { x: 4.0, y: 4.0 },
                    size: Size {
                        width: 1.0,
                        height: 16.0,
                    },
                }],
                selection_bounds: Vec::new(),
                measurement_request_ids: vec!["title".to_string()],
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayTextEditCommandPolicy for FakeWidget {
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

    impl SlipwayTextInputVisualStylePolicy for FakeWidget {
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
                test_rgb(255, 255, 255),
                test_rgb(203, 213, 225),
                1.0,
                4.0,
                test_rgb(15, 23, 42),
            )
        }
    }

    impl SlipwayTextInputTypographyPolicy for FakeWidget {
        fn text_input_typography(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TextInputTypographyDeclaration {
            TextInputTypographyDeclaration::explicit(
                self.id.clone(),
                TextStyle::plain().with_font_family("system-ui"),
            )
        }
    }

    impl SlipwayTextUndoRedoPolicy for FakeWidget {
        fn text_undo_redo(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TextUndoRedoEvidence {
            TextUndoRedoEvidence {
                target: self.id.clone(),
                can_undo: false,
                can_redo: false,
                undo_depth: Some(0),
                redo_depth: Some(0),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayIntrinsicSizing for FakeWidget {
        fn intrinsic_size(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> IntrinsicSize {
            IntrinsicSize {
                target: self.id.clone(),
                min_content: Size {
                    width: 80.0,
                    height: 24.0,
                },
                max_content: Size {
                    width: 240.0,
                    height: 96.0,
                },
                preferred: Size {
                    width: 160.0,
                    height: 48.0,
                },
                baseline: Some(18.0),
                aspect_ratio: None,
                wrap_affects_size: true,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwaySizePolicy for FakeWidget {
        fn size_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> SizePolicyDeclaration {
            SizePolicyDeclaration {
                target: self.id.clone(),
                width: SizePolicy::Fill { weight: 1.0 },
                height: SizePolicy::FitContent,
            }
        }
    }

    impl SlipwayResizePolicy for FakeWidget {
        fn resize_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> ResizePolicyDeclaration {
            ResizePolicyDeclaration {
                target: self.id.clone(),
                horizontal: ResizeAxisPolicy {
                    can_grow: true,
                    can_shrink: true,
                    priority: ResizePriority::Normal,
                },
                vertical: ResizeAxisPolicy {
                    can_grow: false,
                    can_shrink: false,
                    priority: ResizePriority::High,
                },
                preserve_aspect_ratio: false,
                minimum_preserved_size: Some(Size {
                    width: 80.0,
                    height: 24.0,
                }),
            }
        }
    }

    impl SlipwayOverflowPolicy for FakeWidget {
        fn overflow_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> OverflowPolicyDeclaration {
            OverflowPolicyDeclaration {
                target: self.id.clone(),
                x: OverflowBehavior::Clip,
                y: OverflowBehavior::Scroll,
            }
        }
    }

    impl SlipwayAutoLayoutPolicy for FakeWidget {
        fn auto_layout_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> AutoLayoutPolicyDeclaration {
            AutoLayoutPolicyDeclaration {
                target: self.id.clone(),
                horizontal: AutoLayoutRequirement::Required,
                vertical: AutoLayoutRequirement::Optional,
                dependencies: vec![
                    LayoutMeasurementDependency::AuthoredState,
                    LayoutMeasurementDependency::BackendWrappedTextMetrics,
                ],
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayResponsiveVariants for FakeWidget {
        fn responsive_variant(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
        ) -> ResponsiveVariant {
            let key = if input.viewport.size.width < 400.0 {
                "compact"
            } else {
                "wide"
            };
            ResponsiveVariant {
                target: self.id.clone(),
                key: key.to_string(),
                active_breakpoints: vec![key.to_string()],
                reason: Some("authored widget selected the variant".to_string()),
            }
        }
    }

    impl SlipwayTextFlowPolicy for FakeWidget {
        fn text_flow_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> TextFlowPolicy {
            TextFlowPolicy {
                target: self.id.clone(),
                line_mode: TextLineMode::MultiLine,
                wrap: TextWrapMode::Word,
                line_clamp: Some(2),
                allow_ellipsis: true,
                baseline: Some(18.0),
                caret_bounds: vec![Rect {
                    origin: Point { x: 4.0, y: 4.0 },
                    size: Size {
                        width: 1.0,
                        height: 16.0,
                    },
                }],
                viewport: Some(TextViewport {
                    scroll_x: 0.0,
                    scroll_y: 0.0,
                    visible_range: Some(TextSelectionRange {
                        anchor: 0,
                        focus: 12,
                    }),
                }),
            }
        }
    }

    impl SlipwayTextMeasurementPolicy for FakeWidget {
        fn text_measurement_policy(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: &LayoutInput,
        ) -> TextMeasurementPolicyDeclaration {
            TextMeasurementPolicyDeclaration {
                target: self.id.clone(),
                required: true,
                purposes: vec![
                    TextMeasurementPurpose::IntrinsicSize,
                    TextMeasurementPurpose::OverflowDetection,
                ],
                requests: vec![self.text_measurement_request(input)],
                cache_policies: self.text_measurement_cache_policy(external, local, input),
                diagnostics: Vec::new(),
            }
        }

        fn text_measurement_evidence<P>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: &LayoutInput,
            provider: &mut P,
        ) -> TextMeasurementEvidence
        where
            P: SlipwayTextMetricProvider,
        {
            let policy = self.text_measurement_policy(external, local, input);
            let receipts = policy
                .requests
                .iter()
                .cloned()
                .map(|request| provider.measure_text(request))
                .collect();
            TextMeasurementEvidence {
                target: self.id.clone(),
                policy,
                receipts,
                cache: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayTextMeasurementCachePolicy for FakeWidget {
        fn text_measurement_cache_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> Vec<TextMeasurementCachePolicyDeclaration> {
            vec![self.text_measurement_cache_policy_for_title()]
        }
    }

    impl SlipwayCachedTextMeasurementPolicy for FakeWidget {
        fn cached_text_measurement_evidence<P, C>(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: &LayoutInput,
            provider: &mut P,
            cache: &mut C,
        ) -> TextMeasurementEvidence
        where
            P: SlipwayTextMetricProvider,
            C: SlipwayTextMeasurementCache,
        {
            let policy = self.text_measurement_policy(external, local, input);
            let mut receipts = Vec::new();
            let mut cache_events = Vec::new();

            for request in &policy.requests {
                let cache_policy = policy
                    .cache_policies
                    .iter()
                    .find(|cache_policy| cache_policy.request_id == request.request_id);

                if let Some(cache_policy) = cache_policy {
                    match cache.lookup_text_measurement(cache_policy) {
                        TextMeasurementCacheLookup::Hit { receipt, evidence } => {
                            receipts.push(receipt);
                            cache_events.push(evidence);
                        }
                        TextMeasurementCacheLookup::Miss { evidence }
                        | TextMeasurementCacheLookup::Bypassed { evidence }
                        | TextMeasurementCacheLookup::Unsupported { evidence } => {
                            cache_events.push(evidence);
                            let receipt = provider.measure_text(request.clone());
                            let store_evidence =
                                cache.store_text_measurement(cache_policy, &receipt);
                            receipts.push(receipt);
                            cache_events.push(store_evidence);
                        }
                    }
                } else {
                    receipts.push(provider.measure_text(request.clone()));
                }
            }

            TextMeasurementEvidence {
                target: self.id.clone(),
                policy,
                receipts,
                cache: cache_events,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayFitOverflowEvidence for FakeWidget {
        fn fit_overflow_evidence(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
            text_measurement: Option<&TextMeasurementEvidence>,
        ) -> Vec<FitOverflowEvidence> {
            let measured = text_measurement.and_then(|evidence| {
                evidence.receipts.iter().find_map(|receipt| match receipt {
                    TextMeasurementReceipt::Valid(valid) => Some(valid.facts.measured_size),
                    TextMeasurementReceipt::Invalid { .. }
                    | TextMeasurementReceipt::Unsupported { .. } => None,
                })
            });
            vec![FitOverflowEvidence {
                target: self.id.clone(),
                bounds: input.viewport.into_rect(),
                measured_content: measured,
                horizontal: AxisFitEvidence {
                    available: input.viewport.size.width,
                    measured: measured
                        .map(|size| size.width)
                        .unwrap_or(input.viewport.size.width),
                    status: AxisFitStatus::Fits,
                },
                vertical: AxisFitEvidence {
                    available: input.viewport.size.height,
                    measured: measured
                        .map(|size| size.height)
                        .unwrap_or(input.viewport.size.height),
                    status: AxisFitStatus::Fits,
                },
                measurement_request_ids: vec!["title".to_string()],
                diagnostics: Vec::new(),
            }]
        }
    }

    impl SlipwayEventRoutingPolicy for FakeWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> EventRoutingPolicyDeclaration {
            EventRoutingPolicyDeclaration {
                target: self.id.clone(),
                event_target: event.target().clone(),
                route: EventRoute {
                    route_id: Some("event-policy-route".to_string()),
                    address: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                    path: vec![self.id.clone()],
                    phase: EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayEventDispositionPolicy for FakeWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            route: &EventRoute,
        ) -> EventPropagationEvidence {
            let disposition = EventDisposition {
                handled: true,
                propagate: false,
                default_action_allowed: true,
            };
            EventPropagationEvidence {
                target: self.id.clone(),
                event: event.clone(),
                steps: vec![EventPropagationStep {
                    stage: EventPropagationStage::Target,
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

    impl SlipwayPointerCapturePolicy for FakeWidget {
        fn pointer_capture_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            pointer: PointerDetails,
        ) -> PointerCapturePolicyDeclaration {
            PointerCapturePolicyDeclaration {
                target: self.id.clone(),
                requests: vec![PointerCaptureRequest {
                    target: self.id.clone(),
                    pointer_id: pointer.pointer_id,
                    phase: PointerCapturePhase::Capture,
                }],
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayDebugEventTracePolicy for FakeWidget {
        fn debug_event_trace_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> DebugEventTracePolicyDeclaration {
            DebugEventTracePolicyDeclaration {
                target: self.id.clone(),
                request_only: true,
                include_route: true,
                include_messages: true,
                include_state_changes: true,
                include_repaint_request: true,
            }
        }
    }

    impl SlipwayContainerLayoutPolicy for FakeWidget {
        fn container_layout_policy(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> ContainerLayoutPolicyDeclaration {
            ContainerLayoutPolicyDeclaration {
                target: self.id.clone(),
                kind: ContainerLayoutKind::Column,
                child_order: vec![external.child.clone()],
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayChildConstraintPolicy for FakeWidget {
        fn child_constraints(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
        ) -> Vec<ChildConstraintPolicyDeclaration> {
            vec![ChildConstraintPolicyDeclaration {
                parent: self.id.clone(),
                child: external.child.clone(),
                input: input.clone(),
                placement: Some(ParentLocalRect::from_parent_local(
                    input.viewport.into_rect(),
                )),
                diagnostics: Vec::new(),
            }]
        }
    }

    impl SlipwayLayoutInvalidationPolicy for FakeWidget {
        fn layout_invalidation_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> LayoutInvalidationPolicyDeclaration {
            LayoutInvalidationPolicyDeclaration {
                target: self.id.clone(),
                dependencies: vec![LayoutMeasurementDependency::AuthoredState],
                revisions: vec![CacheRevisionToken {
                    name: "layout".to_string(),
                    value: "rev-1".to_string(),
                }],
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayLayoutEvidencePolicy for FakeWidget {
        fn layout_evidence(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            output: &LayoutOutput,
        ) -> LayoutEvidence {
            LayoutEvidence {
                target: self.id.clone(),
                bounds: output.bounds,
                child_placements: output.child_placements.clone(),
                invalidated: false,
                diagnostics: output.diagnostics.clone(),
            }
        }
    }

    impl SlipwayScrollBehaviorPolicy for FakeWidget {
        fn scroll_behavior_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
        ) -> ScrollBehaviorPolicyDeclaration {
            ScrollBehaviorPolicyDeclaration {
                target: self.id.clone(),
                region_id: Some(PresentationRegionId::from("root-scroll")),
                address: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                axes: ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                extent: Size {
                    width: input.viewport.size.width,
                    height: 1200.0,
                },
                viewport: input.viewport,
                content_bounds: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: input.viewport.size.width,
                        height: 1200.0,
                    },
                }),
                offset: Point { x: 0.0, y: 24.0 },
                consumption: ScrollConsumptionPolicy {
                    wheel: true,
                    drag: false,
                    keyboard: true,
                    programmatic: true,
                },
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayWheelRoutingPolicy for FakeWidget {
        fn wheel_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            region: &PresentationRegionId,
        ) -> WheelRoutingPolicyDeclaration {
            // Region-targeted authoring: the declaration-time call names the
            // region being declared (ADR-0002 B3), so per-region modes are
            // expressible. Only the widget's own `scroll_behavior_policy`
            // region ("root-scroll") authors a non-default mode; every other
            // region stays the default.
            WheelRoutingPolicyDeclaration {
                target: self.id.clone(),
                routing: if region.as_str() == "root-scroll" {
                    WheelRouting::SelfFirst
                } else {
                    WheelRouting::NearestScrollable
                },
                modifiers: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayViewportObservationPolicy for FakeWidget {
        fn viewport_observation(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> ViewportObservationEvidence {
            let viewport = frame_identity().viewport;
            ViewportObservationEvidence {
                target: self.id.clone(),
                viewport: TargetLocalRect::new(viewport),
                visible_rect: TargetLocalRect::new(viewport),
                scroll: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayVirtualCollectionPolicy for FakeWidget {
        fn virtual_collection_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> VirtualCollectionPolicyDeclaration {
            VirtualCollectionPolicyDeclaration {
                target: self.id.clone(),
                item_count: 100,
                visible_range: Some(ItemRange { start: 2, end: 12 }),
                realization_hint: VirtualizationHint::Preferred,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayLayerPolicy for FakeWidget {
        fn layer_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> LayerPolicy {
            LayerPolicy {
                target: self.id.clone(),
                z_index: 10,
                clip_root: Some(self.id.clone()),
                transform_root: None,
                paint_containment: true,
                hit_test_containment: false,
            }
        }
    }

    impl SlipwayScrollPolicy for FakeWidget {
        fn scroll_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: &LayoutInput,
        ) -> ScrollPolicy {
            ScrollPolicy {
                target: self.id.clone(),
                region_id: Some(PresentationRegionId::from("root-scroll")),
                address: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                axes: ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                extent: Size {
                    width: input.viewport.size.width,
                    height: 1200.0,
                },
                viewport: input.viewport.into_rect(),
                content_bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: input.viewport.size.width,
                        height: 1200.0,
                    },
                },
                offset: Point { x: 0.0, y: 24.0 },
                snap_points: vec![ScrollSnapPoint {
                    target: self.id.clone(),
                    position: Point { x: 0.0, y: 100.0 },
                }],
                wheel_routing: WheelRouting::SelfFirst,
                consumption: ScrollConsumptionPolicy {
                    wheel: true,
                    drag: false,
                    keyboard: true,
                    programmatic: true,
                },
            }
        }
    }

    impl SlipwayCollectionPolicy for FakeWidget {
        fn collection_policy(
            &self,
            external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> CollectionPolicy {
            CollectionPolicy {
                target: self.id.clone(),
                item_count: 100,
                row_count: Some(100),
                column_count: Some(3),
                visible_rows: Some(ItemRange { start: 2, end: 12 }),
                visible_columns: Some(ItemRange { start: 0, end: 3 }),
                selected_items: vec![external.child.clone()],
                virtualization: VirtualizationHint::Preferred,
            }
        }
    }

    impl SlipwayInteractionStateStyle for FakeWidget {
        fn interaction_state_styles(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> Vec<InteractionStateStyle> {
            vec![InteractionStateStyle {
                target: self.id.clone(),
                state: InteractionState::Hover,
                style_key: "hover".to_string(),
                paint: vec![PaintOp::Fill {
                    shape: ShapeDeclaration {
                        id: Some("hover-bg".to_string()),
                        kind: ShapeKind::RoundedRectangle,
                        bounds: Rect {
                            origin: Point { x: 0.0, y: 0.0 },
                            size: Size {
                                width: 160.0,
                                height: 48.0,
                            },
                        },
                        path: None,
                        clip: None,
                    },
                    color: Color {
                        red: 0.1,
                        green: 0.2,
                        blue: 0.3,
                        alpha: 1.0,
                    },
                }],
                size_policy: Some(SizePolicyDeclaration {
                    target: self.id.clone(),
                    width: SizePolicy::Fill { weight: 1.0 },
                    height: SizePolicy::FitContent,
                }),
                overflow_policy: Some(OverflowPolicyDeclaration {
                    target: self.id.clone(),
                    x: OverflowBehavior::Clip,
                    y: OverflowBehavior::Scroll,
                }),
            }]
        }
    }

    impl SlipwayCanvasProvider for FakeWidget {
        fn canvas_surfaces(&self) -> Vec<ProviderSurfaceRequest> {
            vec![ProviderSurfaceRequest {
                target: WidgetId::from("canvas"),
                provider_id: "fake-provider".to_string(),
                kind: ProviderSurfaceKind::Canvas,
                bounds: frame_identity().viewport,
                payload_ref: Some("payload".to_string()),
                dirty_regions: Vec::new(),
            }]
        }
    }

    impl SlipwayGpuSurfaceProvider for FakeWidget {
        fn gpu_surfaces(&self) -> Vec<ProviderSurfaceRequest> {
            Vec::new()
        }
    }

    impl SlipwayMediaProvider for FakeWidget {
        fn media_surfaces(&self) -> Vec<ProviderSurfaceRequest> {
            Vec::new()
        }
    }

    impl SlipwayPlotProvider for FakeWidget {
        fn plot_surfaces(&self) -> Vec<ProviderSurfaceRequest> {
            Vec::new()
        }
    }

    impl SlipwayProviderHitTestPolicy for FakeWidget {
        fn provider_hit_test(&self, request: HitTestInput) -> ProviderHitTestEvidence {
            ProviderHitTestEvidence {
                target: request.target,
                provider_id: "fake-provider".to_string(),
                point: request.point,
                hit: Some("series-1".to_string()),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayProviderSnapshotPolicy for FakeWidget {
        fn provider_snapshot(
            &mut self,
            request: ProviderSnapshotRequest,
        ) -> ProviderSnapshotEvidence {
            ProviderSnapshotEvidence {
                target: request.target,
                provider_id: request.provider_id,
                snapshot_ref: Some("snapshot-ref".to_string()),
                frame: request.frame,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayFontResolutionPolicy for FakeWidget {
        fn resolve_font(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: FontResolutionRequest,
        ) -> FontResolutionEvidence {
            let source = ResourceSourceDeclaration {
                source_id: "inter-system".to_string(),
                kind: ResourceSourceKind::SystemFamily,
                family: Some(request.family.clone()),
                asset_ref: None,
                revision: Vec::new(),
            };
            FontResolutionEvidence {
                request,
                resolved_ref: Some("font-ref".to_string()),
                fallback_chain: vec!["system-ui".to_string()],
                installation: Some(ResourceInstallationEvidence {
                    resource_id: "font-ref".to_string(),
                    source: Some(source.clone()),
                    status: ResourceInstallationStatus::Installed,
                    evidence_source: EvidenceSource::backend_presented("fake-backend", "font"),
                    diagnostics: Vec::new(),
                }),
                refusal: None,
                valid_source: Some(SourceValidityEvidence {
                    source_id: source.source_id,
                    validity: SourceValidityKind::Valid,
                    diagnostics: Vec::new(),
                }),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayAssetResolutionPolicy for FakeWidget {
        fn resolve_asset(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: AssetResolutionRequest,
        ) -> AssetResolutionEvidence {
            AssetResolutionEvidence {
                request,
                resolved_ref: Some("asset-ref".to_string()),
                installation: None,
                refusal: None,
                valid_source: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayImageDecodeProvider for FakeWidget {
        fn decode_image(&mut self, request: ImageDecodeRequest) -> ImageDecodeEvidence {
            ImageDecodeEvidence {
                request,
                decoded_size: Some(Size {
                    width: 32.0,
                    height: 32.0,
                }),
                pixel_ref: Some("pixels".to_string()),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayStyleTokenPolicy for FakeWidget {
        fn resolve_style_token(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            request: StyleTokenRequest,
        ) -> StyleTokenEvidence {
            StyleTokenEvidence {
                request,
                value: Some("#000000".to_string()),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayTimeSourcePolicy for FakeWidget {
        fn time_source(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> TimeSourceSnapshot {
            TimeSourceSnapshot {
                source_id: "time".to_string(),
                millis: 1_700_000_000_000,
                revision: CacheRevisionToken {
                    name: "time".to_string(),
                    value: "fixed".to_string(),
                },
            }
        }
    }

    impl SlipwayRandomSourcePolicy for FakeWidget {
        fn random_source(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> RandomSourceSnapshot {
            RandomSourceSnapshot {
                source_id: "rng".to_string(),
                seed: "seed".to_string(),
                draw_index: 0,
                revision: CacheRevisionToken {
                    name: "rng".to_string(),
                    value: "seed:0".to_string(),
                },
            }
        }
    }

    impl SlipwayExternalDataSnapshotPolicy for FakeWidget {
        fn external_data_snapshot(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> ExternalDataSnapshot {
            ExternalDataSnapshot {
                source_id: "data".to_string(),
                snapshot_ref: "snapshot".to_string(),
                revision: CacheRevisionToken {
                    name: "data".to_string(),
                    value: "rev-1".to_string(),
                },
            }
        }
    }

    impl SlipwayAnimationTimelinePolicy for FakeWidget {
        fn animation_timeline_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> AnimationTimelinePolicyDeclaration {
            AnimationTimelinePolicyDeclaration {
                target: self.id.clone(),
                timeline_id: "main".to_string(),
                time_millis: 0.0,
                paused: true,
                revision: CacheRevisionToken {
                    name: "timeline".to_string(),
                    value: "paused".to_string(),
                },
            }
        }
    }

    impl SlipwayCommandInvocationPolicy for FakeWidget {
        fn command_invocation_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            command: &CommandEvent,
        ) -> CommandInvocationPolicyDeclaration {
            CommandInvocationPolicyDeclaration {
                command_id: command.command.clone(),
                target: self.id.clone(),
                enabled: true,
                expects_state_change: true,
            }
        }
    }

    impl SlipwayCommandStatusPolicy for FakeWidget {
        fn command_status(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            command_id: &str,
        ) -> CommandStatusEvidence {
            CommandStatusEvidence {
                command_id: command_id.to_string(),
                enabled: true,
                checked: None,
                label: Some("Copy".to_string()),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayShortcutRoutingPolicy for FakeWidget {
        fn shortcut_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            shortcut: &ShortcutDeclaration,
        ) -> ShortcutRoutingPolicyDeclaration {
            ShortcutRoutingPolicyDeclaration {
                shortcut: shortcut.clone(),
                route: EventRoute {
                    route_id: Some("shortcut-route".to_string()),
                    address: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                    path: vec![self.id.clone()],
                    phase: EventRoutePhase::Target,
                },
                command_id: shortcut.command_id.clone(),
            }
        }
    }

    impl SlipwayUndoRedoPolicy for FakeWidget {
        fn undo_redo_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> UndoRedoPolicyDeclaration {
            UndoRedoPolicyDeclaration {
                target: self.id.clone(),
                can_undo: false,
                can_redo: false,
                undo_command: Some("undo".to_string()),
                redo_command: Some("redo".to_string()),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayLayoutIntent for FakeWidget {
        fn layout_intent(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: &LayoutInput,
        ) -> LayoutIntentProbe {
            LayoutIntentProbe {
                target: self.id.clone(),
                intrinsic_size: Some(self.intrinsic_size(external, local, input)),
                size_policy: Some(self.size_policy(external, local, input)),
                resize_policy: Some(self.resize_policy(external, local, input)),
                overflow_policy: Some(self.overflow_policy(external, local, input)),
                auto_layout: Some(self.auto_layout_policy(external, local, input)),
                responsive_variant: Some(self.responsive_variant(external, local, input)),
                text_flow: Some(self.text_flow_policy(external, local, input)),
                text_measurement_cache: self.text_measurement_cache_policy(external, local, input),
                text_measurement: None,
                fit_overflow: Vec::new(),
                layer: Some(self.layer_policy(external, local, input)),
                scroll: Some(self.scroll_policy(external, local, input)),
                collection: Some(self.collection_policy(external, local, input)),
                interaction_styles: self.interaction_state_styles(external, local, input),
            }
        }
    }

    impl SlipwayBackendCapabilityProbe for FakeBackend {
        fn backend_capabilities(&self) -> BackendCapabilityReport {
            BackendCapabilityReport {
                backend_id: "fake-backend".to_string(),
                capabilities: vec![
                    Capability::TextInput,
                    Capability::Layout,
                    Capability::BackendCapabilityNegotiation,
                ],
                profiles: vec![
                    CapabilityProfileKind::TextInput,
                    CapabilityProfileKind::ScrollableContainer,
                ],
                visible_capabilities: vec![
                    BackendVisibleCapability::HitRegions,
                    BackendVisibleCapability::TextEditRegions,
                    BackendVisibleCapability::ScrollRegions,
                    BackendVisibleCapability::BackendPresentedEvidence,
                ],
            }
        }
    }

    impl SlipwayUnsupportedCapabilityEvidence for FakeBackend {
        fn unsupported_capabilities(
            &self,
            required: &[Capability],
        ) -> Vec<UnsupportedCapabilityEvidence> {
            required
                .iter()
                .filter(|capability| {
                    !self
                        .backend_capabilities()
                        .capabilities
                        .iter()
                        .any(|owned| owned == *capability)
                })
                .map(|capability| UnsupportedCapabilityEvidence {
                    backend_id: "fake-backend".to_string(),
                    target: None,
                    capability: capability.clone(),
                    visible_capability: None,
                    requirement_id: None,
                    reason: "not declared by fake backend".to_string(),
                    source: EvidenceSource::backend_presented("fake-backend", "capability-refusal"),
                    diagnostics: Vec::new(),
                })
                .collect()
        }
    }

    impl SlipwayBackendParityAdmission for FakeBackend {
        fn backend_parity_admission(
            &self,
            required_profiles: &[CapabilityProfileKind],
        ) -> BackendParityAdmission {
            let report = self.backend_capabilities();
            let source =
                EvidenceSource::backend_presented(report.backend_id.clone(), "parity-admission");
            let visible_requirements: Vec<BackendVisibleCapabilityRequirement> = required_profiles
                .iter()
                .map(|profile| BackendVisibleCapabilityRequirement {
                    requirement_id: format!("profile::{profile:?}"),
                    target: None,
                    capability: BackendVisibleCapability::Custom(format!("{profile:?}")),
                    required: true,
                })
                .collect();
            let unsupported: Vec<UnsupportedCapabilityEvidence> = required_profiles
                .iter()
                .filter(|profile| !report.profiles.iter().any(|owned| owned == *profile))
                .map(|profile| UnsupportedCapabilityEvidence {
                    backend_id: report.backend_id.clone(),
                    target: None,
                    capability: Capability::CapabilityAdmission,
                    visible_capability: Some(BackendVisibleCapability::Custom(format!(
                        "{profile:?}"
                    ))),
                    requirement_id: Some(format!("profile::{profile:?}")),
                    reason: format!("profile {profile:?} is not declared"),
                    source: source.clone(),
                    diagnostics: Vec::new(),
                })
                .collect();
            BackendParityAdmission {
                backend_id: report.backend_id,
                accepted: unsupported.is_empty(),
                required_profiles: required_profiles.to_vec(),
                visible_requirements,
                unsupported,
                source,
                diagnostics: Vec::new(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct CounterWidget {
        id: WidgetId,
        origin_x: f32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct AppExternal;

    #[derive(Clone, Debug, PartialEq)]
    struct CounterLocal {
        count: u32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct AppLocal;

    #[derive(Clone, Debug, PartialEq)]
    enum AppMessage {
        Counted(WidgetId),
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TwoCounterApp {
        widgets: (CounterWidget, CounterWidget),
    }

    #[derive(Clone, Debug, PartialEq)]
    struct VerticalEchoApp {
        widgets: (CounterWidget, CounterWidget),
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ScrollLocal {
        offset_y: f32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ScrollEchoApp {
        widgets: (CounterWidget, CounterWidget, CounterWidget),
    }

    #[derive(Clone, Debug, PartialEq)]
    struct SlotEchoWidget {
        id: WidgetId,
        layout_offset_x: f32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct SlotEchoLocal {
        layout_offset_x: f32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct DuplicateSlotApp {
        widgets: (SlotEchoWidget, SlotEchoWidget),
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BubblingWidget {
        id: WidgetId,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BubblingApp {
        widgets: (BubblingWidget,),
    }

    #[derive(Clone, Debug, PartialEq)]
    struct NestedRouteApp<W> {
        id: WidgetId,
        widgets: (W,),
    }

    impl<W> SlipwayApp for NestedRouteApp<W>
    where
        W: SlipwayWidget<ExternalState = AppExternal, AppMessage = AppMessage>
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy
            + SlipwayViewDefinition,
    {
        type ExternalState = AppExternal;
        type LocalState = ();
        type AppMessage = AppMessage;
        type Widgets = (W,);

        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {}
    }

    fn nested_route_app<W>(id: &str, child: W) -> SlipwayAppWidget<NestedRouteApp<W>>
    where
        W: SlipwayWidget<ExternalState = AppExternal, AppMessage = AppMessage>
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy
            + SlipwayViewDefinition,
    {
        SlipwayAppWidget::new(NestedRouteApp {
            id: WidgetId::from(id),
            widgets: (child,),
        })
    }

    macro_rules! impl_core_test_event_policy {
        ($($type:ty),+ $(,)?) => {
            $(
                impl SlipwayEventRoutingPolicy for $type {
                    fn event_routing_policy(
                        &self,
                        _external: &Self::ExternalState,
                        _local: &Self::LocalState,
                        event: &InputEvent,
                    ) -> EventRoutingPolicyDeclaration {
                        let target = self.id();
                        let address = event.target_slot().cloned();
                        let path = address
                            .as_ref()
                            .map(|address| address.path.clone())
                            .unwrap_or_else(|| vec![target.clone()]);
                        EventRoutingPolicyDeclaration {
                            target: target.clone(),
                            event_target: event.target().clone(),
                            route: EventRoute {
                                route_id: Some(format!("{}:test-route", target.as_str())),
                                address,
                                path,
                                phase: EventRoutePhase::Target,
                            },
                            capture: Vec::new(),
                            diagnostics: Vec::new(),
                        }
                    }
                }

                impl SlipwayEventDispositionPolicy for $type {
                    fn event_disposition(
                        &self,
                        _external: &Self::ExternalState,
                        _local: &Self::LocalState,
                        event: &InputEvent,
                        route: &EventRoute,
                    ) -> EventPropagationEvidence {
                        let target = self.id();
                        let handled = event.target() == &target;
                        let disposition = EventDisposition {
                            handled,
                            propagate: false,
                            default_action_allowed: true,
                        };
                        EventPropagationEvidence {
                            target: target.clone(),
                            event: event.clone(),
                            steps: vec![EventPropagationStep {
                                stage: EventPropagationStage::Target,
                                node: route.path.last().cloned().or(Some(target)),
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

    impl_core_test_event_policy!(CounterWidget, SlotEchoWidget);

    impl SlipwayWidgetTypes for BubblingWidget {
        type ExternalState = AppExternal;
        type LocalState = CounterLocal;
        type AppMessage = AppMessage;
    }

    impl SlipwaySsot for BubblingWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::CommandInput, Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for BubblingWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            local.count += 1;
            EventOutcome::message(
                self.id.clone(),
                "bubbled-child",
                AppMessage::Counted(self.id.clone()),
            )
        }
    }

    impl SlipwayView for BubblingWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            CounterLocal { count: 0 }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: LayoutOutputBuilder,
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
                value: local.count.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for BubblingWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = layout_view(self, external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                layout,
                paint: Vec::new(),
            }
        }
    }

    impl SlipwayEventRoutingPolicy for BubblingWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> EventRoutingPolicyDeclaration {
            EventRoutingPolicyDeclaration {
                target: self.id(),
                event_target: event.target().clone(),
                route: EventRoute {
                    route_id: Some("bubble-child-route".to_string()),
                    address: event.target_slot().cloned(),
                    path: vec![self.id()],
                    phase: EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayEventDispositionPolicy for BubblingWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            _route: &EventRoute,
        ) -> EventPropagationEvidence {
            let handled = event.target() == &self.id();
            let disposition = EventDisposition {
                handled,
                propagate: true,
                default_action_allowed: true,
            };
            EventPropagationEvidence {
                target: self.id(),
                event: event.clone(),
                steps: vec![EventPropagationStep {
                    stage: EventPropagationStage::Target,
                    node: Some(self.id()),
                    disposition,
                    emitted_messages: Vec::new(),
                    changes: Vec::new(),
                }],
                final_disposition: disposition,
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayWidgetTypes for CounterWidget {
        type ExternalState = AppExternal;
        type LocalState = CounterLocal;
        type AppMessage = AppMessage;
    }

    impl SlipwaySsot for CounterWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::CommandInput,
                Capability::Paint,
                Capability::StateObservation,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode {
                id: self.id.clone(),
                children: Vec::new(),
                local_state_slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for CounterWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            local.count += 1;
            EventOutcome {
                handled: true,
                propagate: false,
                emitted_messages: vec![EmittedMessage {
                    target: self.id.clone(),
                    name: "counted".to_string(),
                    message: AppMessage::Counted(self.id.clone()),
                }],
                changes: vec![ChangeEvidence {
                    target: self.id.clone(),
                    slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                    field: "count".to_string(),
                    before: Some((local.count - 1).to_string()),
                    after: Some(local.count.to_string()),
                }],
                observations: self.observe_state(_external, local),
                probes: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayView for CounterWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            CounterLocal { count: 0 }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Text {
                bounds: layout.bounds.into_rect(),
                content: format!("{}:{}", self.id.as_str(), local.count),
                color: Color {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 1.0,
                },
                style: TextStyle::plain(),
            }]
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![StateObservation {
                target: self.id.clone(),
                slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
                name: "count".to_string(),
                value: local.count.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for CounterWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = layout_view(self, external, local, input.layout_input);
            let paint = self.paint(external, local, &layout);

            let paint_order = match self.id.as_str() {
                "layer-top" => PaintOrderDeclaration::layered_order(self.id(), 1, 0),
                "layer-bottom" => PaintOrderDeclaration::layered_order(self.id(), 0, 0),
                // Child-local overflow allowance (the roaming-overlay
                // pattern) for the composed-view propagation tests.
                "overflow" => PaintOrderDeclaration::source_order(self.id()).with_overflow_bounds(
                    TargetLocalRect::new(Rect {
                        origin: Point { x: -8.0, y: -4.0 },
                        size: Size {
                            width: 48.0,
                            height: 40.0,
                        },
                    }),
                ),
                _ => PaintOrderDeclaration::source_order(self.id()),
            };

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                hit_regions: vec![HitRegionDeclaration {
                    id: PresentationRegionId::from(format!("{}.hit", self.id.as_str())),
                    target: self.id(),
                    address: None,
                    bounds: layout.bounds,
                    event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
                    order: HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    route: EventRoute {
                        route_id: Some(format!("{}.route", self.id.as_str())),
                        address: None,
                        path: vec![self.id()],
                        phase: EventRoutePhase::Target,
                    },
                    cursor: CursorCapability::Pointer,
                    enabled: true,
                    capture: PointerCaptureIntent::None,
                    capture_evidence: Vec::new(),
                }],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
                paint,
                paint_order,
            }
        }
    }

    impl SlipwayWidgetTypes for SlotEchoWidget {
        type ExternalState = AppExternal;
        type LocalState = SlotEchoLocal;
        type AppMessage = AppMessage;
    }

    impl SlipwaySsot for SlotEchoWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::Layout, Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode {
                id: self.id.clone(),
                children: Vec::new(),
                local_state_slot: Some(WidgetSlotAddress::new(self.id.clone(), 0)),
            }
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for SlotEchoWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for SlotEchoWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            SlotEchoLocal {
                layout_offset_x: self.layout_offset_x,
            }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
            input: LayoutInput,
            output: LayoutOutputBuilder,
        ) -> LayoutOutput {
            let mut bounds = input.viewport;
            bounds.origin.x += local.layout_offset_x;
            output.finish(bounds)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Text {
                bounds: layout.bounds.into_rect(),
                content: format!("{}:{:.0}", self.id.as_str(), local.layout_offset_x),
                color: Color {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 1.0,
                },
                style: TextStyle::plain(),
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

    impl SlipwayViewDefinition for SlotEchoWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = layout_view(self, external, local, input.layout_input);
            let paint = self.paint(external, local, &layout);

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                hit_regions: vec![HitRegionDeclaration {
                    id: PresentationRegionId::from(format!("{}.hit", self.id.as_str())),
                    target: self.id(),
                    address: None,
                    bounds: layout.bounds,
                    event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
                    order: HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    route: EventRoute {
                        route_id: Some(format!("{}.route", self.id.as_str())),
                        address: None,
                        path: vec![self.id()],
                        phase: EventRoutePhase::Target,
                    },
                    cursor: CursorCapability::Pointer,
                    enabled: true,
                    capture: PointerCaptureIntent::None,
                    capture_evidence: Vec::new(),
                }],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                layout,
                paint,
            }
        }
    }

    impl SlipwayApp for TwoCounterApp {
        type ExternalState = AppExternal;
        type LocalState = AppLocal;
        type AppMessage = AppMessage;
        type Widgets = (CounterWidget, CounterWidget);

        fn id(&self) -> WidgetId {
            WidgetId::from("app")
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {
            AppLocal
        }

        fn layout_plan(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            children: Vec<ChildLayoutSeed>,
        ) -> AppLayoutPlan {
            AppLayoutPlan {
                bounds: input.viewport,
                children: children
                    .into_iter()
                    .enumerate()
                    .map(|(index, seed)| {
                        let bounds = Rect {
                            origin: Point {
                                x: index as f32 * 32.0,
                                y: 0.0,
                            },
                            size: Size {
                                width: 24.0,
                                height: 16.0,
                            },
                        };
                        ChildLayoutPlan::explicit_border(
                            seed,
                            ContentLocalRect::new(bounds),
                            BoxSpacing::ZERO,
                        )
                    })
                    .collect(),
                diagnostics: Vec::new(),
            }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }
    }

    impl SlipwayApp for VerticalEchoApp {
        type ExternalState = AppExternal;
        type LocalState = AppLocal;
        type AppMessage = AppMessage;
        type Widgets = (CounterWidget, CounterWidget);

        fn id(&self) -> WidgetId {
            WidgetId::from("vertical-app")
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {
            AppLocal
        }

        fn layout_plan(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            children: Vec<ChildLayoutSeed>,
        ) -> AppLayoutPlan {
            AppLayoutPlan {
                bounds: input.viewport,
                children: children
                    .into_iter()
                    .enumerate()
                    .map(|(index, seed)| {
                        let height = if index == 0 { 12.0 } else { 20.0 };
                        let y = if index == 0 { 0.0 } else { 12.0 };
                        let viewport = Rect {
                            origin: Point { x: 0.0, y },
                            size: Size {
                                width: input.viewport.size.width,
                                height,
                            },
                        };
                        ChildLayoutPlan::explicit_border(
                            seed,
                            ContentLocalRect::new(viewport),
                            BoxSpacing::ZERO,
                        )
                    })
                    .collect(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayApp for ScrollEchoApp {
        type ExternalState = AppExternal;
        type LocalState = ScrollLocal;
        type AppMessage = AppMessage;
        type Widgets = (CounterWidget, CounterWidget, CounterWidget);

        fn id(&self) -> WidgetId {
            WidgetId::from("scroll-app")
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {
            ScrollLocal { offset_y: 0.0 }
        }

        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Wheel(wheel) => {
                    local.offset_y += wheel.delta_y;
                    EventOutcome {
                        handled: true,
                        propagate: false,
                        emitted_messages: Vec::new(),
                        changes: vec![ChangeEvidence {
                            target: self.id(),
                            slot: Some(WidgetSlotAddress::new(self.id(), 0)),
                            field: "offset_y".to_string(),
                            before: Some((local.offset_y - wheel.delta_y).to_string()),
                            after: Some(local.offset_y.to_string()),
                        }],
                        observations: self.observe_state(_external, local),
                        probes: Vec::new(),
                        diagnostics: Vec::new(),
                    }
                }
                InputEvent::Command(command) if command.command == "scroll-home" => {
                    let before = local.offset_y;
                    local.offset_y = 0.0;
                    EventOutcome {
                        handled: true,
                        propagate: false,
                        emitted_messages: Vec::new(),
                        changes: vec![ChangeEvidence {
                            target: self.id(),
                            slot: Some(WidgetSlotAddress::new(self.id(), 0)),
                            field: "offset_y".to_string(),
                            before: Some(before.to_string()),
                            after: Some(local.offset_y.to_string()),
                        }],
                        observations: self.observe_state(_external, local),
                        probes: Vec::new(),
                        diagnostics: Vec::new(),
                    }
                }
                _ => EventOutcome::ignored(),
            }
        }

        fn layout_plan(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
            input: LayoutInput,
            children: Vec<ChildLayoutSeed>,
        ) -> AppLayoutPlan {
            AppLayoutPlan {
                bounds: input.viewport,
                children: children
                    .into_iter()
                    .enumerate()
                    .map(|(index, seed)| {
                        let viewport = Rect {
                            origin: Point {
                                x: input.viewport.origin.x,
                                y: input.viewport.origin.y + index as f32 * 20.0 - local.offset_y,
                            },
                            size: Size {
                                width: input.viewport.size.width,
                                height: 20.0,
                            },
                        };
                        ChildLayoutPlan::explicit_border(
                            seed,
                            ContentLocalRect::new(viewport),
                            BoxSpacing::ZERO,
                        )
                    })
                    .collect(),
                diagnostics: Vec::new(),
            }
        }

        fn observe_state(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![StateObservation {
                target: self.id(),
                slot: Some(WidgetSlotAddress::new(self.id(), 0)),
                name: "offset_y".to_string(),
                value: local.offset_y.to_string(),
            }]
        }
    }

    impl SlipwayApp for DuplicateSlotApp {
        type ExternalState = AppExternal;
        type LocalState = AppLocal;
        type AppMessage = AppMessage;
        type Widgets = (SlotEchoWidget, SlotEchoWidget);

        fn id(&self) -> WidgetId {
            WidgetId::from("duplicate-slot-app")
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {
            AppLocal
        }

        fn layout_plan(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            children: Vec<ChildLayoutSeed>,
        ) -> AppLayoutPlan {
            AppLayoutPlan {
                bounds: input.viewport,
                children: children
                    .into_iter()
                    .enumerate()
                    .map(|(index, seed)| {
                        let viewport = Rect {
                            origin: Point {
                                x: index as f32 * 100.0,
                                y: index as f32 * 20.0,
                            },
                            size: Size {
                                width: 40.0,
                                height: 16.0,
                            },
                        };
                        ChildLayoutPlan::requested_outer(
                            seed,
                            ContentLocalRect::new(viewport),
                            LayoutConstraints {
                                min: Size {
                                    width: 0.0,
                                    height: 0.0,
                                },
                                max: viewport.size,
                            },
                            BoxSpacing::ZERO,
                        )
                    })
                    .collect(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayApp for BubblingApp {
        type ExternalState = AppExternal;
        type LocalState = AppLocal;
        type AppMessage = AppMessage;
        type Widgets = (BubblingWidget,);

        fn id(&self) -> WidgetId {
            WidgetId::from("bubble-app")
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {
            AppLocal
        }

        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Command(command) if command.command == "count" => {
                    EventOutcome::message(
                        self.id(),
                        "app-observed",
                        AppMessage::Counted(command.target),
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
            AppLayoutPlan {
                bounds: input.viewport,
                children: children
                    .into_iter()
                    .map(|seed| {
                        ChildLayoutPlan::explicit_border(
                            seed,
                            ContentLocalRect::new(input.viewport.into_rect()),
                            BoxSpacing::ZERO,
                        )
                    })
                    .collect(),
                diagnostics: Vec::new(),
            }
        }
    }

    fn two_counter_app_widget() -> SlipwayAppWidget<TwoCounterApp> {
        SlipwayAppWidget::new(TwoCounterApp {
            widgets: (
                CounterWidget {
                    id: WidgetId::from("one"),
                    origin_x: 0.0,
                },
                CounterWidget {
                    id: WidgetId::from("two"),
                    origin_x: 32.0,
                },
            ),
        })
    }

    fn vertical_echo_app_widget() -> SlipwayAppWidget<VerticalEchoApp> {
        SlipwayAppWidget::new(VerticalEchoApp {
            widgets: (
                CounterWidget {
                    id: WidgetId::from("top"),
                    origin_x: 0.0,
                },
                CounterWidget {
                    id: WidgetId::from("bottom"),
                    origin_x: 0.0,
                },
            ),
        })
    }

    fn scroll_echo_app_widget() -> SlipwayAppWidget<ScrollEchoApp> {
        SlipwayAppWidget::new(ScrollEchoApp {
            widgets: (
                CounterWidget {
                    id: WidgetId::from("row-one"),
                    origin_x: 0.0,
                },
                CounterWidget {
                    id: WidgetId::from("row-two"),
                    origin_x: 0.0,
                },
                CounterWidget {
                    id: WidgetId::from("row-three"),
                    origin_x: 0.0,
                },
            ),
        })
    }

    fn duplicate_slot_app_widget() -> SlipwayAppWidget<DuplicateSlotApp> {
        SlipwayAppWidget::new(DuplicateSlotApp {
            widgets: (
                SlotEchoWidget {
                    id: WidgetId::from("duplicate"),
                    layout_offset_x: 1.0,
                },
                SlotEchoWidget {
                    id: WidgetId::from("duplicate"),
                    layout_offset_x: 2.0,
                },
            ),
        })
    }

    fn command(target: &str) -> InputEvent {
        InputEvent::Command(CommandEvent {
            target: WidgetId::from(target),
            target_slot: None,
            command: "count".to_string(),
            payload_ref: None,
            source: None,
        })
    }

    fn command_with_slot(target: &str, target_slot: WidgetSlotAddress) -> InputEvent {
        InputEvent::Command(CommandEvent {
            target: WidgetId::from(target),
            target_slot: Some(target_slot),
            command: "count".to_string(),
            payload_ref: None,
            source: None,
        })
    }

    fn wheel(target: &str, delta_y: f32) -> InputEvent {
        InputEvent::Wheel(WheelEvent {
            target: WidgetId::from(target),
            target_slot: None,
            region_id: None,
            delta_x: 0.0,
            delta_y,
        })
    }

    fn scroll_home_command() -> InputEvent {
        InputEvent::Command(CommandEvent {
            target: WidgetId::from("scroll-app"),
            target_slot: None,
            command: "scroll-home".to_string(),
            payload_ref: None,
            source: None,
        })
    }

    fn app_layout_input() -> LayoutInput {
        let viewport = TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 96.0,
                height: 32.0,
            },
        });
        LayoutInput {
            viewport,
            content: viewport,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: Size {
                    width: 96.0,
                    height: 32.0,
                },
            },
        }
    }

    #[derive(Debug, PartialEq)]
    struct VisitedChild {
        id: WidgetId,
        slot: WidgetSlotAddress,
        state_label: String,
    }

    #[derive(Default)]
    struct ChildPaintVisitor {
        visited: Vec<VisitedChild>,
    }

    impl SlipwayWidgetListVisitor<AppExternal, AppMessage> for ChildPaintVisitor {
        fn visit_child<W>(
            &mut self,
            widget: &W,
            external: &AppExternal,
            local: &W::LocalState,
            slot: WidgetSlotAddress,
        ) where
            W: SlipwayWidget<ExternalState = AppExternal, AppMessage = AppMessage>
                + SlipwayViewDefinition,
        {
            let layout = LayoutOutput {
                bounds: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 64.0,
                        height: 16.0,
                    },
                }),
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            };
            let view = widget.view_definition(
                external,
                local,
                ViewDefinitionInput::new(
                    frame_identity(),
                    LayoutInput {
                        viewport: layout.bounds,
                        content: layout.bounds,
                        constraints: LayoutConstraints {
                            min: Size {
                                width: 0.0,
                                height: 0.0,
                            },
                            max: layout.bounds.size,
                        },
                    },
                ),
            );
            let state_label = widget
                .paint(external, local, &layout)
                .into_iter()
                .find_map(|op| match op {
                    PaintOp::Text { content, .. } => Some(content),
                    _ => None,
                })
                .expect("test child paints a state label");

            self.visited.push(VisitedChild {
                id: view.target,
                slot,
                state_label,
            });
        }
    }

    #[test]
    fn composed_widget_requires_all_contract_parts() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };

        assert_authored(&widget);
    }

    #[test]
    fn ordinary_widget_child_access_defaults_to_no_authored_children() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("topology-only-child"),
        };
        let local = widget.initial_local_state();
        let mut visitor = FakeChildAccessVisitor::default();

        widget.visit_authored_children(&external, &local, &mut visitor);

        assert_eq!(visitor.visited, 0);
    }

    #[test]
    fn tuple_child_visitor_visits_children_in_declaration_order() {
        let widgets = (
            CounterWidget {
                id: WidgetId::from("one"),
                origin_x: 0.0,
            },
            CounterWidget {
                id: WidgetId::from("two"),
                origin_x: 32.0,
            },
        );
        let external = AppExternal;
        let local = widgets.initial_child_local_state();
        let parent_slot = WidgetSlotAddress::new(WidgetId::from("app"), 0);
        let mut visitor = ChildPaintVisitor::default();

        widgets.visit_children(&external, &local, &parent_slot, &mut visitor);

        assert_eq!(
            visitor
                .visited
                .iter()
                .map(|child| child.id.clone())
                .collect::<Vec<_>>(),
            vec![WidgetId::from("one"), WidgetId::from("two")]
        );
        assert_eq!(
            visitor
                .visited
                .iter()
                .map(|child| child.state_label.as_str())
                .collect::<Vec<_>>(),
            vec!["one:0", "two:0"]
        );
    }

    #[test]
    fn child_visitor_distinguishes_duplicate_child_ids_by_slot_address() {
        let widget = duplicate_slot_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let parent_slot = WidgetSlotAddress::new(widget.id(), 0);
        let mut visitor = ChildPaintVisitor::default();

        widget
            .app
            .widgets()
            .visit_children(&external, &local.widgets, &parent_slot, &mut visitor);

        assert_eq!(visitor.visited.len(), 2);
        assert_eq!(visitor.visited[0].id, WidgetId::from("duplicate"));
        assert_eq!(visitor.visited[1].id, WidgetId::from("duplicate"));
        assert_eq!(visitor.visited[0].slot.ordinal, 0);
        assert_eq!(visitor.visited[1].slot.ordinal, 1);
        assert_ne!(visitor.visited[0].slot, visitor.visited[1].slot);
        assert_eq!(
            visitor
                .visited
                .iter()
                .map(|child| child.state_label.as_str())
                .collect::<Vec<_>>(),
            vec!["duplicate:1", "duplicate:2"]
        );
    }

    #[test]
    fn app_widget_child_access_exposes_actual_tuple_children() {
        let widget = duplicate_slot_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let mut visitor = ChildPaintVisitor::default();

        widget.visit_authored_children(&external, &local, &mut visitor);

        assert_eq!(visitor.visited.len(), 2);
        assert_eq!(visitor.visited[0].id, WidgetId::from("duplicate"));
        assert_eq!(visitor.visited[1].id, WidgetId::from("duplicate"));
        assert_eq!(visitor.visited[0].slot.ordinal, 0);
        assert_eq!(visitor.visited[1].slot.ordinal, 1);
        assert_ne!(visitor.visited[0].slot, visitor.visited[1].slot);
        assert_eq!(
            visitor
                .visited
                .iter()
                .map(|child| child.state_label.as_str())
                .collect::<Vec<_>>(),
            vec!["duplicate:1", "duplicate:2"]
        );
    }

    #[test]
    fn app_widget_view_definition_composes_child_declarations_without_child_paint_duplication() {
        let widget = duplicate_slot_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );

        assert_eq!(view.target, WidgetId::from("duplicate-slot-app"));
        assert_eq!(view.layout.child_placements.len(), 2);
        assert!(view.paint.is_empty());
        assert_eq!(view.hit_regions.len(), 2);
        assert_eq!(view.hit_regions[0].target, WidgetId::from("duplicate"));
        assert_eq!(view.hit_regions[1].target, WidgetId::from("duplicate"));

        let first_address = view.hit_regions[0]
            .address
            .as_ref()
            .expect("first duplicate child hit region is slot-addressed");
        let second_address = view.hit_regions[1]
            .address
            .as_ref()
            .expect("second duplicate child hit region is slot-addressed");
        assert_eq!(first_address.ordinal, 0);
        assert_eq!(second_address.ordinal, 1);
        assert_ne!(first_address, second_address);
        assert_eq!(
            view.hit_regions[0].route.address.as_ref(),
            Some(first_address)
        );
        assert_eq!(
            view.hit_regions[1].route.address.as_ref(),
            Some(second_address)
        );
        assert_eq!(view.hit_regions[0].route.path, first_address.path);
        assert_eq!(view.hit_regions[1].route.path, second_address.path);
    }

    #[test]
    fn app_widget_visible_backend_view_definition_keeps_children_in_backend_tree() {
        let widget = duplicate_slot_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.visible_backend_view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );

        assert_eq!(view.target, WidgetId::from("duplicate-slot-app"));
        assert_eq!(view.layout.child_placements.len(), 2);
        assert!(view.paint.is_empty());
        assert_eq!(view.hit_regions.len(), 2);
        assert_eq!(view.hit_regions[0].target, WidgetId::from("duplicate"));
        assert_eq!(view.hit_regions[1].target, WidgetId::from("duplicate"));
        assert!(view.hit_regions[0].address.is_some());
        assert!(view.hit_regions[1].address.is_some());
        assert!(view.focus_regions.is_empty());
        assert!(view.scroll_regions.is_empty());
    }

    #[test]
    fn child_visitor_receives_matching_concrete_local_state() {
        let widgets = (
            CounterWidget {
                id: WidgetId::from("left"),
                origin_x: 0.0,
            },
            CounterWidget {
                id: WidgetId::from("right"),
                origin_x: 32.0,
            },
        );
        let external = AppExternal;
        let local = (CounterLocal { count: 5 }, CounterLocal { count: 8 });
        let parent_slot = WidgetSlotAddress::new(WidgetId::from("app"), 0);
        let mut visitor = ChildPaintVisitor::default();

        widgets.visit_children(&external, &local, &parent_slot, &mut visitor);

        assert_eq!(
            visitor
                .visited
                .iter()
                .map(|child| child.state_label.as_str())
                .collect::<Vec<_>>(),
            vec!["left:5", "right:8"]
        );
    }

    #[test]
    fn app_widget_exposes_two_child_topology_nodes_with_local_state_slots() {
        let widget = two_counter_app_widget();
        let topology = widget.topology(&AppExternal);

        assert_authored(&widget);
        assert_eq!(topology.id, WidgetId::from("app"));
        assert_eq!(topology.children.len(), 2);
        assert_eq!(topology.children[0].id, WidgetId::from("one"));
        assert_eq!(topology.children[1].id, WidgetId::from("two"));

        let first = topology.children[0]
            .local_state_slot
            .as_ref()
            .expect("first child has tuple local-state slot");
        let second = topology.children[1]
            .local_state_slot
            .as_ref()
            .expect("second child has tuple local-state slot");

        assert_eq!(
            first.path,
            vec![WidgetId::from("app"), WidgetId::from("one")]
        );
        assert_eq!(
            second.path,
            vec![WidgetId::from("app"), WidgetId::from("two")]
        );
    }

    #[test]
    fn targeted_events_mutate_only_targeted_widget_local_state() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let mut local = widget.initial_local_state();

        let one = widget.handle_event(&external, &mut local, command("one"));

        assert!(one.handled);
        assert_eq!(local.widgets.0.count, 1);
        assert_eq!(local.widgets.1.count, 0);

        let two = widget.handle_event(&external, &mut local, command("two"));

        assert!(two.handled);
        assert_eq!(local.widgets.0.count, 1);
        assert_eq!(local.widgets.1.count, 1);
        assert_eq!(
            two.changes[0].slot.as_ref().map(|slot| slot.path.clone()),
            Some(vec![WidgetId::from("app"), WidgetId::from("two")])
        );
    }

    #[test]
    fn declared_child_propagation_reaches_app_reducer() {
        let widget = SlipwayAppWidget::new(BubblingApp {
            widgets: (BubblingWidget {
                id: WidgetId::from("bubble-child"),
            },),
        });
        let external = AppExternal;
        let mut local = widget.initial_local_state();

        let outcome = widget.handle_event(&external, &mut local, command("bubble-child"));

        assert!(outcome.handled);
        assert!(!outcome.propagate);
        assert_eq!(local.widgets.0.count, 1);
        assert_eq!(
            outcome
                .emitted_messages
                .iter()
                .map(|message| message.name.as_str())
                .collect::<Vec<_>>(),
            vec!["bubbled-child", "app-observed"]
        );
        assert_eq!(
            outcome
                .emitted_messages
                .iter()
                .map(|message| message.target.clone())
                .collect::<Vec<_>>(),
            vec![WidgetId::from("bubble-child"), WidgetId::from("bubble-app")]
        );
    }

    #[test]
    fn declared_event_disposition_reports_handler_policy_mismatch() {
        let widget = CounterWidget {
            id: WidgetId::from("one"),
            origin_x: 0.0,
        };
        let event = command("one");
        let declaration =
            declared_event_handling(&widget, &AppExternal, &CounterLocal { count: 0 }, &event);
        let outcome =
            apply_event_handling_declaration(declaration, EventOutcome::<AppMessage>::ignored());

        assert!(!outcome.handled);
        assert!(outcome.propagate);
        assert!(outcome.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_ignored_declared_handled"
                && diagnostic.severity == DiagnosticSeverity::Warning
        }));

        let declaration =
            declared_event_handling(&widget, &AppExternal, &CounterLocal { count: 0 }, &event);
        let physical_outcome = apply_physical_event_handling_declaration(
            declaration,
            EventOutcome::<AppMessage>::ignored(),
        );

        assert!(!physical_outcome.handled);
        assert!(physical_outcome.emitted_messages.is_empty());
        assert!(physical_outcome.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_ignored_declared_handled"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));

        let declaration =
            declared_event_handling(&widget, &AppExternal, &CounterLocal { count: 0 }, &event);
        let mut emitted_while_ignored = EventOutcome::<AppMessage>::ignored();
        emitted_while_ignored.emitted_messages.push(EmittedMessage {
            target: WidgetId::from("one"),
            name: "must-not-reduce".to_string(),
            message: AppMessage::Counted(WidgetId::from("one")),
        });
        emitted_while_ignored.observations.push(StateObservation {
            target: WidgetId::from("one"),
            slot: None,
            name: "must-not-observe".to_string(),
            value: "dirty".to_string(),
        });
        let physical_outcome =
            apply_physical_event_handling_declaration(declaration, emitted_while_ignored);

        assert!(!physical_outcome.handled);
        assert!(physical_outcome.emitted_messages.is_empty());
        assert!(physical_outcome.observations.is_empty());
    }

    // ----- NC-8 sync-by-construction fixtures (roadmap Phase 6 item 4,
    // ADR-0003): the consumer app's KPI-modal shape, authored twice — the
    // NEW single-table form and the OLD hand-duplicated form with the
    // audited drift. -----

    #[derive(Clone, Debug, PartialEq)]
    struct ModalLocal {
        open: bool,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TableModalWidget {
        id: WidgetId,
    }

    impl SlipwayWidgetTypes for TableModalWidget {
        type ExternalState = AppExternal;
        type LocalState = ModalLocal;
        type AppMessage = AppMessage;
    }

    impl SlipwaySsot for TableModalWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::PointerInput, Capability::WheelInput]
        }
        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }
        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayEventRoutingPolicy for TableModalWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> EventRoutingPolicyDeclaration {
            EventRoutingPolicyDeclaration {
                target: self.id(),
                event_target: event.target().clone(),
                route: EventRoute {
                    route_id: None,
                    address: event.target_slot().cloned(),
                    path: vec![self.id()],
                    phase: EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    event_handling_table! {
        impl TableModalWidget {
            |widget, external, local| match event {
                // The close press is handled only while the modal is OPEN
                // — exactly the state-dependence the fixture app
                // hand-duplicated and let drift (NC-8).
                InputEvent::Pointer(pointer)
                    if pointer.kind == PointerEventKind::Press && local.open =>
                {
                    local.open = false;
                    EventOutcome::message(
                        widget.id(),
                        "modal-closed",
                        AppMessage::Counted(widget.id()),
                    )
                },
                InputEvent::Wheel(wheel) if wheel.delta_y < 0.0 => EventOutcome::handled(),
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct DriftModalWidget {
        id: WidgetId,
    }

    impl SlipwayWidgetTypes for DriftModalWidget {
        type ExternalState = AppExternal;
        type LocalState = ModalLocal;
        type AppMessage = AppMessage;
    }

    impl SlipwaySsot for DriftModalWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
        }
        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::PointerInput]
        }
        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }
        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayEventRoutingPolicy for DriftModalWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> EventRoutingPolicyDeclaration {
            EventRoutingPolicyDeclaration {
                target: self.id(),
                event_target: event.target().clone(),
                route: EventRoute {
                    route_id: None,
                    address: event.target_slot().cloned(),
                    path: vec![self.id()],
                    phase: EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayLogic for DriftModalWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            if event.target() != &self.id() {
                return EventOutcome::ignored();
            }
            match &event {
                InputEvent::Pointer(pointer)
                    if pointer.kind == PointerEventKind::Press && local.open =>
                {
                    local.open = false;
                    EventOutcome::handled()
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayEventDispositionPolicy for DriftModalWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            route: &EventRoute,
        ) -> EventPropagationEvidence {
            // The audited drift, verbatim in shape: a second hand-written
            // predicate (the fixture app's `handles_kpi_card`) that
            // declares every press handled and never consults the local
            // state the handler consults.
            let handled = event.target() == &self.id()
                && matches!(
                    event,
                    InputEvent::Pointer(pointer) if pointer.kind == PointerEventKind::Press
                );
            target_event_disposition(self.id(), event, route, handled)
        }
    }

    fn modal_press(target: &str) -> InputEvent {
        InputEvent::Pointer(PointerEvent {
            target: WidgetId::from(target),
            target_slot: None,
            position: Point { x: 4.0, y: 4.0 },
            target_bounds: None,
            kind: PointerEventKind::Press,
            button: Some(PointerButton::Primary),
            details: PointerDetails::default(),
        })
    }

    fn modal_wheel(target: &str, delta_y: f32) -> InputEvent {
        InputEvent::Wheel(WheelEvent {
            target: WidgetId::from(target),
            target_slot: None,
            region_id: None,
            delta_x: 0.0,
            delta_y,
        })
    }

    #[test]
    fn event_handling_table_disposition_matches_handler_by_construction() {
        let widget = TableModalWidget {
            id: WidgetId::from("modal"),
        };
        let cases: Vec<(InputEvent, ModalLocal, bool)> = vec![
            (modal_press("modal"), ModalLocal { open: true }, true),
            (modal_press("modal"), ModalLocal { open: false }, false),
            (modal_wheel("modal", -1.0), ModalLocal { open: true }, true),
            (modal_wheel("modal", 1.0), ModalLocal { open: true }, false),
            (command("modal"), ModalLocal { open: true }, false),
            (modal_press("elsewhere"), ModalLocal { open: true }, false),
        ];

        for (event, local, expected_handled) in cases {
            let declaration = declared_event_handling(&widget, &AppExternal, &local, &event);
            let declared = declaration.disposition.final_disposition.handled;
            assert_eq!(
                declared, expected_handled,
                "declared disposition for {event:?} with {local:?}"
            );

            // The backend physical gate: declared-unhandled events are
            // refused WITHOUT calling the handler; declared-handled events
            // run the handler and reconcile.
            if declared {
                let mut handler_local = local.clone();
                let raw_outcome =
                    widget.handle_event(&AppExternal, &mut handler_local, event.clone());
                assert!(raw_outcome.handled, "handler agrees for {event:?}");
                let outcome = apply_physical_event_handling_declaration(declaration, raw_outcome);
                assert!(
                    !outcome.diagnostics.iter().any(|diagnostic| {
                        diagnostic.code.starts_with("event_declaration.handler_")
                    }),
                    "no reconciliation mismatch for {event:?}: {:?}",
                    outcome.diagnostics
                );
            } else {
                // The handler, asked anyway (the semantic path), agrees:
                // the same pattern+guard tokens decided both sides.
                let mut handler_local = local.clone();
                let raw_outcome =
                    widget.handle_event(&AppExternal, &mut handler_local, event.clone());
                assert!(!raw_outcome.handled, "handler agrees for {event:?}");
                assert_eq!(handler_local, local, "ignored events do not mutate");
                let refusal: EventOutcome<AppMessage> =
                    refuse_event_declared_unhandled(declaration);
                assert!(!refusal.handled);
                assert!(refusal.propagate);
            }
        }
    }

    #[test]
    fn hand_written_disposition_drift_recurs_while_the_table_form_cannot_express_it() {
        // OLD form: the fixture app's NC-8 drift shape — declared handled
        // for a press the handler ignores — still produces the live Error
        // pair on the physical path (the unchanged runtime backstop).
        let drift = DriftModalWidget {
            id: WidgetId::from("modal"),
        };
        let closed = ModalLocal { open: false };
        let event = modal_press("modal");
        let declaration = declared_event_handling(&drift, &AppExternal, &closed, &event);
        assert!(declaration.disposition.final_disposition.handled);
        let mut local = closed.clone();
        let raw_outcome = drift.handle_event(&AppExternal, &mut local, event.clone());
        assert!(!raw_outcome.handled);
        let outcome = apply_physical_event_handling_declaration(declaration, raw_outcome);
        assert!(outcome.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == EVENT_DECLARATION_HANDLER_IGNORED_DECLARED_HANDLED
                && diagnostic.severity == DiagnosticSeverity::Error
        }));

        // NEW form: the same widget shape authored via
        // `event_handling_table!` cannot express that drift — the arm's
        // pattern+guard IS the declaration, so the closed-modal press is
        // declared unhandled AND ignored by the same tokens, and neither
        // reconciliation diagnostic can fire.
        let table = TableModalWidget {
            id: WidgetId::from("modal"),
        };
        let declaration = declared_event_handling(&table, &AppExternal, &closed, &event);
        assert!(!declaration.disposition.final_disposition.handled);
        let mut local = closed.clone();
        let raw_outcome = table.handle_event(&AppExternal, &mut local, event.clone());
        assert!(!raw_outcome.handled);
        let outcome = apply_physical_event_handling_declaration(declaration, raw_outcome);
        assert!(
            !outcome
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.starts_with("event_declaration.handler_")),
            "{:?}",
            outcome.diagnostics
        );

        // And the open-modal press is handled on BOTH sides, with the
        // emitted message intact (nothing stripped).
        let open = ModalLocal { open: true };
        let declaration = declared_event_handling(&table, &AppExternal, &open, &event);
        assert!(declaration.disposition.final_disposition.handled);
        let mut local = open.clone();
        let raw_outcome = table.handle_event(&AppExternal, &mut local, event);
        assert!(raw_outcome.handled);
        assert!(!local.open);
        let outcome = apply_physical_event_handling_declaration(declaration, raw_outcome);
        assert_eq!(outcome.emitted_messages.len(), 1);
        assert!(
            !outcome
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.starts_with("event_declaration.handler_"))
        );
    }

    #[test]
    fn duplicate_child_events_route_by_exact_slot_before_widget_id() {
        let widgets = (
            CounterWidget {
                id: WidgetId::from("duplicate"),
                origin_x: 0.0,
            },
            CounterWidget {
                id: WidgetId::from("duplicate"),
                origin_x: 32.0,
            },
        );
        let external = AppExternal;
        let mut local = widgets.initial_child_local_state();
        let parent_slot = WidgetSlotAddress::new(WidgetId::from("app"), 0);
        let second_slot = parent_slot.child(WidgetId::from("duplicate"), 1);
        let event = command_with_slot("duplicate", second_slot.clone());

        assert_eq!(event.target_slot(), Some(&second_slot));

        let outcome = widgets.route_event(&external, &mut local, &parent_slot, event);

        assert!(outcome.handled);
        assert_eq!(local.0.count, 0);
        assert_eq!(local.1.count, 1);
        assert_eq!(
            outcome.changes[0].slot.as_ref().map(|slot| slot.ordinal),
            Some(1)
        );
    }

    #[test]
    fn canonical_widget_slot_mounting_is_idempotent_through_three_levels() {
        let root = WidgetSlotAddress::new(WidgetId::from("root"), 0);
        let app_1 = root.child(WidgetId::from("app-1"), 0);
        let app_2 = app_1.child(WidgetId::from("app-2"), 0);
        let app_3 = app_2.child(WidgetId::from("app-3"), 0);
        let leaf = app_3.child(WidgetId::from("leaf"), 7);

        let rows = [
            (
                WidgetSlotAddress::new(WidgetId::from("app-1"), 0),
                root.clone(),
                app_1.clone(),
            ),
            (
                WidgetSlotAddress::new(WidgetId::from("app-1"), 0)
                    .child(WidgetId::from("app-2"), 0),
                app_1.clone(),
                app_2.clone(),
            ),
            (
                WidgetSlotAddress::new(WidgetId::from("app-3"), 0).child(WidgetId::from("leaf"), 7),
                app_3.clone(),
                leaf.clone(),
            ),
            (leaf.clone(), app_3.clone(), leaf.clone()),
        ];

        for (child, parent, expected) in rows {
            let mounted = mount_widget_slot_address(child, &parent);
            assert_eq!(mounted, expected);
            assert_eq!(
                mount_widget_slot_address(mounted.clone(), &parent),
                mounted,
                "mounting an already-rooted address must not add a prefix"
            );
        }
    }

    #[test]
    fn nested_app_routing_preserves_full_address_at_one_two_and_three_hops() {
        let external = AppExternal;

        let one = nested_route_app(
            "root",
            nested_route_app(
                "app-1",
                CounterWidget {
                    id: WidgetId::from("leaf"),
                    origin_x: 0.0,
                },
            ),
        );
        let one_slot = WidgetSlotAddress::new(WidgetId::from("root"), 0)
            .child(WidgetId::from("app-1"), 0)
            .child(WidgetId::from("leaf"), 0);
        let mut one_local = one.initial_local_state();
        let one_event = command_with_slot("leaf", one_slot.clone());
        let one_declaration = declared_event_handling(&one, &external, &one_local, &one_event);
        assert_eq!(
            one_declaration.routing.route.address,
            Some(one_slot.clone())
        );
        let one_outcome = one.handle_event(&external, &mut one_local, one_event);
        assert!(one_outcome.handled);
        assert_eq!(one_local.widgets.0.widgets.0.count, 1);
        assert_eq!(one_outcome.changes[0].slot, Some(one_slot.clone()));
        assert_eq!(one_outcome.observations[0].slot, Some(one_slot));

        let two = nested_route_app(
            "root",
            nested_route_app(
                "app-1",
                nested_route_app(
                    "app-2",
                    CounterWidget {
                        id: WidgetId::from("leaf"),
                        origin_x: 0.0,
                    },
                ),
            ),
        );
        let two_slot = WidgetSlotAddress::new(WidgetId::from("root"), 0)
            .child(WidgetId::from("app-1"), 0)
            .child(WidgetId::from("app-2"), 0)
            .child(WidgetId::from("leaf"), 0);
        let mut two_local = two.initial_local_state();
        let two_outcome = two.handle_event(
            &external,
            &mut two_local,
            command_with_slot("leaf", two_slot.clone()),
        );
        assert!(two_outcome.handled);
        assert_eq!(two_local.widgets.0.widgets.0.widgets.0.count, 1);
        assert_eq!(two_outcome.changes[0].slot, Some(two_slot));

        let three = nested_route_app(
            "root",
            nested_route_app(
                "app-1",
                nested_route_app(
                    "app-2",
                    nested_route_app(
                        "app-3",
                        CounterWidget {
                            id: WidgetId::from("leaf"),
                            origin_x: 0.0,
                        },
                    ),
                ),
            ),
        );
        let three_slot = WidgetSlotAddress::new(WidgetId::from("root"), 0)
            .child(WidgetId::from("app-1"), 0)
            .child(WidgetId::from("app-2"), 0)
            .child(WidgetId::from("app-3"), 0)
            .child(WidgetId::from("leaf"), 0);
        let mut three_local = three.initial_local_state();
        let three_outcome = three.handle_event(
            &external,
            &mut three_local,
            command_with_slot("leaf", three_slot.clone()),
        );
        assert!(three_outcome.handled);
        assert_eq!(three_local.widgets.0.widgets.0.widgets.0.widgets.0.count, 1);
        assert_eq!(three_outcome.changes[0].slot, Some(three_slot.clone()));

        let mut wrong_local = three.initial_local_state();
        let mut wrong_ordinal = three_slot;
        wrong_ordinal.ordinal = 9;
        let wrong_outcome = three.handle_event(
            &external,
            &mut wrong_local,
            command_with_slot("leaf", wrong_ordinal),
        );
        assert!(!wrong_outcome.handled);
        assert_eq!(wrong_local.widgets.0.widgets.0.widgets.0.widgets.0.count, 0);
    }

    #[test]
    fn nested_sibling_collision_uses_ancestor_branch_not_leaf_ordinal() {
        let leaf = || CounterWidget {
            id: WidgetId::from("leaf"),
            origin_x: 0.0,
        };
        let widgets = (
            nested_route_app("left-app", leaf()),
            nested_route_app("right-app", leaf()),
        );
        let external = AppExternal;
        let mut local = widgets.initial_child_local_state();
        let root = WidgetSlotAddress::new(WidgetId::from("root"), 0);
        let right = root
            .child(WidgetId::from("right-app"), 1)
            .child(WidgetId::from("leaf"), 0);

        let outcome = widgets.route_event(
            &external,
            &mut local,
            &root,
            command_with_slot("leaf", right.clone()),
        );

        assert!(outcome.handled);
        assert_eq!(local.0.widgets.0.count, 0);
        assert_eq!(local.1.widgets.0.count, 1);
        assert_eq!(outcome.changes[0].slot, Some(right));

        let mut refused_local = widgets.initial_child_local_state();
        let wrong_branch = root
            .child(WidgetId::from("missing-app"), 0)
            .child(WidgetId::from("leaf"), 0);
        let wrong_outcome = widgets.route_event(
            &external,
            &mut refused_local,
            &root,
            command_with_slot("leaf", wrong_branch),
        );
        assert!(!wrong_outcome.handled);

        let ambiguous = WidgetSlotAddress {
            widget: WidgetId::from("leaf"),
            path: vec![
                WidgetId::from("root"),
                WidgetId::from("left-app"),
                WidgetId::from("root"),
                WidgetId::from("left-app"),
                WidgetId::from("leaf"),
            ],
            ordinal: 0,
        };
        let ambiguous_outcome = widgets.route_event(
            &external,
            &mut refused_local,
            &root,
            command_with_slot("leaf", ambiguous),
        );
        assert!(!ambiguous_outcome.handled);
        assert_eq!(refused_local.0.widgets.0.count, 0);
        assert_eq!(refused_local.1.widgets.0.count, 0);
    }

    #[test]
    fn duplicate_child_event_with_nonmatching_slot_does_not_fallback_to_widget_id() {
        let widgets = (
            CounterWidget {
                id: WidgetId::from("duplicate"),
                origin_x: 0.0,
            },
            CounterWidget {
                id: WidgetId::from("duplicate"),
                origin_x: 32.0,
            },
        );
        let external = AppExternal;
        let mut local = widgets.initial_child_local_state();
        let parent_slot = WidgetSlotAddress::new(WidgetId::from("app"), 0);
        let missing_slot = parent_slot.child(WidgetId::from("duplicate"), 99);

        let outcome = widgets.route_event(
            &external,
            &mut local,
            &parent_slot,
            command_with_slot("duplicate", missing_slot),
        );

        assert!(!outcome.handled);
        assert_eq!(local.0.count, 0);
        assert_eq!(local.1.count, 0);
    }

    #[test]
    fn child_route_refuses_declared_unhandled_without_mutating_local_state() {
        let widgets = (CounterWidget {
            id: WidgetId::from("one"),
            origin_x: 0.0,
        },);
        let external = AppExternal;
        let mut local = widgets.initial_child_local_state();
        let parent_slot = WidgetSlotAddress::new(WidgetId::from("app"), 0);
        let child_slot = parent_slot.child(WidgetId::from("one"), 0);
        let event = command_with_slot("other", child_slot);

        let outcome = widgets.route_event(&external, &mut local, &parent_slot, event);

        assert!(!outcome.handled);
        assert_eq!(local.0.count, 0);
        assert!(outcome.emitted_messages.is_empty());
        assert!(outcome.changes.is_empty());
        assert!(outcome.diagnostics.iter().any(|diagnostic| diagnostic.code
            == EVENT_DECLARATION_HANDLER_HANDLED_DECLARED_UNHANDLED
            && diagnostic.severity == DiagnosticSeverity::Error));
    }

    #[test]
    fn app_layout_returns_child_placements_with_slot_addresses() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = layout_view(&widget, &external, &local, app_layout_input());

        assert_eq!(layout.child_placements.len(), 2);
        assert_eq!(layout.child_placements[0].child, WidgetId::from("one"));
        assert_eq!(layout.child_placements[1].child, WidgetId::from("two"));
        assert_eq!(layout.child_placements[0].bounds.as_rect().origin.x, 0.0);
        assert_eq!(layout.child_placements[1].bounds.as_rect().origin.x, 32.0);
        assert!(layout.child_placements[0].local_state_slot.is_some());
        assert!(layout.child_placements[1].local_state_slot.is_some());
    }

    #[test]
    fn vertical_app_gives_each_child_a_distinct_layout_viewport() {
        let widget = vertical_echo_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = layout_view(&widget, &external, &local, app_layout_input());

        assert_eq!(layout.child_placements.len(), 2);
        assert_eq!(layout.child_placements[0].child, WidgetId::from("top"));
        assert_eq!(layout.child_placements[1].child, WidgetId::from("bottom"));
        assert_eq!(layout.child_placements[0].bounds.as_rect().origin.y, 0.0);
        assert_eq!(
            layout.child_placements[0].bounds.as_rect().size.height,
            12.0
        );
        assert_eq!(layout.child_placements[1].bounds.as_rect().origin.y, 12.0);
        assert_eq!(
            layout.child_placements[1].bounds.as_rect().size.height,
            20.0
        );
        assert_ne!(
            layout.child_placements[0].bounds,
            layout.child_placements[1].bounds
        );
    }

    #[test]
    fn child_layout_observes_per_child_viewport_not_root_viewport() {
        let widget = vertical_echo_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let root_input = app_layout_input();
        let layout = layout_view(&widget, &external, &local, root_input.clone());

        assert_eq!(layout.bounds, root_input.viewport);
        assert_ne!(
            layout.child_placements[0].bounds.into_rect(),
            root_input.viewport.into_rect()
        );
        assert_ne!(
            layout.child_placements[1].bounds.into_rect(),
            root_input.viewport.into_rect()
        );
        assert_eq!(layout.child_placements[0].bounds.as_rect().size.width, 96.0);
        assert_eq!(layout.child_placements[1].bounds.as_rect().size.width, 96.0);
    }

    #[test]
    fn scroll_app_local_state_changes_through_routed_wheel_and_command_events() {
        let widget = scroll_echo_app_widget();
        let external = AppExternal;
        let mut local = widget.initial_local_state();

        let wheel_outcome = widget.handle_event(&external, &mut local, wheel("scroll-app", 15.0));

        assert!(wheel_outcome.handled);
        assert_eq!(local.app.offset_y, 15.0);
        assert_eq!(local.widgets.0.count, 0);
        assert_eq!(local.widgets.1.count, 0);
        assert_eq!(local.widgets.2.count, 0);
        assert_eq!(
            wheel_outcome.changes[0].target,
            WidgetId::from("scroll-app")
        );

        let command_outcome = widget.handle_event(&external, &mut local, scroll_home_command());

        assert!(command_outcome.handled);
        assert_eq!(local.app.offset_y, 0.0);
    }

    #[test]
    fn scroll_app_lays_out_n_children_offset_by_scroll_state() {
        let widget = scroll_echo_app_widget();
        let external = AppExternal;
        let mut local = widget.initial_local_state();
        assert!(
            widget
                .handle_event(&external, &mut local, wheel("scroll-app", 15.0))
                .handled
        );

        let layout = layout_view(&widget, &external, &local, app_layout_input());
        let origins: Vec<f32> = layout
            .child_placements
            .iter()
            .map(|placement| placement.bounds.as_rect().origin.y)
            .collect();

        assert_eq!(origins, vec![-15.0, 5.0, 25.0]);
        assert_eq!(layout.child_placements.len(), 3);
    }

    #[test]
    fn final_layout_preserves_real_child_slot_addresses() {
        let widget = scroll_echo_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = layout_view(&widget, &external, &local, app_layout_input());

        let slots: Vec<Vec<WidgetId>> = layout
            .child_placements
            .iter()
            .map(|placement| {
                placement
                    .local_state_slot
                    .as_ref()
                    .expect("child placement has tuple slot")
                    .path
                    .clone()
            })
            .collect();

        assert_eq!(
            slots,
            vec![
                vec![WidgetId::from("scroll-app"), WidgetId::from("row-one")],
                vec![WidgetId::from("scroll-app"), WidgetId::from("row-two")],
                vec![WidgetId::from("scroll-app"), WidgetId::from("row-three")],
            ]
        );
    }

    #[test]
    fn duplicate_child_ids_are_matched_by_slot_for_layout_and_paint() {
        let widget = duplicate_slot_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = layout_view(&widget, &external, &local, app_layout_input());

        assert_eq!(layout.child_placements.len(), 2);
        assert_eq!(
            layout.child_placements[0].child,
            WidgetId::from("duplicate")
        );
        assert_eq!(
            layout.child_placements[1].child,
            WidgetId::from("duplicate")
        );
        assert_eq!(layout.child_placements[0].bounds.as_rect().origin.x, 0.0);
        assert_eq!(layout.child_placements[0].bounds.as_rect().origin.y, 0.0);
        assert_eq!(layout.child_placements[1].bounds.as_rect().origin.x, 100.0);
        assert_eq!(layout.child_placements[1].bounds.as_rect().origin.y, 20.0);

        let slots: Vec<WidgetSlotAddress> = layout
            .child_placements
            .iter()
            .map(|placement| {
                placement
                    .local_state_slot
                    .clone()
                    .expect("duplicate child placement has a slot")
            })
            .collect();
        assert_eq!(slots[0].ordinal, 0);
        assert_eq!(slots[1].ordinal, 1);
        assert_eq!(
            slots[0].path,
            vec![widget.id(), WidgetId::from("duplicate")]
        );
        assert_eq!(
            slots[1].path,
            vec![widget.id(), WidgetId::from("duplicate")]
        );

        let paint = widget.paint(&external, &local, &layout);
        let painted: Vec<(String, Rect)> = paint
            .iter()
            .filter_map(|op| match op {
                PaintOp::Text {
                    content, bounds, ..
                } => Some((content.clone(), *bounds)),
                _ => None,
            })
            .collect();

        assert_eq!(
            painted,
            vec![
                (
                    "duplicate:1".to_string(),
                    layout.child_placements[0].bounds.into_rect()
                ),
                (
                    "duplicate:2".to_string(),
                    layout.child_placements[1].bounds.into_rect()
                ),
            ]
        );
    }

    #[test]
    fn child_paint_uses_final_child_placements() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = layout_view(&widget, &external, &local, app_layout_input());
        let paint = widget.paint(&external, &local, &layout);

        let text_bounds: Vec<Rect> = paint
            .iter()
            .filter_map(|op| match op {
                PaintOp::Text { bounds, .. } => Some(*bounds),
                _ => None,
            })
            .collect();

        assert_eq!(
            text_bounds,
            vec![
                layout.child_placements[0].bounds.into_rect(),
                layout.child_placements[1].bounds.into_rect()
            ]
        );
        assert_eq!(text_bounds[1].origin.x, 32.0);
    }

    #[test]
    fn app_view_definition_mounts_child_regions_from_target_local_input() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = layout_view(&widget, &external, &local, app_layout_input());
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );

        assert_eq!(view.hit_regions.len(), 2);
        assert_eq!(
            view.hit_regions[0].bounds.into_rect(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: layout.child_placements[0].bounds.as_rect().size,
            }
        );
        assert_eq!(
            view.hit_regions[1].bounds.into_rect(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: layout.child_placements[1].bounds.as_rect().size,
            }
        );
        assert!(
            !view.diagnostics.iter().any(|diagnostic| diagnostic.code
                == "view_contract.child_input_viewport_not_target_local")
        );
        assert!(!view_definition_has_blocking_contract_diagnostic(
            &view_definition_contract_diagnostics(&view)
        ));
    }

    // Default-equivalence guard for the Step-210 overflow composition: an
    // app with NO overflow-declaring child keeps the exact pre-existing
    // composed paint order.
    #[test]
    fn app_view_definition_without_child_overflow_keeps_source_order_paint_order() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );

        assert_eq!(
            view.paint_order,
            PaintOrderDeclaration::source_order(WidgetId::from("app"))
        );
        assert!(!view.paint_order.allow_overflow_paint);
        assert!(view.paint_order.overflow_bounds.is_none());
    }

    // Step-210 composition rule: a child-declared overflow allowance
    // (`PaintOrderDeclaration::with_overflow_bounds`, the roaming-overlay
    // pattern) survives into the composed app view, translated by the
    // child's placement and unioned with the root layout bounds —
    // otherwise composed-level re-validation refuses the very regions the
    // child's own admission accepted.
    #[test]
    fn app_view_definition_propagates_child_overflow_bounds_into_composed_paint_order() {
        let widget = SlipwayAppWidget::new(TwoCounterApp {
            widgets: (
                CounterWidget {
                    id: WidgetId::from("one"),
                    origin_x: 0.0,
                },
                // Declares child-local overflow bounds (-8,-4,48,40); the
                // app plan places child index 1 at (32, 0).
                CounterWidget {
                    id: WidgetId::from("overflow"),
                    origin_x: 32.0,
                },
            ),
        });
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );

        assert!(view.paint_order.allow_overflow_paint);
        // Union of the translated child allowance (24,-4,48,40) and the
        // root layout bounds (0,0,96,32).
        assert_eq!(
            view.paint_order
                .overflow_bounds
                .expect("composed view must keep the child allowance")
                .into_rect(),
            Rect {
                origin: Point { x: 0.0, y: -4.0 },
                size: Size {
                    width: 96.0,
                    height: 40.0,
                },
            }
        );
        assert!(!view_definition_has_blocking_contract_diagnostic(
            &view_definition_contract_diagnostics(&view)
        ));
    }

    // NC-11 fixture (roadmap Phase 6 item 5): the naive-consumer app's
    // EXACT modal tie shape — an opaque full-viewport overlay child
    // (`PaintLayerKey::new(100)`, `allow_overlap = true` for the paint
    // overlap, overflow bounds declared) whose full-bounds hit region
    // carries the DEFAULT `HitRegionOrder { 0, 0, 0 }`, mounted beside a
    // card child whose hit region carries the same default order. Each
    // widget alone has one hit region (nothing to pair); only the
    // composed view can see the tie.
    #[derive(Clone, Debug, PartialEq)]
    struct ModalOverlayTieWidget {
        id: WidgetId,
        hit_z_index: i32,
    }

    impl SlipwayWidgetTypes for ModalOverlayTieWidget {
        type ExternalState = AppExternal;
        type LocalState = CounterLocal;
        type AppMessage = AppMessage;
    }

    impl SlipwaySsot for ModalOverlayTieWidget {
        fn id(&self) -> WidgetId {
            self.id.clone()
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

    impl SlipwayLogic for ModalOverlayTieWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for ModalOverlayTieWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            CounterLocal { count: 0 }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }

        fn paint(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Layer {
                id: Some(format!("{}:layer", self.id.as_str())),
                key: PaintLayerKey::new(100),
                input_transparency: PaintInputTransparency::Opaque,
                wheel_transparency: None,
                clip: None,
                ops: vec![PaintOp::Fill {
                    shape: ShapeDeclaration {
                        id: Some(format!("{}:backdrop", self.id.as_str())),
                        kind: ShapeKind::Rectangle,
                        bounds: layout.bounds.into_rect(),
                        path: None,
                        clip: None,
                    },
                    color: Color {
                        red: 1.0,
                        green: 1.0,
                        blue: 1.0,
                        alpha: 1.0,
                    },
                }],
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

    impl SlipwayViewDefinition for ModalOverlayTieWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = layout_view(self, external, local, input.layout_input);
            let paint = self.paint(external, local, &layout);
            let bounds = layout.bounds;
            let mut paint_order = PaintOrderDeclaration::layer(self.id(), 100);
            paint_order.allow_overlap = true;
            paint_order = paint_order.with_overflow_bounds(bounds);

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                hit_regions: vec![HitRegionDeclaration {
                    id: PresentationRegionId::from(format!("{}:hit", self.id.as_str())),
                    target: self.id(),
                    address: Some(WidgetSlotAddress::new(self.id(), 0)),
                    bounds,
                    event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
                    order: HitRegionOrder {
                        z_index: self.hit_z_index,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    route: EventRoute {
                        route_id: Some(format!("{}:route", self.id.as_str())),
                        address: Some(WidgetSlotAddress::new(self.id(), 0)),
                        path: vec![self.id()],
                        phase: EventRoutePhase::Target,
                    },
                    cursor: CursorCapability::Pointer,
                    enabled: true,
                    capture: PointerCaptureIntent::OnPress,
                    capture_evidence: Vec::new(),
                }],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                wheel_traversal_boundary: Default::default(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                paint_order,
                layout,
                paint,
            }
        }
    }

    impl_core_test_event_policy!(ModalOverlayTieWidget);

    #[derive(Clone, Debug, PartialEq)]
    struct ModalTieApp {
        widgets: (CounterWidget, ModalOverlayTieWidget),
    }

    impl SlipwayApp for ModalTieApp {
        type ExternalState = AppExternal;
        type LocalState = AppLocal;
        type AppMessage = AppMessage;
        type Widgets = (CounterWidget, ModalOverlayTieWidget);

        fn id(&self) -> WidgetId {
            WidgetId::from("modal-tie-app")
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {
            AppLocal
        }

        fn layout_plan(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            children: Vec<ChildLayoutSeed>,
        ) -> AppLayoutPlan {
            let viewport = input.viewport.into_rect();
            AppLayoutPlan {
                bounds: input.viewport,
                children: children
                    .into_iter()
                    .enumerate()
                    .map(|(index, seed)| {
                        // The card sits inside the viewport; the modal
                        // covers the ENTIRE viewport (the consumer's
                        // full-viewport detail modal).
                        let bounds = if index == 0 {
                            Rect {
                                origin: Point { x: 16.0, y: 8.0 },
                                size: Size {
                                    width: 48.0,
                                    height: 16.0,
                                },
                            }
                        } else {
                            viewport
                        };
                        ChildLayoutPlan::explicit_border(
                            seed,
                            ContentLocalRect::new(bounds),
                            BoxSpacing::ZERO,
                        )
                    })
                    .collect(),
                diagnostics: Vec::new(),
            }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            input: LayoutInput,
            output: LayoutOutputBuilder,
        ) -> LayoutOutput {
            output.finish(input.viewport)
        }
    }

    fn modal_tie_app_widget(hit_z_index: i32) -> SlipwayAppWidget<ModalTieApp> {
        SlipwayAppWidget::new(ModalTieApp {
            widgets: (
                CounterWidget {
                    id: WidgetId::from("card"),
                    origin_x: 0.0,
                },
                ModalOverlayTieWidget {
                    id: WidgetId::from("modal"),
                    hit_z_index,
                },
            ),
        })
    }

    // NC-11 acceptance (roadmap Phase 6 item 5): the consumer's exact
    // modal tie shape MUST be flagged at pre-flight. Composed scope pin:
    // per-widget pre-flight cannot see this tie (each child view has one
    // hit region), so the composed `SlipwayAppWidget` view — the view the
    // visible backends admit — is the catching surface. The child's
    // paint-only `allow_overlap` must not shield the composed check.
    #[test]
    fn composed_app_view_flags_consumer_modal_hit_tie_at_preflight() {
        let widget = modal_tie_app_widget(0);
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );

        // Per-widget pre-flight is structurally blind here: one region.
        let modal_view = widget.app.widgets.1.view_definition(
            &external,
            &CounterLocal { count: 0 },
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );
        assert_eq!(modal_view.hit_regions.len(), 1);
        assert!(modal_view.paint_order.allow_overlap);

        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
        let overlap = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "view_contract.ambiguous_hit_overlap")
            .expect("the composed consumer modal tie shape must be refused at pre-flight");
        assert!(
            overlap.message.contains("`card.hit`"),
            "{}",
            overlap.message
        );
        assert!(
            overlap.message.contains("`modal:hit`"),
            "{}",
            overlap.message
        );
    }

    // The consumer's NC-3 z-index repair (modal hit region at z 100) is
    // the legitimate resolution of the same shape: distinct orders admit
    // cleanly, pinning the guard against false positives on layered
    // overlaps.
    #[test]
    fn composed_app_view_admits_consumer_modal_shape_with_distinct_hit_order() {
        let widget = modal_tie_app_widget(100);
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );

        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.ambiguous_hit_overlap"),
            "{diagnostics:?}"
        );
        assert!(
            !diagnostics.iter().any(|diagnostic| diagnostic.code
                == "view_contract.child_input_viewport_not_target_local")
        );
    }

    // Step-212 wrapper-less page scroll: an app that overrides
    // `SlipwayApp::app_scroll_regions` gets its declarations appended to
    // the COMPOSED view (the root/page-scroll pattern), and an app that
    // does not override the hook keeps a byte-identical composed view
    // (no app-declared region appears).
    #[derive(Clone, Debug, PartialEq)]
    struct PageScrollApp {
        widgets: (CounterWidget, CounterWidget),
    }

    impl SlipwayApp for PageScrollApp {
        type ExternalState = AppExternal;
        type LocalState = AppLocal;
        type AppMessage = AppMessage;
        type Widgets = (CounterWidget, CounterWidget);

        fn id(&self) -> WidgetId {
            WidgetId::from("page-app")
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {
            AppLocal
        }

        fn app_scroll_regions(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _frame: &FrameIdentity,
            input: &LayoutInput,
            layout: &LayoutOutput,
        ) -> Vec<ScrollRegionDeclaration> {
            let viewport = input.viewport.into_rect();
            vec![ScrollRegionDeclaration::explicit(
                PresentationRegionId::from("page-app:page-scroll"),
                self.id(),
                Some(WidgetSlotAddress::new(self.id(), 0)),
                TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: viewport.size,
                }),
                TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: layout.bounds.size.width.max(viewport.size.width),
                        height: (layout.bounds.size.height * 2.0).max(viewport.size.height),
                    },
                }),
                Point { x: 0.0, y: 0.0 },
                ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                WheelRouting::NearestScrollable,
                HitRegionOrder {
                    z_index: -1,
                    paint_order: 0,
                    traversal_order: 0,
                },
                ScrollConsumptionPolicy::exclusive_wheel(),
                true,
            )]
        }
    }

    #[test]
    fn app_view_definition_appends_app_scroll_regions() {
        let widget = SlipwayAppWidget::new(PageScrollApp {
            widgets: (
                CounterWidget {
                    id: WidgetId::from("one"),
                    origin_x: 0.0,
                },
                CounterWidget {
                    id: WidgetId::from("two"),
                    origin_x: 1.0,
                },
            ),
        });
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );
        let page = view
            .scroll_regions
            .iter()
            .find(|region| region.id == PresentationRegionId::from("page-app:page-scroll"))
            .expect("app-declared scroll region must appear in the composed view");
        assert_eq!(page.target, WidgetId::from("page-app"));
        assert_eq!(
            page.address,
            Some(WidgetSlotAddress::new(WidgetId::from("page-app"), 0))
        );

        // Default byte-identical: an app that does NOT override the hook
        // gains no app-targeted scroll region.
        let widget = two_counter_app_widget();
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );
        assert!(
            view.scroll_regions
                .iter()
                .all(|region| region.target != WidgetId::from("app")),
            "the empty default must add no app-level scroll region"
        );
    }

    #[test]
    fn view_contract_blocks_non_target_local_layout_bounds() {
        let widget = CounterWidget {
            id: WidgetId::from("counter"),
            origin_x: 0.0,
        };
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 8.0, y: 4.0 },
                        size: Size {
                            width: 24.0,
                            height: 16.0,
                        },
                    }),
                    content: TargetLocalRect::new(Rect {
                        origin: Point { x: 8.0, y: 4.0 },
                        size: Size {
                            width: 24.0,
                            height: 16.0,
                        },
                    }),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: Size {
                            width: 24.0,
                            height: 16.0,
                        },
                    },
                },
            ),
        );
        let diagnostics = view_definition_contract_diagnostics(&view);

        assert!(view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code
            == "view_contract.layout_bounds_not_target_local"
            && diagnostic.severity == DiagnosticSeverity::Error));
    }

    #[test]
    fn requested_child_input_is_prepared_target_local() {
        let widget = duplicate_slot_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), app_layout_input()),
        );
        let diagnostics = view_definition_contract_diagnostics(&view);

        assert!(
            !view.diagnostics.iter().any(|diagnostic| diagnostic.code
                == "view_contract.child_input_viewport_not_target_local")
        );
        assert!(
            !diagnostics.iter().any(|diagnostic| diagnostic.code
                == "view_contract.child_input_viewport_not_target_local")
        );
    }

    #[test]
    fn app_paint_includes_child_paint_from_both_widgets() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let mut local = widget.initial_local_state();
        assert!(
            widget
                .handle_event(&external, &mut local, command("two"))
                .handled
        );
        let layout = layout_view(&widget, &external, &local, app_layout_input());
        let paint = widget.paint(&external, &local, &layout);

        let labels: Vec<&str> = paint
            .iter()
            .filter_map(|op| match op {
                PaintOp::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(labels, vec!["one:0", "two:1"]);
    }

    fn paint_test_rect(x: f32) -> Rect {
        Rect {
            origin: Point { x, y: 0.0 },
            size: Size {
                width: 10.0,
                height: 10.0,
            },
        }
    }

    fn paint_test_color() -> Color {
        test_rgb(0, 0, 0)
    }

    fn paint_test_text(label: &str, x: f32) -> PaintOp {
        PaintOp::styled_text(
            paint_test_rect(x),
            label,
            paint_test_color(),
            TextStyle::plain(),
        )
    }

    fn paint_test_unit(id: &str, traversal_order: usize, paint: Vec<PaintOp>) -> PaintUnit {
        PaintUnit::source_order(WidgetId::from(id), None, traversal_order, paint)
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

    #[test]
    fn keyed_paint_subtrees_expand_into_separate_sorted_units() {
        let mut sibling_order = PaintOrderDeclaration::layer(WidgetId::from("sibling"), 7);
        sibling_order.order = Some(0);
        let sibling = PaintUnit {
            target: WidgetId::from("sibling"),
            address: None,
            order: sibling_order,
            traversal_order: 2,
            paint: vec![paint_test_text("sibling", 30.0)],
        };
        let paint = flatten_ordered_paint_units(vec![
            paint_test_unit(
                "root",
                1,
                vec![
                    paint_test_text("default", 0.0),
                    PaintOp::keyed_layer(
                        PaintLayerKey::ordered(10, 0),
                        vec![paint_test_text("top-key", 10.0)],
                    ),
                    PaintOp::keyed_layer_pass_through(
                        PaintLayerKey::ordered(5, 0),
                        vec![paint_test_text("middle-key", 20.0)],
                    ),
                ],
            ),
            sibling,
        ]);

        assert_eq!(
            paint_text_labels(&paint),
            vec!["default", "middle-key", "sibling", "top-key"]
        );
        assert!(paint.iter().any(|op| matches!(
            op,
            PaintOp::Layer {
                input_transparency: PaintInputTransparency::PassThrough,
                ..
            }
        )));
    }

    #[test]
    fn unkeyed_paint_remains_in_default_bottom_unit() {
        let units = expand_paint_unit_layers(paint_test_unit(
            "root",
            3,
            vec![
                paint_test_text("default", 0.0),
                PaintOp::keyed_layer(
                    PaintLayerKey::ordered(8, 0),
                    vec![paint_test_text("keyed", 10.0)],
                ),
            ],
        ));

        assert_eq!(units.len(), 2);
        assert_eq!(units[0].order.mode, PaintOrderMode::SourceOrder);
        assert_eq!(paint_text_labels(&units[0].paint), vec!["default"]);
        assert_eq!(units[1].order.mode, PaintOrderMode::ExplicitLayered);
        assert_eq!(units[1].order.z_index, 8);
        assert_eq!(units[1].order.order, Some(0));
        assert_eq!(paint_text_labels(&units[1].paint), vec!["keyed"]);
    }

    #[test]
    fn keyed_layer_wheel_transparency_defaults_to_automatic_and_resolves() {
        // Default (unspecified) opaque layer: automatic => blocks the wheel,
        // exactly as before the explicit wheel-transparency axis existed.
        let default_layer = PaintOp::keyed_layer(
            PaintLayerKey::ordered(10, 0),
            vec![paint_test_text("body", 0.0)],
        );
        let PaintOp::Layer {
            input_transparency,
            wheel_transparency,
            ..
        } = &default_layer
        else {
            panic!("keyed_layer builds a Layer");
        };
        assert_eq!(*input_transparency, PaintInputTransparency::Opaque);
        assert_eq!(*wheel_transparency, None);
        assert!(paint_layer_blocks_wheel(
            *input_transparency,
            *wheel_transparency
        ));

        // Explicit pointer-opaque + wheel-pass-through: still a pointer
        // occluder (input_transparency stays Opaque so an occlusion region is
        // still emitted) but the wheel channel resolves transparent.
        let wheel_through = PaintOp::keyed_layer(
            PaintLayerKey::ordered(10, 0),
            vec![paint_test_text("body", 0.0)],
        )
        .with_wheel_transparency(PaintInputTransparency::PassThrough);
        let PaintOp::Layer {
            input_transparency,
            wheel_transparency,
            ..
        } = &wheel_through
        else {
            panic!("keyed_layer builds a Layer");
        };
        assert_eq!(*input_transparency, PaintInputTransparency::Opaque);
        assert_eq!(
            *wheel_transparency,
            Some(PaintInputTransparency::PassThrough)
        );
        assert!(!paint_layer_blocks_wheel(
            *input_transparency,
            *wheel_transparency
        ));

        // Explicit wheel-opaque forces blocking even on a pointer-pass-through
        // layer; and the automatic pass-through layer still passes the wheel.
        assert!(paint_layer_blocks_wheel(
            PaintInputTransparency::PassThrough,
            Some(PaintInputTransparency::Opaque)
        ));
        assert!(!paint_layer_blocks_wheel(
            PaintInputTransparency::PassThrough,
            None
        ));
    }

    fn dispatch_graph_test_layout(width: f32, height: f32) -> LayoutOutput {
        LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size { width, height },
            }),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn dispatch_graph_test_hit(
        id: &str,
        bounds: Rect,
        order: HitRegionOrder,
        capture: PointerCaptureIntent,
    ) -> HitRegionDeclaration {
        let target = WidgetId::from("graph-root");
        HitRegionDeclaration {
            id: PresentationRegionId::from(id),
            target: target.clone(),
            address: None,
            bounds: TargetLocalRect::new(bounds),
            event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
            order,
            route: EventRoute {
                route_id: Some(id.to_string()),
                address: None,
                path: vec![target],
                phase: EventRoutePhase::Target,
            },
            cursor: CursorCapability::Pointer,
            enabled: true,
            capture,
            capture_evidence: Vec::new(),
        }
    }

    fn dispatch_graph_test_scroll(
        id: &str,
        viewport: Rect,
        content_height: f32,
        order: HitRegionOrder,
    ) -> ScrollRegionDeclaration {
        ScrollRegionDeclaration::explicit(
            PresentationRegionId::from(id),
            WidgetId::from("graph-root"),
            None,
            TargetLocalRect::new(viewport),
            TargetLocalRect::new(Rect {
                origin: viewport.origin,
                size: Size {
                    width: viewport.size.width,
                    height: content_height,
                },
            }),
            Point { x: 0.0, y: 0.0 },
            ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            WheelRouting::NearestScrollable,
            order,
            ScrollConsumptionPolicy::exclusive_wheel(),
            true,
        )
    }

    fn dispatch_graph_test_focus(id: &str, bounds: Rect) -> FocusRegionDeclaration {
        FocusRegionDeclaration {
            id: PresentationRegionId::from(id),
            target: WidgetId::from("graph-focus-target"),
            address: None,
            bounds: TargetLocalRect::new(bounds),
            member: None,
            enabled: true,
            text_edit: None,
        }
    }

    fn dispatch_graph_test_fixture() -> (
        PresentationGeometryIndex,
        Vec<HitRegionDeclaration>,
        Vec<FocusRegionDeclaration>,
        Vec<ScrollRegionDeclaration>,
    ) {
        let layout = dispatch_graph_test_layout(200.0, 200.0);
        let geometry_index = PresentationGeometryIndex::from_layout(&layout);
        let hit_regions = vec![
            dispatch_graph_test_hit(
                "hit-back",
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
                HitRegionOrder {
                    z_index: 0,
                    paint_order: 0,
                    traversal_order: 0,
                },
                PointerCaptureIntent::None,
            ),
            dispatch_graph_test_hit(
                "hit-front",
                Rect {
                    origin: Point { x: 50.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
                HitRegionOrder {
                    z_index: 1,
                    paint_order: 0,
                    traversal_order: 0,
                },
                PointerCaptureIntent::DuringDrag,
            ),
        ];
        let focus_regions = vec![dispatch_graph_test_focus(
            "focus-a",
            Rect {
                origin: Point { x: 0.0, y: 120.0 },
                size: Size {
                    width: 80.0,
                    height: 30.0,
                },
            },
        )];
        let scroll_regions = vec![
            dispatch_graph_test_scroll(
                "scroll-root",
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 200.0,
                        height: 200.0,
                    },
                },
                400.0,
                HitRegionOrder {
                    z_index: -1,
                    paint_order: 0,
                    traversal_order: 0,
                },
            ),
            dispatch_graph_test_scroll(
                "scroll-inner",
                Rect {
                    origin: Point { x: 20.0, y: 20.0 },
                    size: Size {
                        width: 100.0,
                        height: 100.0,
                    },
                },
                300.0,
                HitRegionOrder {
                    z_index: 1,
                    paint_order: 1,
                    traversal_order: 1,
                },
            ),
        ];
        (geometry_index, hit_regions, focus_regions, scroll_regions)
    }

    fn dispatch_graph_edges<'a>(
        graph: &'a DispatchGraph,
        kind: DispatchGraphEdgeKind,
        channel: DispatchGraphChannel,
    ) -> Vec<&'a DispatchGraphEdge> {
        graph
            .edges
            .iter()
            .filter(|edge| edge.kind == kind && edge.channel == channel)
            .collect()
    }

    #[test]
    fn dispatch_graph_nodes_carry_kinds_and_root_local_bounds() {
        let (geometry_index, hit_regions, focus_regions, scroll_regions) =
            dispatch_graph_test_fixture();
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &[],
        );

        assert_eq!(graph.target, WidgetId::from("graph-root"));
        assert_eq!(graph.nodes.len(), 5);
        let front = graph
            .nodes
            .iter()
            .find(|node| node.id == "hit-front")
            .expect("hit-front node");
        assert_eq!(front.kind, DispatchGraphNodeKind::Hit);
        assert_eq!(front.bounds.origin.x, 50.0);
        assert_eq!(front.capture, Some(PointerCaptureIntent::DuringDrag));
        let inner = graph
            .nodes
            .iter()
            .find(|node| node.id == "scroll-inner")
            .expect("scroll-inner node");
        assert_eq!(inner.kind, DispatchGraphNodeKind::Scroll);
        assert_eq!(inner.consumes_wheel, Some(true));
    }

    #[test]
    fn dispatch_graph_hit_order_edge_matches_selector_winner() {
        let (geometry_index, hit_regions, focus_regions, scroll_regions) =
            dispatch_graph_test_fixture();
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &[],
        );

        // The graph's pointer HitOrder edge must agree with the actual
        // selector at a point inside the overlap band (x in 50..100).
        let winner = select_declared_hit_region_at_root_local_point_with_geometry_index(
            &geometry_index,
            &hit_regions,
            Point { x: 75.0, y: 50.0 },
        )
        .expect("overlap point selects a hit region");
        let pointer_edges = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::HitOrder,
            DispatchGraphChannel::Pointer,
        );
        assert_eq!(pointer_edges.len(), 1);
        assert_eq!(pointer_edges[0].from, winner.id.as_str());
        assert_eq!(pointer_edges[0].from, "hit-front");
        assert_eq!(pointer_edges[0].to, "hit-back");

        // Wheel HitOrder precedence between the overlapping scroll regions
        // mirrors the scroll selector's order-key comparison.
        let wheel_edges = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::HitOrder,
            DispatchGraphChannel::Wheel,
        );
        assert_eq!(wheel_edges.len(), 1);
        assert_eq!(wheel_edges[0].from, "scroll-inner");
        assert_eq!(wheel_edges[0].to, "scroll-root");
    }

    #[test]
    fn dispatch_graph_occlusion_edges_respect_wheel_transparency() {
        let (geometry_index, hit_regions, focus_regions, scroll_regions) =
            dispatch_graph_test_fixture();
        let occlusions = vec![
            // Fully opaque overlay: blocks pointer AND wheel beneath it.
            DispatchGraphOcclusionRegion {
                target: WidgetId::from("graph-root"),
                address: None,
                order: HitRegionOrder {
                    z_index: 10,
                    paint_order: 0,
                    traversal_order: 0,
                },
                authored_z_order: Some(0),
                bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 80.0,
                        height: 80.0,
                    },
                },
                blocks_wheel: true,
            },
            // Pointer-opaque, wheel-transparent overlay (de24dda0a):
            // pointer edges only, no wheel edges.
            DispatchGraphOcclusionRegion {
                target: WidgetId::from("graph-root"),
                address: None,
                order: HitRegionOrder {
                    z_index: 10,
                    paint_order: 1,
                    traversal_order: 0,
                },
                authored_z_order: Some(1),
                bounds: Rect {
                    origin: Point { x: 60.0, y: 10.0 },
                    size: Size {
                        width: 60.0,
                        height: 60.0,
                    },
                },
                blocks_wheel: false,
            },
        ];
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &occlusions,
        );

        let opaque_id = "occlusion:graph-root:10:0:0:0";
        let wheel_through_id = "occlusion:graph-root:10:1:0:0";
        assert!(graph.nodes.iter().any(|node| {
            node.id == opaque_id
                && node.kind == DispatchGraphNodeKind::Occlusion
                && node.blocks_wheel == Some(true)
        }));
        assert!(graph.nodes.iter().any(|node| {
            node.id == wheel_through_id
                && node.kind == DispatchGraphNodeKind::Occlusion
                && node.blocks_wheel == Some(false)
        }));

        let pointer_occlusion = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::Occlusion,
            DispatchGraphChannel::Pointer,
        );
        let wheel_occlusion = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::Occlusion,
            DispatchGraphChannel::Wheel,
        );

        // The opaque overlay blocks both channels beneath it.
        assert!(
            pointer_occlusion
                .iter()
                .any(|edge| edge.from == opaque_id && edge.to == "hit-back")
        );
        assert!(
            pointer_occlusion
                .iter()
                .any(|edge| edge.from == opaque_id && edge.to == "hit-front")
        );
        assert!(
            wheel_occlusion
                .iter()
                .any(|edge| edge.from == opaque_id && edge.to == "scroll-root")
        );
        assert!(
            wheel_occlusion
                .iter()
                .any(|edge| edge.from == opaque_id && edge.to == "scroll-inner")
        );

        // The wheel-transparent overlay still occludes the pointer but has
        // NO wheel edges at all.
        assert!(
            pointer_occlusion
                .iter()
                .any(|edge| edge.from == wheel_through_id && edge.to == "hit-front")
        );
        assert!(
            wheel_occlusion
                .iter()
                .all(|edge| edge.from != wheel_through_id)
        );
    }

    /// NC-2 (roadmap Phase 6 item 1): the pointer `Occlusion` edges apply
    /// the same-owner rule of `paint_occlusion_blocks_declared_hit_region`,
    /// so the graph oracle agrees with repaired dispatch:
    ///
    /// * a widget's own opaque layer at the SAME z WITHOUT an authored
    ///   within-z order never occludes that widget's own hit region (the
    ///   consumer modal shape — occluder tie-break fields are the defaulted
    ///   slot ordinal);
    /// * a FOREIGN opaque layer with the identical order key still occludes
    ///   (stacking semantics unchanged);
    /// * a widget's own layer with an AUTHORED within-z order fronting the
    ///   region's declared `paint_order` still occludes (the authored
    ///   overlay-card stack, Step 168).
    #[test]
    fn dispatch_graph_pointer_occlusion_edges_apply_same_owner_same_z_rule() {
        let (geometry_index, mut hit_regions, focus_regions, scroll_regions) =
            dispatch_graph_test_fixture();
        // Put the front fixture region at the occluders' z so all three
        // occluders tie with it on z_index: hit-front is (100, 0, 0).
        hit_regions[1].order = HitRegionOrder {
            z_index: 100,
            paint_order: 0,
            traversal_order: 0,
        };
        let overlay_bounds = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 200.0,
                height: 200.0,
            },
        };
        let occlusions = vec![
            // The modal shape: same owner as hit-front ("graph-root"),
            // same z 100, defaulted tie-break fields (slot ordinal 7),
            // NO authored within-z order -> must NOT occlude hit-front.
            DispatchGraphOcclusionRegion {
                target: WidgetId::from("graph-root"),
                address: None,
                order: HitRegionOrder {
                    z_index: 100,
                    paint_order: 7,
                    traversal_order: 7,
                },
                authored_z_order: None,
                bounds: overlay_bounds,
                blocks_wheel: true,
            },
            // Identical key but FOREIGN owner -> still occludes hit-front.
            DispatchGraphOcclusionRegion {
                target: WidgetId::from("graph-foreign-overlay"),
                address: None,
                order: HitRegionOrder {
                    z_index: 100,
                    paint_order: 7,
                    traversal_order: 7,
                },
                authored_z_order: None,
                bounds: overlay_bounds,
                blocks_wheel: true,
            },
            // Same owner, same z, but an AUTHORED within-z order fronting
            // the region's declared paint_order (the overlay-card stack)
            // -> still occludes hit-front.
            DispatchGraphOcclusionRegion {
                target: WidgetId::from("graph-root"),
                address: None,
                order: HitRegionOrder {
                    z_index: 100,
                    paint_order: 3,
                    traversal_order: 0,
                },
                authored_z_order: Some(3),
                bounds: overlay_bounds,
                blocks_wheel: true,
            },
        ];
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &occlusions,
        );

        let pointer_occlusion = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::Occlusion,
            DispatchGraphChannel::Pointer,
        );
        let own_unordered_id = "occlusion:graph-root:100:7:7:0";
        let foreign_id = "occlusion:graph-foreign-overlay:100:7:7:0";
        let own_authored_id = "occlusion:graph-root:100:3:0:0";
        assert!(
            pointer_occlusion
                .iter()
                .all(|edge| !(edge.from == own_unordered_id && edge.to == "hit-front")),
            "a widget's own unordered same-z layer must not occlude its own hit region: {pointer_occlusion:?}"
        );
        assert!(
            pointer_occlusion
                .iter()
                .any(|edge| edge.from == foreign_id && edge.to == "hit-front"),
            "a foreign layer with the identical key still occludes: {pointer_occlusion:?}"
        );
        assert!(
            pointer_occlusion
                .iter()
                .any(|edge| edge.from == own_authored_id && edge.to == "hit-front"),
            "an authored-order own layer fronting the declared paint_order still occludes: {pointer_occlusion:?}"
        );
        // All three occluders still front the z-0 back region.
        for occluder in [own_unordered_id, foreign_id, own_authored_id] {
            assert!(
                pointer_occlusion
                    .iter()
                    .any(|edge| edge.from == occluder && edge.to == "hit-back"),
                "different-z regions keep full-key occlusion: {occluder}"
            );
        }
    }

    #[test]
    fn paint_occlusion_predicate_same_owner_rule() {
        let owner = WidgetId::from("occlusion-owner");
        let foreign = WidgetId::from("occlusion-foreign");
        let region_order = HitRegionOrder {
            z_index: 100,
            paint_order: 0,
            traversal_order: 0,
        };
        let slot_poisoned = HitRegionOrder {
            z_index: 100,
            paint_order: 7,
            traversal_order: 7,
        };
        // NC-2: same owner, same z, no authored order -> never blocks,
        // regardless of the defaulted tie-break fields.
        assert!(!paint_occlusion_blocks_declared_hit_region(
            &owner,
            &slot_poisoned,
            None,
            &owner,
            &region_order,
        ));
        // Foreign owner with the identical key -> blocks (strictly front).
        assert!(paint_occlusion_blocks_declared_hit_region(
            &foreign,
            &slot_poisoned,
            None,
            &owner,
            &region_order,
        ));
        // Same owner, same z, authored order fronting the declared
        // paint_order -> blocks (the authored overlay-card stack).
        assert!(paint_occlusion_blocks_declared_hit_region(
            &owner,
            &HitRegionOrder {
                z_index: 100,
                paint_order: 3,
                traversal_order: 0,
            },
            Some(3),
            &owner,
            &region_order,
        ));
        // Same owner, same z, authored order EQUAL to the declared
        // paint_order -> does not block (the region rides its own layer).
        assert!(!paint_occlusion_blocks_declared_hit_region(
            &owner,
            &HitRegionOrder {
                z_index: 100,
                paint_order: 0,
                traversal_order: 5,
            },
            Some(0),
            &owner,
            &region_order,
        ));
        // Same owner at a HIGHER z -> full-key comparison still blocks (a
        // deliberately fronted own layer covers own lower-z regions).
        assert!(paint_occlusion_blocks_declared_hit_region(
            &owner,
            &HitRegionOrder {
                z_index: 200,
                paint_order: 0,
                traversal_order: 0,
            },
            None,
            &owner,
            &region_order,
        ));
        // Equal orders are not front-of each other (foreign tie).
        assert!(!paint_occlusion_blocks_declared_hit_region(
            &foreign,
            &region_order,
            None,
            &owner,
            &region_order,
        ));
    }

    #[test]
    fn dispatch_graph_chaining_edge_targets_next_scroll_owner() {
        let (geometry_index, hit_regions, focus_regions, scroll_regions) =
            dispatch_graph_test_fixture();
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &[],
        );

        let chaining = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::Chaining,
            DispatchGraphChannel::Wheel,
        );
        // The nested region chains to the root; the root has no ancestor to
        // chain to at its own probe point... except the inner region also
        // contains the root's center? The root viewport center (100,100) is
        // outside scroll-inner (20..120 x 20..120)? x=100<120, y=100<120 so
        // it IS inside. The root therefore chains to the inner region at
        // that probe point, which faithfully mirrors the selector: were the
        // root unable to consume, the front-most other containing consumer
        // at that point is the inner region.
        assert!(
            chaining
                .iter()
                .any(|edge| edge.from == "scroll-inner" && edge.to == "scroll-root")
        );

        // Removing the inner region leaves the root with no chain target.
        let root_only = vec![scroll_regions[0].clone()];
        let graph_root_only = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &root_only,
            &[],
        );
        assert!(
            dispatch_graph_edges(
                &graph_root_only,
                DispatchGraphEdgeKind::Chaining,
                DispatchGraphChannel::Wheel
            )
            .is_empty()
        );
    }

    #[test]
    fn dispatch_graph_self_first_scroll_region_wins_wheel_hit_order_edge() {
        let (geometry_index, hit_regions, focus_regions, mut scroll_regions) =
            dispatch_graph_test_fixture();
        // Default fixture (all NearestScrollable): the fronter inner region
        // wins the wheel HitOrder pair (asserted by
        // `dispatch_graph_hit_order_edge_matches_selector_winner`). Declaring
        // SelfFirst on the back root region flips the derived precedence
        // edge, because the graph builder calls the same routing-aware
        // selector dispatch uses.
        scroll_regions[0].wheel_routing = WheelRouting::SelfFirst;
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &[],
        );

        let wheel_edges = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::HitOrder,
            DispatchGraphChannel::Wheel,
        );
        assert_eq!(wheel_edges.len(), 1);
        assert_eq!(wheel_edges[0].from, "scroll-root");
        assert_eq!(wheel_edges[0].to, "scroll-inner");
    }

    #[test]
    fn dispatch_graph_parent_first_scroll_region_defers_wheel_edges() {
        let (geometry_index, hit_regions, focus_regions, mut scroll_regions) =
            dispatch_graph_test_fixture();
        // The fronter inner region declares ParentFirst: it defers to the
        // strictly-containing root ancestor, flipping the wheel HitOrder
        // precedence edge while the at-limit chaining edge still targets the
        // ancestor owner.
        scroll_regions[1].wheel_routing = WheelRouting::ParentFirst;
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &[],
        );

        let wheel_edges = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::HitOrder,
            DispatchGraphChannel::Wheel,
        );
        assert_eq!(wheel_edges.len(), 1);
        assert_eq!(wheel_edges[0].from, "scroll-root");
        assert_eq!(wheel_edges[0].to, "scroll-inner");

        let chaining = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::Chaining,
            DispatchGraphChannel::Wheel,
        );
        assert!(
            chaining
                .iter()
                .any(|edge| edge.from == "scroll-inner" && edge.to == "scroll-root"),
            "the ParentFirst region still chains to its ancestor owner"
        );
    }

    #[test]
    fn dispatch_graph_capture_and_focus_route_edges() {
        let (geometry_index, hit_regions, focus_regions, scroll_regions) =
            dispatch_graph_test_fixture();
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &[],
        );

        let capture = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::Capture,
            DispatchGraphChannel::Pointer,
        );
        assert_eq!(capture.len(), 1);
        assert_eq!(capture[0].from, "hit-front");
        assert_eq!(capture[0].to, "graph-root");

        let focus_route = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::FocusRoute,
            DispatchGraphChannel::FocusRouted,
        );
        assert_eq!(focus_route.len(), 1);
        assert_eq!(focus_route[0].from, "focus-a");
        assert_eq!(focus_route[0].to, "graph-focus-target");
    }

    #[test]
    fn dispatch_graph_does_not_fabricate_spatial_edges_for_focus_routed_kinds() {
        let (geometry_index, hit_regions, focus_regions, scroll_regions) =
            dispatch_graph_test_fixture();
        let graph = derive_dispatch_graph_with_geometry_index(
            &WidgetId::from("graph-root"),
            &geometry_index,
            &hit_regions,
            &focus_regions,
            &scroll_regions,
            &[],
        );

        for edge in &graph.edges {
            match edge.kind {
                DispatchGraphEdgeKind::HitOrder
                | DispatchGraphEdgeKind::Occlusion
                | DispatchGraphEdgeKind::Chaining => {
                    assert_ne!(edge.channel, DispatchGraphChannel::FocusRouted);
                }
                DispatchGraphEdgeKind::Capture => {
                    assert_eq!(edge.channel, DispatchGraphChannel::Pointer);
                }
                DispatchGraphEdgeKind::FocusRoute => {
                    assert_eq!(edge.channel, DispatchGraphChannel::FocusRouted);
                }
            }
        }
    }

    #[test]
    fn dispatch_graph_view_derivation_collects_occlusions_from_authored_paint() {
        let layout = dispatch_graph_test_layout(200.0, 200.0);
        let target = WidgetId::from("graph-root");
        let overlay_bounds = Rect {
            origin: Point { x: 10.0, y: 10.0 },
            size: Size {
                width: 90.0,
                height: 90.0,
            },
        };
        let view = ViewDefinition {
            target: target.clone(),
            frame: FrameIdentity {
                surface_id: "surface".to_string(),
                surface_instance_id: "instance".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: layout.bounds.into_rect(),
            },
            paint: vec![
                PaintOp::keyed_layer(
                    PaintLayerKey::ordered(10, 0),
                    vec![PaintOp::Fill {
                        shape: ShapeDeclaration {
                            id: Some("overlay-body".to_string()),
                            kind: ShapeKind::Rectangle,
                            bounds: overlay_bounds,
                            path: None,
                            clip: None,
                        },
                        color: test_rgb(20, 20, 20),
                    }],
                ),
                PaintOp::keyed_layer(
                    PaintLayerKey::ordered(10, 1),
                    vec![PaintOp::Fill {
                        shape: ShapeDeclaration {
                            id: Some("wheel-through-body".to_string()),
                            kind: ShapeKind::Rectangle,
                            bounds: overlay_bounds,
                            path: None,
                            clip: None,
                        },
                        color: test_rgb(30, 30, 30),
                    }],
                )
                .with_wheel_transparency(PaintInputTransparency::PassThrough),
            ],
            paint_order: PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: vec![dispatch_graph_test_scroll(
                "scroll-root",
                Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 200.0,
                        height: 200.0,
                    },
                },
                400.0,
                HitRegionOrder {
                    z_index: -1,
                    paint_order: 0,
                    traversal_order: 0,
                },
            )],
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
            layout,
        };

        let occlusions = dispatch_graph_occlusion_regions_for_view(&view);
        assert_eq!(occlusions.len(), 2);
        assert_eq!(occlusions[0].order.z_index, 10);
        assert_eq!(occlusions[0].bounds, overlay_bounds);
        assert!(occlusions[0].blocks_wheel);
        assert!(!occlusions[1].blocks_wheel);

        let graph = derive_dispatch_graph_for_view(&view);
        let wheel_occlusion = dispatch_graph_edges(
            &graph,
            DispatchGraphEdgeKind::Occlusion,
            DispatchGraphChannel::Wheel,
        );
        assert_eq!(wheel_occlusion.len(), 1);
        assert_eq!(wheel_occlusion[0].from, "occlusion:graph-root:10:0:0:0");
        assert_eq!(wheel_occlusion[0].to, "scroll-root");
    }

    #[test]
    fn equal_paint_layer_keys_preserve_deterministic_fallback_order() {
        let paint = flatten_ordered_paint_units(vec![paint_test_unit(
            "root",
            1,
            vec![
                PaintOp::keyed_layer(
                    PaintLayerKey::ordered(4, 1),
                    vec![paint_test_text("first", 0.0)],
                ),
                PaintOp::keyed_layer(
                    PaintLayerKey::ordered(4, 1),
                    vec![paint_test_text("second", 10.0)],
                ),
            ],
        )]);

        assert_eq!(paint_text_labels(&paint), vec!["first", "second"]);
    }

    #[test]
    fn translation_and_bounds_collection_visit_keyed_paint_subtrees() {
        let clip = ClipDeclaration {
            id: Some("layer-clip".to_string()),
            bounds: paint_test_rect(1.0),
            path: None,
        };
        let op = PaintOp::keyed_layer(PaintLayerKey::new(1), vec![paint_test_text("inside", 2.0)])
            .with_layer_clip(clip);

        let translated = translate_paint_op(op, Point { x: 5.0, y: 7.0 });
        let PaintOp::Layer { clip, ops, .. } = &translated else {
            panic!("expected keyed paint layer");
        };
        assert_eq!(clip.as_ref().map(|clip| clip.bounds.origin.x), Some(6.0));
        assert_eq!(clip.as_ref().map(|clip| clip.bounds.origin.y), Some(7.0));
        assert_eq!(paint_text_labels(ops), vec!["inside"]);
        let PaintOp::Text { bounds, .. } = &ops[0] else {
            panic!("expected translated text child");
        };
        assert_eq!(bounds.origin.x, 7.0);
        assert_eq!(bounds.origin.y, 7.0);

        let mut bounds = Vec::new();
        collect_paint_bounds(&translated, &mut bounds);
        assert_eq!(bounds.len(), 2);
        assert_eq!(bounds[0].origin.x, 6.0);
        assert_eq!(bounds[1].origin.x, 7.0);
    }

    #[test]
    fn paint_units_sort_by_layer_then_explicit_order_then_source_traversal() {
        let unit = |id: &str, z_index: i32, order: Option<usize>, traversal_order: usize| {
            let target = WidgetId::from(id);
            let mut declaration = PaintOrderDeclaration::layer(target.clone(), z_index);
            declaration.order = order;
            PaintUnit {
                target: target.clone(),
                address: None,
                order: declaration,
                traversal_order,
                paint: vec![PaintOp::styled_text(
                    Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 1.0,
                            height: 1.0,
                        },
                    },
                    id,
                    Color {
                        red: 0.0,
                        green: 0.0,
                        blue: 0.0,
                        alpha: 1.0,
                    },
                    TextStyle::plain(),
                )],
            }
        };

        let paint = flatten_ordered_paint_units(vec![
            unit("modal", 10, Some(0), 0),
            unit("base-late", 0, Some(1), 1),
            unit("base-early", 0, Some(0), 2),
            unit("modal-same-order", 10, Some(0), 3),
        ]);
        let labels: Vec<&str> = paint
            .iter()
            .filter_map(|op| match op {
                PaintOp::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(
            labels,
            vec!["base-early", "base-late", "modal", "modal-same-order"]
        );
    }

    #[test]
    fn app_paint_orders_child_view_units_by_declared_layer() {
        let widget = SlipwayAppWidget::new(TwoCounterApp {
            widgets: (
                CounterWidget {
                    id: WidgetId::from("layer-top"),
                    origin_x: 0.0,
                },
                CounterWidget {
                    id: WidgetId::from("layer-bottom"),
                    origin_x: 32.0,
                },
            ),
        });
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = layout_view(&widget, &external, &local, app_layout_input());
        let paint = widget.paint(&external, &local, &layout);
        let labels: Vec<&str> = paint
            .iter()
            .filter_map(|op| match op {
                PaintOp::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(labels, vec!["layer-bottom:0", "layer-top:0"]);
    }

    #[test]
    fn app_state_observations_identify_both_child_widgets() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let mut local = widget.initial_local_state();
        assert!(
            widget
                .handle_event(&external, &mut local, command("one"))
                .handled
        );
        assert!(
            widget
                .handle_event(&external, &mut local, command("two"))
                .handled
        );

        let observations = widget.observe_state(&external, &local);

        assert_eq!(observations.len(), 2);
        assert_eq!(observations[0].target, WidgetId::from("one"));
        assert_eq!(observations[0].value, "1");
        assert_eq!(observations[1].target, WidgetId::from("two"));
        assert_eq!(observations[1].value, "1");
        assert_eq!(
            observations[0].slot.as_ref().map(|slot| slot.path.clone()),
            Some(vec![WidgetId::from("app"), WidgetId::from("one")])
        );
        assert_eq!(
            observations[1].slot.as_ref().map(|slot| slot.path.clone()),
            Some(vec![WidgetId::from("app"), WidgetId::from("two")])
        );
    }

    fn font_request() -> FontResolutionRequest {
        FontResolutionRequest {
            family: "Primary".to_string(),
            fallback_families: vec!["Fallback".to_string()],
            weight: FontWeight::Normal,
            style: FontStyle::Normal,
            source: None,
        }
    }

    // Revert-and-fail guard for the Step 209 egui root-gate fix: this
    // test compiles ONLY while core provides the
    // `SlipwayFontResolutionPolicy` impl for `SlipwayAppWidget<A>`
    // (deleting the blanket impl is a compile failure here), and it pins
    // the honest-refusal default of `SlipwayApp::resolve_app_font`.
    #[test]
    fn app_widget_font_resolution_defaults_to_honest_refusal() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();

        let evidence =
            SlipwayFontResolutionPolicy::resolve_font(&widget, &external, &local, font_request());

        assert_eq!(evidence.resolved_ref, None);
        assert!(evidence.installation.is_none());
        assert!(evidence.valid_source.is_none());
        assert_eq!(
            evidence.fallback_chain,
            vec!["Primary".to_string(), "Fallback".to_string()]
        );
        let refusal = evidence.refusal.expect("default must refuse honestly");
        assert_eq!(refusal.evidence_source.label, "app_font_resolution_default");
        assert_eq!(evidence.diagnostics.len(), 1);
        assert_eq!(evidence.diagnostics[0].code, "app-font-resolution-refused");
        assert_eq!(
            evidence.diagnostics[0].severity,
            DiagnosticSeverity::Unsupported
        );
        assert_eq!(evidence.diagnostics[0].target, Some(WidgetId::from("app")));
    }

    #[test]
    fn app_widget_font_resolution_delegates_to_app_override() {
        struct FontApp {
            inner: TwoCounterApp,
        }

        impl SlipwayApp for FontApp {
            type ExternalState = AppExternal;
            type LocalState = AppLocal;
            type AppMessage = AppMessage;
            type Widgets = (CounterWidget, CounterWidget);

            fn id(&self) -> WidgetId {
                WidgetId::from("font-app")
            }

            fn widgets(&self) -> &Self::Widgets {
                self.inner.widgets()
            }

            fn initial_local_state(&self) -> Self::LocalState {
                AppLocal
            }

            fn resolve_app_font(
                &self,
                _external: &Self::ExternalState,
                _local: &Self::LocalState,
                request: FontResolutionRequest,
            ) -> FontResolutionEvidence {
                FontResolutionEvidence {
                    request,
                    resolved_ref: Some("app-declared-font".to_string()),
                    fallback_chain: Vec::new(),
                    installation: None,
                    refusal: None,
                    valid_source: None,
                    diagnostics: Vec::new(),
                }
            }
        }

        let widget = SlipwayAppWidget::new(FontApp {
            inner: TwoCounterApp {
                widgets: (
                    CounterWidget {
                        id: WidgetId::from("one"),
                        origin_x: 0.0,
                    },
                    CounterWidget {
                        id: WidgetId::from("two"),
                        origin_x: 32.0,
                    },
                ),
            },
        });
        let external = AppExternal;
        let local = widget.initial_local_state();

        let evidence =
            SlipwayFontResolutionPolicy::resolve_font(&widget, &external, &local, font_request());

        assert_eq!(evidence.resolved_ref, Some("app-declared-font".to_string()));
        assert!(evidence.refusal.is_none());
        assert!(evidence.diagnostics.is_empty());
    }

    // The measurement-projection channel (roadmap Phase 6 item 3b slice
    // (iii), NC-4): `SlipwayAppWidget<A>` must forward
    // `SlipwayLogic::project_text_metrics` to the app hook, and the
    // default hook must stay a no-op (byte-identical for apps that never
    // opt in — the provider is not consulted at all).
    #[test]
    fn app_widget_forwards_project_text_metrics_to_app_hook() {
        #[derive(Default)]
        struct CountingProvider {
            requests: Vec<String>,
        }

        impl SlipwayTextMetricProvider for CountingProvider {
            fn text_metric_source(&self) -> TextMetricSource {
                TextMetricSource {
                    provider_id: "counting-provider".to_string(),
                    backend_id: None,
                    api_name: "counting".to_string(),
                    kind: TextMetricSourceKind::OfficialBackendApi,
                }
            }

            fn measure_text(&mut self, request: TextMeasurementRequest) -> TextMeasurementReceipt {
                self.requests.push(request.content.clone());
                TextMeasurementReceipt::Unsupported {
                    request,
                    diagnostics: Vec::new(),
                }
            }
        }

        struct MeasuringApp {
            inner: TwoCounterApp,
        }

        impl SlipwayApp for MeasuringApp {
            type ExternalState = AppExternal;
            type LocalState = AppLocal;
            type AppMessage = AppMessage;
            type Widgets = (CounterWidget, CounterWidget);

            fn id(&self) -> WidgetId {
                WidgetId::from("measuring-app")
            }

            fn widgets(&self) -> &Self::Widgets {
                self.inner.widgets()
            }

            fn initial_local_state(&self) -> Self::LocalState {
                AppLocal
            }

            fn project_text_metrics(
                &self,
                _external: &mut Self::ExternalState,
                metrics: &mut dyn SlipwayTextMetricProvider,
            ) {
                let _ = metrics.measure_text(TextMeasurementRequest {
                    target: WidgetId::from("measuring-app"),
                    request_id: "badge".to_string(),
                    content: "measured label".to_string(),
                    style: TextStyle::plain(),
                    available_bounds: None,
                    flow: None,
                    purposes: vec![TextMeasurementPurpose::IntrinsicSize],
                });
            }
        }

        let widget = SlipwayAppWidget::new(MeasuringApp {
            inner: TwoCounterApp {
                widgets: (
                    CounterWidget {
                        id: WidgetId::from("one"),
                        origin_x: 0.0,
                    },
                    CounterWidget {
                        id: WidgetId::from("two"),
                        origin_x: 32.0,
                    },
                ),
            },
        });
        let mut external = AppExternal;
        let mut provider = CountingProvider::default();

        SlipwayLogic::project_text_metrics(&widget, &mut external, &mut provider);
        assert_eq!(provider.requests, vec!["measured label".to_string()]);

        // Default no-op: an app that never overrides the hook consults
        // no provider through the same forwarding path.
        let default_widget = two_counter_app_widget();
        let mut untouched = CountingProvider::default();
        SlipwayLogic::project_text_metrics(&default_widget, &mut external, &mut untouched);
        assert!(untouched.requests.is_empty());
    }

    #[test]
    fn focus_traversal_is_optional_contract_io_only() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();

        assert_focus_traversal(&widget);
        assert_eq!(
            widget.focus_member(&external, &local),
            Some(FocusTraversalMember {
                target: WidgetId::from("root"),
                scope: None,
                tab_order: Some(10),
            })
        );
        assert_eq!(
            widget.next_focus(
                &external,
                &local,
                FocusTraversalInput {
                    current: Some(WidgetId::from("root")),
                    scope: None,
                },
            ),
            Some(WidgetId::from("child"))
        );
        assert_eq!(
            widget.previous_focus(
                &external,
                &local,
                FocusTraversalInput {
                    current: Some(WidgetId::from("root")),
                    scope: None,
                },
            ),
            None
        );
    }

    #[test]
    fn layout_intent_contracts_are_optional_trait_io_only() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 320.0,
                    height: 240.0,
                },
            }),
            content: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 320.0,
                    height: 240.0,
                },
            }),
            constraints: LayoutConstraints {
                min: Size {
                    width: 120.0,
                    height: 40.0,
                },
                max: Size {
                    width: 640.0,
                    height: 480.0,
                },
            },
        };

        assert_layout_intent_contracts(&widget);
        assert_layout_intent_aggregate(&widget);
        assert_text_input_capability(&widget);
        assert_scrollable_container_capability(&widget);
        assert_popup_capability(&widget);
        assert_provider_surface_capability(&widget);
        assert_command_surface_capability(&widget);
        assert_deterministic_source_capability(&widget);
        let backend = FakeBackend;
        assert_backend_admission_capability(&backend);

        let intrinsic = widget.intrinsic_size(&external, &local, &input);
        let size = widget.size_policy(&external, &local, &input);
        let resize = widget.resize_policy(&external, &local, &input);
        let overflow = widget.overflow_policy(&external, &local, &input);
        let auto = widget.auto_layout_policy(&external, &local, &input);
        let responsive = widget.responsive_variant(&external, &local, &input);
        let text = widget.text_flow_policy(&external, &local, &input);
        let mut provider = FakeOfficialMetricProvider;
        let text_measurement =
            widget.text_measurement_evidence(&external, &local, &input, &mut provider);
        let mut cached_provider = FakeOfficialMetricProvider;
        let mut cache = FakeTextMeasurementCache::default();
        let cached_miss_then_store = widget.cached_text_measurement_evidence(
            &external,
            &local,
            &input,
            &mut cached_provider,
            &mut cache,
        );
        let cached_hit = widget.cached_text_measurement_evidence(
            &external,
            &local,
            &input,
            &mut cached_provider,
            &mut cache,
        );
        let fit = widget.fit_overflow_evidence(&external, &local, &input, Some(&text_measurement));
        let layer = widget.layer_policy(&external, &local, &input);
        let scroll = widget.scroll_policy(&external, &local, &input);
        let collection = widget.collection_policy(&external, &local, &input);
        let styles = widget.interaction_state_styles(&external, &local, &input);
        let text_buffer = widget.text_buffer(&external, &local);
        let text_selection = widget.text_selection(&external, &local);
        let event = InputEvent::Pointer(PointerEvent {
            target: widget.id(),
            target_slot: None,
            position: Point { x: 2.0, y: 3.0 },
            target_bounds: None,
            kind: PointerEventKind::Press,
            button: Some(PointerButton::Primary),
            details: PointerDetails::default(),
        });
        let route_policy = widget.event_routing_policy(&external, &local, &event);
        let disposition = widget.event_disposition(&external, &local, &event, &route_policy.route);
        let container = widget.container_layout_policy(&external, &local, &input);
        let scroll_behavior = widget.scroll_behavior_policy(&external, &local, &input);
        let provider_surface = widget.canvas_surfaces();
        let font = widget.resolve_font(
            &external,
            &local,
            FontResolutionRequest {
                family: "Inter".to_string(),
                fallback_families: vec!["system-ui".to_string(), "Malgun Gothic".to_string()],
                weight: FontWeight::Normal,
                style: FontStyle::Normal,
                source: Some(ResourceSourceDeclaration {
                    source_id: "inter-system".to_string(),
                    kind: ResourceSourceKind::SystemFamily,
                    family: Some("Inter".to_string()),
                    asset_ref: None,
                    revision: Vec::new(),
                }),
            },
        );
        let time = widget.time_source(&external, &local);
        let command = widget.command_invocation_policy(
            &external,
            &local,
            &CommandEvent {
                target: widget.id(),
                target_slot: None,
                command: "copy".to_string(),
                payload_ref: None,
                source: None,
            },
        );
        let admission = backend.backend_parity_admission(&[CapabilityProfileKind::TextInput]);

        assert_eq!(intrinsic.preferred.width, 160.0);
        assert_eq!(size.width, SizePolicy::Fill { weight: 1.0 });
        assert!(resize.horizontal.can_shrink);
        assert_eq!(overflow.y, OverflowBehavior::Scroll);
        assert_eq!(auto.horizontal, AutoLayoutRequirement::Required);
        assert_eq!(
            auto.dependencies,
            vec![
                LayoutMeasurementDependency::AuthoredState,
                LayoutMeasurementDependency::BackendWrappedTextMetrics
            ]
        );
        assert_eq!(responsive.key, "compact");
        assert_eq!(text.wrap, TextWrapMode::Word);
        assert_eq!(text_measurement.policy.requests.len(), 1);
        match &text_measurement.receipts[0] {
            TextMeasurementReceipt::Valid(valid) => {
                assert_eq!(valid.source.kind, TextMetricSourceKind::OfficialBackendApi);
                assert_eq!(valid.facts.measured_size.width, 96.0);
            }
            other => panic!("expected valid wrapped metric receipt, got {other:?}"),
        }
        assert_eq!(
            cached_miss_then_store
                .cache
                .iter()
                .map(|event| event.status)
                .collect::<Vec<_>>(),
            vec![
                TextMeasurementCacheStatus::Miss,
                TextMeasurementCacheStatus::Stored
            ]
        );
        assert_eq!(
            cached_hit
                .cache
                .iter()
                .map(|event| event.status)
                .collect::<Vec<_>>(),
            vec![TextMeasurementCacheStatus::Hit]
        );
        assert_eq!(fit[0].measurement_request_ids, vec!["title".to_string()]);
        assert_eq!(layer.z_index, 10);
        assert_eq!(scroll.wheel_routing, WheelRouting::SelfFirst);
        assert_eq!(
            collection.visible_rows,
            Some(ItemRange { start: 2, end: 12 })
        );
        assert_eq!(styles[0].state, InteractionState::Hover);
        assert_eq!(text_buffer.text, "Official metric wrapper");
        assert_eq!(text_selection.carets.primary, Some(8));
        assert_eq!(route_policy.route.path, vec![WidgetId::from("root")]);
        assert!(disposition.final_disposition.handled);
        assert_eq!(container.kind, ContainerLayoutKind::Column);
        assert_eq!(scroll_behavior.axes.vertical, true);
        assert_eq!(provider_surface[0].kind, ProviderSurfaceKind::Canvas);
        assert_eq!(font.resolved_ref, Some("font-ref".to_string()));
        assert_eq!(time.source_id, "time");
        assert_eq!(command.command_id, "copy");
        assert!(admission.accepted);

        let probe = ProbeProduct::LayoutIntent(LayoutIntentProbe {
            target: widget.id(),
            intrinsic_size: Some(intrinsic),
            size_policy: Some(size),
            resize_policy: Some(resize),
            overflow_policy: Some(overflow),
            auto_layout: Some(auto),
            responsive_variant: Some(responsive),
            text_flow: Some(text),
            text_measurement_cache: widget.text_measurement_cache_policy(&external, &local, &input),
            text_measurement: Some(text_measurement),
            fit_overflow: fit,
            layer: Some(layer),
            scroll: Some(scroll),
            collection: Some(collection),
            interaction_styles: styles,
        });

        match probe {
            ProbeProduct::LayoutIntent(layout) => {
                assert_eq!(layout.target, WidgetId::from("root"));
                assert!(layout.intrinsic_size.is_some());
                assert!(layout.auto_layout.is_some());
                assert!(layout.text_measurement.is_some());
                assert!(!layout.fit_overflow.is_empty());
                assert!(layout.scroll.is_some());
            }
            _ => unreachable!("constructed layout intent probe"),
        }

        let aggregate = widget.layout_intent(&external, &local, &input);
        assert_eq!(aggregate.target, WidgetId::from("root"));
        assert_eq!(
            aggregate.responsive_variant.map(|variant| variant.key),
            Some("compact".to_string())
        );
    }

    #[test]
    fn text_edit_and_scroll_helpers_assemble_from_capability_traits() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(frame_identity().viewport),
            content: TargetLocalRect::new(frame_identity().viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame_identity().viewport.size,
            },
        };

        let focus = text_edit_focus_region_from_capability(
            &widget,
            &external,
            &local,
            PresentationRegionId::from("root:text"),
            Some(WidgetSlotAddress::new(widget.id(), 0)),
            TargetLocalRect::new(Rect {
                origin: Point { x: 4.0, y: 4.0 },
                size: Size {
                    width: 180.0,
                    height: 32.0,
                },
            }),
            widget.focus_member(&external, &local),
            true,
            &input,
            None,
        );
        let layout = LayoutOutput {
            bounds: input.viewport,
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let scroll = scroll_region_from_scrollable_capability(
            &widget, &external, &local, &layout, None, None, true,
        );

        assert_eq!(focus.target, widget.id());
        assert!(focus.text_edit.is_some());
        let text_edit = focus.text_edit.as_ref().expect("helper creates text edit");
        assert_eq!(text_edit.buffer.target, widget.id());
        assert!(
            text_edit
                .edit_commands
                .iter()
                .any(|command| { command.enabled && command.kind == TextEditKind::InsertText })
        );
        assert!(
            text_edit
                .edit_commands
                .iter()
                .any(|command| { command.enabled && command.kind == TextEditKind::DeleteBackward })
        );
        assert_eq!(scroll.id, PresentationRegionId::from("root-scroll"));
        assert_eq!(scroll.target, widget.id());
        assert!(scroll.axes.vertical);

        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), input),
        );
        view.focus_regions = vec![focus];
        view.scroll_regions = vec![scroll];
        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(!view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
    }

    #[test]
    fn overlapping_wheel_consumers_from_ordered_helper_pass_contract() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(frame_identity().viewport),
            content: TargetLocalRect::new(frame_identity().viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame_identity().viewport.size,
            },
        };
        let layout = LayoutOutput {
            bounds: input.viewport,
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let back = scroll_region_from_scrollable_capability_with_order(
            &widget,
            &external,
            &local,
            &layout,
            Some(PresentationRegionId::from("back")),
            None,
            true,
            HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
        );
        let front = scroll_region_from_scrollable_capability_with_order(
            &widget,
            &external,
            &local,
            &layout,
            Some(PresentationRegionId::from("front")),
            None,
            true,
            HitRegionOrder {
                z_index: 1,
                paint_order: 0,
                traversal_order: 0,
            },
        );

        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), input),
        );
        view.scroll_regions = vec![back, front];
        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "view_contract.ambiguous_wheel_overlap" })
        );
        assert!(!view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
    }

    #[test]
    fn overlapping_wheel_consumers_from_helper_with_identical_orders_stay_blocked() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(frame_identity().viewport),
            content: TargetLocalRect::new(frame_identity().viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame_identity().viewport.size,
            },
        };
        let layout = LayoutOutput {
            bounds: input.viewport,
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let back = scroll_region_from_scrollable_capability_with_order(
            &widget,
            &external,
            &local,
            &layout,
            Some(PresentationRegionId::from("back")),
            None,
            true,
            HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
        );
        let front = scroll_region_from_scrollable_capability_with_order(
            &widget,
            &external,
            &local,
            &layout,
            Some(PresentationRegionId::from("front")),
            None,
            true,
            HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
        );

        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), input),
        );
        view.scroll_regions = vec![back, front];
        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| { diagnostic.code == "view_contract.ambiguous_wheel_overlap" })
        );
    }

    #[test]
    fn wheel_owner_selection_prefers_front_ordered_helper_region() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(frame_identity().viewport),
            content: TargetLocalRect::new(frame_identity().viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame_identity().viewport.size,
            },
        };
        let layout = LayoutOutput {
            bounds: input.viewport,
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let back = scroll_region_from_scrollable_capability_with_order(
            &widget,
            &external,
            &local,
            &layout,
            Some(PresentationRegionId::from("back")),
            None,
            true,
            HitRegionOrder {
                z_index: 0,
                paint_order: 0,
                traversal_order: 0,
            },
        );
        let front = scroll_region_from_scrollable_capability_with_order(
            &widget,
            &external,
            &local,
            &layout,
            Some(PresentationRegionId::from("front")),
            None,
            true,
            HitRegionOrder {
                z_index: 1,
                paint_order: 0,
                traversal_order: 0,
            },
        );

        let dispatch = resolve_declared_wheel_dispatch(
            &layout,
            &[back.clone(), front.clone()],
            Point { x: 20.0, y: 20.0 },
            0.0,
            -4.0,
        )
        .expect("front ordered scroll region consumes");
        assert_eq!(
            dispatch.selected_region,
            PresentationRegionId::from("front")
        );

        let dispatch = resolve_declared_wheel_dispatch(
            &layout,
            &[front, back],
            Point { x: 20.0, y: 20.0 },
            0.0,
            -4.0,
        )
        .expect("front ordered scroll region consumes regardless of declaration order");
        assert_eq!(
            dispatch.selected_region,
            PresentationRegionId::from("front")
        );
    }

    #[test]
    fn scrollable_capability_helper_snapshots_wheel_routing_per_declared_region() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(frame_identity().viewport),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };

        // No explicit id: the helper resolves the scroll_behavior_policy id
        // ("root-scroll") and the snapshot call must carry it, letting the
        // region-targeted FakeWidget policy author its non-default mode.
        let own = scroll_region_from_scrollable_capability(
            &widget, &external, &local, &layout, None, None, true,
        );
        assert_eq!(own.id, PresentationRegionId::from("root-scroll"));
        assert_eq!(own.wheel_routing, WheelRouting::SelfFirst);

        // Explicit id: the snapshot call must carry the explicit id, so the
        // same widget's policy leaves this region on the default mode. Before
        // ADR-0002 B3 the helper passed `region_id: None`, which made
        // per-region authoring impossible.
        let other = scroll_region_from_scrollable_capability(
            &widget,
            &external,
            &local,
            &layout,
            Some(PresentationRegionId::from("other-region")),
            None,
            true,
        );
        assert_eq!(other.id, PresentationRegionId::from("other-region"));
        assert_eq!(other.wheel_routing, WheelRouting::NearestScrollable);
    }

    // Step-210 declared scroll-indicator control: helpers and `explicit`
    // default to `Auto` (backend-automatic, byte-identical), and
    // `with_scroll_indicator` is the per-region override.
    #[test]
    fn scroll_region_indicator_defaults_to_auto_and_overrides_via_builder() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(frame_identity().viewport),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };

        let region = scroll_region_from_scrollable_capability(
            &widget, &external, &local, &layout, None, None, true,
        );
        assert_eq!(region.indicator, ScrollIndicatorMode::Auto);
        assert_eq!(ScrollIndicatorMode::default(), ScrollIndicatorMode::Auto);

        let hidden = region
            .clone()
            .with_scroll_indicator(ScrollIndicatorMode::Hidden);
        assert_eq!(hidden.indicator, ScrollIndicatorMode::Hidden);
        let visible = region.with_scroll_indicator(ScrollIndicatorMode::Visible);
        assert_eq!(visible.indicator, ScrollIndicatorMode::Visible);
    }

    #[test]
    fn wheel_routing_policy_signature_receives_declared_region_identity() {
        // W1 (post-B3 honest signature): the policy is invoked with the
        // resolved identity of the region being declared — no synthetic
        // wheel event — and a single impl still authors per-region modes.
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();

        let own = widget.wheel_routing_policy(
            &external,
            &local,
            &PresentationRegionId::from("root-scroll"),
        );
        assert_eq!(own.routing, WheelRouting::SelfFirst);

        let other = widget.wheel_routing_policy(
            &external,
            &local,
            &PresentationRegionId::from("other-region"),
        );
        assert_eq!(other.routing, WheelRouting::NearestScrollable);
    }

    #[test]
    fn text_edit_and_scroll_contracts_block_unusable_manual_declarations() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(frame_identity().viewport),
            content: TargetLocalRect::new(frame_identity().viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame_identity().viewport.size,
            },
        };
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), input),
        );

        let text_edit = view.focus_regions[0]
            .text_edit
            .as_mut()
            .expect("fake widget declares text edit");
        text_edit.buffer.target = WidgetId::from("other");
        text_edit.typography.target = WidgetId::from("other-typography");
        text_edit.typography.style.font_size = 0.0;
        text_edit
            .edit_commands
            .retain(|command| command.kind != TextEditKind::DeleteBackward);

        view.scroll_regions[0].axes = ScrollAxes {
            horizontal: false,
            vertical: false,
        };
        view.scroll_regions[0].offset = Point { x: 5.0, y: 5000.0 };

        let diagnostics = view_definition_contract_diagnostics(&view);
        let codes = diagnostics
            .iter()
            .map(|diagnostic| diagnostic.code.as_str())
            .collect::<Vec<_>>();

        assert!(view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
        assert!(codes.contains(&"view_contract.text_edit_buffer_target_mismatch"));
        assert!(codes.contains(&"view_contract.text_edit_typography_target_mismatch"));
        assert!(codes.contains(&"view_contract.text_edit_typography_invalid_font_size"));
        assert!(codes.contains(&"view_contract.text_edit_missing_delete_command"));
        assert!(codes.contains(&"view_contract.scroll_axes_empty"));
        assert!(codes.contains(&"view_contract.scroll_offset_on_disabled_x_axis"));
        assert!(codes.contains(&"view_contract.scroll_offset_on_disabled_y_axis"));
        assert!(codes.contains(&"view_contract.scroll_offset_out_of_range"));
    }

    #[test]
    fn text_input_capability_requires_enabled_text_edit_focus_region() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(frame_identity().viewport),
            content: TargetLocalRect::new(frame_identity().viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame_identity().viewport.size,
            },
        };
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), input),
        );
        view.focus_regions.clear();

        let plain_diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &view,
            &[Capability::PointerInput],
        );
        assert!(!plain_diagnostics.iter().any(|diagnostic| diagnostic.code
            == "view_contract.text_input_missing_text_edit_focus_region"));

        let text_diagnostics =
            view_definition_contract_diagnostics_for_capabilities(&view, &[Capability::TextInput]);
        assert!(view_definition_has_blocking_contract_diagnostic(
            &text_diagnostics
        ));
        assert!(text_diagnostics.iter().any(|diagnostic| diagnostic.code
            == "view_contract.text_input_missing_text_edit_focus_region"
            && diagnostic.severity == DiagnosticSeverity::Error));
    }

    #[test]
    fn interaction_capabilities_require_enabled_declaration_regions() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let input = LayoutInput {
            viewport: TargetLocalRect::new(frame_identity().viewport),
            content: TargetLocalRect::new(frame_identity().viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame_identity().viewport.size,
            },
        };
        let base_view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame_identity(), input),
        );

        let mut pointer_view = base_view.clone();
        pointer_view.hit_regions.clear();
        let pointer_diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &pointer_view,
            &[Capability::PointerInput],
        );
        assert!(view_definition_has_blocking_contract_diagnostic(
            &pointer_diagnostics
        ));
        assert!(pointer_diagnostics.iter().any(|diagnostic| diagnostic.code
            == "view_contract.pointer_capability_missing_hit_region"
            && diagnostic.severity == DiagnosticSeverity::Error));

        let mut focus_view = base_view.clone();
        focus_view.focus_regions.clear();
        let focus_diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &focus_view,
            &[Capability::FocusInput, Capability::KeyboardInput],
        );
        assert!(view_definition_has_blocking_contract_diagnostic(
            &focus_diagnostics
        ));
        assert!(focus_diagnostics.iter().any(|diagnostic| {
            diagnostic.code
            == "view_contract.focus_capability_missing_focus_region"
            && diagnostic.severity == DiagnosticSeverity::Error
            // The fix-hint names the real constructor, not a nonexistent
            // "capability-backed focus helper" (LE-M19).
            && diagnostic
                .message
                .contains("focus_region_from_focus_capability")
        }));

        let mut scroll_view = base_view;
        scroll_view.scroll_regions.clear();
        let scroll_diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &scroll_view,
            &[Capability::WheelInput, Capability::ScrollRegionPresentation],
        );
        assert!(view_definition_has_blocking_contract_diagnostic(
            &scroll_diagnostics
        ));
        assert!(scroll_diagnostics.iter().any(|diagnostic| diagnostic.code
            == "view_contract.scroll_capability_missing_scroll_region"
            && diagnostic.severity == DiagnosticSeverity::Error));
    }

    #[test]
    fn focus_region_from_focus_capability_builds_admissible_plain_focus_region() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    content: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            ),
        );

        let region = focus_region_from_focus_capability(
            &widget,
            &external,
            &local,
            PresentationRegionId::from("root:plain-focus"),
            None,
            TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 40.0,
                    height: 20.0,
                },
            }),
            true,
        );

        // The helper snapshots the widget's focus-traversal member and
        // never assembles a text-edit payload (LE-M19).
        assert_eq!(region.target, WidgetId::from("root"));
        assert_eq!(
            region.member,
            Some(FocusTraversalMember {
                target: WidgetId::from("root"),
                scope: None,
                tab_order: Some(10),
            })
        );
        assert!(region.text_edit.is_none());
        assert!(region.enabled);

        view.focus_regions = vec![region];
        let diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &view,
            &[
                Capability::FocusInput,
                Capability::KeyboardInput,
                Capability::FocusRegionPresentation,
            ],
        );
        assert!(
            !diagnostics.iter().any(|diagnostic| diagnostic.code
                == "view_contract.focus_capability_missing_focus_region")
        );
        assert!(!view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
    }

    // NC-10 deliverability advisory (roadmap Phase 6 item 5): declaring
    // KeyboardInput with ONLY plain focus regions draws a Warning naming
    // the iced undeliverability (real ingress and physical control both
    // reach text-edit focus regions only) — the consumer's Escape-to-close
    // handler shape. Non-blocking: egui delivers after focus.
    #[test]
    fn keyboard_capability_on_plain_focus_region_draws_deliverability_advisory() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    content: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            ),
        );
        view.focus_regions = vec![focus_region_from_focus_capability(
            &widget,
            &external,
            &local,
            PresentationRegionId::from("root:plain-focus"),
            None,
            TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 40.0,
                    height: 20.0,
                },
            }),
            true,
        )];

        let diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &view,
            &[Capability::FocusInput, Capability::KeyboardInput],
        );

        let advisory = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code == "view_contract.keyboard_capability_plain_focus_delivery_limited"
            })
            .expect("KeyboardInput on plain-only focus regions draws the advisory");
        assert_eq!(advisory.severity, DiagnosticSeverity::Warning);
        // The advisory names the physical-control refusal the author will
        // otherwise meet cold, and the fixing constructor.
        assert!(
            advisory
                .message
                .contains("native-physical-control-text-focus-widget-unavailable"),
            "{}",
            advisory.message
        );
        assert!(
            advisory
                .message
                .contains("text_edit_focus_region_from_capability"),
            "{}",
            advisory.message
        );
        // Advisory, not admission refusal.
        assert!(!view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
    }

    // Suppression matrix for the NC-10 advisory: an enabled text-edit
    // focus region gives keyboard a deliverable target on both visible
    // backends; FocusInput alone claims no keyboard delivery; regions
    // missing entirely stay owned by the blocking error.
    #[test]
    fn keyboard_deliverability_advisory_is_suppressed_outside_the_audit_shape() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let base_view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    content: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            ),
        );
        let advisory_code = "view_contract.keyboard_capability_plain_focus_delivery_limited";

        // The FakeWidget base view declares a text-edit focus region:
        // KeyboardInput has a deliverable target, no advisory.
        assert!(
            base_view
                .focus_regions
                .iter()
                .any(|region| region.enabled && region.text_edit.is_some())
        );
        let text_edit_diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &base_view,
            &[Capability::KeyboardInput, Capability::TextInput],
        );
        assert!(
            !text_edit_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == advisory_code),
            "{text_edit_diagnostics:?}"
        );

        // FocusInput alone (no KeyboardInput) on a plain focus region:
        // nothing claims keyboard delivery, no advisory.
        let mut plain_view = base_view.clone();
        plain_view.focus_regions = vec![focus_region_from_focus_capability(
            &widget,
            &external,
            &local,
            PresentationRegionId::from("root:plain-focus"),
            None,
            TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 40.0,
                    height: 20.0,
                },
            }),
            true,
        )];
        let focus_only_diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &plain_view,
            &[Capability::FocusInput],
        );
        assert!(
            !focus_only_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == advisory_code),
            "{focus_only_diagnostics:?}"
        );

        // No enabled focus region at all: the blocking error owns the
        // shape; the advisory must not stack on it.
        let mut missing_view = base_view.clone();
        missing_view.focus_regions.clear();
        let missing_diagnostics = view_definition_contract_diagnostics_for_capabilities(
            &missing_view,
            &[Capability::KeyboardInput],
        );
        assert!(
            missing_diagnostics.iter().any(|diagnostic| diagnostic.code
                == "view_contract.focus_capability_missing_focus_region")
        );
        assert!(
            !missing_diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == advisory_code),
            "{missing_diagnostics:?}"
        );
    }

    #[test]
    fn geometry_refusals_embed_region_id_declared_rect_and_permitted_bounds() {
        // The audit's P4 probe shape (LE-M18): geometry authored in window
        // coordinates against a target-local layout. The refusal must show
        // the numbers so the coordinate-space mistake is self-diagnosable.
        let target = WidgetId::from("probe");
        let window_coords = Rect {
            origin: Point { x: 500.0, y: 400.0 },
            size: Size {
                width: 200.0,
                height: 100.0,
            },
        };
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 100.0,
                },
            }),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };

        let view = ViewDefinition {
            target: target.clone(),
            frame: frame_identity(),
            layout,
            paint: vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some("probe-paint".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: window_coords,
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 0.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 1.0,
                },
            }],
            paint_order: PaintOrderDeclaration::source_order(target.clone()),
            hit_regions: vec![HitRegionDeclaration {
                id: PresentationRegionId::from("probe-hit"),
                target: target.clone(),
                address: None,
                bounds: TargetLocalRect::new(window_coords),
                event_coordinate_space: PointerEventCoordinateSpace::TargetLocal,
                order: HitRegionOrder::default(),
                route: EventRoute {
                    route_id: Some("probe-route".to_string()),
                    address: None,
                    path: vec![target.clone()],
                    phase: EventRoutePhase::Target,
                },
                cursor: CursorCapability::Default,
                enabled: true,
                capture: PointerCaptureIntent::None,
                capture_evidence: Vec::new(),
            }],
            focus_regions: vec![FocusRegionDeclaration {
                id: PresentationRegionId::from("probe-focus"),
                target: target.clone(),
                address: None,
                bounds: TargetLocalRect::new(window_coords),
                member: None,
                enabled: true,
                text_edit: None,
            }],
            scroll_regions: vec![
                ScrollRegionDeclaration {
                    id: PresentationRegionId::from("probe-scroll-viewport"),
                    target: target.clone(),
                    address: None,
                    viewport: TargetLocalRect::new(window_coords),
                    content_bounds: TargetLocalRect::new(window_coords),
                    offset: Point { x: 0.0, y: 0.0 },
                    axes: ScrollAxes {
                        horizontal: false,
                        vertical: true,
                    },
                    wheel_routing: WheelRouting::NearestScrollable,
                    indicator: ScrollIndicatorMode::Auto,
                    order: HitRegionOrder::default(),
                    virtual_viewport: None,
                    consumption: ScrollConsumptionPolicy::exclusive_wheel(),
                    evidence: Vec::new(),
                    enabled: true,
                    diagnostics: Vec::new(),
                },
                ScrollRegionDeclaration {
                    id: PresentationRegionId::from("probe-scroll-offset"),
                    target: target.clone(),
                    address: None,
                    viewport: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 200.0,
                            height: 100.0,
                        },
                    }),
                    content_bounds: TargetLocalRect::new(Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 200.0,
                            height: 300.0,
                        },
                    }),
                    offset: Point { x: 0.0, y: 999.0 },
                    axes: ScrollAxes {
                        horizontal: false,
                        vertical: true,
                    },
                    wheel_routing: WheelRouting::NearestScrollable,
                    indicator: ScrollIndicatorMode::Auto,
                    order: HitRegionOrder::default(),
                    virtual_viewport: None,
                    consumption: ScrollConsumptionPolicy::exclusive_wheel(),
                    evidence: Vec::new(),
                    enabled: true,
                    diagnostics: Vec::new(),
                },
            ],
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };

        let diagnostics = view_definition_contract_diagnostics(&view);
        fn message_for<'a>(diagnostics: &'a [Diagnostic], code: &str) -> &'a str {
            &diagnostics
                .iter()
                .find(|diagnostic| diagnostic.code == code)
                .unwrap_or_else(|| panic!("expected {code}"))
                .message
        }

        let hit = message_for(&diagnostics, "view_contract.hit_bounds_outside_layout");
        assert!(hit.contains("`probe-hit`"), "{hit}");
        assert!(hit.contains("(500, 400, 200, 100)"), "{hit}");
        assert!(hit.contains("(0, 0, 200, 100)"), "{hit}");
        assert!(hit.contains(TARGET_LOCAL_BOUNDS_HINT), "{hit}");

        let focus = message_for(&diagnostics, "view_contract.focus_bounds_outside_layout");
        assert!(focus.contains("`probe-focus`"), "{focus}");
        assert!(focus.contains("(500, 400, 200, 100)"), "{focus}");
        assert!(focus.contains("(0, 0, 200, 100)"), "{focus}");
        assert!(focus.contains(TARGET_LOCAL_BOUNDS_HINT), "{focus}");

        let viewport = message_for(&diagnostics, "view_contract.scroll_viewport_outside_layout");
        assert!(viewport.contains("`probe-scroll-viewport`"), "{viewport}");
        assert!(viewport.contains("(500, 400, 200, 100)"), "{viewport}");
        assert!(viewport.contains("(0, 0, 200, 100)"), "{viewport}");
        assert!(viewport.contains(TARGET_LOCAL_BOUNDS_HINT), "{viewport}");

        let offset = message_for(&diagnostics, "view_contract.scroll_offset_out_of_range");
        assert!(offset.contains("`probe-scroll-offset`"), "{offset}");
        assert!(offset.contains("(0, 999)"), "{offset}");
        assert!(offset.contains("(0, 200)"), "{offset}");
        assert!(offset.contains(TARGET_LOCAL_BOUNDS_HINT), "{offset}");

        let paint = message_for(&diagnostics, "view_contract.paint_bounds_outside_layout");
        assert!(paint.contains("(500, 400, 200, 100)"), "{paint}");
        assert!(paint.contains("(0, 0, 200, 100)"), "{paint}");
        assert!(paint.contains(TARGET_LOCAL_BOUNDS_HINT), "{paint}");
    }

    #[derive(Default)]
    struct FakeRenderer {
        calls: u32,
    }

    impl SlipwayOffscreenRenderer for FakeRenderer {
        fn render_offscreen(
            &mut self,
            packet: RenderPacket,
        ) -> Result<RenderEvidence, RenderRefusal> {
            self.calls += 1;
            let width = packet.frame.viewport.size.width as u32;
            let height = packet.frame.viewport.size.height as u32;
            Ok(RenderEvidence {
                target: packet.target,
                frame: packet.frame,
                source: EvidenceSource::canonical_offscreen("fake-renderer"),
                provider_id: "fake-renderer".to_string(),
                artifact_ref: Some("memory://fake-renderer/frame".to_string()),
                artifact_path: None,
                pixel_hash: Some("hash-from-provider".to_string()),
                width: Some(width),
                height: Some(height),
                diagnostics: packet.diagnostics,
            })
        }
    }

    #[test]
    fn view_definition_is_an_authored_contract() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let frame = frame_identity();
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

        assert_view_definition(&widget);

        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(frame.clone(), layout_input),
        );
        assert_eq!(view.target, WidgetId::from("root"));
        assert_eq!(view.frame, frame);
        assert_eq!(view.hit_regions.len(), 1);
        assert_eq!(
            view.hit_regions[0].id,
            PresentationRegionId::from("root-hit")
        );
        assert_eq!(view.hit_regions[0].cursor, CursorCapability::Pointer);
        assert!(view.hit_regions[0].enabled);
        assert_eq!(view.focus_regions.len(), 1);
        assert!(view.focus_regions[0].text_edit.is_some());
        assert_eq!(view.scroll_regions.len(), 1);
        assert_eq!(
            view.scroll_regions[0].id,
            PresentationRegionId::from("root-scroll")
        );
        assert!(view.scroll_regions[0].consumption.wheel);
        assert_eq!(view.semantic_slots[0].node.role, "test");
    }

    #[test]
    fn offscreen_renderer_is_provider_trait_only() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let frame = frame_identity();
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(frame.viewport),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let packet = RenderPacket {
            target: widget.id(),
            frame: frame.clone(),
            layout,
            paint: Vec::new(),
            surfaces: Vec::new(),
            diagnostics: Vec::new(),
            prepared_geometry: None,
        };
        let mut renderer = FakeRenderer::default();

        let evidence = renderer
            .render_offscreen(packet)
            .expect("fake provider returns evidence");

        assert_eq!(renderer.calls, 1);
        assert_eq!(evidence.frame, frame);
        assert_eq!(evidence.source.label(), EVIDENCE_SOURCE_CANONICAL_OFFSCREEN);
        assert_eq!(evidence.provider_id, "fake-renderer");
    }

    #[test]
    fn evidence_source_labels_are_exact_contract_names() {
        let offscreen = EvidenceSource::canonical_offscreen("cpu-provider");
        let presented = EvidenceSource::backend_presented("visible-backend", "pass-1");

        assert_eq!(offscreen.label(), "canonical_offscreen");
        assert_eq!(presented.label(), "backend_presented");
        assert_eq!(offscreen.label(), EVIDENCE_SOURCE_CANONICAL_OFFSCREEN);
        assert_eq!(presented.label(), EVIDENCE_SOURCE_BACKEND_PRESENTED);
    }

    #[test]
    fn widget_slot_initializes_and_addresses_local_state() {
        let slot = WidgetSlot::new(FakeWidget {
            id: WidgetId::from("root"),
        });

        assert_eq!(slot.local_state, Local { count: 0 });
        assert_eq!(
            slot.address(3),
            WidgetSlotAddress {
                widget: WidgetId::from("root"),
                path: vec![WidgetId::from("root")],
                ordinal: 3,
            }
        );
    }

    #[test]
    fn topology_provides_child_traversal_order() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let topology = widget.topology(&external);

        assert_eq!(
            topology.traverse_depth_first(),
            ChildTraversal {
                root: WidgetId::from("root"),
                order: vec![WidgetId::from("root"), WidgetId::from("child")],
            }
        );
    }

    #[test]
    fn collector_uses_request_scoped_take() {
        let mut collector = ProbeCollector::new();
        collector.push(ProbeProduct::State(StateProbe {
            target: WidgetId::from("root"),
            observations: Vec::new(),
        }));

        let first = collector.take();
        let second = collector.take();

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
        assert!(collector.is_empty());
    }

    #[test]
    fn event_outcome_carries_message_change_and_probe_evidence() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let mut local = widget.initial_local_state();
        let event = InputEvent::Pointer(PointerEvent {
            target: WidgetId::from("root"),
            target_slot: None,
            position: Point { x: 1.0, y: 2.0 },
            target_bounds: None,
            kind: PointerEventKind::Press,
            button: Some(PointerButton::Primary),
            details: PointerDetails::default(),
        });

        let outcome = widget.handle_event(&external, &mut local, event);

        assert!(outcome.handled);
        assert_eq!(local.count, 1);
        assert_eq!(outcome.emitted_messages[0].name, "counted");
        assert_eq!(outcome.changes[0].field, "count");
        assert_eq!(outcome.observations[0].value, "1");
        assert_eq!(outcome.probes.len(), 1);
    }

    #[test]
    fn view_contract_rejects_enabled_hit_region_without_route_target() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    content: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            ),
        );
        view.hit_regions[0].route.path.clear();

        let diagnostics = view_definition_contract_diagnostics(&view);

        assert!(view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.hit_route_empty")
        );
    }

    #[test]
    fn view_contract_rejects_ambiguous_same_order_hit_overlap() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    content: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            ),
        );
        let mut duplicate = view.hit_regions[0].clone();
        duplicate.id = PresentationRegionId::from("duplicate");
        view.hit_regions.push(duplicate);

        let diagnostics = view_definition_contract_diagnostics(&view);

        assert!(view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
        let overlap = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "view_contract.ambiguous_hit_overlap")
            .expect("identically ordered overlapping hit regions are refused");
        // The refusal names both offending regions and the fix API (LE-M17).
        assert!(
            overlap.message.contains("`root-hit`"),
            "{}",
            overlap.message
        );
        assert!(
            overlap.message.contains("`duplicate`"),
            "{}",
            overlap.message
        );
        assert!(
            overlap.message.contains("HitRegionOrder"),
            "{}",
            overlap.message
        );
        assert!(
            overlap
                .message
                .contains("hit_region_from_pointer_capability"),
            "{}",
            overlap.message
        );
    }

    // NC-11 split (roadmap Phase 6 item 5): `allow_overlap` is a PAINT
    // declaration; it must NOT disarm the hit-ambiguity guard. Before the
    // split this exact view admitted silently — revert the gate to
    // `allow_overlap` and this fails.
    #[test]
    fn paint_overlap_allowance_does_not_disarm_hit_ambiguity_check() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    content: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            ),
        );
        let mut duplicate = view.hit_regions[0].clone();
        duplicate.id = PresentationRegionId::from("duplicate");
        view.hit_regions.push(duplicate);
        view.paint_order.allow_overlap = true;

        let diagnostics = view_definition_contract_diagnostics(&view);

        let overlap = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.code == "view_contract.ambiguous_hit_overlap")
            .expect("paint-overlap allowance must not accept hit ambiguity");
        // The refusal teaches the split: the paint flag is named as NOT
        // an acceptance, and the explicit acceptance is named.
        assert!(
            overlap.message.contains("allow_overlap"),
            "{}",
            overlap.message
        );
        assert!(
            overlap.message.contains("allow_ambiguous_hits"),
            "{}",
            overlap.message
        );
    }

    // The explicit acceptance — and ONLY it — silences the guard.
    #[test]
    fn allow_ambiguous_hits_is_the_only_hit_ambiguity_acceptance() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    content: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            ),
        );
        let mut duplicate = view.hit_regions[0].clone();
        duplicate.id = PresentationRegionId::from("duplicate");
        view.hit_regions.push(duplicate);
        view.paint_order.allow_ambiguous_hits = true;

        let diagnostics = view_definition_contract_diagnostics(&view);

        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.ambiguous_hit_overlap"),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn view_contract_reports_paint_overflow_without_blocking_interaction() {
        let widget = FakeWidget {
            id: WidgetId::from("root"),
        };
        let external = External {
            child: WidgetId::from("child"),
        };
        let local = widget.initial_local_state();
        let mut view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput::new(
                frame_identity(),
                LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    content: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            ),
        );
        view.paint.push(PaintOp::Fill {
            shape: ShapeDeclaration {
                id: Some("overflow".to_string()),
                kind: ShapeKind::Rectangle,
                bounds: Rect {
                    origin: Point { x: -10.0, y: 0.0 },
                    size: Size {
                        width: 20.0,
                        height: 20.0,
                    },
                },
                path: None,
                clip: None,
            },
            color: Color {
                red: 0.0,
                green: 0.0,
                blue: 0.0,
                alpha: 1.0,
            },
        });

        let diagnostics = view_definition_contract_diagnostics(&view);

        assert!(!view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
        assert!(diagnostics.iter().any(|diagnostic| diagnostic.code
            == "view_contract.paint_bounds_outside_layout"
            && diagnostic.severity == DiagnosticSeverity::Warning));
    }

    /// The NC-13 consumer-app shape (`slipway-test`): a dashboard card
    /// column ~992 px tall painted in a 640 px window with ZERO scroll
    /// regions — content the user cannot reach, admitted silently before
    /// the Phase-6 item-2 advisory landed.
    fn dashboard_card_column_view() -> ViewDefinition {
        let target = WidgetId::from("dashboard");
        let cards = (0..4)
            .map(|index| PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some(format!("card-{index}")),
                    kind: ShapeKind::Rectangle,
                    bounds: Rect {
                        origin: Point {
                            x: 0.0,
                            y: index as f32 * 248.0,
                        },
                        size: Size {
                            width: 640.0,
                            height: 248.0,
                        },
                    },
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 1.0,
                    green: 1.0,
                    blue: 1.0,
                    alpha: 1.0,
                },
            })
            .collect();
        ViewDefinition {
            target: target.clone(),
            frame: FrameIdentity {
                surface_id: "surface".to_string(),
                surface_instance_id: "instance".to_string(),
                revision: 1,
                frame_index: 1,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 640.0,
                        height: 640.0,
                    },
                },
            },
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 640.0,
                        height: 992.0,
                    },
                }),
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            },
            paint: cards,
            paint_order: PaintOrderDeclaration::source_order(target),
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: Vec::new(),
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        }
    }

    fn dashboard_page_scroll_region(view: &ViewDefinition) -> ScrollRegionDeclaration {
        ScrollRegionDeclaration {
            id: PresentationRegionId::from("dashboard:page-scroll"),
            target: view.target.clone(),
            address: None,
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 640.0,
                    height: 640.0,
                },
            }),
            content_bounds: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 640.0,
                    height: 992.0,
                },
            }),
            offset: Point { x: 0.0, y: 0.0 },
            axes: ScrollAxes {
                horizontal: false,
                vertical: true,
            },
            wheel_routing: WheelRouting::NearestScrollable,
            indicator: ScrollIndicatorMode::Auto,
            order: HitRegionOrder::default(),
            virtual_viewport: None,
            consumption: ScrollConsumptionPolicy::exclusive_wheel(),
            evidence: Vec::new(),
            enabled: true,
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn content_overflow_without_scroll_region_fires_the_advisory_and_stays_non_blocking() {
        let view = dashboard_card_column_view();
        let diagnostics = view_definition_contract_diagnostics(&view);
        let advisory = diagnostics
            .iter()
            .find(|diagnostic| {
                diagnostic.code == "view_contract.content_overflow_without_scroll_region"
            })
            .expect("uncovered painted overflow draws the advisory");
        assert_eq!(advisory.severity, DiagnosticSeverity::Warning);
        // The message carries the overflow distance, both rects, the fixing
        // helper, and the fixing doc page (NC-13: the advisory must TEACH
        // the scroll question, not just flag it).
        assert!(advisory.message.contains("352"), "{}", advisory.message);
        assert!(
            advisory.message.contains("(0, 0, 640, 640)"),
            "{}",
            advisory.message
        );
        assert!(
            advisory.message.contains("(0, 0, 640, 992)"),
            "{}",
            advisory.message
        );
        assert!(
            advisory
                .message
                .contains("scroll_region_from_scrollable_capability"),
            "{}",
            advisory.message
        );
        assert!(
            advisory.message.contains("api/routing-and-scroll.md"),
            "{}",
            advisory.message
        );
        // Advisory, NOT a refusal: NC-13 asked for inducement, and a new
        // blocker would break existing intentionally-clipping apps.
        assert!(!view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
    }

    #[test]
    fn covering_enabled_scroll_region_suppresses_the_overflow_advisory() {
        let mut view = dashboard_card_column_view();
        view.scroll_regions
            .push(dashboard_page_scroll_region(&view));
        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(
            !diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "view_contract.content_overflow_without_scroll_region"
            }),
            "a scroll region whose content_bounds cover the painted extent \
             is exactly the fix: {diagnostics:?}"
        );

        // A DISABLED region is not coverage — the user still cannot reach
        // the overflow.
        view.scroll_regions[0].enabled = false;
        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "view_contract.content_overflow_without_scroll_region"
        }));
    }

    #[test]
    fn declared_overflow_bounds_suppress_the_overflow_advisory() {
        // The Step-210 roaming-overlay pattern: a declared overflow
        // allowance legitimately exceeds layout and must NOT draw the
        // advisory (paint outside the allowance is already the
        // `paint_bounds_outside_overflow_bounds` error).
        let mut view = dashboard_card_column_view();
        view.paint_order = PaintOrderDeclaration::source_order(view.target.clone())
            .with_overflow_bounds(TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 640.0,
                    height: 992.0,
                },
            }));
        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(
            !diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "view_contract.content_overflow_without_scroll_region"
            }),
            "{diagnostics:?}"
        );
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "view_contract.paint_bounds_outside_overflow_bounds"
        }));
    }

    #[test]
    fn clipped_overflow_suppresses_the_overflow_advisory() {
        // A group clip that clips the overflow is intentional clipping:
        // the clipped extent never reaches pixels, so it is not
        // unreachable content.
        let mut view = dashboard_card_column_view();
        let cards = std::mem::take(&mut view.paint);
        view.paint = vec![PaintOp::Group {
            id: Some("card-column-clip".to_string()),
            clip: Some(ClipDeclaration {
                id: None,
                bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 640.0,
                        height: 640.0,
                    },
                },
                path: None,
            }),
            ops: cards,
        }];
        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(
            !diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "view_contract.content_overflow_without_scroll_region"
            }),
            "{diagnostics:?}"
        );
    }

    #[test]
    fn prepared_geometry_capture_is_gated_and_uses_mount_order() {
        let view = dashboard_card_column_view();
        let ordinary = validate_and_index_view(&view).expect("ordinary geometry preparation");
        assert!(ordinary.captured_records().is_none());

        let captured = validate_and_index_view_with_capture(
            &view,
            GeometryCaptureIntent::RenderPacketEvidence,
        )
        .expect("captured geometry preparation");
        let records = captured.captured_records().expect("capture requested");
        assert_eq!(records.len(), view.layout.child_placements.len());
        for (record, placement) in records.iter().zip(&view.layout.child_placements) {
            assert_eq!(record.target, placement.child);
            assert_eq!(Some(&record.address), placement.local_state_slot.as_ref());
        }
    }

    #[test]
    fn prepared_geometry_applies_mount_once_and_records_all_boxes() {
        let mut view = dashboard_card_column_view();
        let child = WidgetId::from("prepared-panel");
        let spacing = BoxSpacing::new(
            EdgeInsets::trbl(8.0, 24.0, 12.0, 4.0),
            EdgeInsets::trbl(6.0, 28.0, 18.0, 10.0),
        );
        let seed = ChildLayoutSeed {
            child: child.clone(),
            local_state_slot: Some(WidgetSlotAddress::new(child, 0)),
        };
        let plan = ChildLayoutPlan::explicit_border(
            seed.clone(),
            ContentLocalRect::new(Rect {
                origin: Point { x: 20.0, y: 15.0 },
                size: Size {
                    width: 100.0,
                    height: 60.0,
                },
            }),
            spacing,
        );
        let input = LayoutInput {
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 120.0,
                },
            }),
            content: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 120.0,
                },
            }),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: Size {
                    width: 200.0,
                    height: 120.0,
                },
            },
        };
        let result = ChildLayoutResult {
            seed,
            layout: prepare_leaf_layout(
                LayoutOutputBuilder::for_input(&input),
                TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 100.0,
                        height: 60.0,
                    },
                }),
            ),
            diagnostics: Vec::new(),
        };
        view.layout = prepare_resolved_layout(
            LayoutOutputBuilder::for_input(&input),
            view.layout.bounds,
            [(plan, result)],
        )
        .expect("valid prepared placement");
        let prepared = validate_and_index_view_with_capture(
            &view,
            GeometryCaptureIntent::RenderPacketEvidence,
        )
        .expect("valid asymmetric geometry");
        let record = &prepared.captured_records().unwrap()[0];
        assert_eq!(
            record.parent_local.outer.as_rect().origin,
            Point { x: 16.0, y: 7.0 }
        );
        assert_eq!(
            record.target_local.content.origin,
            Point { x: 10.0, y: 6.0 }
        );
        assert_eq!(
            record.unscrolled_root.content.origin,
            Point { x: 30.0, y: 21.0 }
        );
        assert_eq!(record.final_presented, record.unscrolled_root);
        assert_eq!(record.effective_clip_final, record.final_presented.border);
    }

    #[test]
    fn final_child_border_layout_input_applies_padding_only() {
        let placement = ChildPlacement {
            child: WidgetId::from("text"),
            bounds: ParentLocalRect::from_parent_local(Rect {
                origin: Point { x: 16.0, y: 192.0 },
                size: Size {
                    width: 608.0,
                    height: 76.0,
                },
            }),
            local_state_slot: None,
            spacing: BoxSpacing::new(
                EdgeInsets::trbl(4.0, 20.0, 12.0, 6.0),
                EdgeInsets::trbl(9.0, 17.0, 25.0, 11.0),
            ),
        };
        let input = child_layout_input_for_placement(&placement);
        assert_eq!(
            input.viewport.into_rect(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 608.0,
                    height: 76.0
                },
            }
        );
        assert_eq!(
            input.content.into_rect(),
            Rect {
                origin: Point { x: 11.0, y: 9.0 },
                size: Size {
                    width: 580.0,
                    height: 42.0
                },
            }
        );
        assert_ne!(
            input.viewport.into_rect(),
            Rect {
                origin: Point { x: 6.0, y: 4.0 },
                size: Size {
                    width: 582.0,
                    height: 60.0
                },
            }
        );
        assert_ne!(
            input.content.into_rect(),
            Rect {
                origin: Point { x: 17.0, y: 13.0 },
                size: Size {
                    width: 554.0,
                    height: 26.0
                },
            }
        );
    }

    #[test]
    fn preparation_uses_one_placement_traversal_and_no_index_rebuild() {
        let view = dashboard_card_column_view();
        PREPARATION_PLACEMENT_VISITS.with(|visits| visits.set(0));
        FROM_LAYOUT_PLACEMENT_VISITS.with(|visits| visits.set(0));

        validate_and_index_view_with_capture(&view, GeometryCaptureIntent::None)
            .expect("valid view");

        PREPARATION_PLACEMENT_VISITS
            .with(|visits| assert_eq!(visits.get(), view.layout.child_placements.len()));
        FROM_LAYOUT_PLACEMENT_VISITS.with(|visits| assert_eq!(visits.get(), 0));
    }

    #[test]
    fn every_contract_error_blocks_the_mandatory_gate() {
        let mut target_mismatch = dashboard_card_column_view();
        target_mismatch.paint_order.target = WidgetId::from("wrong-target");
        let diagnostics = validate_and_index_view(&target_mismatch).unwrap_err();
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.code == "view_contract.paint_order_target_mismatch"
            })
        );

        let mut missing_overflow = dashboard_card_column_view();
        missing_overflow.paint_order.allow_overflow_paint = true;
        missing_overflow.paint_order.overflow_bounds = None;
        let diagnostics = validate_and_index_view(&missing_overflow).unwrap_err();
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.overflow_bounds_missing")
        );
    }

    #[test]
    fn nested_scroll_overlay_and_ancestor_clip_are_applied_once() {
        let root = WidgetSlotAddress::new(WidgetId::from("root"), 0);
        let parent = root.child(WidgetId::from("parent"), 0);
        let child = parent.child(WidgetId::from("child"), 0);
        let grand = child.child(WidgetId::from("grand"), 0);
        let overlay = child.child(WidgetId::from("overlay"), 1);
        let placement =
            |id: &str, address: WidgetSlotAddress, x, y, width, height| ChildPlacement {
                child: WidgetId::from(id),
                bounds: ParentLocalRect::from_parent_local(Rect {
                    origin: Point { x, y },
                    size: Size { width, height },
                }),
                local_state_slot: Some(address),
                spacing: BoxSpacing::ZERO,
            };
        let mut paint_order = PaintOrderDeclaration::source_order(WidgetId::from("root"));
        paint_order.mounted_geometry = vec![
            MountedGeometryDeclaration {
                address: parent.clone(),
                parent_address: Some(root.clone()),
                authored_overflow: None,
                overlay_anchor: None,
            },
            MountedGeometryDeclaration {
                address: child.clone(),
                parent_address: Some(parent.clone()),
                authored_overflow: None,
                overlay_anchor: None,
            },
            MountedGeometryDeclaration {
                address: grand.clone(),
                parent_address: Some(child.clone()),
                authored_overflow: Some(TargetLocalRect::new(Rect {
                    origin: Point { x: -4.0, y: -4.0 },
                    size: Size {
                        width: 30.0,
                        height: 30.0,
                    },
                })),
                overlay_anchor: None,
            },
            MountedGeometryDeclaration {
                address: overlay.clone(),
                parent_address: Some(child.clone()),
                authored_overflow: None,
                overlay_anchor: Some(AddressedOverlayAnchor {
                    address: child.clone(),
                    point: Point { x: 10.0, y: 5.0 },
                    delta: Translation { x: 3.0, y: 4.0 },
                }),
            },
        ];
        let scroll = |id: &str, target: &str, address: WidgetSlotAddress, w, h, x, y| {
            ScrollRegionDeclaration::explicit(
                PresentationRegionId::from(id),
                WidgetId::from(target),
                Some(address),
                TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: w,
                        height: h,
                    },
                }),
                TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: w + 50.0,
                        height: h + 50.0,
                    },
                }),
                Point { x, y },
                ScrollAxes {
                    horizontal: true,
                    vertical: true,
                },
                WheelRouting::NearestScrollable,
                HitRegionOrder::default(),
                ScrollConsumptionPolicy::exclusive_wheel(),
                true,
            )
        };
        let mut view = ViewDefinition {
            target: WidgetId::from("root"),
            frame: FrameIdentity {
                surface_id: "test".to_string(),
                surface_instance_id: "test".to_string(),
                revision: 0,
                frame_index: 0,
                viewport: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 300.0,
                        height: 200.0,
                    },
                },
            },
            layout: LayoutOutput {
                bounds: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 300.0,
                        height: 200.0,
                    },
                }),
                child_placements: vec![
                    placement("parent", parent.clone(), 50.0, 40.0, 100.0, 100.0),
                    placement("child", child.clone(), 67.0, 53.0, 50.0, 50.0),
                    placement("grand", grand.clone(), 74.0, 61.0, 20.0, 20.0),
                    placement("overlay", overlay.clone(), 80.0, 70.0, 12.0, 10.0),
                ],
                diagnostics: Vec::new(),
            },
            paint: Vec::new(),
            paint_order,
            hit_regions: Vec::new(),
            focus_regions: Vec::new(),
            scroll_regions: vec![
                scroll("parent-scroll", "parent", parent, 100.0, 100.0, 5.0, 7.0),
                scroll("child-scroll", "child", child, 50.0, 50.0, 2.0, 3.0),
            ],
            wheel_traversal_boundary: Default::default(),
            semantic_slots: Vec::new(),
            probe_metadata: Vec::new(),
            diagnostics: Vec::new(),
        };
        view.scroll_regions[1].order.traversal_order = 1;
        let prepared = validate_and_index_view_with_capture(
            &view,
            GeometryCaptureIntent::RenderPacketEvidence,
        )
        .expect("numeric composition fixture is valid");
        let records = prepared.captured_records().unwrap();
        let grand = records
            .iter()
            .find(|record| record.target.as_str() == "grand")
            .unwrap();
        assert_eq!(
            grand.parent_local.border.as_rect().origin,
            Point { x: 7.0, y: 8.0 }
        );
        assert_eq!(grand.scroll_translation, Translation { x: -7.0, y: -10.0 });
        assert_eq!(
            grand.final_presented.border.origin,
            Point { x: 67.0, y: 51.0 }
        );
        assert_eq!(
            grand.ancestor_clip_final.unwrap(),
            Rect {
                origin: Point { x: 62.0, y: 46.0 },
                size: Size {
                    width: 50.0,
                    height: 50.0
                }
            }
        );
        assert_eq!(
            grand.effective_clip_final,
            Rect {
                origin: Point { x: 63.0, y: 47.0 },
                size: Size {
                    width: 30.0,
                    height: 30.0
                }
            }
        );

        let overlay = records
            .iter()
            .find(|record| record.target.as_str() == "overlay")
            .unwrap();
        assert_eq!(
            overlay.overlay_translation,
            Translation { x: 75.0, y: 55.0 }
        );
        assert_eq!(
            overlay.final_presented.border.origin,
            Point { x: 75.0, y: 55.0 }
        );
    }
}
