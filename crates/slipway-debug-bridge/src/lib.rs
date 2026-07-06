use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use slipway_core::{
    DeclaredEventDispatchEvidence, Diagnostic, EVIDENCE_SOURCE_BACKEND_PRESENTED,
    EventResultIdentity, FrameIdentity, InputEvent, KeyEventKind, KeyboardDetails, Modifiers,
    Point, PointerButton, PointerDetails, PointerEventKind, PresentationRegionId, ProbeProduct,
    ProbeRequest, RenderEvidence, RenderPacket, RenderRefusal, TextEditKind, TextSelectionRange,
    WidgetId,
};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugStatus {
    pub admitted: bool,
    pub detail: String,
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
    Diagnostics(Vec<Diagnostic>),
    ControlTrace(DebugControlTrace),
    Error(DebugFailure),
}

impl DebugReplyProduct {
    fn frame_identity_mismatch(&self, expected: &FrameIdentity) -> bool {
        match self {
            Self::Render(RenderProduct::Evidence(evidence)) => &evidence.frame != expected,
            Self::Render(RenderProduct::Refusal(refusal)) => &refusal.frame != expected,
            Self::ControlTrace(trace) => &trace.frame != expected,
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

        Ok(Some(DebugCommandLease {
            request_id: envelope.command.request_id.clone(),
            frame: envelope.command.frame_identity().clone(),
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
        let product = checked_product(
            frame.clone(),
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
            product: checked_product(self.frame, product),
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

fn checked_product(frame: FrameIdentity, product: DebugReplyProduct) -> DebugReplyProduct {
    if product.frame_identity_mismatch(&frame) {
        DebugReplyProduct::Error(DebugFailure {
            code: "frame-identity-mismatch".to_string(),
            message: "handler returned evidence for a different frame identity".to_string(),
            dispatch_evidence: None,
        })
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
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum McpProbeMethod {
    Status,
    Probe,
    Render,
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
}
