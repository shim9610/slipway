use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use slipway_core::{
    DeclaredEventDispatchEvidence, Diagnostic, EVIDENCE_SOURCE_BACKEND_PRESENTED,
    EventResultIdentity, FrameIdentity, InputEvent, KeyEventKind, KeyboardDetails, Modifiers,
    Point, PointerButton, PointerDetails, PointerEventKind, PresentationRegionId, ProbeProduct,
    ProbeRequest, RenderEvidence, RenderPacket, RenderRefusal, Size, TextCompositionEvent,
    TextCompositionPhase, TextEditKind, TextSelectionRange, WidgetId,
};
use std::collections::{BTreeMap, VecDeque};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub const VISIBLE_FRAME_TRACE_FILE_ENV: &str = "SLIPWAY_VISIBLE_FRAME_TRACE_FILE";
pub const VISIBLE_FRAME_TRACE_CAPACITY_ENV: &str = "SLIPWAY_VISIBLE_FRAME_TRACE_CAPACITY";
pub const VISIBLE_FRAME_BUDGET_HZ: u64 = 240;
pub const VISIBLE_FRAME_BUDGET_NS: u128 = 1_000_000_000u128 / VISIBLE_FRAME_BUDGET_HZ as u128;
const DEFAULT_VISIBLE_FRAME_TRACE_CAPACITY: usize = 8192;

pub const PRESENTED_PIXELS_PASS_ID: &str = "presented-pixels/direct-surface-copy";
pub const DEBUG_COMPOSITION_PASS_ID: &str = "physical-input/debug-injected/composition";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleFrameTimingSummary {
    pub backend: String,
    pub budget_hz: u64,
    pub budget_ns: u128,
    pub budget_passed: bool,
    pub over_budget_samples: usize,
    pub root_frame_budget_passed: bool,
    pub root_frame_over_budget_samples: usize,
    pub capacity: usize,
    pub dropped_samples: u64,
    pub total_samples: usize,
    pub kinds: Vec<VisibleFrameTimingKindSummary>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VisibleFrameTimingKindSummary {
    pub kind: String,
    pub samples: usize,
    pub p50_ns: u128,
    pub p95_ns: u128,
    pub p99_ns: u128,
    pub max_ns: u128,
    pub over_budget_samples: usize,
}

#[derive(Clone, Debug)]
pub struct VisibleFrameTimingRecorder {
    backend: &'static str,
    path: Option<PathBuf>,
    capacity: usize,
    samples: VecDeque<VisibleFrameTimingSample>,
    next_sequence: u64,
    dropped_samples: u64,
}

#[derive(Clone, Debug)]
struct VisibleFrameTimingSample {
    sequence: u64,
    kind: &'static str,
    duration_ns: u128,
    event_count: usize,
    viewport: Option<Size>,
}

impl VisibleFrameTimingRecorder {
    pub fn from_env(backend: &'static str) -> Self {
        let path = std::env::var_os(VISIBLE_FRAME_TRACE_FILE_ENV).map(PathBuf::from);
        let capacity = std::env::var(VISIBLE_FRAME_TRACE_CAPACITY_ENV)
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .filter(|value| *value > 0)
            .unwrap_or(DEFAULT_VISIBLE_FRAME_TRACE_CAPACITY);

        Self {
            backend,
            path,
            capacity,
            samples: VecDeque::with_capacity(capacity.min(1024)),
            next_sequence: 0,
            dropped_samples: 0,
        }
    }

    pub fn disabled(backend: &'static str) -> Self {
        Self {
            backend,
            path: None,
            capacity: DEFAULT_VISIBLE_FRAME_TRACE_CAPACITY,
            samples: VecDeque::new(),
            next_sequence: 0,
            dropped_samples: 0,
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.path.is_some()
    }

    pub fn record(
        &mut self,
        kind: &'static str,
        duration: Duration,
        event_count: usize,
        viewport: Option<Size>,
    ) {
        if self.path.is_none() {
            return;
        }
        if self.samples.len() == self.capacity {
            self.samples.pop_front();
            self.dropped_samples += 1;
        }
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.samples.push_back(VisibleFrameTimingSample {
            sequence,
            kind,
            duration_ns: duration.as_nanos(),
            event_count,
            viewport,
        });
    }

    pub fn summary(&self) -> VisibleFrameTimingSummary {
        let mut by_kind: BTreeMap<&'static str, Vec<u128>> = BTreeMap::new();
        for sample in &self.samples {
            by_kind
                .entry(sample.kind)
                .or_default()
                .push(sample.duration_ns);
        }

        let kinds: Vec<_> = by_kind
            .into_iter()
            .map(|(kind, mut durations)| {
                durations.sort_unstable();
                let samples = durations.len();
                let max_ns = durations.last().copied().unwrap_or(0);
                let over_budget_samples = durations
                    .iter()
                    .filter(|duration| **duration > VISIBLE_FRAME_BUDGET_NS)
                    .count();
                VisibleFrameTimingKindSummary {
                    kind: kind.to_string(),
                    samples,
                    p50_ns: percentile_ns(&durations, 50),
                    p95_ns: percentile_ns(&durations, 95),
                    p99_ns: percentile_ns(&durations, 99),
                    max_ns,
                    over_budget_samples,
                }
            })
            .collect();
        let over_budget_samples = kinds
            .iter()
            .map(|kind| kind.over_budget_samples)
            .sum::<usize>();
        let root_frame_over_budget_samples = self
            .samples
            .iter()
            .filter(|sample| is_visible_root_frame_timing_kind(sample.kind))
            .filter(|sample| sample.duration_ns > VISIBLE_FRAME_BUDGET_NS)
            .count();

        VisibleFrameTimingSummary {
            backend: self.backend.to_string(),
            budget_hz: VISIBLE_FRAME_BUDGET_HZ,
            budget_ns: VISIBLE_FRAME_BUDGET_NS,
            budget_passed: over_budget_samples == 0,
            over_budget_samples,
            root_frame_budget_passed: root_frame_over_budget_samples == 0,
            root_frame_over_budget_samples,
            capacity: self.capacity,
            dropped_samples: self.dropped_samples,
            total_samples: self.samples.len(),
            kinds,
        }
    }

    pub fn flush_to_file(&self) -> std::io::Result<Option<PathBuf>> {
        let Some(path) = &self.path else {
            return Ok(None);
        };
        write_visible_frame_timing_file(path, &self.summary(), &self.samples)?;
        Ok(Some(path.clone()))
    }
}

impl Drop for VisibleFrameTimingRecorder {
    fn drop(&mut self) {
        let _ = self.flush_to_file();
    }
}

fn is_visible_root_frame_timing_kind(kind: &str) -> bool {
    matches!(kind, "iced.redraw_requested" | "egui.render_ui")
}

fn percentile_ns(sorted: &[u128], percentile: usize) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = sorted.len().saturating_mul(percentile).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn write_visible_frame_timing_file(
    path: &Path,
    summary: &VisibleFrameTimingSummary,
    samples: &VecDeque<VisibleFrameTimingSample>,
) -> std::io::Result<()> {
    let mut output = String::new();
    let budget_status = if summary.budget_passed {
        "PASS"
    } else {
        "FAIL"
    };
    let root_frame_budget_status = if summary.root_frame_budget_passed {
        "PASS"
    } else {
        "FAIL"
    };
    let _ = writeln!(
        output,
        "# slipway-visible-frame-timing version=1 backend={} budget_hz={} budget_ns={} budget_status={} over_budget_total={} root_frame_budget_status={} root_frame_over_budget_total={} samples={} dropped={} capacity={}",
        summary.backend,
        summary.budget_hz,
        summary.budget_ns,
        budget_status,
        summary.over_budget_samples,
        root_frame_budget_status,
        summary.root_frame_over_budget_samples,
        summary.total_samples,
        summary.dropped_samples,
        summary.capacity
    );
    let _ = writeln!(
        output,
        "# summary kind samples p50_ns p95_ns p99_ns max_ns over_budget"
    );
    for kind in &summary.kinds {
        let _ = writeln!(
            output,
            "# summary {} {} {} {} {} {} {}",
            kind.kind,
            kind.samples,
            kind.p50_ns,
            kind.p95_ns,
            kind.p99_ns,
            kind.max_ns,
            kind.over_budget_samples
        );
    }
    let _ = writeln!(
        output,
        "sequence,backend,kind,duration_ns,event_count,viewport_width,viewport_height"
    );
    for sample in samples {
        let (width, height) = sample
            .viewport
            .map(|viewport| (viewport.width.to_string(), viewport.height.to_string()))
            .unwrap_or_else(|| ("".to_string(), "".to_string()));
        let _ = writeln!(
            output,
            "{},{},{},{},{},{},{}",
            sample.sequence,
            summary.backend,
            sample.kind,
            sample.duration_ns,
            sample.event_count,
            width,
            height
        );
    }
    std::fs::write(path, output)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentedScreenshotAdmission {
    Exact,
    Current,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PresentedScreenshotSelector {
    Exact { expected_frame: FrameIdentity },
    Current { request_context: FrameIdentity },
}

impl PresentedScreenshotSelector {
    pub fn admission(&self) -> PresentedScreenshotAdmission {
        match self {
            Self::Exact { .. } => PresentedScreenshotAdmission::Exact,
            Self::Current { .. } => PresentedScreenshotAdmission::Current,
        }
    }

    pub fn correlation_frame(&self) -> &FrameIdentity {
        match self {
            Self::Exact { expected_frame } => expected_frame,
            Self::Current { request_context } => request_context,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresentedScreenshotRequest {
    pub selector: PresentedScreenshotSelector,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentedTransferFunction {
    Linear,
    Srgb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentedAlphaMode {
    Opaque,
    Premultiplied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentedSurfaceFormat {
    Rgba8Unorm,
    Rgba8UnormSrgb,
    Bgra8Unorm,
    Bgra8UnormSrgb,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PresentedCapturePath {
    DirectAcquiredSurfaceTextureCopy,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresentedPixels {
    pub selector: PresentedScreenshotSelector,
    pub captured_frame: FrameIdentity,
    pub source: slipway_core::EvidenceSource,
    pub capture_path: PresentedCapturePath,
    pub source_format: PresentedSurfaceFormat,
    pub transfer: PresentedTransferFunction,
    pub alpha: PresentedAlphaMode,
    pub width: u32,
    pub height: u32,
    pub bytes: Arc<[u8]>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PresentedScreenshotRefusal {
    pub selector: PresentedScreenshotSelector,
    pub captured_frame: Option<FrameIdentity>,
    pub backend_id: Option<String>,
    pub code: String,
    pub reason: String,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum PresentedScreenshotProduct {
    Captured(PresentedPixels),
    Refusal(PresentedScreenshotRefusal),
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugCommand {
    pub request_id: String,
    pub kind: DebugCommandKind,
}

impl DebugCommand {
    pub fn status(request_id: impl Into<String>, frame: FrameIdentity) -> Self {
        Self {
            request_id: request_id.into(),
            kind: DebugCommandKind::Status { frame },
        }
    }

    pub fn probe(
        request_id: impl Into<String>,
        frame: FrameIdentity,
        request: ProbeRequest,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            kind: DebugCommandKind::Probe { frame, request },
        }
    }

    pub fn render(request_id: impl Into<String>, packet: RenderPacket) -> Self {
        Self {
            request_id: request_id.into(),
            kind: DebugCommandKind::Render { packet },
        }
    }

    pub fn screenshot(request_id: impl Into<String>, request: PresentedScreenshotRequest) -> Self {
        Self {
            request_id: request_id.into(),
            kind: DebugCommandKind::Screenshot { request },
        }
    }

    pub fn control(request_id: impl Into<String>, frame: FrameIdentity, event: InputEvent) -> Self {
        Self {
            request_id: request_id.into(),
            kind: DebugCommandKind::Control {
                frame,
                event,
                trace: false,
            },
        }
    }

    pub fn control_with_trace(
        request_id: impl Into<String>,
        frame: FrameIdentity,
        event: InputEvent,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            kind: DebugCommandKind::Control {
                frame,
                event,
                trace: true,
            },
        }
    }

    pub fn physical_control_with_trace(
        request_id: impl Into<String>,
        frame: FrameIdentity,
        operation: DebugPhysicalControl,
    ) -> Self {
        Self {
            request_id: request_id.into(),
            kind: DebugCommandKind::PhysicalControl {
                frame,
                operation,
                trace: true,
            },
        }
    }

    pub fn resize(request_id: impl Into<String>, frame: FrameIdentity) -> Self {
        Self {
            request_id: request_id.into(),
            kind: DebugCommandKind::Resize { frame },
        }
    }

    pub fn frame_identity(&self) -> &FrameIdentity {
        match &self.kind {
            DebugCommandKind::Status { frame }
            | DebugCommandKind::Probe { frame, .. }
            | DebugCommandKind::Control { frame, .. }
            | DebugCommandKind::PhysicalControl { frame, .. }
            | DebugCommandKind::Resize { frame } => frame,
            DebugCommandKind::Render { packet } => &packet.frame,
            DebugCommandKind::Screenshot { request } => request.selector.correlation_frame(),
        }
    }

    pub fn screenshot_admission(&self) -> Option<PresentedScreenshotAdmission> {
        match &self.kind {
            DebugCommandKind::Screenshot { request } => Some(request.selector.admission()),
            _ => None,
        }
    }

    pub fn control_trace_enabled(&self) -> bool {
        matches!(
            self.kind,
            DebugCommandKind::Control { trace: true, .. }
                | DebugCommandKind::PhysicalControl { trace: true, .. }
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebugPhysicalControl {
    Pointer {
        position: Point,
        kind: PointerEventKind,
        button: Option<PointerButton>,
        details: PointerDetails,
        pointer_is_pressed: bool,
    },
    Wheel {
        position: Point,
        delta_x: f32,
        delta_y: f32,
    },
    Focus {
        selector: DebugPhysicalControlDeclarationSelector,
        focused: bool,
    },
    Text {
        selector: DebugPhysicalControlDeclarationSelector,
        text: String,
    },
    TextEdit {
        selector: DebugPhysicalControlDeclarationSelector,
        kind: TextEditKind,
        text: Option<String>,
        selection_before: Option<TextSelectionRange>,
        selection_after: Option<TextSelectionRange>,
    },
    Keyboard {
        selector: DebugPhysicalControlDeclarationSelector,
        key: String,
        kind: KeyEventKind,
        modifiers: Modifiers,
        details: KeyboardDetails,
    },
    Command {
        selector: DebugPhysicalControlDeclarationSelector,
        command: String,
        payload_ref: Option<String>,
    },
    Scroll {
        selector: DebugPhysicalControlDeclarationSelector,
        offset_x: f32,
        offset_y: f32,
    },
    TextComposition {
        selector: DebugPhysicalControlDeclarationSelector,
        updates: Vec<DebugTextCompositionUpdate>,
        commit: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugTextCompositionUpdate {
    pub preedit_text: String,
    pub cursor_range: Option<TextSelectionRange>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebugPhysicalControlDeclarationSelector {
    Target { target: WidgetId },
    Region { region: PresentationRegionId },
    Position { position: Point },
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebugCommandKind {
    Status {
        frame: FrameIdentity,
    },
    Probe {
        frame: FrameIdentity,
        request: ProbeRequest,
    },
    Render {
        packet: RenderPacket,
    },
    Screenshot {
        request: PresentedScreenshotRequest,
    },
    Control {
        frame: FrameIdentity,
        event: InputEvent,
        trace: bool,
    },
    PhysicalControl {
        frame: FrameIdentity,
        operation: DebugPhysicalControl,
        trace: bool,
    },
    Resize {
        frame: FrameIdentity,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MessageDisposition {
    Consumed,
    Ignored,
    ReductionUnavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugControlTraceStageKind {
    Generated,
    Routed,
    Consumed,
    Ignored,
    Reduced,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugControlMode {
    SemanticDirect,
    PhysicalEquivalent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugControlTraceStage {
    pub stage: DebugControlTraceStageKind,
    pub actor: String,
    pub target: Option<WidgetId>,
    pub detail: String,
}

impl DebugControlTraceStage {
    pub fn generated(target: impl Into<WidgetId>, detail: impl Into<String>) -> Self {
        Self::new(
            DebugControlTraceStageKind::Generated,
            "slipway-debug-control",
            Some(target.into()),
            detail,
        )
    }

    pub fn routed(target: impl Into<WidgetId>, detail: impl Into<String>) -> Self {
        Self::new(
            DebugControlTraceStageKind::Routed,
            "slipway-runtime",
            Some(target.into()),
            detail,
        )
    }

    pub fn consumed(target: impl Into<WidgetId>, detail: impl Into<String>) -> Self {
        Self::new(
            DebugControlTraceStageKind::Consumed,
            "slipway-widget",
            Some(target.into()),
            detail,
        )
    }

    pub fn ignored(target: impl Into<WidgetId>, detail: impl Into<String>) -> Self {
        Self::new(
            DebugControlTraceStageKind::Ignored,
            "slipway-widget",
            Some(target.into()),
            detail,
        )
    }

    pub fn reduced(
        actor: impl Into<String>,
        target: Option<WidgetId>,
        detail: impl Into<String>,
    ) -> Self {
        Self::new(DebugControlTraceStageKind::Reduced, actor, target, detail)
    }

    pub fn new(
        stage: DebugControlTraceStageKind,
        actor: impl Into<String>,
        target: Option<WidgetId>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            stage,
            actor: actor.into(),
            target,
            detail: detail.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugMessageTraceEntry {
    pub source: WidgetId,
    pub name: String,
    pub disposition: MessageDisposition,
}

impl DebugMessageTraceEntry {
    pub fn emitted(
        source: impl Into<WidgetId>,
        name: impl Into<String>,
        disposition: MessageDisposition,
    ) -> Self {
        Self {
            source: source.into(),
            name: name.into(),
            disposition,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugControlTrace {
    pub request_id: String,
    pub frame: FrameIdentity,
    pub mode: DebugControlMode,
    pub dispatch_evidence: Option<DeclaredEventDispatchEvidence>,
    pub result_identity: Option<EventResultIdentity>,
    pub routed_event_target: WidgetId,
    pub event_summary: String,
    pub handled: bool,
    pub stages: Vec<DebugControlTraceStage>,
    pub messages: Vec<DebugMessageTraceEntry>,
    pub revision_before: u64,
    pub revision_after: u64,
    pub diagnostics: Vec<Diagnostic>,
}

impl DebugControlTrace {
    pub fn new(
        request_id: impl Into<String>,
        frame: FrameIdentity,
        event: &InputEvent,
        handled: bool,
        revision_before: u64,
        revision_after: u64,
        diagnostics: Vec<Diagnostic>,
    ) -> Self {
        let routed_event_target = event.target().clone();
        let mut stages = vec![
            DebugControlTraceStage::generated(
                routed_event_target.clone(),
                "semantic debug control request injected an input event; this is not physical-equivalent evidence",
            ),
            DebugControlTraceStage::routed(
                routed_event_target.clone(),
                "runtime routed the semantic input event to the declared target",
            ),
        ];
        if handled {
            stages.push(DebugControlTraceStage::consumed(
                routed_event_target.clone(),
                "widget event handler consumed the input event",
            ));
        } else {
            stages.push(DebugControlTraceStage::ignored(
                routed_event_target.clone(),
                "widget event handler ignored the input event",
            ));
        }

        Self {
            request_id: request_id.into(),
            frame,
            mode: DebugControlMode::SemanticDirect,
            dispatch_evidence: None,
            result_identity: None,
            routed_event_target,
            event_summary: event_summary(event),
            handled,
            stages,
            messages: Vec::new(),
            revision_before,
            revision_after,
            diagnostics,
        }
    }

    pub fn with_messages(mut self, messages: Vec<DebugMessageTraceEntry>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_mode(mut self, mode: DebugControlMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn with_dispatch_evidence(
        mut self,
        evidence: Option<DeclaredEventDispatchEvidence>,
    ) -> Self {
        if evidence
            .as_ref()
            .is_some_and(|evidence| evidence.source.label == EVIDENCE_SOURCE_BACKEND_PRESENTED)
        {
            self.mode = DebugControlMode::PhysicalEquivalent;
            let target = self.routed_event_target.clone();
            if let Some(stage) = self.stages.get_mut(0) {
                *stage = DebugControlTraceStage::new(
                    DebugControlTraceStageKind::Generated,
                    "slipway-backend-native",
                    Some(target.clone()),
                    "backend-presented physical input entered the backend event lifecycle",
                );
            }
            if let Some(stage) = self.stages.get_mut(1) {
                *stage = DebugControlTraceStage::routed(
                    target,
                    "runtime routed backend-presented physical input to the declared target",
                );
            }
        }
        self.dispatch_evidence = evidence;
        self
    }

    pub fn with_result_identity(mut self, identity: EventResultIdentity) -> Self {
        self.result_identity = Some(identity);
        self
    }

    pub fn with_reduction_stage(mut self, stage: DebugControlTraceStage) -> Self {
        self.stages.push(stage);
        self
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CompositionPhaseProvenance {
    Native,
    Derived { from: TextCompositionPhase },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DebugCompositionIngressObservation {
    IcedQueueSlice { sequence_index: usize },
    EguiRawInputSpan { event_index: usize },
    Derived { from_sequence_index: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugCompositionPhaseTrace {
    pub phase: TextCompositionPhase,
    pub backend_event: String,
    pub provenance: CompositionPhaseProvenance,
    pub event: TextCompositionEvent,
    pub ingress_observation: DebugCompositionIngressObservation,
    pub dispatch_evidence: DeclaredEventDispatchEvidence,
    pub app_handled: bool,
    pub result_identity: Option<EventResultIdentity>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugCompositionCommitMutation {
    pub trace: DebugControlTrace,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugCompositionTrace {
    pub request_id: String,
    pub frame: FrameIdentity,
    pub backend_id: String,
    pub target: WidgetId,
    pub selected_region: PresentationRegionId,
    pub focused_before: bool,
    pub focused_after: bool,
    pub phases: Vec<DebugCompositionPhaseTrace>,
    pub commit_mutation: Option<DebugCompositionCommitMutation>,
    pub completed: bool,
    pub failure: Option<DebugFailure>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugStatus {
    pub admitted: bool,
    pub detail: String,
    /// Runtime state revision at the time the status reply was assembled.
    pub revision: u64,
    /// Visible backend that registered itself with the runtime, when one has.
    /// `None` means no visible backend has identified itself (for example a
    /// headless runtime).
    pub backend_id: Option<String>,
    /// Number of backend input traces currently retained in the bounded ring.
    pub trace_buffer_depth: usize,
    /// Capacity of the backend input trace ring.
    pub trace_buffer_capacity: usize,
    /// Debug replies the runtime handler refused (`DebugReplyProduct::Error`)
    /// since the runtime started. Interceptor-produced backend refusals are
    /// not counted here.
    pub refused_debug_replies: u64,
    /// Backend input traces recorded as unhandled since the runtime started.
    pub unhandled_backend_input_traces: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugReply {
    pub request_id: String,
    pub frame: FrameIdentity,
    pub product: DebugReplyProduct,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebugReplyProduct {
    Status(DebugStatus),
    Probes(Vec<ProbeProduct>),
    Render(RenderProduct),
    Screenshot(PresentedScreenshotProduct),
    Diagnostics(Vec<Diagnostic>),
    ControlTrace(DebugControlTrace),
    CompositionTrace(DebugCompositionTrace),
    Error(DebugFailure),
}

impl DebugReplyProduct {
    fn frame_identity_mismatch(&self, expected: &FrameIdentity) -> bool {
        match self {
            Self::Render(RenderProduct::Evidence(evidence)) => &evidence.frame != expected,
            Self::Render(RenderProduct::Refusal(refusal)) => &refusal.frame != expected,
            Self::Screenshot(PresentedScreenshotProduct::Captured(pixels)) => {
                pixels.selector.correlation_frame() != expected
            }
            Self::Screenshot(PresentedScreenshotProduct::Refusal(refusal)) => {
                refusal.selector.correlation_frame() != expected
            }
            Self::ControlTrace(trace) => &trace.frame != expected,
            Self::CompositionTrace(trace) => &trace.frame != expected,
            Self::Probes(products) => products.iter().any(|product| {
                probe_frame_identity(product).is_some_and(|frame| frame != expected)
            }),
            Self::Status(_) | Self::Diagnostics(_) | Self::Error(_) => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum RenderProduct {
    Evidence(RenderEvidence),
    Refusal(RenderRefusal),
}

pub fn validate_presented_screenshot_product(
    reply_frame: &FrameIdentity,
    admitted: PresentedScreenshotAdmission,
    product: &PresentedScreenshotProduct,
) -> Result<(), DebugFailure> {
    let selector = match product {
        PresentedScreenshotProduct::Captured(pixels) => &pixels.selector,
        PresentedScreenshotProduct::Refusal(refusal) => &refusal.selector,
    };
    if selector.correlation_frame() != reply_frame {
        return Err(screenshot_validation_failure(
            "screenshot-requested-frame-mismatch",
            "presented screenshot requested frame does not match the admitted command frame",
        ));
    }
    if selector.admission() != admitted {
        return Err(screenshot_validation_failure(
            "screenshot-admission-mismatch",
            "presented screenshot admission does not match the admitted command selector",
        ));
    }

    let PresentedScreenshotProduct::Captured(pixels) = product else {
        return Ok(());
    };

    if let PresentedScreenshotSelector::Exact { expected_frame } = selector {
        let captured = &pixels.captured_frame;
        let next_frame_index = expected_frame.frame_index.checked_add(1);
        if captured.surface_id != expected_frame.surface_id
            || captured.surface_instance_id != expected_frame.surface_instance_id
            || captured.revision != expected_frame.revision
            || captured.viewport != expected_frame.viewport
            || Some(captured.frame_index) != next_frame_index
        {
            return Err(screenshot_validation_failure(
                "screenshot-captured-frame-transition-invalid",
                "presented screenshot must capture exactly the next presentation of the admitted surface identity",
            ));
        }
    }

    if pixels.source.label() != EVIDENCE_SOURCE_BACKEND_PRESENTED
        || pixels
            .source
            .backend_id
            .as_deref()
            .is_none_or(str::is_empty)
        || pixels.source.pass_id.as_deref() != Some(PRESENTED_PIXELS_PASS_ID)
        || pixels.source.provider_id.is_some()
    {
        return Err(screenshot_validation_failure(
            "screenshot-presented-provenance-invalid",
            "presented screenshot requires direct backend-presented provenance",
        ));
    }

    let expected_transfer = match pixels.source_format {
        PresentedSurfaceFormat::Rgba8Unorm | PresentedSurfaceFormat::Bgra8Unorm => {
            PresentedTransferFunction::Linear
        }
        PresentedSurfaceFormat::Rgba8UnormSrgb | PresentedSurfaceFormat::Bgra8UnormSrgb => {
            PresentedTransferFunction::Srgb
        }
    };
    if pixels.transfer != expected_transfer {
        return Err(screenshot_validation_failure(
            "screenshot-transfer-function-mismatch",
            "presented screenshot transfer function does not match its acquired surface format",
        ));
    }

    let expected_len = usize::try_from(pixels.width)
        .ok()
        .and_then(|width| {
            usize::try_from(pixels.height)
                .ok()
                .and_then(|height| width.checked_mul(height))
        })
        .and_then(|pixels| pixels.checked_mul(4));
    if pixels.width == 0 || pixels.height == 0 || expected_len != Some(pixels.bytes.len()) {
        return Err(screenshot_validation_failure(
            "screenshot-artifact-invalid-rgba",
            "presented screenshot bytes must be tightly packed non-empty top-row-first RGBA8",
        ));
    }

    Ok(())
}

fn screenshot_validation_failure(code: &str, message: &str) -> DebugFailure {
    DebugFailure {
        code: code.to_string(),
        message: message.to_string(),
        dispatch_evidence: None,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DebugFailure {
    pub code: String,
    pub message: String,
    pub dispatch_evidence: Option<DeclaredEventDispatchEvidence>,
}

#[derive(Debug, Eq, PartialEq)]
pub enum DebugBridgeError {
    CommandQueueFull,
    CommandQueueDisconnected,
    ReplyQueueEmpty,
    ReplyQueueFull,
    ReplyQueueDisconnected,
}

pub trait SlipwayDebugCommandHandler {
    fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct;
}

#[derive(Clone)]
pub struct DebugBridgeClient {
    command_tx: Sender<DebugEnvelope>,
}

pub struct DebugBridgeRuntime {
    command_rx: Receiver<DebugEnvelope>,
}

pub struct DebugRequestHandle {
    request_id: String,
    reply_rx: Receiver<DebugReply>,
}

pub struct DebugCommandLease {
    request_id: String,
    frame: FrameIdentity,
    screenshot_admission: Option<PresentedScreenshotAdmission>,
    command: DebugCommand,
    reply_tx: Sender<DebugReply>,
}

struct DebugEnvelope {
    command: DebugCommand,
    reply_tx: Sender<DebugReply>,
}

pub fn bounded_debug_bridge(capacity: usize) -> (DebugBridgeClient, DebugBridgeRuntime) {
    let (command_tx, command_rx) = bounded(capacity);
    (
        DebugBridgeClient { command_tx },
        DebugBridgeRuntime { command_rx },
    )
}

impl DebugBridgeClient {
    pub fn submit(&self, command: DebugCommand) -> Result<DebugRequestHandle, DebugBridgeError> {
        let request_id = command.request_id.clone();
        let (reply_tx, reply_rx) = bounded(1);
        let envelope = DebugEnvelope { command, reply_tx };

        self.command_tx
            .try_send(envelope)
            .map_err(|error| match error {
                TrySendError::Full(_) => DebugBridgeError::CommandQueueFull,
                TrySendError::Disconnected(_) => DebugBridgeError::CommandQueueDisconnected,
            })?;

        Ok(DebugRequestHandle {
            request_id,
            reply_rx,
        })
    }
}

impl DebugBridgeRuntime {
    pub fn take_one(&self) -> Result<Option<DebugCommandLease>, DebugBridgeError> {
        let envelope = match self.command_rx.try_recv() {
            Ok(envelope) => envelope,
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => {
                return Err(DebugBridgeError::CommandQueueDisconnected);
            }
        };

        let screenshot_admission = envelope.command.screenshot_admission();
        Ok(Some(DebugCommandLease {
            request_id: envelope.command.request_id.clone(),
            frame: envelope.command.frame_identity().clone(),
            screenshot_admission,
            command: envelope.command,
            reply_tx: envelope.reply_tx,
        }))
    }

    pub fn drain_one<H>(&self, handler: &mut H) -> Result<Option<DebugReply>, DebugBridgeError>
    where
        H: SlipwayDebugCommandHandler,
    {
        self.drain_one_with_interceptor(handler, &mut |_| None)
    }

    pub fn drain_one_with_interceptor<H, I>(
        &self,
        handler: &mut H,
        intercept: &mut I,
    ) -> Result<Option<DebugReply>, DebugBridgeError>
    where
        H: SlipwayDebugCommandHandler,
        I: FnMut(&DebugCommand) -> Option<DebugReplyProduct>,
    {
        let envelope = match self.command_rx.try_recv() {
            Ok(envelope) => envelope,
            Err(TryRecvError::Empty) => return Ok(None),
            Err(TryRecvError::Disconnected) => {
                return Err(DebugBridgeError::CommandQueueDisconnected);
            }
        };

        let request_id = envelope.command.request_id.clone();
        let frame = envelope.command.frame_identity().clone();
        let screenshot_admission = envelope.command.screenshot_admission();
        let product = checked_product(
            frame.clone(),
            screenshot_admission,
            match intercept(&envelope.command) {
                Some(product) => product,
                None => handler.handle_debug_command(envelope.command),
            },
        );
        let reply = DebugReply {
            request_id,
            frame,
            product,
        };

        envelope
            .reply_tx
            .try_send(reply.clone())
            .map_err(|error| match error {
                TrySendError::Full(_) => DebugBridgeError::ReplyQueueFull,
                TrySendError::Disconnected(_) => DebugBridgeError::ReplyQueueDisconnected,
            })?;

        Ok(Some(reply))
    }
}

impl DebugRequestHandle {
    pub fn request_id(&self) -> &str {
        &self.request_id
    }

    pub fn try_recv(&self) -> Result<Option<DebugReply>, DebugBridgeError> {
        match self.reply_rx.try_recv() {
            Ok(reply) => Ok(Some(reply)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => Err(DebugBridgeError::ReplyQueueDisconnected),
        }
    }
}

impl DebugCommandLease {
    pub fn command(&self) -> &DebugCommand {
        &self.command
    }

    pub fn complete(self, product: DebugReplyProduct) -> Result<DebugReply, DebugBridgeError> {
        let reply = DebugReply {
            request_id: self.request_id,
            frame: self.frame.clone(),
            product: checked_product(self.frame, self.screenshot_admission, product),
        };

        self.reply_tx
            .try_send(reply.clone())
            .map_err(|error| match error {
                TrySendError::Full(_) => DebugBridgeError::ReplyQueueFull,
                TrySendError::Disconnected(_) => DebugBridgeError::ReplyQueueDisconnected,
            })?;

        Ok(reply)
    }
}

fn checked_product(
    frame: FrameIdentity,
    screenshot_admission: Option<PresentedScreenshotAdmission>,
    product: DebugReplyProduct,
) -> DebugReplyProduct {
    if product.frame_identity_mismatch(&frame) {
        DebugReplyProduct::Error(DebugFailure {
            code: "frame-identity-mismatch".to_string(),
            message: "handler returned evidence for a different frame identity".to_string(),
            dispatch_evidence: None,
        })
    } else if let DebugReplyProduct::Screenshot(screenshot) = &product {
        let Some(admitted) = screenshot_admission else {
            return DebugReplyProduct::Error(DebugFailure {
                code: "screenshot-admission-missing".to_string(),
                message: "screenshot product returned for a non-screenshot command".to_string(),
                dispatch_evidence: None,
            });
        };
        match validate_presented_screenshot_product(&frame, admitted, screenshot) {
            Ok(()) => product,
            Err(failure) => DebugReplyProduct::Error(failure),
        }
    } else {
        product
    }
}

fn event_summary(event: &InputEvent) -> String {
    match event {
        InputEvent::Pointer(event) => format!("pointer:{:?}", event.kind),
        InputEvent::Keyboard(event) => format!("keyboard:{:?}:{}", event.kind, event.key),
        InputEvent::Text(event) => format!("text:{}", event.text),
        InputEvent::TextEdit(event) => format!("text-edit:{:?}", event.kind),
        InputEvent::TextComposition(event) => format!("text-composition:{:?}", event.phase),
        InputEvent::Selection(event) => format!("selection:{} ranges", event.state.ranges.len()),
        InputEvent::Wheel(event) => format!("wheel:{},{}", event.delta_x, event.delta_y),
        InputEvent::Scroll(event) => {
            format!(
                "scroll:{}:{},{}",
                event.region_id.as_str(),
                event.offset_x,
                event.offset_y
            )
        }
        InputEvent::Focus(event) => format!("focus:{}", event.focused),
        InputEvent::Command(event) => format!("command:{}", event.command),
        InputEvent::Clipboard(event) => format!("clipboard:{:?}", event.kind),
        InputEvent::DragDrop(event) => format!("drag-drop:{:?}", event.phase),
        InputEvent::File(event) => format!("file:{} files", event.files.len()),
    }
}

fn probe_frame_identity(product: &ProbeProduct) -> Option<&FrameIdentity> {
    match product {
        ProbeProduct::ViewDefinition(view) => Some(&view.frame),
        ProbeProduct::RenderPacket(packet) => Some(&packet.frame),
        ProbeProduct::RenderEvidence(evidence) => Some(&evidence.frame),
        ProbeProduct::DispatchGraph(probe) => Some(&probe.frame),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpProbeMethod {
    Status,
    Probe,
    Render,
    Screenshot,
    Control,
    Resize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpProbeRequest {
    pub method: McpProbeMethod,
    pub command: DebugCommand,
}

#[derive(Clone, Debug, PartialEq)]
pub struct McpProbeResponse {
    pub method: McpProbeMethod,
    pub reply: DebugReply,
}

pub trait SlipwayMcpProbeTransport {
    fn submit_probe_command(
        &mut self,
        request: McpProbeRequest,
    ) -> Result<DebugRequestHandle, DebugBridgeError>;

    fn response_from_reply(
        &mut self,
        method: McpProbeMethod,
        reply: DebugReply,
    ) -> McpProbeResponse;
}

#[cfg(test)]
mod tests {
    use super::*;
    use slipway_core::{
        Color, CommandEvent, DiagnosticSeverity, EvidenceSource, LayoutOutput, PaintOp, Point,
        Rect, RenderSurfaceDeclaration, ShapeDeclaration, ShapeKind, Size,
        SlipwayOffscreenRenderer, TargetLocalRect, WidgetId,
    };

    fn frame(index: u64) -> FrameIdentity {
        FrameIdentity {
            surface_id: "surface".to_string(),
            surface_instance_id: "instance".to_string(),
            revision: 4,
            frame_index: index,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 200.0,
                    height: 100.0,
                },
            },
        }
    }

    fn packet(frame: FrameIdentity) -> RenderPacket {
        let layout = LayoutOutput {
            bounds: TargetLocalRect::new(frame.viewport),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        RenderPacket {
            target: WidgetId::from("widget"),
            frame,
            layout,
            paint: vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some("body".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: Rect {
                        origin: Point { x: 0.0, y: 0.0 },
                        size: Size {
                            width: 20.0,
                            height: 10.0,
                        },
                    },
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 1.0,
                    green: 0.0,
                    blue: 0.0,
                    alpha: 1.0,
                },
            }],
            surfaces: vec![RenderSurfaceDeclaration {
                id: WidgetId::from("surface-widget"),
                provider_id: "provider".to_string(),
                bounds: Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 10.0,
                        height: 10.0,
                    },
                },
                payload_ref: Some("payload".to_string()),
                dirty_regions: Vec::new(),
                capabilities: vec!["test".to_string()],
            }],
            diagnostics: Vec::new(),
        }
    }

    fn control_event() -> InputEvent {
        InputEvent::Command(CommandEvent {
            target: WidgetId::from("widget"),
            target_slot: None,
            command: "activate".to_string(),
            payload_ref: Some("payload://control".to_string()),
            source: None,
        })
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
                source: EvidenceSource::canonical_offscreen("provider"),
                provider_id: "provider".to_string(),
                artifact_ref: Some("artifact://frame".to_string()),
                artifact_path: None,
                pixel_hash: Some("abc".to_string()),
                width: Some(width),
                height: Some(height),
                diagnostics: packet.diagnostics,
            })
        }
    }

    struct Handler {
        calls: u32,
        renderer: FakeRenderer,
        last_control: Option<(FrameIdentity, InputEvent, bool)>,
        last_resize: Option<FrameIdentity>,
    }

    impl Handler {
        fn new() -> Self {
            Self {
                calls: 0,
                renderer: FakeRenderer::default(),
                last_control: None,
                last_resize: None,
            }
        }
    }

    impl SlipwayDebugCommandHandler for Handler {
        fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
            self.calls += 1;
            match command.kind {
                DebugCommandKind::Status { .. } => DebugReplyProduct::Status(DebugStatus {
                    admitted: true,
                    detail: "ready".to_string(),
                    revision: 0,
                    backend_id: None,
                    trace_buffer_depth: 0,
                    trace_buffer_capacity: 0,
                    refused_debug_replies: 0,
                    unhandled_backend_input_traces: 0,
                }),
                DebugCommandKind::Probe { request, .. } => DebugReplyProduct::Probes(
                    request
                        .kinds
                        .into_iter()
                        .map(|kind| {
                            ProbeProduct::Diagnostic(slipway_core::Diagnostic {
                                target: None,
                                severity: DiagnosticSeverity::Info,
                                code: format!("{kind:?}"),
                                message: "probe requested".to_string(),
                            })
                        })
                        .collect(),
                ),
                DebugCommandKind::Render { packet } => match self.renderer.render_offscreen(packet)
                {
                    Ok(evidence) => DebugReplyProduct::Render(RenderProduct::Evidence(evidence)),
                    Err(refusal) => DebugReplyProduct::Render(RenderProduct::Refusal(refusal)),
                },
                DebugCommandKind::Screenshot { request } => DebugReplyProduct::Screenshot(
                    PresentedScreenshotProduct::Refusal(PresentedScreenshotRefusal {
                        selector: request.selector,
                        captured_frame: None,
                        backend_id: None,
                        code: "screenshot-no-visible-backend".to_string(),
                        reason: "test handler has no visible backend".to_string(),
                        diagnostics: Vec::new(),
                    }),
                ),
                DebugCommandKind::Control {
                    frame,
                    event,
                    trace,
                } => {
                    self.last_control = Some((frame, event, trace));
                    DebugReplyProduct::Diagnostics(vec![Diagnostic {
                        target: None,
                        severity: DiagnosticSeverity::Info,
                        code: "control-received".to_string(),
                        message: "control command received".to_string(),
                    }])
                }
                DebugCommandKind::PhysicalControl { frame, trace, .. } => {
                    self.last_resize = Some(frame);
                    DebugReplyProduct::Diagnostics(vec![Diagnostic {
                        target: None,
                        severity: DiagnosticSeverity::Info,
                        code: "physical-control-received".to_string(),
                        message: format!("physical control command received trace={trace}"),
                    }])
                }
                DebugCommandKind::Resize { frame } => {
                    self.last_resize = Some(frame);
                    DebugReplyProduct::Diagnostics(vec![Diagnostic {
                        target: None,
                        severity: DiagnosticSeverity::Info,
                        code: "resize-received".to_string(),
                        message: "resize command received".to_string(),
                    }])
                }
            }
        }
    }

    #[test]
    fn no_command_means_no_probe_or_renderer_call() {
        let (_client, runtime) = bounded_debug_bridge(1);
        let mut handler = Handler::new();

        let drained = runtime.drain_one(&mut handler).expect("drain succeeds");

        assert!(drained.is_none());
        assert_eq!(handler.calls, 0);
        assert_eq!(handler.renderer.calls, 0);
    }

    #[test]
    fn status_command_round_trips_request_and_frame_identity() {
        let (client, runtime) = bounded_debug_bridge(1);
        let mut handler = Handler::new();
        let command = DebugCommand::status("req-1", frame(9));

        let handle = client.submit(command).expect("command queued");
        let drained = runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("reply generated");
        let received = handle
            .try_recv()
            .expect("reply channel readable")
            .expect("reply available");

        assert_eq!(handle.request_id(), "req-1");
        assert_eq!(drained, received);
        assert_eq!(received.request_id, "req-1");
        assert_eq!(received.frame.frame_index, 9);
        assert!(matches!(received.product, DebugReplyProduct::Status(_)));
    }

    #[test]
    fn bounded_bridge_reports_backpressure() {
        let (client, _runtime) = bounded_debug_bridge(1);

        let _first = client
            .submit(DebugCommand::status("req-1", frame(1)))
            .expect("first command fits");
        let second = client.submit(DebugCommand::status("req-2", frame(2)));

        assert!(matches!(second, Err(DebugBridgeError::CommandQueueFull)));
    }

    #[test]
    fn render_command_invokes_provider_trait_only_when_drained() {
        let (client, runtime) = bounded_debug_bridge(1);
        let mut handler = Handler::new();
        let frame = frame(11);
        let handle = client
            .submit(DebugCommand::render("render-1", packet(frame.clone())))
            .expect("render command queued");

        assert_eq!(handler.renderer.calls, 0);

        runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("reply generated");
        let reply = handle
            .try_recv()
            .expect("reply channel readable")
            .expect("reply available");

        assert_eq!(handler.renderer.calls, 1);
        assert_eq!(reply.frame, frame);
        match reply.product {
            DebugReplyProduct::Render(RenderProduct::Evidence(evidence)) => {
                assert_eq!(evidence.frame, frame);
                assert_eq!(evidence.provider_id, "provider");
                assert_eq!(
                    evidence.source,
                    EvidenceSource::canonical_offscreen("provider")
                );
            }
            other => panic!("expected render evidence, got {other:?}"),
        }
    }

    #[test]
    fn control_command_round_trips_request_frame_and_event() {
        let (client, runtime) = bounded_debug_bridge(1);
        let mut handler = Handler::new();
        let frame = frame(12);
        let event = control_event();
        let command = DebugCommand::control("control-1", frame.clone(), event.clone());

        assert_eq!(command.frame_identity(), &frame);

        let handle = client.submit(command).expect("control command queued");
        let drained = runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("reply generated");
        let received = handle
            .try_recv()
            .expect("reply channel readable")
            .expect("reply available");

        assert_eq!(drained, received);
        assert_eq!(received.request_id, "control-1");
        assert_eq!(received.frame, frame);
        assert_eq!(handler.last_control, Some((frame, event, false)));
        match received.product {
            DebugReplyProduct::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, "control-received");
            }
            other => panic!("expected control diagnostic, got {other:?}"),
        }
    }

    #[test]
    fn control_trace_is_explicit_and_request_scoped_data() {
        let frame = frame(14);
        let event = control_event();
        let ordinary = DebugCommand::control("control-ordinary", frame.clone(), event.clone());
        let traced =
            DebugCommand::control_with_trace("control-traced", frame.clone(), event.clone());

        assert!(!ordinary.control_trace_enabled());
        assert!(traced.control_trace_enabled());
        assert_eq!(traced.frame_identity(), &frame);
        match traced.kind {
            DebugCommandKind::Control {
                frame: traced_frame,
                event: traced_event,
                trace,
            } => {
                assert_eq!(traced_frame, frame);
                assert_eq!(traced_event, event);
                assert!(trace);
            }
            other => panic!("expected control command, got {other:?}"),
        }
    }

    #[test]
    fn control_trace_product_carries_messages_revisions_and_dispositions() {
        let frame = frame(15);
        let event = control_event();
        let diagnostic = Diagnostic {
            target: Some(WidgetId::from("widget")),
            severity: DiagnosticSeverity::Info,
            code: "handled".to_string(),
            message: "event handled".to_string(),
        };
        let trace = DebugControlTrace::new(
            "trace-1",
            frame.clone(),
            &event,
            true,
            4,
            5,
            vec![diagnostic.clone()],
        )
        .with_messages(vec![
            DebugMessageTraceEntry::emitted("widget", "activate", MessageDisposition::Consumed),
            DebugMessageTraceEntry::emitted("widget", "unused", MessageDisposition::Ignored),
            DebugMessageTraceEntry::emitted(
                "widget",
                "unreduced",
                MessageDisposition::ReductionUnavailable,
            ),
        ])
        .with_reduction_stage(DebugControlTraceStage::reduced(
            "test-reducer",
            Some(WidgetId::from("widget")),
            "test reducer observed trace messages",
        ));

        assert_eq!(trace.request_id, "trace-1");
        assert_eq!(trace.frame, frame);
        assert_eq!(trace.mode, DebugControlMode::SemanticDirect);
        assert_eq!(trace.routed_event_target, WidgetId::from("widget"));
        assert_eq!(trace.event_summary, "command:activate");
        assert!(trace.handled);
        assert_eq!(trace.stages.len(), 4);
        assert_eq!(trace.stages[0].stage, DebugControlTraceStageKind::Generated);
        assert_eq!(trace.stages[0].target, Some(WidgetId::from("widget")));
        assert_eq!(trace.stages[1].stage, DebugControlTraceStageKind::Routed);
        assert_eq!(trace.stages[1].target, Some(WidgetId::from("widget")));
        assert_eq!(trace.stages[2].stage, DebugControlTraceStageKind::Consumed);
        assert_eq!(trace.stages[2].target, Some(WidgetId::from("widget")));
        assert_eq!(trace.stages[3].stage, DebugControlTraceStageKind::Reduced);
        assert_eq!(trace.stages[3].actor, "test-reducer");
        assert_eq!(trace.revision_before, 4);
        assert_eq!(trace.revision_after, 5);
        assert_eq!(trace.diagnostics, vec![diagnostic]);
        assert_eq!(trace.messages.len(), 3);
        assert_eq!(trace.messages[0].disposition, MessageDisposition::Consumed);
        assert_eq!(trace.messages[1].disposition, MessageDisposition::Ignored);
        assert_eq!(
            trace.messages[2].disposition,
            MessageDisposition::ReductionUnavailable
        );

        let product = DebugReplyProduct::ControlTrace(trace);
        assert!(!product.frame_identity_mismatch(&frame));
    }

    #[test]
    fn resize_command_round_trips_request_frame_and_viewport() {
        let (client, runtime) = bounded_debug_bridge(1);
        let mut handler = Handler::new();
        let mut resized_frame = frame(13);
        resized_frame.viewport = Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 320.0,
                height: 180.0,
            },
        };
        let command = DebugCommand::resize("resize-1", resized_frame.clone());

        assert_eq!(command.frame_identity(), &resized_frame);
        assert_eq!(command.frame_identity().viewport.size.width, 320.0);
        assert_eq!(command.frame_identity().viewport.size.height, 180.0);

        let handle = client.submit(command).expect("resize command queued");
        let drained = runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("reply generated");
        let received = handle
            .try_recv()
            .expect("reply channel readable")
            .expect("reply available");

        assert_eq!(drained, received);
        assert_eq!(received.request_id, "resize-1");
        assert_eq!(received.frame, resized_frame);
        assert_eq!(handler.last_resize, Some(resized_frame));
        match received.product {
            DebugReplyProduct::Diagnostics(diagnostics) => {
                assert_eq!(diagnostics[0].code, "resize-received");
            }
            other => panic!("expected resize diagnostic, got {other:?}"),
        }
    }

    #[test]
    fn mismatched_render_evidence_becomes_error_reply() {
        struct BadHandler;

        impl SlipwayDebugCommandHandler for BadHandler {
            fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
                let mut wrong = command.frame_identity().clone();
                wrong.frame_index += 1;
                DebugReplyProduct::Render(RenderProduct::Evidence(RenderEvidence {
                    target: WidgetId::from("widget"),
                    frame: wrong,
                    source: EvidenceSource::canonical_offscreen("bad"),
                    provider_id: "bad".to_string(),
                    artifact_ref: None,
                    artifact_path: None,
                    pixel_hash: None,
                    width: None,
                    height: None,
                    diagnostics: Vec::new(),
                }))
            }
        }

        let (client, runtime) = bounded_debug_bridge(1);
        let mut handler = BadHandler;
        let handle = client
            .submit(DebugCommand::render("bad-1", packet(frame(20))))
            .expect("command queued");

        runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("reply generated");
        let reply = handle
            .try_recv()
            .expect("reply channel readable")
            .expect("reply available");

        assert!(matches!(reply.product, DebugReplyProduct::Error(_)));
        assert_eq!(reply.frame.frame_index, 20);
    }

    #[test]
    fn mismatched_frame_inside_probe_product_becomes_error_reply() {
        struct BadProbeHandler;

        impl SlipwayDebugCommandHandler for BadProbeHandler {
            fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
                let mut wrong = command.frame_identity().clone();
                wrong.frame_index += 1;
                DebugReplyProduct::Probes(vec![ProbeProduct::RenderEvidence(RenderEvidence {
                    target: WidgetId::from("widget"),
                    frame: wrong,
                    source: EvidenceSource::canonical_offscreen("bad-probe"),
                    provider_id: "bad-probe".to_string(),
                    artifact_ref: None,
                    artifact_path: None,
                    pixel_hash: None,
                    width: None,
                    height: None,
                    diagnostics: Vec::new(),
                })])
            }
        }

        let (client, runtime) = bounded_debug_bridge(1);
        let mut handler = BadProbeHandler;
        let handle = client
            .submit(DebugCommand::probe(
                "bad-probe-1",
                frame(30),
                slipway_core::ProbeRequest {
                    target: None,
                    kinds: vec![slipway_core::ProbeKind::RenderEvidence],
                    event_trace_limit: None,
                },
            ))
            .expect("command queued");

        runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("reply generated");
        let reply = handle
            .try_recv()
            .expect("reply channel readable")
            .expect("reply available");

        assert!(matches!(reply.product, DebugReplyProduct::Error(_)));
        assert_eq!(reply.frame.frame_index, 30);
    }

    #[test]
    fn visible_frame_timing_summary_uses_bounded_samples_and_240hz_budget() {
        let mut recorder = VisibleFrameTimingRecorder::disabled("iced");
        recorder.path = Some(PathBuf::from("not-written-in-this-test.csv"));
        recorder.capacity = 3;

        recorder.record("iced.draw_present", Duration::from_micros(1000), 0, None);
        recorder.record("iced.draw_present", Duration::from_micros(2000), 1, None);
        recorder.record("iced.draw_present", Duration::from_micros(5000), 2, None);
        recorder.record("iced.draw_present", Duration::from_micros(6000), 3, None);

        recorder.path = None;
        let summary = recorder.summary();
        assert_eq!(summary.backend, "iced");
        assert_eq!(summary.budget_hz, 240);
        assert_eq!(summary.budget_ns, 4_166_666);
        assert!(!summary.budget_passed);
        assert_eq!(summary.over_budget_samples, 2);
        assert!(summary.root_frame_budget_passed);
        assert_eq!(summary.root_frame_over_budget_samples, 0);
        assert_eq!(summary.total_samples, 3);
        assert_eq!(summary.dropped_samples, 1);
        assert_eq!(summary.kinds.len(), 1);
        assert_eq!(summary.kinds[0].kind, "iced.draw_present");
        assert_eq!(summary.kinds[0].samples, 3);
        assert_eq!(summary.kinds[0].p50_ns, 5_000_000);
        assert_eq!(summary.kinds[0].p95_ns, 6_000_000);
        assert_eq!(summary.kinds[0].p99_ns, 6_000_000);
        assert_eq!(summary.kinds[0].max_ns, 6_000_000);
        assert_eq!(summary.kinds[0].over_budget_samples, 2);

        let path = std::env::temp_dir().join(format!(
            "slipway-visible-frame-timing-test-{}.csv",
            std::process::id()
        ));
        write_visible_frame_timing_file(&path, &summary, &recorder.samples)
            .expect("timing file writes");
        let output = std::fs::read_to_string(&path).expect("timing file readable");
        let _ = std::fs::remove_file(&path);
        assert!(output.contains("budget_hz=240"));
        assert!(output.contains("budget_ns=4166666"));
        assert!(output.contains("budget_status=FAIL"));
        assert!(output.contains("over_budget_total=2"));
        assert!(output.contains("root_frame_budget_status=PASS"));
        assert!(output.contains("root_frame_over_budget_total=0"));
    }

    fn presented_pixels(
        selector: PresentedScreenshotSelector,
        captured_frame: FrameIdentity,
    ) -> PresentedPixels {
        PresentedPixels {
            selector,
            captured_frame,
            source: slipway_core::EvidenceSource::backend_presented(
                "test-backend",
                PRESENTED_PIXELS_PASS_ID,
            ),
            capture_path: PresentedCapturePath::DirectAcquiredSurfaceTextureCopy,
            source_format: PresentedSurfaceFormat::Rgba8UnormSrgb,
            transfer: PresentedTransferFunction::Srgb,
            alpha: PresentedAlphaMode::Opaque,
            width: 1,
            height: 1,
            bytes: Arc::from([1, 2, 3, 255]),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn screenshot_selectors_exhaustively_report_honest_correlation_fields() {
        let exact_frame = frame(40);
        let current_context = frame(3);
        let exact = PresentedScreenshotSelector::Exact {
            expected_frame: exact_frame.clone(),
        };
        let current = PresentedScreenshotSelector::Current {
            request_context: current_context.clone(),
        };

        assert_eq!(exact.admission(), PresentedScreenshotAdmission::Exact);
        assert_eq!(exact.correlation_frame(), &exact_frame);
        assert_eq!(current.admission(), PresentedScreenshotAdmission::Current);
        assert_eq!(current.correlation_frame(), &current_context);

        let exact_command = DebugCommand::screenshot(
            "exact",
            PresentedScreenshotRequest {
                selector: exact.clone(),
            },
        );
        let current_command = DebugCommand::screenshot(
            "current",
            PresentedScreenshotRequest {
                selector: current.clone(),
            },
        );
        assert_eq!(exact_command.frame_identity(), &exact_frame);
        assert_eq!(current_command.frame_identity(), &current_context);
        assert_eq!(
            exact_command.screenshot_admission(),
            Some(PresentedScreenshotAdmission::Exact)
        );
        assert_eq!(
            current_command.screenshot_admission(),
            Some(PresentedScreenshotAdmission::Current)
        );
    }

    #[test]
    fn current_product_keeps_context_as_correlation_not_expected_identity() {
        let request_context = frame(5);
        let mut captured = frame(90);
        captured.surface_id = "presented-surface".to_string();
        captured.surface_instance_id = "presented-instance".to_string();
        captured.revision = 77;
        let product =
            DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Captured(presented_pixels(
                PresentedScreenshotSelector::Current {
                    request_context: request_context.clone(),
                },
                captured,
            )));

        assert_eq!(
            checked_product(
                request_context,
                Some(PresentedScreenshotAdmission::Current),
                product.clone()
            ),
            product
        );
    }

    #[test]
    fn screenshot_validator_rejects_wrong_admission_for_capture_and_refusal() {
        let correlation = frame(50);
        let mut captured = correlation.clone();
        captured.frame_index += 1;
        let selector = PresentedScreenshotSelector::Exact {
            expected_frame: correlation.clone(),
        };
        let products = [
            PresentedScreenshotProduct::Captured(presented_pixels(selector.clone(), captured)),
            PresentedScreenshotProduct::Refusal(PresentedScreenshotRefusal {
                selector,
                captured_frame: None,
                backend_id: Some("test-backend".to_string()),
                code: "test-refusal".to_string(),
                reason: "test refusal".to_string(),
                diagnostics: Vec::new(),
            }),
        ];

        for product in products {
            let failure = validate_presented_screenshot_product(
                &correlation,
                PresentedScreenshotAdmission::Current,
                &product,
            )
            .expect_err("product admission must match the consumed request");
            assert_eq!(failure.code, "screenshot-admission-mismatch");
        }
    }

    #[test]
    fn screenshot_validator_rejects_wrong_correlation_and_exact_captured_identity() {
        let expected = frame(60);
        let selector = PresentedScreenshotSelector::Exact {
            expected_frame: expected.clone(),
        };
        let refusal = PresentedScreenshotProduct::Refusal(PresentedScreenshotRefusal {
            selector: selector.clone(),
            captured_frame: None,
            backend_id: None,
            code: "test-refusal".to_string(),
            reason: "test refusal".to_string(),
            diagnostics: Vec::new(),
        });
        let failure = validate_presented_screenshot_product(
            &frame(61),
            PresentedScreenshotAdmission::Exact,
            &refusal,
        )
        .expect_err("refusal correlation must match the reply frame");
        assert_eq!(failure.code, "screenshot-requested-frame-mismatch");

        let mut wrong_captured = expected.clone();
        wrong_captured.frame_index += 1;
        wrong_captured.surface_instance_id = "wrong-instance".to_string();
        let captured =
            PresentedScreenshotProduct::Captured(presented_pixels(selector, wrong_captured));
        let failure = validate_presented_screenshot_product(
            &expected,
            PresentedScreenshotAdmission::Exact,
            &captured,
        )
        .expect_err("exact capture must preserve the complete admitted identity");
        assert_eq!(failure.code, "screenshot-captured-frame-transition-invalid");
    }

    #[test]
    fn screenshot_validator_keeps_rgba_checks_for_current() {
        let request_context = frame(70);
        let mut pixels = presented_pixels(
            PresentedScreenshotSelector::Current {
                request_context: request_context.clone(),
            },
            frame(200),
        );
        pixels.bytes = Arc::from([0, 0, 0]);

        let failure = validate_presented_screenshot_product(
            &request_context,
            PresentedScreenshotAdmission::Current,
            &PresentedScreenshotProduct::Captured(pixels),
        )
        .expect_err("current captures retain artifact validation");
        assert_eq!(failure.code, "screenshot-artifact-invalid-rgba");
    }

    #[test]
    fn bridge_retains_admission_after_handler_consumes_command() {
        struct WrongAdmissionHandler;

        impl SlipwayDebugCommandHandler for WrongAdmissionHandler {
            fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
                let correlation = command.frame_identity().clone();
                let mut captured = correlation.clone();
                captured.frame_index += 1;
                DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Captured(
                    presented_pixels(
                        PresentedScreenshotSelector::Exact {
                            expected_frame: correlation,
                        },
                        captured,
                    ),
                ))
            }
        }

        let context = frame(80);
        let (client, runtime) = bounded_debug_bridge(1);
        let handle = client
            .submit(DebugCommand::screenshot(
                "wrong-admission",
                PresentedScreenshotRequest {
                    selector: PresentedScreenshotSelector::Current {
                        request_context: context,
                    },
                },
            ))
            .expect("command queued");
        runtime
            .drain_one(&mut WrongAdmissionHandler)
            .expect("runtime drains")
            .expect("reply generated");
        let reply = handle
            .try_recv()
            .expect("reply channel readable")
            .expect("reply available");

        let DebugReplyProduct::Error(failure) = reply.product else {
            panic!("wrong admission must become an error reply");
        };
        assert_eq!(failure.code, "screenshot-admission-mismatch");
    }

    #[test]
    fn screenshot_contract_source_guards_prevent_compatibility_backdoors() {
        let source = include_str!("lib.rs");
        assert!(!source.contains(concat!("pub expected_", "frame: FrameIdentity")));
        assert!(!source.contains(concat!("pub requested_", "frame: FrameIdentity")));
        assert!(!source.contains(concat!("continu", "ation")));
        assert!(!source.contains(concat!("Mut", "ex")));
        assert!(!source.contains(concat!("Rw", "Lock")));
        assert!(!source.contains(concat!("unsafe", " ")));
        assert!(!source.contains(concat!("pixel_", "fallback")));
    }
}
