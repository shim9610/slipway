use std::{collections::HashMap, mem};

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
pub struct ParentLocalRect(Rect);

impl ParentLocalRect {
    pub fn new(rect: Rect) -> Self {
        Self(rect)
    }

    pub fn into_rect(self) -> Rect {
        self.0
    }
}

impl std::ops::Deref for ParentLocalRect {
    type Target = Rect;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::ops::DerefMut for ParentLocalRect {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl From<ParentLocalRect> for Rect {
    fn from(value: ParentLocalRect) -> Self {
        value.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EdgeInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayoutConstraints {
    pub min: Size,
    pub max: Size,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutInput {
    pub viewport: TargetLocalRect,
    pub constraints: LayoutConstraints,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildPlacement {
    pub child: WidgetId,
    pub bounds: ParentLocalRect,
    pub local_state_slot: Option<WidgetSlotAddress>,
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
    pub input: LayoutInput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildLayoutPlan {
    pub request: ChildLayoutRequest,
    pub placement: Option<ParentLocalRect>,
}

impl ChildLayoutPlan {
    pub fn requested(child: impl Into<WidgetId>, input: LayoutInput) -> Self {
        Self {
            request: ChildLayoutRequest {
                child: child.into(),
                local_state_slot: None,
                input,
            },
            placement: None,
        }
    }

    pub fn requested_for_seed(seed: ChildLayoutSeed, input: LayoutInput) -> Self {
        Self {
            request: ChildLayoutRequest {
                child: seed.child,
                local_state_slot: seed.local_state_slot,
                input,
            },
            placement: None,
        }
    }

    pub fn for_seed(seed: ChildLayoutSeed, input: LayoutInput) -> Self {
        Self::requested_for_seed(seed, input)
    }

    pub fn placed(
        child: impl Into<WidgetId>,
        input: LayoutInput,
        placement: ParentLocalRect,
    ) -> Self {
        Self {
            request: ChildLayoutRequest {
                child: child.into(),
                local_state_slot: None,
                input,
            },
            placement: Some(placement),
        }
    }

    pub fn placed_for_seed(
        seed: ChildLayoutSeed,
        input: LayoutInput,
        placement: ParentLocalRect,
    ) -> Self {
        Self {
            request: ChildLayoutRequest {
                child: seed.child,
                local_state_slot: seed.local_state_slot,
                input,
            },
            placement: Some(placement),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChildLayoutResult {
    pub request: ChildLayoutRequest,
    pub layout: LayoutOutput,
    pub local_state_slot: Option<WidgetSlotAddress>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AppLayoutPlan {
    pub bounds: TargetLocalRect,
    pub children: Vec<ChildLayoutPlan>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LayoutOutput {
    pub bounds: TargetLocalRect,
    pub child_placements: Vec<ChildPlacement>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresentationGeometryIndex {
    root_target_rect: Rect,
    target_rects_by_address: HashMap<WidgetSlotAddress, Rect>,
    first_target_rects_by_id: HashMap<WidgetId, Rect>,
}

impl PresentationGeometryIndex {
    pub fn from_layout(layout: &LayoutOutput) -> Self {
        let root_target_rect = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: layout.bounds.size,
        };
        let mut target_rects_by_address = HashMap::new();
        let mut first_target_rects_by_id = HashMap::new();

        for placement in &layout.child_placements {
            let rect = placement.bounds.into_rect();
            if let Some(address) = placement.local_state_slot.as_ref() {
                target_rects_by_address.insert(address.clone(), rect);
            }
            first_target_rects_by_id
                .entry(placement.child.clone())
                .or_insert(rect);
        }

        Self {
            root_target_rect,
            target_rects_by_address,
            first_target_rects_by_id,
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
}

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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WheelRouting {
    SelfFirst,
    ParentFirst,
    NearestScrollable,
    Custom,
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

#[derive(Clone, Debug, PartialEq)]
pub struct TextStyle {
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: FontWeight,
    pub font_style: FontStyle,
    pub decoration: TextDecoration,
    pub baseline: BaselineShift,
}

impl TextStyle {
    pub fn plain() -> Self {
        Self::default()
    }
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_family: DEFAULT_TEXT_FONT_FAMILY.to_string(),
            font_size: DEFAULT_TEXT_FONT_SIZE,
            font_weight: FontWeight::default(),
            font_style: FontStyle::default(),
            decoration: TextDecoration::default(),
            baseline: BaselineShift::default(),
        }
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
}

impl PaintOp {
    pub fn text(bounds: Rect, content: impl Into<String>, color: Color) -> Self {
        Self::styled_text(bounds, content, color, TextStyle::default())
    }

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Capability {
    PointerInput,
    KeyboardInput,
    TextInput,
    WheelInput,
    FocusInput,
    FocusTraversal,
    SemanticObservation,
    HitTesting,
    PointerCapture,
    HitRegionPresentation,
    FocusRegionPresentation,
    TextEditRegionPresentation,
    ScrollRegionPresentation,
    ShapePathClipPresentation,
    FontResourceInstallation,
    BackendPresentedEvidence,
    CanonicalOffscreenEvidence,
    ClipboardInput,
    DragDropInput,
    FileInput,
    Overlay,
    CommandSurface,
    RenderSurface,
    IntrinsicSizing,
    SizePolicy,
    ResizePolicy,
    OverflowPolicy,
    ResponsiveVariants,
    TextFlowPolicy,
    LayerPolicy,
    ScrollPolicy,
    CollectionPolicy,
    InteractionStateStyle,
    CapabilityAdmission,
    TextEditingPolicy,
    EventRoutingPolicy,
    ContainerLayoutPolicy,
    ScrollBehaviorPolicy,
    ProviderSurfacePolicy,
    ResourceResolutionPolicy,
    DeterministicSourcePolicy,
    CommandPolicy,
    BackendCapabilityNegotiation,
    CommandInput,
    Layout,
    Paint,
    StateObservation,
    ChildTraversal,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PointerCaptureIntent {
    None,
    OnPress,
    DuringDrag,
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

#[derive(Clone, Debug, PartialEq)]
pub struct ScrollBehaviorPolicyDeclaration {
    pub target: WidgetId,
    pub region_id: Option<PresentationRegionId>,
    pub address: Option<WidgetSlotAddress>,
    pub axes: ScrollAxes,
    pub extent: Size,
    pub viewport: TargetLocalRect,
    pub content_bounds: TargetLocalRect,
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
    pub bounds: Rect,
    pub payload_ref: Option<String>,
    pub dirty_regions: Vec<Rect>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderHitTestEvidence {
    pub target: WidgetId,
    pub provider_id: String,
    pub point: Point,
    pub hit: Option<String>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProviderSnapshotRequest {
    pub target: WidgetId,
    pub provider_id: String,
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

#[derive(Clone, Debug, PartialEq)]
pub struct ViewDefinitionInput {
    pub frame: FrameIdentity,
    pub layout_input: LayoutInput,
}

#[derive(Clone, Debug, PartialEq)]
pub struct HitRegionOrder {
    pub z_index: i32,
    pub paint_order: usize,
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

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
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
pub struct TextEditRegionDeclaration {
    pub buffer: TextBufferSnapshot,
    pub selection: TextSelectionPolicyDeclaration,
    pub composition: ImeCompositionPolicyDeclaration,
    pub caret: CaretGeometryEvidence,
    pub edit_commands: Vec<TextEditCommandDeclaration>,
    pub undo_redo: Option<TextUndoRedoEvidence>,
    pub viewport: Option<TextViewport>,
    pub line_mode: TextLineMode,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct FocusRegionDeclaration {
    pub id: PresentationRegionId,
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    pub bounds: TargetLocalRect,
    pub member: Option<FocusTraversalMember>,
    pub enabled: bool,
    pub text_edit: Option<TextEditRegionDeclaration>,
}

#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub struct ScrollRegionDeclaration {
    pub id: PresentationRegionId,
    pub target: WidgetId,
    pub address: Option<WidgetSlotAddress>,
    pub viewport: TargetLocalRect,
    pub content_bounds: TargetLocalRect,
    pub offset: Point,
    pub axes: ScrollAxes,
    pub wheel_routing: WheelRouting,
    pub virtual_viewport: Option<VirtualViewportRange>,
    pub consumption: ScrollConsumptionPolicy,
    pub evidence: Vec<ScrollConsumptionEvidence>,
    pub enabled: bool,
    pub diagnostics: Vec<Diagnostic>,
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

#[derive(Clone, Debug, PartialEq)]
pub struct PaintOrderDeclaration {
    pub target: WidgetId,
    pub mode: PaintOrderMode,
    pub z_index: i32,
    pub order: Option<usize>,
    pub allow_overlap: bool,
    pub allow_overflow_paint: bool,
    pub diagnostics: Vec<Diagnostic>,
}

impl PaintOrderDeclaration {
    pub fn source_order(target: impl Into<WidgetId>) -> Self {
        Self {
            target: target.into(),
            mode: PaintOrderMode::SourceOrder,
            z_index: 0,
            order: None,
            allow_overlap: false,
            allow_overflow_paint: false,
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
            allow_overflow_paint: false,
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
            allow_overflow_paint: false,
            diagnostics: Vec::new(),
        }
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
    pub semantic_slots: Vec<SemanticSlotDeclaration>,
    pub probe_metadata: Vec<ProbeMetadataDeclaration>,
    pub diagnostics: Vec<Diagnostic>,
}

pub fn view_definition_contract_diagnostics(view: &ViewDefinition) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    if view.paint_order.target != view.target {
        diagnostics.push(Diagnostic::error(
            Some(view.target.clone()),
            "view_contract.paint_order_target_mismatch",
            "ViewDefinition paint_order target must match the view target",
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

    let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
    validate_hit_regions(view, &geometry_index, &mut diagnostics);
    validate_focus_regions(view, &geometry_index, &mut diagnostics);
    validate_scroll_regions(view, &geometry_index, &mut diagnostics);
    validate_paint_bounds(view, &mut diagnostics);
    validate_view_contract_diagnostics(view, &mut diagnostics);

    diagnostics
}

pub fn view_definition_contract_diagnostics_for_capabilities(
    view: &ViewDefinition,
    capabilities: &[Capability],
) -> Vec<Diagnostic> {
    let mut diagnostics = view_definition_contract_diagnostics(view);
    validate_view_capabilities(view, capabilities, &mut diagnostics);
    diagnostics
}

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

#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredEventDispatchEvidence {
    pub source: EvidenceSource,
    pub frame: FrameIdentity,
    pub kind: DeclaredEventDispatchKind,
    pub input_position: Option<Point>,
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

    let geometry_index = PresentationGeometryIndex::from_layout(&view.layout);
    match evidence.kind {
        DeclaredEventDispatchKind::Pointer => {
            validate_pointer_dispatch_evidence(
                view,
                &geometry_index,
                input,
                evidence,
                &mut diagnostics,
            );
        }
        DeclaredEventDispatchKind::Wheel => {
            validate_wheel_dispatch_evidence(
                view,
                &geometry_index,
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
                &geometry_index,
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
        if evidence.candidate_regions != expected_candidates {
            diagnostics.push(dispatch_contract_error(
                &input.event,
                BACKEND_INPUT_DISPATCH_EVIDENCE_CANDIDATES_MISMATCH,
                "pointer dispatch evidence candidates did not match current hit regions",
            ));
        }
        let selected = select_declared_hit_region_at_root_local_point_with_geometry_index(
            geometry_index,
            &view.hit_regions,
            position,
        );
        validate_selected_region_id(
            &input.event,
            evidence.selected_region.as_ref(),
            selected.map(|region| &region.id),
            "pointer dispatch evidence selected region did not match current hit resolution",
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
                    "pointer dispatch event did not match the current hit-region declaration",
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
    let (dispatch, _) = resolve_declared_wheel_dispatch_with_evidence_and_geometry_index(
        evidence.source.clone(),
        evidence.frame.clone(),
        geometry_index,
        &view.scroll_regions,
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

impl From<InputEvent> for BackendInputEvent {
    fn from(event: InputEvent) -> Self {
        Self::direct(event)
    }
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

pub fn select_declared_hit_region_at_point<'a>(
    hit_regions: &'a [HitRegionDeclaration],
    target_local_position: Point,
) -> Option<&'a HitRegionDeclaration> {
    hit_regions
        .iter()
        .filter(|region| region.enabled)
        .filter(|region| rect_contains_point(region.bounds, target_local_position))
        .max_by(|a, b| {
            a.order
                .z_index
                .cmp(&b.order.z_index)
                .then(a.order.paint_order.cmp(&b.order.paint_order))
                .then(a.order.traversal_order.cmp(&b.order.traversal_order))
        })
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
        None,
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
    scroll_regions.iter().rev().find(|region| {
        region.enabled
            && region.consumption.wheel
            && (region.axes.horizontal || region.axes.vertical)
            && rect_contains_point(region.viewport, target_local_position)
    })
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
        .max_by(|a, b| {
            a.order
                .z_index
                .cmp(&b.order.z_index)
                .then(a.order.paint_order.cmp(&b.order.paint_order))
                .then(a.order.traversal_order.cmp(&b.order.traversal_order))
        })
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

pub fn select_declared_scroll_region_at_root_local_point_with_geometry_index<'a>(
    geometry_index: &PresentationGeometryIndex,
    scroll_regions: &'a [ScrollRegionDeclaration],
    root_local_position: Point,
) -> Option<&'a ScrollRegionDeclaration> {
    scroll_regions.iter().rev().find(|region| {
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
            pointer_is_pressed || matches!(kind, PointerEventKind::Move | PointerEventKind::Release)
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

pub fn resolve_declared_wheel_dispatch(
    layout: &LayoutOutput,
    scroll_regions: &[ScrollRegionDeclaration],
    root_local_position: Point,
    delta_x: f32,
    delta_y: f32,
) -> Option<DeclaredWheelDispatch> {
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    resolve_declared_wheel_dispatch_with_geometry_index(
        &geometry_index,
        scroll_regions,
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
    let region = select_declared_scroll_region_at_root_local_point_with_geometry_index(
        geometry_index,
        scroll_regions,
        root_local_position,
    )?;
    Some(DeclaredWheelDispatch {
        selected_region: region.id.clone(),
        candidate_regions,
        input: InputEvent::Wheel(WheelEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            delta_x,
            delta_y,
        }),
        route: region_event_route(&region.id, &region.target, &region.address),
        capture_event: region.consumption.wheel,
    })
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
    let geometry_index = PresentationGeometryIndex::from_layout(layout);
    resolve_declared_wheel_dispatch_with_evidence_and_geometry_index(
        source,
        frame,
        &geometry_index,
        scroll_regions,
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
    let dispatch = resolve_declared_wheel_dispatch_with_geometry_index(
        geometry_index,
        scroll_regions,
        root_local_position,
        delta_x,
        delta_y,
    );
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
        } else if !view.paint_order.allow_overflow_paint
            && !rect_contains_rect(
                declared_target_local_bounds(
                    declared_target_rect_for_region_address_with_geometry_index(
                        geometry_index,
                        &region.target,
                        region.address.as_ref(),
                    ),
                ),
                region.bounds,
            )
        {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.hit_bounds_outside_layout",
                "Enabled hit regions must stay inside layout bounds unless overflow paint is explicitly allowed",
            ));
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

    if view.paint_order.allow_overlap {
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
                    "Enabled hit regions overlap with identical ordering; declare overlap/layering explicitly or make the hit geometry disjoint",
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
        } else if region.enabled
            && !view.paint_order.allow_overflow_paint
            && !rect_contains_rect(
                declared_target_local_bounds(
                    declared_target_rect_for_region_address_with_geometry_index(
                        geometry_index,
                        &region.target,
                        region.address.as_ref(),
                    ),
                ),
                region.bounds,
            )
        {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.focus_bounds_outside_layout",
                "Enabled focus regions must stay inside layout bounds unless overflow paint is explicitly allowed",
            ));
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
            "Widgets declaring focus, keyboard input, or focus-region presentation must expose at least one enabled focus region; use a capability-backed focus helper or remove the capability",
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

        if !has_insert {
            diagnostics.push(Diagnostic::error(
                target.clone(),
                "view_contract.text_edit_missing_insert_command",
                "Editable text edit regions must declare an enabled InsertText command",
            ));
        }

        if !has_delete {
            diagnostics.push(Diagnostic::error(
                target,
                "view_contract.text_edit_missing_delete_command",
                "Editable text edit regions must declare at least one delete command",
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
        } else if region.enabled
            && !view.paint_order.allow_overflow_paint
            && !rect_contains_rect(
                declared_target_local_bounds(
                    declared_target_rect_for_region_address_with_geometry_index(
                        geometry_index,
                        &region.target,
                        region.address.as_ref(),
                    ),
                ),
                region.viewport,
            )
        {
            diagnostics.push(Diagnostic::error(
                Some(region.target.clone()),
                "view_contract.scroll_viewport_outside_layout",
                "Enabled scroll viewport must stay inside layout bounds unless overflow paint is explicitly allowed",
            ));
        }

        validate_scroll_region_contract(region, diagnostics);
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
                "Scroll offsets must fit inside content bounds for the declared viewport",
            ));
        }
    }
}

fn validate_paint_bounds(view: &ViewDefinition, diagnostics: &mut Vec<Diagnostic>) {
    if view.paint_order.allow_overflow_paint || !rect_is_valid(view.layout.bounds) {
        return;
    }

    let mut paint_bounds = Vec::new();
    for op in &view.paint {
        collect_paint_bounds(op, &mut paint_bounds);
    }

    for bounds in paint_bounds {
        if rect_is_valid(bounds) && !rect_contains_rect(view.layout.bounds, bounds) {
            diagnostics.push(Diagnostic::warning(
                Some(view.target.clone()),
                "view_contract.paint_bounds_outside_layout",
                "Paint bounds extend outside layout bounds without explicit overflow paint allowance",
            ));
        }
    }
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
    }
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

pub fn ordered_paint_units(mut units: Vec<PaintUnit>) -> Vec<PaintUnit> {
    units.sort_by_key(paint_unit_sort_key);
    units
}

pub fn flatten_ordered_paint_units(units: Vec<PaintUnit>) -> Vec<PaintOp> {
    ordered_paint_units(units)
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

#[derive(Clone, Debug, PartialEq)]
pub struct EventOutcome<M> {
    pub handled: bool,
    pub propagate: bool,
    pub emitted_messages: Vec<EmittedMessage<M>>,
    pub changes: Vec<ChangeEvidence>,
    pub observations: Vec<StateObservation>,
    pub probes: Vec<ProbeProduct>,
    pub diagnostics: Vec<Diagnostic>,
}

impl<M> EventOutcome<M> {
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
            "Event disposition policy declared this event handled, but the widget handler returned ignored",
        ));
    } else if !declaration.disposition.final_disposition.handled && outcome.handled {
        outcome.diagnostics.push(Diagnostic::warning(
            Some(target),
            EVENT_DECLARATION_HANDLER_HANDLED_DECLARED_UNHANDLED,
            "Widget handler handled an event that its disposition policy did not declare as handled",
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

pub trait SlipwayWidgetTypes {
    type ExternalState;
    type LocalState;
    type AppMessage;
}

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

pub trait SlipwayLogic: SlipwayWidgetTypes {
    fn handle_event(
        &self,
        external: &Self::ExternalState,
        local: &mut Self::LocalState,
        event: InputEvent,
    ) -> EventOutcome<Self::AppMessage>;
}

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

pub trait SlipwaySemantics: SlipwayWidgetTypes {
    fn semantics(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<SemanticNode>;
}

pub trait SlipwayHitTesting: SlipwayWidgetTypes {
    fn hit_test(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: HitTestInput,
    ) -> HitTestOutput;
}

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

pub trait SlipwayCommandContracts: SlipwayWidgetTypes {
    fn commands(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<CommandDeclaration>;
}

pub trait SlipwayRenderSurfaces: SlipwayWidgetTypes {
    fn render_surfaces(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<RenderSurfaceDeclaration>;
}

pub trait SlipwayTextBufferPolicy: SlipwayWidgetTypes {
    fn text_buffer(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextBufferSnapshot;
}

pub trait SlipwayTextSelectionPolicy: SlipwayWidgetTypes {
    fn text_selection(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextSelectionPolicyDeclaration;
}

pub trait SlipwayImeCompositionPolicy: SlipwayWidgetTypes {
    fn ime_composition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> ImeCompositionPolicyDeclaration;
}

pub trait SlipwayCaretGeometryPolicy: SlipwayWidgetTypes {
    fn caret_geometry(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        measurement: Option<&TextMeasurementEvidence>,
    ) -> CaretGeometryEvidence;
}

pub trait SlipwayTextEditCommandPolicy: SlipwayWidgetTypes {
    fn text_edit_commands(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> Vec<TextEditCommandDeclaration>;
}

pub trait SlipwayTextUndoRedoPolicy: SlipwayWidgetTypes {
    fn text_undo_redo(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TextUndoRedoEvidence;
}

pub trait SlipwayEventRoutingPolicy: SlipwayWidgetTypes {
    fn event_routing_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        event: &InputEvent,
    ) -> EventRoutingPolicyDeclaration;
}

pub trait SlipwayEventDispositionPolicy: SlipwayWidgetTypes {
    fn event_disposition(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        event: &InputEvent,
        route: &EventRoute,
    ) -> EventPropagationEvidence;
}

pub trait SlipwayPointerCapturePolicy: SlipwayWidgetTypes {
    fn pointer_capture_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        pointer: PointerDetails,
    ) -> PointerCapturePolicyDeclaration;
}

pub trait SlipwayDebugEventTracePolicy: SlipwayWidgetTypes {
    fn debug_event_trace_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> DebugEventTracePolicyDeclaration;
}

pub trait SlipwayContainerLayoutPolicy: SlipwayWidgetTypes {
    fn container_layout_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ContainerLayoutPolicyDeclaration;
}

pub trait SlipwayChildConstraintPolicy: SlipwayWidgetTypes {
    fn child_constraints(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> Vec<ChildConstraintPolicyDeclaration>;
}

pub trait SlipwayLayoutInvalidationPolicy: SlipwayWidgetTypes {
    fn layout_invalidation_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> LayoutInvalidationPolicyDeclaration;
}

pub trait SlipwayLayoutEvidencePolicy: SlipwayWidgetTypes {
    fn layout_evidence(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        output: &LayoutOutput,
    ) -> LayoutEvidence;
}

pub trait SlipwayScrollBehaviorPolicy: SlipwayWidgetTypes {
    fn scroll_behavior_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: &LayoutInput,
    ) -> ScrollBehaviorPolicyDeclaration;
}

pub trait SlipwayWheelRoutingPolicy: SlipwayWidgetTypes {
    fn wheel_routing_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        wheel: &WheelEvent,
    ) -> WheelRoutingPolicyDeclaration;
}

pub trait SlipwayViewportObservationPolicy: SlipwayWidgetTypes {
    fn viewport_observation(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> ViewportObservationEvidence;
}

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

pub trait SlipwayTimeSourcePolicy: SlipwayWidgetTypes {
    fn time_source(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> TimeSourceSnapshot;
}

pub trait SlipwayRandomSourcePolicy: SlipwayWidgetTypes {
    fn random_source(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> RandomSourceSnapshot;
}

pub trait SlipwayExternalDataSnapshotPolicy: SlipwayWidgetTypes {
    fn external_data_snapshot(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> ExternalDataSnapshot;
}

pub trait SlipwayAnimationTimelinePolicy: SlipwayWidgetTypes {
    fn animation_timeline_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
    ) -> AnimationTimelinePolicyDeclaration;
}

pub trait SlipwayCommandInvocationPolicy: SlipwayWidgetTypes {
    fn command_invocation_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        command: &CommandEvent,
    ) -> CommandInvocationPolicyDeclaration;
}

pub trait SlipwayCommandStatusPolicy: SlipwayWidgetTypes {
    fn command_status(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        command_id: &str,
    ) -> CommandStatusEvidence;
}

pub trait SlipwayShortcutRoutingPolicy: SlipwayWidgetTypes {
    fn shortcut_routing_policy(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        shortcut: &ShortcutDeclaration,
    ) -> ShortcutRoutingPolicyDeclaration;
}

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

pub trait SlipwayProbeSource: SlipwayWidgetTypes {
    fn probe_products(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        request: &ProbeRequest,
        frame: &FrameIdentity,
    ) -> Vec<ProbeProduct>;
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

pub trait SlipwayView: SlipwayWidgetTypes {
    fn initial_local_state(&self) -> Self::LocalState;

    fn layout(
        &self,
        external: &Self::ExternalState,
        local: &Self::LocalState,
        input: LayoutInput,
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

pub trait SlipwayWidget: SlipwaySsot + SlipwayLogic + SlipwayView {}

impl<W> SlipwayWidget for W where W: SlipwaySsot + SlipwayLogic + SlipwayView {}

pub trait SlipwayTextInputCapability:
    SlipwayWidget
    + SlipwayTextBufferPolicy
    + SlipwayTextSelectionPolicy
    + SlipwayImeCompositionPolicy
    + SlipwayCaretGeometryPolicy
    + SlipwayTextEditCommandPolicy
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

pub trait SlipwayPointerRegionCapability:
    SlipwayWidget + SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy
{
}

impl<W> SlipwayPointerRegionCapability for W where
    W: SlipwayWidget + SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy
{
}

#[allow(clippy::too_many_arguments)]
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

#[allow(clippy::too_many_arguments)]
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
            edit_commands: widget.text_edit_commands(external, local),
            undo_redo: Some(widget.text_undo_redo(external, local)),
            viewport: text_flow.viewport,
            line_mode: text_flow.line_mode,
            diagnostics: Vec::new(),
        }),
    }
}

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

pub fn scroll_region_from_scrollable_capability<W>(
    widget: &W,
    external: &W::ExternalState,
    local: &W::LocalState,
    input: &LayoutInput,
    region_id: Option<PresentationRegionId>,
    address: Option<WidgetSlotAddress>,
    enabled: bool,
) -> ScrollRegionDeclaration
where
    W: SlipwayScrollableContainerCapability,
{
    let policy = widget.scroll_behavior_policy(external, local, input);
    ScrollRegionDeclaration {
        id: region_id.or(policy.region_id).unwrap_or_else(|| {
            PresentationRegionId::from(format!("{}:scroll", widget.id().as_str()))
        }),
        target: policy.target,
        address: address.or(policy.address),
        viewport: policy.viewport,
        content_bounds: policy.content_bounds,
        offset: policy.offset,
        axes: policy.axes,
        wheel_routing: widget
            .wheel_routing_policy(
                external,
                local,
                &WheelEvent {
                    target: widget.id(),
                    target_slot: None,
                    delta_x: 0.0,
                    delta_y: 0.0,
                },
            )
            .routing,
        virtual_viewport: None,
        consumption: policy.consumption,
        evidence: Vec::new(),
        enabled,
        diagnostics: policy.diagnostics,
    }
}

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
        requests: &[ChildLayoutRequest],
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
                .map(|seed| ChildLayoutPlan::requested_for_seed(seed, input.clone()))
                .collect(),
            diagnostics: Vec::new(),
        }
    }

    fn layout(
        &self,
        _external: &Self::ExternalState,
        _local: &Self::LocalState,
        input: LayoutInput,
        children: Vec<ChildPlacement>,
    ) -> LayoutOutput {
        LayoutOutput {
            bounds: input.viewport,
            child_placements: children,
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
}

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
    ) -> LayoutOutput {
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        let seeds = self.app.widgets().child_layout_seeds(&root_slot);
        let plan = self
            .app
            .layout_plan(external, &local.app, input.clone(), seeds);
        let requests: Vec<ChildLayoutRequest> = plan
            .children
            .iter()
            .map(|child| child.request.clone())
            .collect();
        let child_results =
            self.app
                .widgets()
                .layout_children(external, &local.widgets, &root_slot, &requests);
        let mut diagnostics = plan.diagnostics.clone();
        let mut placements = Vec::new();

        for child_plan in &plan.children {
            if !rect_origin_is_zero(child_plan.request.input.viewport) {
                diagnostics.push(Diagnostic::error(
                    Some(child_plan.request.child.clone()),
                    "view_contract.child_input_viewport_not_target_local",
                    "Child layout input viewport origin must be 0,0; place children with ChildPlacement bounds and keep child layout target-local",
                ));
            }

            if let Some(result) = child_results
                .iter()
                .find(|result| child_layout_result_matches_plan(result, child_plan))
            {
                diagnostics.extend(result.layout.diagnostics.clone());
                placements.push(ChildPlacement {
                    child: child_plan.request.child.clone(),
                    bounds: child_plan
                        .placement
                        .unwrap_or_else(|| ParentLocalRect::new(result.layout.bounds.into_rect())),
                    local_state_slot: result.local_state_slot.clone(),
                });
            } else {
                diagnostics.push(Diagnostic {
                    target: Some(child_plan.request.child.clone()),
                    severity: DiagnosticSeverity::Warning,
                    code: "missing-child-layout".to_string(),
                    message: "app layout plan requested a child that is not in the widget list"
                        .to_string(),
                });
            }
        }

        let mut output = self.app.layout(external, &local.app, input, placements);
        output.diagnostics.splice(0..0, diagnostics);
        output.bounds = plan.bounds;
        output
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
        let mut layout = self.layout(external, local, input.layout_input.clone());
        let root_paint = self.app.paint(external, &local.app, &layout);
        let root_slot = WidgetSlotAddress::new(self.app.id(), 0);
        let child_views = self.app.widgets().child_view_definitions(
            external,
            &local.widgets,
            &root_slot,
            &input.frame,
            &layout,
        );

        let mut hit_regions = Vec::new();
        let mut focus_regions = Vec::new();
        let mut scroll_regions = Vec::new();
        let mut semantic_slots = Vec::new();
        let mut probe_metadata = Vec::new();
        let mut diagnostics = layout.diagnostics.clone();

        for child in child_views {
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

        ViewDefinition {
            target: self.id(),
            frame: input.frame,
            layout,
            paint: root_paint,
            paint_order: PaintOrderDeclaration::source_order(self.id()),
            hit_regions,
            focus_regions,
            scroll_regions,
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
        _requests: &[ChildLayoutRequest],
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
            $($widget::LocalState: Clone,)+
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
                requests: &[ChildLayoutRequest],
            ) -> Vec<ChildLayoutResult> {
                let mut results = Vec::new();
                for request in requests {
                    $(
                        let child = self.$index.id();
                        let child_slot = parent_slot.child(child.clone(), $index);
                        if child_layout_request_matches_slot(request, &child, &child_slot) {
                            let child_layout = self.$index.layout(
                                external,
                                &local.$index,
                                request.input.clone(),
                            );
                            results.push(ChildLayoutResult {
                                request: request.clone(),
                                layout: child_layout,
                                local_state_slot: Some(child_slot),
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
                $(
                    let child_slot = parent_slot.child(self.$index.id(), $index);
                    let event_matches_child = if let Some(target_slot) = &target_slot {
                        target_slot == &child_slot
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
                        let local_before = local.$index.clone();
                        let outcome = self.$index.handle_event(
                            external,
                            &mut local.$index,
                            event.clone(),
                        );
                        let outcome = apply_physical_event_handling_declaration(declaration, outcome);
                        if event_outcome_has_physical_declaration_mismatch(&outcome) {
                            local.$index = local_before;
                        }
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
                        let child_layout = LayoutOutput {
                            bounds: TargetLocalRect::new(Rect {
                                origin: Point { x: 0.0, y: 0.0 },
                                size: placement.bounds.size,
                            }),
                            child_placements: Vec::new(),
                            diagnostics: Vec::new(),
                        };
                        ops.extend(mount_child_paint_ops(
                            self.$index.paint(external, &local.$index, &child_layout),
                            placement.bounds,
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
                        let child_layout = LayoutOutput {
                            bounds: TargetLocalRect::new(Rect {
                                origin: Point { x: 0.0, y: 0.0 },
                                size: placement.bounds.size,
                            }),
                            child_placements: Vec::new(),
                            diagnostics: Vec::new(),
                        };
                        let view = self.$index.view_definition(
                            external,
                            &local.$index,
                            ViewDefinitionInput {
                                frame: frame.clone(),
                                layout_input: child_view_definition_input(placement.bounds),
                            },
                        );
                        let mut unit = PaintUnit::from_view(view, $index + 1);
                        unit.address = placement.local_state_slot.clone().or(Some(child_slot));
                        unit.paint = mount_child_paint_ops(
                            self.$index.paint(external, &local.$index, &child_layout),
                            placement.bounds,
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
            $($widget::LocalState: Clone,)+
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
                            ViewDefinitionInput {
                                frame: frame.clone(),
                                layout_input: child_view_definition_input(placement.bounds),
                            },
                        );
                        views.push(mount_child_view_definition(
                            view,
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
        result.request.local_state_slot.as_ref() == Some(plan_slot)
    } else {
        result.request.child == plan.request.child
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
    }
}

fn mount_child_view_definition(
    mut view: ViewDefinition,
    slot: Option<&WidgetSlotAddress>,
    placement: ParentLocalRect,
) -> ViewDefinition {
    mount_child_view_definition_geometry(&mut view, placement);

    if let Some(slot) = slot {
        for placement in &mut view.layout.child_placements {
            mount_existing_optional_slot_address(&mut placement.local_state_slot, slot);
        }

        for region in &mut view.hit_regions {
            mount_optional_slot_address(&mut region.address, slot);
            mount_optional_slot_address(&mut region.route.address, slot);
            region.route.path = mount_event_route_path(&region.route.path, slot);
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

fn mount_event_route_path(route_path: &[WidgetId], slot: &WidgetSlotAddress) -> Vec<WidgetId> {
    if route_path.first() == Some(&slot.widget) {
        let mut mounted = slot.path.clone();
        mounted.extend(route_path.iter().skip(1).cloned());
        mounted
    } else {
        let mut mounted = slot.path.clone();
        mounted.extend(route_path.iter().cloned());
        mounted
    }
}

fn mount_child_view_definition_geometry(view: &mut ViewDefinition, placement: ParentLocalRect) {
    let offset = placement.origin;

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
        placement.bounds = ParentLocalRect::new(translate_rect(placement.bounds, offset));
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
            .map(|slot| mount_existing_slot(slot, parent_slot))
            .unwrap_or_else(|| parent_slot.clone()),
    );
}

fn mount_existing_optional_slot_address(
    address: &mut Option<WidgetSlotAddress>,
    parent_slot: &WidgetSlotAddress,
) {
    if let Some(slot) = address.take() {
        *address = Some(mount_existing_slot(slot, parent_slot));
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
        node.local_state_slot = Some(mount_existing_slot(slot, parent_slot));
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
                .map(|slot| mount_existing_slot(slot, child_slot))
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
        .map(|slot| mount_existing_slot(slot, child_slot))
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

fn mount_existing_slot(
    slot: WidgetSlotAddress,
    parent_slot: &WidgetSlotAddress,
) -> WidgetSlotAddress {
    if slot.path.starts_with(&parent_slot.path) {
        return slot;
    }

    if slot.widget == parent_slot.widget {
        return parent_slot.clone();
    }

    let mut path = parent_slot.path.clone();
    let mut suffix = slot.path;
    if suffix.first() == Some(&parent_slot.widget) {
        suffix.remove(0);
    }
    path.extend(suffix);

    WidgetSlotAddress {
        widget: slot.widget,
        path,
        ordinal: slot.ordinal,
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
                style: TextStyle::default(),
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

        let plain = PaintOp::text(bounds, "plain", color);
        let PaintOp::Text { style, .. } = plain else {
            panic!("text constructor returns text paint op");
        };
        assert_eq!(style, TextStyle::default());
        assert_eq!(style.font_family, DEFAULT_TEXT_FONT_FAMILY);
        assert_eq!(style.font_size, DEFAULT_TEXT_FONT_SIZE);
        assert_eq!(style.font_weight, FontWeight::Normal);
        assert_eq!(style.font_style, FontStyle::Normal);
        assert_eq!(style.decoration, TextDecoration::none());
        assert_eq!(style.baseline, BaselineShift::Normal);

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
            let layout = self.layout(external, local, input.layout_input);
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

    impl SlipwayProbeSource for FakeWidget {
        fn probe_products(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            request: &ProbeRequest,
            frame: &FrameIdentity,
        ) -> Vec<ProbeProduct> {
            let mut products = Vec::new();
            for kind in &request.kinds {
                match kind {
                    ProbeKind::Topology => {
                        let root = self.topology(external);
                        products.push(ProbeProduct::Topology(TopologyProbe {
                            traversal: root.traverse_depth_first(),
                            root,
                        }));
                    }
                    ProbeKind::State => {
                        products.push(ProbeProduct::State(StateProbe {
                            target: self.id.clone(),
                            observations: self.observe_state(external, local),
                        }));
                    }
                    ProbeKind::ViewDefinition => {
                        let input = LayoutInput {
                            viewport: TargetLocalRect::new(frame.viewport),
                            constraints: LayoutConstraints {
                                min: Size {
                                    width: 0.0,
                                    height: 0.0,
                                },
                                max: frame.viewport.size,
                            },
                        };
                        products.push(ProbeProduct::ViewDefinition(self.view_definition(
                            external,
                            local,
                            ViewDefinitionInput {
                                frame: frame.clone(),
                                layout_input: input,
                            },
                        )));
                    }
                    _ => {}
                }
            }
            products
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

    fn assert_probe_source<W: SlipwayProbeSource>(_widget: &W) {}

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
                bounds: ParentLocalRect::new(Rect {
                    origin: Point { x: 20.0, y: 10.0 },
                    size: Size {
                        width: 50.0,
                        height: 40.0,
                    },
                }),
                local_state_slot: Some(child_slot.clone()),
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
                    bounds: ParentLocalRect::new(Rect {
                        origin: Point { x: 10.0, y: 5.0 },
                        size: Size {
                            width: 40.0,
                            height: 30.0,
                        },
                    }),
                    local_state_slot: Some(first_slot),
                },
                ChildPlacement {
                    child: child.clone(),
                    bounds: ParentLocalRect::new(Rect {
                        origin: Point { x: 80.0, y: 45.0 },
                        size: Size {
                            width: 70.0,
                            height: 50.0,
                        },
                    }),
                    local_state_slot: Some(second_slot.clone()),
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
                    bounds: ParentLocalRect::new(Rect {
                        origin: Point { x: 12.0, y: 6.0 },
                        size: Size {
                            width: 40.0,
                            height: 30.0,
                        },
                    }),
                    local_state_slot: Some(first_slot),
                },
                ChildPlacement {
                    child: child.clone(),
                    bounds: ParentLocalRect::new(Rect {
                        origin: Point { x: 120.0, y: 70.0 },
                        size: Size {
                            width: 80.0,
                            height: 44.0,
                        },
                    }),
                    local_state_slot: Some(second_slot.clone()),
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
                bounds: ParentLocalRect::new(Rect {
                    origin: Point { x: 60.0, y: 40.0 },
                    size: Size {
                        width: 90.0,
                        height: 32.0,
                    },
                }),
                local_state_slot: Some(slot.clone()),
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
        assert_eq!(wheel.delta_y, -4.0);
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
                bounds: ParentLocalRect::new(Rect {
                    origin: Point { x: 60.0, y: 30.0 },
                    size: Size {
                        width: 80.0,
                        height: 50.0,
                    },
                }),
                local_state_slot: Some(child_slot.clone()),
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
        assert_eq!(evidence.selected_region, Some(region.id.clone()));
        let InputEvent::Wheel(wheel) = dispatch.input else {
            panic!("expected wheel input");
        };
        assert_eq!(wheel.target, child);
        assert_eq!(wheel.target_slot, Some(child_slot));
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
        assert_eq!(evidence.generated_event, Some(event));
        assert!(evidence.capture_event);
        assert_eq!(
            evidence
                .route
                .as_ref()
                .and_then(|route| route.route_id.as_deref()),
            Some("main-scroll")
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
                target,
                caret_bounds: Vec::new(),
                selection_bounds: Vec::new(),
                measurement_request_ids: Vec::new(),
                diagnostics: Vec::new(),
            },
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
            ]
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
                placement: Some(ParentLocalRect::new(input.viewport.into_rect())),
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
            _wheel: &WheelEvent,
        ) -> WheelRoutingPolicyDeclaration {
            WheelRoutingPolicyDeclaration {
                target: self.id.clone(),
                routing: WheelRouting::SelfFirst,
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
            let layout = self.layout(external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
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
                style: TextStyle::default(),
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
            let layout = self.layout(external, local, input.layout_input);
            let paint = self.paint(external, local, &layout);

            let paint_order = match self.id.as_str() {
                "layer-top" => PaintOrderDeclaration::layered_order(self.id(), 1, 0),
                "layer-bottom" => PaintOrderDeclaration::layered_order(self.id(), 0, 0),
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
        ) -> LayoutOutput {
            let mut bounds = input.viewport;
            bounds.origin.x += local.layout_offset_x;
            LayoutOutput {
                bounds,
                child_placements: Vec::new(),
                diagnostics: Vec::new(),
            }
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
                style: TextStyle::default(),
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
            let layout = self.layout(external, local, input.layout_input);
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
                        ChildLayoutPlan::placed_for_seed(
                            seed,
                            child_layout_input_for_size(bounds.size),
                            ParentLocalRect::new(bounds),
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
            children: Vec<ChildPlacement>,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: input.viewport,
                child_placements: children,
                diagnostics: Vec::new(),
            }
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
                        ChildLayoutPlan::placed_for_seed(
                            seed,
                            child_layout_input_for_size(viewport.size),
                            ParentLocalRect::new(viewport),
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
                        ChildLayoutPlan::placed_for_seed(
                            seed,
                            child_layout_input_for_size(viewport.size),
                            ParentLocalRect::new(viewport),
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
                        ChildLayoutPlan::requested_for_seed(
                            seed,
                            LayoutInput {
                                viewport: TargetLocalRect::new(viewport),
                                constraints: LayoutConstraints {
                                    min: Size {
                                        width: 0.0,
                                        height: 0.0,
                                    },
                                    max: viewport.size,
                                },
                            },
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
                        ChildLayoutPlan::placed_for_seed(
                            seed,
                            child_layout_input_for_size(input.viewport.size),
                            ParentLocalRect::new(input.viewport.into_rect()),
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
        LayoutInput {
            viewport: TargetLocalRect::new(Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 96.0,
                    height: 32.0,
                },
            }),
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

    fn child_layout_input_for_size(size: Size) -> LayoutInput {
        LayoutInput {
            viewport: TargetLocalRect::new(Rect {
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
                ViewDefinitionInput {
                    frame: frame_identity(),
                    layout_input: LayoutInput {
                        viewport: layout.bounds,
                        constraints: LayoutConstraints {
                            min: Size {
                                width: 0.0,
                                height: 0.0,
                            },
                            max: layout.bounds.size,
                        },
                    },
                },
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: app_layout_input(),
            },
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: app_layout_input(),
            },
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
        let layout = widget.layout(&external, &local, app_layout_input());

        assert_eq!(layout.child_placements.len(), 2);
        assert_eq!(layout.child_placements[0].child, WidgetId::from("one"));
        assert_eq!(layout.child_placements[1].child, WidgetId::from("two"));
        assert_eq!(layout.child_placements[0].bounds.origin.x, 0.0);
        assert_eq!(layout.child_placements[1].bounds.origin.x, 32.0);
        assert!(layout.child_placements[0].local_state_slot.is_some());
        assert!(layout.child_placements[1].local_state_slot.is_some());
    }

    #[test]
    fn vertical_app_gives_each_child_a_distinct_layout_viewport() {
        let widget = vertical_echo_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = widget.layout(&external, &local, app_layout_input());

        assert_eq!(layout.child_placements.len(), 2);
        assert_eq!(layout.child_placements[0].child, WidgetId::from("top"));
        assert_eq!(layout.child_placements[1].child, WidgetId::from("bottom"));
        assert_eq!(layout.child_placements[0].bounds.origin.y, 0.0);
        assert_eq!(layout.child_placements[0].bounds.size.height, 12.0);
        assert_eq!(layout.child_placements[1].bounds.origin.y, 12.0);
        assert_eq!(layout.child_placements[1].bounds.size.height, 20.0);
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
        let layout = widget.layout(&external, &local, root_input.clone());

        assert_eq!(layout.bounds, root_input.viewport);
        assert_ne!(
            layout.child_placements[0].bounds.into_rect(),
            root_input.viewport.into_rect()
        );
        assert_ne!(
            layout.child_placements[1].bounds.into_rect(),
            root_input.viewport.into_rect()
        );
        assert_eq!(layout.child_placements[0].bounds.size.width, 96.0);
        assert_eq!(layout.child_placements[1].bounds.size.width, 96.0);
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
        widget.handle_event(&external, &mut local, wheel("scroll-app", 15.0));

        let layout = widget.layout(&external, &local, app_layout_input());
        let origins: Vec<f32> = layout
            .child_placements
            .iter()
            .map(|placement| placement.bounds.origin.y)
            .collect();

        assert_eq!(origins, vec![-15.0, 5.0, 25.0]);
        assert_eq!(layout.child_placements.len(), 3);
    }

    #[test]
    fn final_layout_preserves_real_child_slot_addresses() {
        let widget = scroll_echo_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let layout = widget.layout(&external, &local, app_layout_input());

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
        let layout = widget.layout(&external, &local, app_layout_input());

        assert_eq!(layout.child_placements.len(), 2);
        assert_eq!(
            layout.child_placements[0].child,
            WidgetId::from("duplicate")
        );
        assert_eq!(
            layout.child_placements[1].child,
            WidgetId::from("duplicate")
        );
        assert_eq!(layout.child_placements[0].bounds.origin.x, 1.0);
        assert_eq!(layout.child_placements[0].bounds.origin.y, 0.0);
        assert_eq!(layout.child_placements[1].bounds.origin.x, 102.0);
        assert_eq!(layout.child_placements[1].bounds.origin.y, 20.0);

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
        let layout = widget.layout(&external, &local, app_layout_input());
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
        let layout = widget.layout(&external, &local, app_layout_input());
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: app_layout_input(),
            },
        );

        assert_eq!(view.hit_regions.len(), 2);
        assert_eq!(
            view.hit_regions[0].bounds.into_rect(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: layout.child_placements[0].bounds.size,
            }
        );
        assert_eq!(
            view.hit_regions[1].bounds.into_rect(),
            Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: layout.child_placements[1].bounds.size,
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(Rect {
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
            },
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
    fn app_view_definition_blocks_non_target_local_child_input() {
        let widget = duplicate_slot_app_widget();
        let external = AppExternal;
        let local = widget.initial_local_state();
        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: app_layout_input(),
            },
        );
        let diagnostics = view_definition_contract_diagnostics(&view);

        assert!(view.diagnostics.iter().any(|diagnostic| diagnostic.code
            == "view_contract.child_input_viewport_not_target_local"
            && diagnostic.severity == DiagnosticSeverity::Error));
        assert!(view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
    }

    #[test]
    fn app_paint_includes_child_paint_from_both_widgets() {
        let widget = two_counter_app_widget();
        let external = AppExternal;
        let mut local = widget.initial_local_state();
        widget.handle_event(&external, &mut local, command("two"));
        let layout = widget.layout(&external, &local, app_layout_input());
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
                paint: vec![PaintOp::text(
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
        let layout = widget.layout(&external, &local, app_layout_input());
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
        widget.handle_event(&external, &mut local, command("one"));
        widget.handle_event(&external, &mut local, command("two"));

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
        let scroll = scroll_region_from_scrollable_capability(
            &widget, &external, &local, &input, None, None, true,
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: input,
            },
        );
        view.focus_regions = vec![focus];
        view.scroll_regions = vec![scroll];
        let diagnostics = view_definition_contract_diagnostics(&view);
        assert!(!view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: input,
            },
        );

        let text_edit = view.focus_regions[0]
            .text_edit
            .as_mut()
            .expect("fake widget declares text edit");
        text_edit.buffer.target = WidgetId::from("other");
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: input,
            },
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: input,
            },
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
        assert!(focus_diagnostics.iter().any(|diagnostic| diagnostic.code
            == "view_contract.focus_capability_missing_focus_region"
            && diagnostic.severity == DiagnosticSeverity::Error));

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
    fn view_definition_and_probe_source_are_authored_contracts() {
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
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: frame.viewport.size,
            },
        };

        assert_view_definition(&widget);
        assert_probe_source(&widget);

        let view = widget.view_definition(
            &external,
            &local,
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
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

        let products = widget.probe_products(
            &external,
            &local,
            &ProbeRequest {
                target: Some(WidgetId::from("root")),
                kinds: vec![
                    ProbeKind::Topology,
                    ProbeKind::State,
                    ProbeKind::ViewDefinition,
                ],
            },
            &view.frame,
        );

        assert!(matches!(products[0], ProbeProduct::Topology(_)));
        assert!(matches!(products[1], ProbeProduct::State(_)));
        assert!(matches!(products[2], ProbeProduct::ViewDefinition(_)));
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            },
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            },
        );
        let mut duplicate = view.hit_regions[0].clone();
        duplicate.id = PresentationRegionId::from("duplicate");
        view.hit_regions.push(duplicate);

        let diagnostics = view_definition_contract_diagnostics(&view);

        assert!(view_definition_has_blocking_contract_diagnostic(
            &diagnostics
        ));
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code == "view_contract.ambiguous_hit_overlap")
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
            ViewDefinitionInput {
                frame: frame_identity(),
                layout_input: LayoutInput {
                    viewport: TargetLocalRect::new(frame_identity().viewport),
                    constraints: LayoutConstraints {
                        min: Size {
                            width: 0.0,
                            height: 0.0,
                        },
                        max: frame_identity().viewport.size,
                    },
                },
            },
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
}
