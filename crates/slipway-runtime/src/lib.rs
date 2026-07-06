use crossbeam_channel::{Receiver, Sender, bounded};
use serde_json::{Value, json};
use slipway_core::{
    BackendInputEvent, BackendInputTrace, ChangeEvidence, ChangeShapeIdentity,
    DeclaredEventDispatchEvidence, DeclaredEventDispatchKind, Diagnostic, DiagnosticIdentity,
    DiagnosticSeverity, EmittedMessage, EmittedMessageEvidence, EventOutcome, EventResultIdentity,
    FrameIdentity, InputEvent, LayoutConstraints, LayoutInput, Point, ProbeKind, ProbeProduct,
    ProbeRequest, Rect, RenderEvidence, RenderPacket, RenderRefusal, Size, SlipwayApp,
    SlipwayAppLocalState, SlipwayAppWidget, SlipwayAuthoredWidget, SlipwayEventDispositionPolicy,
    SlipwayEventRoutingPolicy, SlipwayOffscreenRenderer, SlipwayViewDefinition, SlipwayWidgetTypes,
    TargetLocalRect, TextEditEvent, TextEditKind, TextSelectionRange, WidgetId, WidgetSlot,
};
#[cfg(test)]
use slipway_core::{EventRoute, EventRoutePhase, FocusRegionDeclaration, WidgetSlotAddress};
use slipway_debug_bridge::{
    DebugBridgeClient, DebugBridgeError, DebugBridgeRuntime, DebugCommand, DebugCommandKind,
    DebugCommandLease, DebugControlMode, DebugControlTrace, DebugControlTraceStage, DebugFailure,
    DebugMessageTraceEntry, DebugPhysicalControl, DebugPhysicalControlDeclarationSelector,
    DebugReply, DebugReplyProduct, DebugStatus, MessageDisposition, RenderProduct,
    SlipwayDebugCommandHandler, bounded_debug_bridge,
};
use slipway_debug_mcp::{
    DebugMcpBridgeMessage, DebugMcpConfig, DebugMcpPendingToolCall, DebugMcpRuntimeClient,
    DebugMcpRuntimeEndpoint, DebugMcpRuntimeRequest, DebugMcpRuntimeResponseHandle,
    DebugMcpRuntimeTransportError, DebugMcpServer, bounded_runtime_mcp,
};
use slipway_debug_renderer::CpuDebugRenderer;
use std::collections::VecDeque;
use std::io::{self, BufRead, BufReader, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::thread::{self, JoinHandle};

const DEFAULT_BRIDGE_CAPACITY: usize = 32;
const DEFAULT_BACKEND_INPUT_TRACE_CAPACITY: usize = 32;
const DEFAULT_EVENT_PROBE_TRACE_LIMIT: usize = 1;
const DEFAULT_UI_TURN_DEBUG_BRIDGE_DRAIN_BUDGET: usize = 8;
const DEFAULT_UI_TURN_RUNTIME_MCP_DRAIN_BUDGET: usize = 8;
const DEFAULT_MCP_PENDING_DEBUG_BRIDGE_DRAIN_BUDGET: usize = 8;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct SlipwayRuntimeDrainBudget {
    pub debug_bridge: usize,
    pub runtime_mcp: usize,
    pub mcp_pending_debug_bridge: usize,
}

impl Default for SlipwayRuntimeDrainBudget {
    fn default() -> Self {
        Self {
            debug_bridge: DEFAULT_UI_TURN_DEBUG_BRIDGE_DRAIN_BUDGET,
            runtime_mcp: DEFAULT_UI_TURN_RUNTIME_MCP_DRAIN_BUDGET,
            mcp_pending_debug_bridge: DEFAULT_MCP_PENDING_DEBUG_BRIDGE_DRAIN_BUDGET,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SlipwayRuntimeDrainReport {
    pub debug_replies_drained: usize,
    pub runtime_mcp_replies_drained: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SlipwayBackendInputApplyReport {
    pub handled: bool,
    pub emitted_messages: usize,
    pub applied_messages: usize,
    pub diagnostics: Vec<Diagnostic>,
}

fn emitted_message_evidence<M>(messages: &[EmittedMessage<M>]) -> Vec<EmittedMessageEvidence> {
    messages
        .iter()
        .map(|message| EmittedMessageEvidence {
            target: message.target.clone(),
            name: message.name.clone(),
        })
        .collect()
}

fn event_result_identity_from_outcome<M>(
    outcome: &EventOutcome<M>,
    emitted_messages: Vec<EmittedMessageEvidence>,
) -> EventResultIdentity {
    EventResultIdentity {
        handled: Some(outcome.handled),
        emitted_messages,
        change_shapes: outcome
            .changes
            .iter()
            .map(ChangeShapeIdentity::from)
            .collect(),
        diagnostics: outcome
            .diagnostics
            .iter()
            .map(DiagnosticIdentity::from)
            .collect(),
    }
}

fn compact_backend_trace_changes(changes: &[ChangeEvidence]) -> Vec<ChangeEvidence> {
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

fn push_backend_input_trace(
    traces: &mut VecDeque<BackendInputTrace>,
    mut trace: BackendInputTrace,
) {
    if trace.input.dispatch_evidence.is_none()
        && !trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_MISSING
        })
    {
        trace.diagnostics.push(
            slipway_core::backend_input_dispatch_evidence_missing_diagnostic(&trace.input.event),
        );
    }
    if traces.len() == DEFAULT_BACKEND_INPUT_TRACE_CAPACITY {
        traces.pop_front();
    }
    traces.push_back(trace);
}

fn backend_input_trace_equivalence_diagnostics(traces: &[&BackendInputTrace]) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();
    for &mcp_trace in traces {
        let Some(mcp_evidence) = mcp_trace.input.dispatch_evidence.as_ref() else {
            continue;
        };
        if mcp_evidence.source.label() != slipway_core::EVIDENCE_SOURCE_DEBUG_MCP {
            continue;
        }

        let mcp_dispatch = mcp_evidence.dispatch_identity();
        let mcp_result = mcp_trace.event_probe().result_identity;
        let Some(mcp_pair_key) = event_equivalence_pair_key(mcp_evidence) else {
            continue;
        };
        for &backend_trace in traces {
            let Some(backend_evidence) = backend_trace.input.dispatch_evidence.as_ref() else {
                continue;
            };
            if backend_evidence.source.label() != slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED {
                continue;
            }

            let backend_dispatch = backend_evidence.dispatch_identity();
            if mcp_dispatch.frame != backend_dispatch.frame {
                continue;
            }
            if event_equivalence_pair_key(backend_evidence).as_ref() != Some(&mcp_pair_key) {
                continue;
            }

            let target = Some(mcp_trace.input.event.target().clone());
            if mcp_dispatch != backend_dispatch {
                diagnostics.push(Diagnostic::warning(
                    target,
                    "event_equivalence.dispatch_identity_mismatch",
                    "MCP physical-equivalent input and backend-presented physical input share a frame but resolved different declared dispatch identity",
                ));
                continue;
            }

            let backend_result = backend_trace.event_probe().result_identity;
            if mcp_result != backend_result {
                diagnostics.push(Diagnostic::warning(
                    target,
                    "event_equivalence.result_identity_mismatch",
                    "MCP physical-equivalent input and backend-presented physical input share a dispatch identity but produced different result identity",
                ));
            } else {
                diagnostics.push(Diagnostic {
                    target,
                    severity: DiagnosticSeverity::Info,
                    code: "event_equivalence.identity_match".to_string(),
                    message: "MCP physical-equivalent input and backend-presented physical input share dispatch and result identity for this frame".to_string(),
                });
            }
        }
    }
    diagnostics
}

#[derive(Clone, Debug, PartialEq)]
struct EventEquivalencePairKey {
    dispatch_kind: DeclaredEventDispatchKind,
    input_position: Option<Point>,
    operation: EventOperationKey,
}

#[derive(Clone, Debug, PartialEq)]
enum EventOperationKey {
    Pointer {
        kind: slipway_core::PointerEventKind,
        button: Option<slipway_core::PointerButton>,
    },
    Wheel {
        delta_x: f32,
        delta_y: f32,
    },
    Scroll {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
        region_id: slipway_core::PresentationRegionId,
    },
    Focus {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
        focused: bool,
    },
    Keyboard {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
        kind: slipway_core::KeyEventKind,
    },
    TextMutation {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
    },
    TextComposition {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
        phase: slipway_core::TextCompositionPhase,
    },
    Selection {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
    },
    Command {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
    },
    Clipboard {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
        kind: slipway_core::ClipboardEventKind,
    },
    DragDrop {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
        phase: slipway_core::DragDropPhase,
    },
    File {
        target: WidgetId,
        target_slot: Option<slipway_core::WidgetSlotAddress>,
    },
}

fn event_equivalence_pair_key(
    evidence: &DeclaredEventDispatchEvidence,
) -> Option<EventEquivalencePairKey> {
    let event = evidence.generated_event.as_ref()?;
    Some(EventEquivalencePairKey {
        dispatch_kind: evidence.kind,
        input_position: evidence.input_position,
        operation: match event {
            InputEvent::Pointer(pointer) => EventOperationKey::Pointer {
                kind: pointer.kind,
                button: pointer.button,
            },
            InputEvent::Wheel(wheel) => EventOperationKey::Wheel {
                delta_x: wheel.delta_x,
                delta_y: wheel.delta_y,
            },
            InputEvent::Scroll(scroll) => EventOperationKey::Scroll {
                target: scroll.target.clone(),
                target_slot: scroll.target_slot.clone(),
                region_id: scroll.region_id.clone(),
            },
            InputEvent::Focus(focus) => EventOperationKey::Focus {
                target: focus.target.clone(),
                target_slot: focus.target_slot.clone(),
                focused: focus.focused,
            },
            InputEvent::Keyboard(keyboard) => EventOperationKey::Keyboard {
                target: keyboard.target.clone(),
                target_slot: keyboard.target_slot.clone(),
                kind: keyboard.kind,
            },
            InputEvent::Text(text) => EventOperationKey::TextMutation {
                target: text.target.clone(),
                target_slot: text.target_slot.clone(),
            },
            InputEvent::TextEdit(text_edit) => EventOperationKey::TextMutation {
                target: text_edit.target.clone(),
                target_slot: text_edit.target_slot.clone(),
            },
            InputEvent::TextComposition(composition) => EventOperationKey::TextComposition {
                target: composition.target.clone(),
                target_slot: composition.target_slot.clone(),
                phase: composition.phase,
            },
            InputEvent::Selection(selection) => EventOperationKey::Selection {
                target: selection.target.clone(),
                target_slot: selection.target_slot.clone(),
            },
            InputEvent::Command(command) => EventOperationKey::Command {
                target: command.target.clone(),
                target_slot: command.target_slot.clone(),
            },
            InputEvent::Clipboard(clipboard) => EventOperationKey::Clipboard {
                target: clipboard.target.clone(),
                target_slot: clipboard.target_slot.clone(),
                kind: clipboard.kind,
            },
            InputEvent::DragDrop(drag_drop) => EventOperationKey::DragDrop {
                target: drag_drop.target.clone(),
                target_slot: drag_drop.target_slot.clone(),
                phase: drag_drop.phase,
            },
            InputEvent::File(file) => EventOperationKey::File {
                target: file.target.clone(),
                target_slot: file.target_slot.clone(),
            },
        },
    })
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SlipwayRuntimeConfig {
    pub surface_id: String,
    pub surface_instance_id: String,
    pub debug_bridge_capacity: usize,
    pub debug_mcp: DebugMcpConfig,
    pub ime_policy: SlipwayImePolicy,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum SlipwayImePolicy {
    #[default]
    BackendRequested,
    AlwaysAllowed,
}

impl SlipwayImePolicy {
    pub fn keeps_platform_ime_allowed(self) -> bool {
        matches!(self, Self::AlwaysAllowed)
    }
}

impl Default for SlipwayRuntimeConfig {
    fn default() -> Self {
        Self {
            surface_id: "slipway-surface".to_string(),
            surface_instance_id: "default-instance".to_string(),
            debug_bridge_capacity: DEFAULT_BRIDGE_CAPACITY,
            debug_mcp: admitted_debug_mcp_config(),
            ime_policy: SlipwayImePolicy::BackendRequested,
        }
    }
}

impl SlipwayRuntimeConfig {
    pub fn admitted_debug() -> Self {
        Self {
            debug_mcp: admitted_debug_mcp_config(),
            ..Self::default()
        }
    }

    pub fn no_debug() -> Self {
        Self {
            debug_mcp: no_debug_mcp_config(),
            ..Self::default()
        }
    }

    pub fn with_debug_mcp(mut self, debug_mcp: DebugMcpConfig) -> Self {
        self.debug_mcp = debug_mcp;
        self
    }

    pub fn with_ime_policy(mut self, ime_policy: SlipwayImePolicy) -> Self {
        self.ime_policy = ime_policy;
        self
    }

    pub fn with_platform_ime_always_allowed(self) -> Self {
        self.with_ime_policy(SlipwayImePolicy::AlwaysAllowed)
    }
}

pub struct SlipwayRuntime<W>
where
    W: SlipwayAuthoredWidget,
{
    external: W::ExternalState,
    slot: WidgetSlot<W>,
    renderer: CpuDebugRenderer,
    bridge_client: DebugBridgeClient,
    bridge_runtime: DebugBridgeRuntime,
    mcp_client: DebugMcpRuntimeClient,
    mcp_endpoint: DebugMcpRuntimeEndpoint,
    config: SlipwayRuntimeConfig,
    revision: u64,
    frame_index: u64,
    last_viewport: Rect,
    debug_render_calls: u64,
    backend_input_traces: VecDeque<BackendInputTrace>,
}

pub type SlipwayAppRuntime<A> = SlipwayRuntime<SlipwayAppWidget<A>>;
pub type SlipwayAssembledRuntimeApp<A> = SlipwayAssembledApp<SlipwayAppWidget<A>>;
pub type SlipwayRuntimeAppLocalState<A> = SlipwayAppLocalState<A>;

#[derive(Debug, Eq, PartialEq)]
pub enum SlipwayRuntimeMcpError {
    Bridge(DebugBridgeError),
    PendingReplyUnavailable,
    PendingReplyBudgetExhausted { budget: usize },
}

impl From<DebugBridgeError> for SlipwayRuntimeMcpError {
    fn from(error: DebugBridgeError) -> Self {
        Self::Bridge(error)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum SlipwayRuntimeMcpPumpError {
    Runtime(SlipwayRuntimeMcpError),
    Transport(DebugMcpRuntimeTransportError),
}

impl From<SlipwayRuntimeMcpError> for SlipwayRuntimeMcpPumpError {
    fn from(error: SlipwayRuntimeMcpError) -> Self {
        Self::Runtime(error)
    }
}

impl From<DebugBridgeError> for SlipwayRuntimeMcpPumpError {
    fn from(error: DebugBridgeError) -> Self {
        Self::Runtime(SlipwayRuntimeMcpError::Bridge(error))
    }
}

impl From<DebugMcpRuntimeTransportError> for SlipwayRuntimeMcpPumpError {
    fn from(error: DebugMcpRuntimeTransportError) -> Self {
        Self::Transport(error)
    }
}

pub struct SlipwayRuntimeMcpTransport {
    local_addr: SocketAddr,
    wake_rx: SlipwayRuntimeMcpWakeReceiver,
    stop_tx: Sender<()>,
    listener_thread: Option<JoinHandle<()>>,
}

pub struct SlipwayRuntimePendingNativeMcpCall {
    request: DebugMcpRuntimeRequest,
    pending: DebugMcpPendingToolCall,
}

#[derive(Clone)]
pub struct SlipwayRuntimeMcpWakeReceiver {
    rx: Receiver<()>,
}

impl SlipwayRuntimeMcpTransport {
    pub fn bind_loopback(client: DebugMcpRuntimeClient, wake_capacity: usize) -> io::Result<Self> {
        Self::bind((Ipv4Addr::LOCALHOST, 0), client, wake_capacity)
    }

    pub fn bind(
        addr: (Ipv4Addr, u16),
        client: DebugMcpRuntimeClient,
        wake_capacity: usize,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(addr)?;
        let local_addr = listener.local_addr()?;
        let (wake_tx, wake_rx) = bounded(wake_capacity.max(1));
        let (stop_tx, stop_rx) = bounded(1);
        let listener_thread = thread::Builder::new()
            .name("slipway-runtime-mcp-listener".to_string())
            .spawn(move || runtime_mcp_listener_loop(listener, client, wake_tx, stop_rx))?;

        Ok(Self {
            local_addr,
            wake_rx: SlipwayRuntimeMcpWakeReceiver { rx: wake_rx },
            stop_tx,
            listener_thread: Some(listener_thread),
        })
    }

    pub fn local_addr(&self) -> SocketAddr {
        self.local_addr
    }

    pub fn wake_receiver(&self) -> SlipwayRuntimeMcpWakeReceiver {
        self.wake_rx.clone()
    }

    pub fn drain_wakes(&self) -> usize {
        self.wake_rx.drain_pending()
    }
}

impl Drop for SlipwayRuntimeMcpTransport {
    fn drop(&mut self) {
        let _ = self.stop_tx.try_send(());
        let _ = TcpStream::connect(self.local_addr);
        if let Some(listener_thread) = self.listener_thread.take() {
            let _ = listener_thread.join();
        }
    }
}

impl SlipwayRuntimeMcpWakeReceiver {
    pub fn recv(&self) -> bool {
        self.rx.recv().is_ok()
    }

    pub fn try_recv(&self) -> bool {
        self.rx.try_recv().is_ok()
    }

    pub fn drain_pending(&self) -> usize {
        let mut drained = 0;
        while self.try_recv() {
            drained += 1;
        }
        drained
    }
}

impl SlipwayRuntimePendingNativeMcpCall {
    pub fn request_id(&self) -> &str {
        self.pending.request_id()
    }

    pub fn try_finish_and_respond(self) -> Result<Option<Value>, SlipwayRuntimeMcpPumpError> {
        let response = self
            .pending
            .try_finish()?
            .ok_or(SlipwayRuntimeMcpError::PendingReplyUnavailable)?;
        self.request.respond(Some(response.clone()))?;
        Ok(Some(response))
    }
}

fn runtime_mcp_listener_loop(
    listener: TcpListener,
    client: DebugMcpRuntimeClient,
    wake_tx: Sender<()>,
    stop_rx: Receiver<()>,
) {
    loop {
        if stop_rx.try_recv().is_ok() {
            break;
        }

        let stream = match listener.accept() {
            Ok((stream, _addr)) => stream,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        };

        if stop_rx.try_recv().is_ok() {
            break;
        }

        let client = client.clone();
        let wake_tx = wake_tx.clone();
        let _ = thread::Builder::new()
            .name("slipway-runtime-mcp-connection".to_string())
            .spawn(move || {
                let _ = runtime_mcp_connection_loop(stream, client, wake_tx);
            });
    }
}

fn runtime_mcp_connection_loop(
    mut stream: TcpStream,
    client: DebugMcpRuntimeClient,
    wake_tx: Sender<()>,
) -> io::Result<()> {
    let reader_stream = stream.try_clone()?;
    let reader = BufReader::new(reader_stream);

    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        if let Some(response) = runtime_mcp_handle_line(line, &client, &wake_tx) {
            serde_json::to_writer(&mut stream, &response)?;
            stream.write_all(b"\n")?;
            stream.flush()?;
        }
    }

    Ok(())
}

fn runtime_mcp_handle_line(
    line: String,
    client: &DebugMcpRuntimeClient,
    wake_tx: &Sender<()>,
) -> Option<Value> {
    let id = runtime_mcp_request_id(&line);
    let response = match client.submit(line) {
        Ok(response) => response,
        Err(error) => {
            return Some(runtime_mcp_json_rpc_error(
                id,
                -32000,
                format!("runtime MCP request submit failed: {error:?}"),
            ));
        }
    };

    if wake_tx.send(()).is_err() {
        return Some(runtime_mcp_json_rpc_error(
            id,
            -32000,
            "runtime MCP wake receiver disconnected",
        ));
    }

    match response.recv() {
        Ok(response) => response,
        Err(error) => Some(runtime_mcp_json_rpc_error(
            id,
            -32000,
            format!("runtime MCP response failed: {error:?}"),
        )),
    }
}

fn runtime_mcp_request_id(request: &str) -> Option<Value> {
    serde_json::from_str::<Value>(request)
        .ok()
        .and_then(|message| message.get("id").cloned())
}

fn runtime_mcp_json_rpc_error(id: Option<Value>, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

#[derive(Clone)]
pub struct SlipwayDebugMcpAttachment {
    server: DebugMcpServer,
    bridge_client: DebugBridgeClient,
    runtime_client: DebugMcpRuntimeClient,
}

impl SlipwayDebugMcpAttachment {
    pub fn new(
        server: DebugMcpServer,
        bridge_client: DebugBridgeClient,
        runtime_client: DebugMcpRuntimeClient,
    ) -> Self {
        Self {
            server,
            bridge_client,
            runtime_client,
        }
    }

    pub fn server(&self) -> &DebugMcpServer {
        &self.server
    }

    pub fn bridge_client(&self) -> &DebugBridgeClient {
        &self.bridge_client
    }

    pub fn runtime_client(&self) -> &DebugMcpRuntimeClient {
        &self.runtime_client
    }

    pub fn begin_bridge_message(&self, request: &str) -> DebugMcpBridgeMessage {
        self.server
            .begin_bridge_message(request, &self.bridge_client)
    }

    pub fn submit_runtime_request(
        &self,
        request: impl Into<String>,
    ) -> Result<DebugMcpRuntimeResponseHandle, DebugMcpRuntimeTransportError> {
        self.runtime_client.submit(request)
    }
}

pub struct SlipwayAssembledApp<W>
where
    W: SlipwayAuthoredWidget + slipway_core::SlipwayViewDefinition,
{
    pub runtime: SlipwayRuntime<W>,
    pub debug_mcp: SlipwayDebugMcpAttachment,
}

impl<W> SlipwayAssembledApp<W>
where
    W: SlipwayAuthoredWidget + slipway_core::SlipwayViewDefinition,
{
    pub fn new(widget: W, external: W::ExternalState) -> Self {
        Self::with_config(widget, external, SlipwayRuntimeConfig::default())
    }

    pub fn with_config(
        widget: W,
        external: W::ExternalState,
        config: SlipwayRuntimeConfig,
    ) -> Self {
        let runtime = SlipwayRuntime::with_config(widget, external, config);
        let debug_mcp = runtime.default_debug_mcp_attachment();
        Self { runtime, debug_mcp }
    }

    pub fn into_parts(self) -> (SlipwayRuntime<W>, SlipwayDebugMcpAttachment) {
        (self.runtime, self.debug_mcp)
    }
}

impl<A> SlipwayAssembledApp<SlipwayAppWidget<A>>
where
    A: SlipwayApp,
    SlipwayAppWidget<A>: SlipwayAuthoredWidget + slipway_core::SlipwayViewDefinition,
{
    pub fn from_app(
        app: A,
        external: <SlipwayAppWidget<A> as SlipwayWidgetTypes>::ExternalState,
    ) -> Self {
        Self::from_app_with_config(app, external, SlipwayRuntimeConfig::default())
    }

    pub fn from_app_with_config(
        app: A,
        external: <SlipwayAppWidget<A> as SlipwayWidgetTypes>::ExternalState,
        config: SlipwayRuntimeConfig,
    ) -> Self {
        let runtime = SlipwayRuntime::from_app_with_config(app, external, config);
        let debug_mcp = runtime.default_debug_mcp_attachment();
        Self { runtime, debug_mcp }
    }
}

impl<W> SlipwayRuntime<W>
where
    W: SlipwayAuthoredWidget,
{
    pub fn new(widget: W, external: W::ExternalState) -> Self {
        Self::with_config(widget, external, SlipwayRuntimeConfig::default())
    }

    pub fn with_config(
        widget: W,
        external: W::ExternalState,
        config: SlipwayRuntimeConfig,
    ) -> Self {
        let capacity = config.debug_bridge_capacity.max(1);
        let (bridge_client, bridge_runtime) = bounded_debug_bridge(capacity);
        let (mcp_client, mcp_endpoint) = bounded_runtime_mcp(capacity);
        Self {
            external,
            slot: WidgetSlot::new(widget),
            renderer: CpuDebugRenderer::default(),
            bridge_client,
            bridge_runtime,
            mcp_client,
            mcp_endpoint,
            config,
            revision: 1,
            frame_index: 0,
            last_viewport: Rect {
                origin: slipway_core::Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 800.0,
                    height: 600.0,
                },
            },
            debug_render_calls: 0,
            backend_input_traces: VecDeque::with_capacity(DEFAULT_BACKEND_INPUT_TRACE_CAPACITY),
        }
    }

    pub fn widget(&self) -> &W {
        &self.slot.widget
    }

    pub fn external(&self) -> &W::ExternalState {
        &self.external
    }

    pub fn external_mut(&mut self) -> &mut W::ExternalState {
        &mut self.external
    }

    pub fn local_state(&self) -> &W::LocalState {
        &self.slot.local_state
    }

    pub fn local_state_mut(&mut self) -> &mut W::LocalState {
        &mut self.slot.local_state
    }

    pub fn bridge_client(&self) -> &DebugBridgeClient {
        &self.bridge_client
    }

    pub fn bridge_client_clone(&self) -> DebugBridgeClient {
        self.bridge_client.clone()
    }

    pub fn runtime_mcp_client_clone(&self) -> DebugMcpRuntimeClient {
        self.mcp_client.clone()
    }

    pub fn start_debug_mcp_transport(&self) -> io::Result<SlipwayRuntimeMcpTransport> {
        SlipwayRuntimeMcpTransport::bind_loopback(
            self.runtime_mcp_client_clone(),
            self.config.debug_bridge_capacity,
        )
    }

    pub fn take_debug_command_lease(&self) -> Result<Option<DebugCommandLease>, DebugBridgeError> {
        self.bridge_runtime.take_one()
    }

    pub fn take_pending_native_mcp_call(
        &mut self,
    ) -> Result<Option<SlipwayRuntimePendingNativeMcpCall>, SlipwayRuntimeMcpPumpError> {
        let Some(request) = self.mcp_endpoint.try_recv()? else {
            return Ok(None);
        };

        let server = self.debug_mcp_server();
        let bridge = self.bridge_client_clone();
        match self.begin_live_runtime_mcp_message(&server, &bridge, request.request()) {
            DebugMcpBridgeMessage::Immediate(response) => {
                request.respond(response)?;
                Ok(None)
            }
            DebugMcpBridgeMessage::Pending(pending) => {
                Ok(Some(SlipwayRuntimePendingNativeMcpCall {
                    request,
                    pending,
                }))
            }
        }
    }

    pub fn complete_debug_command_lease_with_app_reducer<F>(
        &mut self,
        lease: DebugCommandLease,
        apply: &mut F,
    ) -> Result<DebugReply, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let product = self.handle_debug_command_with_app_reducer(lease.command().clone(), apply);
        lease.complete(product)
    }

    pub fn debug_mcp_server(&self) -> DebugMcpServer {
        DebugMcpServer::new(self.config.debug_mcp.clone())
    }

    pub fn default_debug_mcp_attachment(&self) -> SlipwayDebugMcpAttachment {
        SlipwayDebugMcpAttachment::new(
            self.debug_mcp_server(),
            self.bridge_client_clone(),
            self.runtime_mcp_client_clone(),
        )
    }

    pub fn frame_identity(&self, viewport: Rect) -> FrameIdentity {
        FrameIdentity {
            surface_id: self.config.surface_id.clone(),
            surface_instance_id: self.config.surface_instance_id.clone(),
            revision: self.revision,
            frame_index: self.frame_index,
            viewport,
        }
    }

    pub fn last_frame_identity(&self) -> FrameIdentity {
        self.frame_identity(self.last_viewport)
    }

    pub fn record_presented_viewport(&mut self, viewport: Rect) {
        self.last_viewport = viewport;
    }

    pub fn debug_render_calls(&self) -> u64 {
        self.debug_render_calls
    }

    pub fn last_backend_input_event(&self) -> Option<&BackendInputEvent> {
        self.backend_input_traces.back().map(|trace| &trace.input)
    }

    pub fn record_backend_input_event(&mut self, event: BackendInputEvent) {
        self.record_backend_input_trace(BackendInputTrace {
            input: event,
            handled: false,
            revision_before: None,
            revision_after: None,
            emitted_messages: Vec::new(),
            local_state: Vec::new(),
            changes: Vec::new(),
            diagnostics: Vec::new(),
        });
    }

    pub fn last_backend_input_trace(&self) -> Option<&BackendInputTrace> {
        self.backend_input_traces.back()
    }

    pub fn backend_input_traces(&self) -> impl DoubleEndedIterator<Item = &BackendInputTrace> {
        self.backend_input_traces.iter()
    }

    pub fn record_backend_input_trace(&mut self, trace: BackendInputTrace) {
        push_backend_input_trace(&mut self.backend_input_traces, trace);
    }

    pub fn with_widget_state_mut<R>(
        &mut self,
        f: impl FnOnce(&W, &W::ExternalState, &mut W::LocalState) -> R,
    ) -> R {
        f(
            &self.slot.widget,
            &self.external,
            &mut self.slot.local_state,
        )
    }

    pub fn apply_app_messages<F>(&mut self, messages: Vec<W::AppMessage>, apply: &mut F)
    where
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        if !messages.is_empty() {
            apply(&mut self.external, messages);
            self.revision += 1;
        }
    }

    pub fn layout_input(&mut self, viewport: Rect) -> LayoutInput {
        self.last_viewport = viewport;
        LayoutInput {
            viewport: TargetLocalRect::new(viewport),
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: viewport.size,
            },
        }
    }

    pub fn render_packet_for_frame(&self, frame: FrameIdentity) -> RenderPacket {
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
        let layout = self
            .slot
            .widget
            .layout(&self.external, &self.slot.local_state, input);
        RenderPacket {
            target: self.slot.widget.id(),
            frame,
            paint: self
                .slot
                .widget
                .paint(&self.external, &self.slot.local_state, &layout),
            surfaces: Vec::new(),
            diagnostics: layout.diagnostics.clone(),
            layout,
        }
    }

    pub fn apply_input_event(&mut self, event: InputEvent) -> EventOutcome<W::AppMessage> {
        let outcome =
            self.slot
                .widget
                .handle_event(&self.external, &mut self.slot.local_state, event);
        if outcome.handled {
            self.revision += 1;
        }
        outcome
    }

    pub fn apply_backend_input_event(
        &mut self,
        event: BackendInputEvent,
    ) -> EventOutcome<W::AppMessage>
    where
        W: SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy + SlipwayViewDefinition,
    {
        let revision_before = self.revision;
        let input = event.event.clone();
        let declaration = slipway_core::declared_event_handling(
            &self.slot.widget,
            &self.external,
            &self.slot.local_state,
            &input,
        );
        if !declaration.disposition.final_disposition.handled {
            let outcome = slipway_core::refuse_event_declared_unhandled(declaration);
            self.record_backend_input_trace(self.backend_trace_from_outcome(
                event,
                &outcome,
                Some(revision_before),
                Some(self.revision),
                Vec::new(),
            ));
            return outcome;
        }
        let raw_outcome = self.slot.widget.handle_event(
            &self.external,
            &mut self.slot.local_state,
            input.clone(),
        );
        let outcome =
            slipway_core::apply_physical_event_handling_declaration(declaration, raw_outcome);
        if outcome.handled {
            self.revision += 1;
        }
        let revision_after = self.revision;
        self.record_backend_input_trace(self.backend_trace_from_outcome(
            event,
            &outcome,
            Some(revision_before),
            Some(revision_after),
            emitted_message_evidence(&outcome.emitted_messages),
        ));
        outcome
    }

    pub fn apply_backend_input_event_with_app_reducer<F>(
        &mut self,
        event: BackendInputEvent,
        apply: &mut F,
    ) -> SlipwayBackendInputApplyReport
    where
        W: SlipwayEventRoutingPolicy + SlipwayEventDispositionPolicy + SlipwayViewDefinition,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let revision_before = self.revision;
        let input = event.event.clone();
        let declaration = slipway_core::declared_event_handling(
            &self.slot.widget,
            &self.external,
            &self.slot.local_state,
            &input,
        );
        if !declaration.disposition.final_disposition.handled {
            let outcome = slipway_core::refuse_event_declared_unhandled(declaration);
            self.record_backend_input_trace(self.backend_trace_from_outcome(
                event,
                &outcome,
                Some(revision_before),
                Some(self.revision),
                Vec::new(),
            ));
            return SlipwayBackendInputApplyReport {
                handled: false,
                emitted_messages: 0,
                applied_messages: 0,
                diagnostics: outcome.diagnostics,
            };
        }
        let raw_outcome = self.slot.widget.handle_event(
            &self.external,
            &mut self.slot.local_state,
            input.clone(),
        );
        let mut outcome =
            slipway_core::apply_physical_event_handling_declaration(declaration, raw_outcome);
        if outcome.handled {
            self.revision += 1;
        }

        let emitted_message_count = outcome.emitted_messages.len();
        let emitted_messages = emitted_message_evidence(&outcome.emitted_messages);
        let app_messages = std::mem::take(&mut outcome.emitted_messages)
            .into_iter()
            .map(|emitted| emitted.message)
            .collect::<Vec<_>>();
        let applied_messages = app_messages.len();
        if !app_messages.is_empty() {
            apply(&mut self.external, app_messages);
            self.revision += 1;
        }

        let revision_after = self.revision;
        self.record_backend_input_trace(self.backend_trace_from_outcome(
            event,
            &outcome,
            Some(revision_before),
            Some(revision_after),
            emitted_messages,
        ));

        SlipwayBackendInputApplyReport {
            handled: outcome.handled,
            emitted_messages: emitted_message_count,
            applied_messages,
            diagnostics: outcome.diagnostics,
        }
    }

    fn backend_trace_from_outcome(
        &self,
        event: BackendInputEvent,
        outcome: &EventOutcome<W::AppMessage>,
        revision_before: Option<u64>,
        revision_after: Option<u64>,
        emitted_messages: Vec<EmittedMessageEvidence>,
    ) -> BackendInputTrace {
        BackendInputTrace {
            input: event,
            handled: outcome.handled,
            revision_before,
            revision_after,
            emitted_messages,
            local_state: Vec::new(),
            changes: compact_backend_trace_changes(&outcome.changes),
            diagnostics: outcome.diagnostics.clone(),
        }
    }

    pub fn drain_debug_once(&mut self) -> Result<Option<DebugReply>, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
    {
        let bridge_runtime = &self.bridge_runtime;
        let mut owner = RuntimeDebugOwner {
            external: &mut self.external,
            slot: &mut self.slot,
            renderer: &mut self.renderer,
            revision: &mut self.revision,
            frame_index: &mut self.frame_index,
            debug_render_calls: &mut self.debug_render_calls,
            backend_input_traces: &mut self.backend_input_traces,
            message_reducer: None,
        };
        bridge_runtime.drain_one(&mut owner)
    }

    pub fn drain_debug_once_with_app_reducer<F>(
        &mut self,
        apply: &mut F,
    ) -> Result<Option<DebugReply>, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let bridge_runtime = &self.bridge_runtime;
        let mut owner = RuntimeDebugOwner {
            external: &mut self.external,
            slot: &mut self.slot,
            renderer: &mut self.renderer,
            revision: &mut self.revision,
            frame_index: &mut self.frame_index,
            debug_render_calls: &mut self.debug_render_calls,
            backend_input_traces: &mut self.backend_input_traces,
            message_reducer: Some(apply),
        };
        bridge_runtime.drain_one(&mut owner)
    }

    pub fn drain_debug_once_with_app_reducer_and_interceptor<F, I>(
        &mut self,
        apply: &mut F,
        intercept: &mut I,
    ) -> Result<Option<DebugReply>, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
        I: FnMut(&DebugCommand) -> Option<DebugReplyProduct>,
    {
        let bridge_runtime = &self.bridge_runtime;
        let mut owner = RuntimeDebugOwner {
            external: &mut self.external,
            slot: &mut self.slot,
            renderer: &mut self.renderer,
            revision: &mut self.revision,
            frame_index: &mut self.frame_index,
            debug_render_calls: &mut self.debug_render_calls,
            backend_input_traces: &mut self.backend_input_traces,
            message_reducer: Some(apply),
        };
        bridge_runtime.drain_one_with_interceptor(&mut owner, intercept)
    }

    pub fn drain_debug_pending(&mut self) -> Result<Vec<DebugReply>, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
    {
        let mut replies = Vec::new();
        while let Some(reply) = self.drain_debug_once()? {
            replies.push(reply);
        }
        Ok(replies)
    }

    pub fn drain_debug_pending_budgeted(
        &mut self,
        max_replies: usize,
    ) -> Result<Vec<DebugReply>, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
    {
        let mut replies = Vec::new();
        for _ in 0..max_replies {
            let Some(reply) = self.drain_debug_once()? else {
                break;
            };
            replies.push(reply);
        }
        Ok(replies)
    }

    pub fn drain_debug_pending_with_app_reducer<F>(
        &mut self,
        apply: &mut F,
    ) -> Result<Vec<DebugReply>, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let mut replies = Vec::new();
        while let Some(reply) = self.drain_debug_once_with_app_reducer(apply)? {
            replies.push(reply);
        }
        Ok(replies)
    }

    pub fn drain_debug_pending_budgeted_with_app_reducer<F>(
        &mut self,
        max_replies: usize,
        apply: &mut F,
    ) -> Result<Vec<DebugReply>, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let mut replies = Vec::new();
        for _ in 0..max_replies {
            let Some(reply) = self.drain_debug_once_with_app_reducer(apply)? else {
                break;
            };
            replies.push(reply);
        }
        Ok(replies)
    }

    pub fn drain_debug_pending_budgeted_with_app_reducer_and_interceptor<F, I>(
        &mut self,
        max_replies: usize,
        apply: &mut F,
        intercept: &mut I,
    ) -> Result<Vec<DebugReply>, DebugBridgeError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
        I: FnMut(&DebugCommand) -> Option<DebugReplyProduct>,
    {
        let mut replies = Vec::new();
        for _ in 0..max_replies {
            let Some(reply) =
                self.drain_debug_once_with_app_reducer_and_interceptor(apply, intercept)?
            else {
                break;
            };
            replies.push(reply);
        }
        Ok(replies)
    }

    pub fn handle_debug_mcp_request(
        &mut self,
        request: &str,
    ) -> Result<Option<Value>, SlipwayRuntimeMcpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
    {
        let server = self.debug_mcp_server();
        let bridge = self.bridge_client_clone();
        match self.begin_live_runtime_mcp_message(&server, &bridge, request) {
            DebugMcpBridgeMessage::Immediate(response) => Ok(response),
            DebugMcpBridgeMessage::Pending(pending) => self.finish_pending_debug_mcp_call(pending),
        }
    }

    pub fn handle_debug_mcp_request_with_app_reducer<F>(
        &mut self,
        request: &str,
        apply: &mut F,
    ) -> Result<Option<Value>, SlipwayRuntimeMcpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let server = self.debug_mcp_server();
        let bridge = self.bridge_client_clone();
        match self.begin_live_runtime_mcp_message(&server, &bridge, request) {
            DebugMcpBridgeMessage::Immediate(response) => Ok(response),
            DebugMcpBridgeMessage::Pending(pending) => {
                self.finish_pending_debug_mcp_call_with_app_reducer(pending, apply)
            }
        }
    }

    pub fn handle_debug_mcp_request_with_app_reducer_budgeted<F>(
        &mut self,
        request: &str,
        mcp_pending_debug_bridge_budget: usize,
        apply: &mut F,
    ) -> Result<Option<Value>, SlipwayRuntimeMcpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let server = self.debug_mcp_server();
        let bridge = self.bridge_client_clone();
        match self.begin_live_runtime_mcp_message(&server, &bridge, request) {
            DebugMcpBridgeMessage::Immediate(response) => Ok(response),
            DebugMcpBridgeMessage::Pending(pending) => self
                .finish_pending_debug_mcp_call_with_app_reducer_budgeted(
                    pending,
                    mcp_pending_debug_bridge_budget,
                    apply,
                ),
        }
    }

    pub fn handle_debug_mcp_request_with_app_reducer_budgeted_and_interceptor<F, I>(
        &mut self,
        request: &str,
        mcp_pending_debug_bridge_budget: usize,
        apply: &mut F,
        intercept: &mut I,
    ) -> Result<Option<Value>, SlipwayRuntimeMcpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
        I: FnMut(&DebugCommand) -> Option<DebugReplyProduct>,
    {
        let server = self.debug_mcp_server();
        let bridge = self.bridge_client_clone();
        match self.begin_live_runtime_mcp_message(&server, &bridge, request) {
            DebugMcpBridgeMessage::Immediate(response) => Ok(response),
            DebugMcpBridgeMessage::Pending(pending) => self
                .finish_pending_debug_mcp_call_with_app_reducer_budgeted_and_interceptor(
                    pending,
                    mcp_pending_debug_bridge_budget,
                    apply,
                    intercept,
                ),
        }
    }

    pub fn drain_runtime_mcp_once(
        &mut self,
    ) -> Result<Option<Option<Value>>, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
    {
        let Some(request) = self.mcp_endpoint.try_recv()? else {
            return Ok(None);
        };
        let response = self.handle_debug_mcp_request(request.request())?;
        request.respond(response.clone())?;
        Ok(Some(response))
    }

    pub fn drain_runtime_mcp_once_with_app_reducer<F>(
        &mut self,
        apply: &mut F,
    ) -> Result<Option<Option<Value>>, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let Some(request) = self.mcp_endpoint.try_recv()? else {
            return Ok(None);
        };
        let response = self.handle_debug_mcp_request_with_app_reducer(request.request(), apply)?;
        request.respond(response.clone())?;
        Ok(Some(response))
    }

    pub fn drain_runtime_mcp_once_with_app_reducer_budgeted<F>(
        &mut self,
        mcp_pending_debug_bridge_budget: usize,
        apply: &mut F,
    ) -> Result<Option<Option<Value>>, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let Some(request) = self.mcp_endpoint.try_recv()? else {
            return Ok(None);
        };
        let response = match self.handle_debug_mcp_request_with_app_reducer_budgeted(
            request.request(),
            mcp_pending_debug_bridge_budget,
            apply,
        ) {
            Ok(response) => response,
            Err(error) => Some(runtime_mcp_json_rpc_error(
                runtime_mcp_request_id(request.request()),
                -32001,
                format!(
                    "runtime MCP request could not complete in this UI drain budget: {error:?}"
                ),
            )),
        };
        request.respond(response.clone())?;
        Ok(Some(response))
    }

    pub fn drain_runtime_mcp_once_with_app_reducer_budgeted_and_interceptor<F, I>(
        &mut self,
        mcp_pending_debug_bridge_budget: usize,
        apply: &mut F,
        intercept: &mut I,
    ) -> Result<Option<Option<Value>>, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
        I: FnMut(&DebugCommand) -> Option<DebugReplyProduct>,
    {
        let Some(request) = self.mcp_endpoint.try_recv()? else {
            return Ok(None);
        };
        let response = match self
            .handle_debug_mcp_request_with_app_reducer_budgeted_and_interceptor(
                request.request(),
                mcp_pending_debug_bridge_budget,
                apply,
                intercept,
            ) {
            Ok(response) => response,
            Err(error) => Some(runtime_mcp_json_rpc_error(
                runtime_mcp_request_id(request.request()),
                -32001,
                format!(
                    "runtime MCP request could not complete in this UI drain budget: {error:?}"
                ),
            )),
        };
        request.respond(response.clone())?;
        Ok(Some(response))
    }

    pub fn drain_runtime_mcp_pending(&mut self) -> Result<Vec<Value>, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
    {
        let mut responses = Vec::new();
        while let Some(response) = self.drain_runtime_mcp_once()? {
            if let Some(response) = response {
                responses.push(response);
            }
        }
        Ok(responses)
    }

    pub fn drain_runtime_mcp_pending_with_app_reducer<F>(
        &mut self,
        apply: &mut F,
    ) -> Result<Vec<Value>, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let mut responses = Vec::new();
        while let Some(response) = self.drain_runtime_mcp_once_with_app_reducer(apply)? {
            if let Some(response) = response {
                responses.push(response);
            }
        }
        Ok(responses)
    }

    pub fn drain_runtime_mcp_pending_budgeted_with_app_reducer<F>(
        &mut self,
        max_requests: usize,
        mcp_pending_debug_bridge_budget: usize,
        apply: &mut F,
    ) -> Result<Vec<Value>, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let mut responses = Vec::new();
        for _ in 0..max_requests {
            let Some(response) = self.drain_runtime_mcp_once_with_app_reducer_budgeted(
                mcp_pending_debug_bridge_budget,
                apply,
            )?
            else {
                break;
            };
            if let Some(response) = response {
                responses.push(response);
            }
        }
        Ok(responses)
    }

    pub fn drain_runtime_mcp_pending_budgeted_with_app_reducer_and_interceptor<F, I>(
        &mut self,
        max_requests: usize,
        mcp_pending_debug_bridge_budget: usize,
        apply: &mut F,
        intercept: &mut I,
    ) -> Result<Vec<Value>, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
        I: FnMut(&DebugCommand) -> Option<DebugReplyProduct>,
    {
        let mut responses = Vec::new();
        for _ in 0..max_requests {
            let Some(response) = self
                .drain_runtime_mcp_once_with_app_reducer_budgeted_and_interceptor(
                    mcp_pending_debug_bridge_budget,
                    apply,
                    intercept,
                )?
            else {
                break;
            };
            if let Some(response) = response {
                responses.push(response);
            }
        }
        Ok(responses)
    }

    pub fn drain_live_debug_turn_with_app_reducer<F>(
        &mut self,
        budget: SlipwayRuntimeDrainBudget,
        apply: &mut F,
    ) -> Result<SlipwayRuntimeDrainReport, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let debug_replies = self
            .drain_debug_pending_budgeted_with_app_reducer(budget.debug_bridge, apply)?
            .len();
        let runtime_mcp_replies = self
            .drain_runtime_mcp_pending_budgeted_with_app_reducer(
                budget.runtime_mcp,
                budget.mcp_pending_debug_bridge,
                apply,
            )?
            .len();

        Ok(SlipwayRuntimeDrainReport {
            debug_replies_drained: debug_replies,
            runtime_mcp_replies_drained: runtime_mcp_replies,
        })
    }

    pub fn drain_live_debug_turn_with_app_reducer_and_interceptor<F, I>(
        &mut self,
        budget: SlipwayRuntimeDrainBudget,
        apply: &mut F,
        intercept: &mut I,
    ) -> Result<SlipwayRuntimeDrainReport, SlipwayRuntimeMcpPumpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
        I: FnMut(&DebugCommand) -> Option<DebugReplyProduct>,
    {
        let debug_replies = self
            .drain_debug_pending_budgeted_with_app_reducer_and_interceptor(
                budget.debug_bridge,
                apply,
                intercept,
            )?
            .len();
        let runtime_mcp_replies = self
            .drain_runtime_mcp_pending_budgeted_with_app_reducer_and_interceptor(
                budget.runtime_mcp,
                budget.mcp_pending_debug_bridge,
                apply,
                intercept,
            )?
            .len();

        Ok(SlipwayRuntimeDrainReport {
            debug_replies_drained: debug_replies,
            runtime_mcp_replies_drained: runtime_mcp_replies,
        })
    }

    fn finish_pending_debug_mcp_call(
        &mut self,
        pending: DebugMcpPendingToolCall,
    ) -> Result<Option<Value>, SlipwayRuntimeMcpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
    {
        loop {
            if let Some(response) = pending.try_finish()? {
                return Ok(Some(response));
            }
            if self.drain_debug_once()?.is_none() {
                return Err(SlipwayRuntimeMcpError::PendingReplyUnavailable);
            }
        }
    }

    fn finish_pending_debug_mcp_call_with_app_reducer<F>(
        &mut self,
        pending: DebugMcpPendingToolCall,
        apply: &mut F,
    ) -> Result<Option<Value>, SlipwayRuntimeMcpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        loop {
            if let Some(response) = pending.try_finish()? {
                return Ok(Some(response));
            }
            if self.drain_debug_once_with_app_reducer(apply)?.is_none() {
                return Err(SlipwayRuntimeMcpError::PendingReplyUnavailable);
            }
        }
    }

    fn finish_pending_debug_mcp_call_with_app_reducer_budgeted<F>(
        &mut self,
        pending: DebugMcpPendingToolCall,
        max_debug_replies: usize,
        apply: &mut F,
    ) -> Result<Option<Value>, SlipwayRuntimeMcpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let max_debug_replies = max_debug_replies.max(1);
        let mut drained = 0usize;
        loop {
            if let Some(response) = pending.try_finish()? {
                return Ok(Some(response));
            }
            if drained >= max_debug_replies {
                return Err(SlipwayRuntimeMcpError::PendingReplyBudgetExhausted {
                    budget: max_debug_replies,
                });
            }
            if self.drain_debug_once_with_app_reducer(apply)?.is_none() {
                return Err(SlipwayRuntimeMcpError::PendingReplyUnavailable);
            }
            drained += 1;
        }
    }

    fn finish_pending_debug_mcp_call_with_app_reducer_budgeted_and_interceptor<F, I>(
        &mut self,
        pending: DebugMcpPendingToolCall,
        max_debug_replies: usize,
        apply: &mut F,
        intercept: &mut I,
    ) -> Result<Option<Value>, SlipwayRuntimeMcpError>
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
        I: FnMut(&DebugCommand) -> Option<DebugReplyProduct>,
    {
        let max_debug_replies = max_debug_replies.max(1);
        let mut drained = 0usize;
        loop {
            if let Some(response) = pending.try_finish()? {
                return Ok(Some(response));
            }
            if drained >= max_debug_replies {
                return Err(SlipwayRuntimeMcpError::PendingReplyBudgetExhausted {
                    budget: max_debug_replies,
                });
            }
            if self
                .drain_debug_once_with_app_reducer_and_interceptor(apply, intercept)?
                .is_none()
            {
                return Err(SlipwayRuntimeMcpError::PendingReplyUnavailable);
            }
            drained += 1;
        }
    }

    fn begin_live_runtime_mcp_message(
        &self,
        server: &DebugMcpServer,
        bridge: &DebugBridgeClient,
        request: &str,
    ) -> DebugMcpBridgeMessage {
        let Ok(mut message) = serde_json::from_str::<Value>(request) else {
            return server.begin_bridge_message(request, bridge);
        };
        insert_live_frame_references(
            &mut message,
            frame_identity_value(&self.last_frame_identity()),
        );
        server.begin_bridge_value(message, bridge)
    }

    pub fn handle_debug_command_with_app_reducer<F>(
        &mut self,
        command: DebugCommand,
        apply: &mut F,
    ) -> DebugReplyProduct
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let mut owner = RuntimeDebugOwner {
            external: &mut self.external,
            slot: &mut self.slot,
            renderer: &mut self.renderer,
            revision: &mut self.revision,
            frame_index: &mut self.frame_index,
            debug_render_calls: &mut self.debug_render_calls,
            backend_input_traces: &mut self.backend_input_traces,
            message_reducer: Some(apply),
        };
        owner.handle_debug_command(command)
    }

    pub fn handle_backend_presented_physical_control_with_app_reducer<F>(
        &mut self,
        command: DebugCommand,
        backend_input: BackendInputEvent,
        apply: &mut F,
    ) -> DebugReplyProduct
    where
        W: slipway_core::SlipwayViewDefinition
            + SlipwayEventRoutingPolicy
            + SlipwayEventDispositionPolicy,
        W::LocalState: Clone,
        F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>),
    {
        let DebugCommand {
            request_id,
            kind:
                DebugCommandKind::PhysicalControl {
                    frame, operation, ..
                },
        } = command
        else {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-command-required".to_string(),
                message:
                    "backend-presented physical ingress only accepts physical_control commands"
                        .to_string(),
                dispatch_evidence: None,
            });
        };

        let Some(dispatch_evidence) = backend_input.dispatch_evidence.clone() else {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-dispatch-evidence-required".to_string(),
                message: "backend-presented physical ingress requires backend dispatch evidence"
                    .to_string(),
                dispatch_evidence: None,
            });
        };

        if dispatch_evidence.source.label() != slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-source-required".to_string(),
                message:
                    "backend-presented physical ingress only accepts backend_presented evidence"
                        .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }

        if dispatch_evidence.frame != frame {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-frame-mismatch".to_string(),
                message:
                    "backend-presented physical ingress frame must match the MCP command frame"
                        .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }

        if dispatch_evidence.generated_event.as_ref() != Some(&backend_input.event) {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-generated-event-mismatch".to_string(),
                message: "backend-presented physical ingress must use the generated backend event"
                    .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }

        if !physical_control_operation_matches_backend_event(
            &operation,
            &backend_input.event,
            Some(&dispatch_evidence),
        ) {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-operation-mismatch".to_string(),
                message:
                    "backend-presented event kind does not match the requested physical operation"
                        .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }

        self.apply_backend_input_event_with_app_reducer(backend_input, apply);
        let Some(trace) = self.last_backend_input_trace().cloned() else {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-visible-trace-missing".to_string(),
                message:
                    "backend-presented physical ingress produced no visible backend input trace"
                        .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        };

        self.backend_presented_physical_control_product_from_trace(
            DebugCommand::physical_control_with_trace(request_id, frame, operation),
            &trace,
        )
    }

    pub fn backend_presented_physical_control_product_from_trace(
        &self,
        command: DebugCommand,
        trace: &BackendInputTrace,
    ) -> DebugReplyProduct {
        let DebugCommand {
            request_id,
            kind:
                DebugCommandKind::PhysicalControl {
                    frame, operation, ..
                },
        } = command
        else {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-command-required".to_string(),
                message:
                    "backend-presented physical trace completion only accepts physical_control commands"
                        .to_string(),
                dispatch_evidence: None,
            });
        };

        let Some(mut dispatch_evidence) = trace.input.dispatch_evidence.clone() else {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-dispatch-evidence-required".to_string(),
                message:
                    "backend-presented physical trace completion requires backend dispatch evidence"
                        .to_string(),
                dispatch_evidence: None,
            });
        };

        if dispatch_evidence.source.label() != slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-source-required".to_string(),
                message:
                    "backend-presented physical trace completion only accepts backend_presented evidence"
                        .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }

        if dispatch_evidence.frame.viewport != frame.viewport {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-frame-mismatch".to_string(),
                message:
                    "backend-presented physical trace viewport must match the MCP command frame"
                        .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }
        dispatch_evidence.frame = frame.clone();

        if dispatch_evidence.generated_event.as_ref() != Some(&trace.input.event) {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-generated-event-mismatch".to_string(),
                message: "backend-presented physical trace must use the generated backend event"
                    .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }

        if !physical_control_operation_matches_backend_event(
            &operation,
            &trace.input.event,
            Some(&dispatch_evidence),
        ) {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-operation-mismatch".to_string(),
                message:
                    "backend-presented trace event kind does not match the requested physical operation"
                        .to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }

        if !trace.handled {
            return DebugReplyProduct::Error(DebugFailure {
                code: "backend-physical-control-not-handled".to_string(),
                message: "backend-presented physical control reached the visible backend input path but was not handled".to_string(),
                dispatch_evidence: Some(dispatch_evidence),
            });
        }

        let revision_before = trace.revision_before.unwrap_or(self.revision);
        let revision_after = trace.revision_after.unwrap_or(self.revision);
        let messages = trace
            .emitted_messages
            .iter()
            .map(|message| {
                DebugMessageTraceEntry::emitted(
                    message.target.clone(),
                    message.name.clone(),
                    MessageDisposition::Consumed,
                )
            })
            .collect::<Vec<_>>();
        let reduced_message_count = messages.len();
        let result_identity = trace.event_probe().result_identity;
        let target = trace.input.event.target().clone();

        DebugReplyProduct::ControlTrace(
            DebugControlTrace::new(
                request_id,
                frame,
                &trace.input.event,
                trace.handled,
                revision_before,
                revision_after,
                trace.diagnostics.clone(),
            )
            .with_mode(DebugControlMode::PhysicalEquivalent)
            .with_dispatch_evidence(Some(dispatch_evidence))
            .with_result_identity(result_identity)
            .with_messages(messages)
            .with_reduction_stage(DebugControlTraceStage::reduced(
                "slipway-app-reducer",
                Some(target),
                format!(
                    "visible backend reducer already applied {reduced_message_count} emitted message(s)"
                ),
            )),
        )
    }

    pub fn backend_presented_physical_control_input_matches(
        &self,
        command: &DebugCommand,
        input: &BackendInputEvent,
    ) -> bool {
        let DebugCommandKind::PhysicalControl { operation, .. } = &command.kind else {
            return false;
        };

        physical_control_operation_matches_backend_event(
            operation,
            &input.event,
            input.dispatch_evidence.as_ref(),
        )
    }
}

pub struct RuntimeDebugOwner<'a, W>
where
    W: SlipwayAuthoredWidget,
{
    external: &'a mut W::ExternalState,
    slot: &'a mut WidgetSlot<W>,
    renderer: &'a mut CpuDebugRenderer,
    revision: &'a mut u64,
    frame_index: &'a mut u64,
    debug_render_calls: &'a mut u64,
    backend_input_traces: &'a mut VecDeque<BackendInputTrace>,
    message_reducer: Option<&'a mut dyn FnMut(&mut W::ExternalState, Vec<W::AppMessage>)>,
}

fn insert_live_render_frame_if_requested(arguments: &mut Value, live_frame: Value) {
    let Some(arguments) = arguments.as_object_mut() else {
        return;
    };

    if let Some(packet) = arguments.get_mut("packet") {
        insert_live_frame_if_requested(packet, live_frame);
    } else if should_replace_frame(arguments.get("frame")) {
        arguments.insert("frame".to_string(), live_frame);
    }
}

fn insert_live_frame_references(message: &mut Value, live_frame: Value) {
    let Some(params) = message.get_mut("params").and_then(Value::as_object_mut) else {
        return;
    };
    let Some(tool) = params
        .get("name")
        .and_then(Value::as_str)
        .map(str::to_owned)
    else {
        return;
    };
    let Some(arguments) = params.get_mut("arguments") else {
        return;
    };

    match tool.as_str() {
        "slipway.debug.render" | "slipway.debug.screenshot" => {
            insert_live_render_frame_if_requested(arguments, live_frame)
        }
        "slipway.debug.status"
        | "slipway.debug.probe"
        | "slipway.debug.control"
        | "slipway.debug.physical_control"
        | "slipway.debug.resize" => insert_live_frame_if_requested(arguments, live_frame),
        _ => {}
    }
}

fn insert_live_frame_if_requested(value: &mut Value, live_frame: Value) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    if should_replace_frame(object.get("frame")) {
        object.insert("frame".to_string(), live_frame);
    }
}

fn physical_control_operation_matches_backend_event(
    operation: &DebugPhysicalControl,
    event: &InputEvent,
    evidence: Option<&DeclaredEventDispatchEvidence>,
) -> bool {
    let operation_matches_event = match (operation, event) {
        (DebugPhysicalControl::Pointer { kind, button, .. }, InputEvent::Pointer(pointer)) => {
            pointer.kind == *kind && pointer.button == *button
        }
        (
            DebugPhysicalControl::Wheel {
                delta_x, delta_y, ..
            },
            InputEvent::Wheel(wheel),
        ) => wheel.delta_x == *delta_x && wheel.delta_y == *delta_y,
        (
            DebugPhysicalControl::Pointer {
                position,
                kind,
                button,
                ..
            },
            InputEvent::Focus(focus),
        ) => {
            *kind == slipway_core::PointerEventKind::Press
                && button.is_some()
                && focus.focused
                && evidence.and_then(|evidence| evidence.input_position) == Some(*position)
        }
        (DebugPhysicalControl::Focus { focused, .. }, InputEvent::Focus(focus)) => {
            focus.focused == *focused
        }
        (DebugPhysicalControl::Text { text, .. }, InputEvent::Text(input)) => input.text == *text,
        (
            DebugPhysicalControl::TextEdit {
                kind,
                text,
                selection_before,
                selection_after,
                ..
            },
            InputEvent::TextEdit(input),
        ) => physical_control_text_edit_matches_backend_event(
            *kind,
            text,
            selection_before,
            selection_after,
            input,
        ),
        (
            DebugPhysicalControl::Keyboard {
                key,
                kind,
                modifiers,
                ..
            },
            InputEvent::Keyboard(input),
        ) => input.key == *key && input.kind == *kind && input.modifiers == *modifiers,
        (
            DebugPhysicalControl::Command {
                command,
                payload_ref,
                ..
            },
            InputEvent::Command(input),
        ) => input.command == *command && input.payload_ref == *payload_ref,
        (
            DebugPhysicalControl::Scroll {
                offset_x, offset_y, ..
            },
            InputEvent::Scroll(input),
        ) => input.offset_x == *offset_x && input.offset_y == *offset_y,
        _ => false,
    };

    operation_matches_event
        && physical_control_selector_matches_backend_event(operation, event, evidence)
}

fn physical_control_text_edit_matches_backend_event(
    requested_kind: TextEditKind,
    requested_text: &Option<String>,
    requested_selection_before: &Option<TextSelectionRange>,
    requested_selection_after: &Option<TextSelectionRange>,
    input: &TextEditEvent,
) -> bool {
    match requested_kind {
        TextEditKind::DeleteBackward | TextEditKind::DeleteForward => matches!(
            input.kind,
            TextEditKind::ReplaceSelection | TextEditKind::ReplaceBuffer
        ),
        _ => {
            input.kind == requested_kind
                && input.text == *requested_text
                && input.selection_before == *requested_selection_before
                && input.selection_after == *requested_selection_after
        }
    }
}

fn physical_control_selector_matches_backend_event(
    operation: &DebugPhysicalControl,
    event: &InputEvent,
    evidence: Option<&DeclaredEventDispatchEvidence>,
) -> bool {
    let Some(selector) = physical_control_selector(operation) else {
        return true;
    };

    match selector {
        DebugPhysicalControlDeclarationSelector::Target { target } => event.target() == target,
        DebugPhysicalControlDeclarationSelector::Region { region } => {
            evidence.and_then(|evidence| evidence.selected_region.as_ref()) == Some(region)
        }
        DebugPhysicalControlDeclarationSelector::Position { position } => {
            evidence.and_then(|evidence| evidence.input_position) == Some(*position)
        }
    }
}

fn physical_control_selector(
    operation: &DebugPhysicalControl,
) -> Option<&DebugPhysicalControlDeclarationSelector> {
    match operation {
        DebugPhysicalControl::Pointer { .. } | DebugPhysicalControl::Wheel { .. } => None,
        DebugPhysicalControl::Focus { selector, .. }
        | DebugPhysicalControl::Text { selector, .. }
        | DebugPhysicalControl::TextEdit { selector, .. }
        | DebugPhysicalControl::Keyboard { selector, .. }
        | DebugPhysicalControl::Command { selector, .. }
        | DebugPhysicalControl::Scroll { selector, .. } => Some(selector),
    }
}

fn should_replace_frame(frame: Option<&Value>) -> bool {
    match frame {
        None => true,
        Some(Value::String(value)) => matches!(value.as_str(), "last" | "current"),
        _ => false,
    }
}

fn frame_identity_value(frame: &FrameIdentity) -> Value {
    serde_json::json!({
        "surface_id": frame.surface_id,
        "surface_instance_id": frame.surface_instance_id,
        "revision": frame.revision,
        "frame_index": frame.frame_index,
        "viewport": {
            "origin": {
                "x": frame.viewport.origin.x,
                "y": frame.viewport.origin.y,
            },
            "size": {
                "width": frame.viewport.size.width,
                "height": frame.viewport.size.height,
            },
        },
    })
}

impl<W> RuntimeDebugOwner<'_, W>
where
    W: SlipwayAuthoredWidget
        + slipway_core::SlipwayViewDefinition
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy,
{
    fn render_packet(&self, frame: FrameIdentity) -> RenderPacket {
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
        let layout = self
            .slot
            .widget
            .layout(self.external, &self.slot.local_state, input);
        RenderPacket {
            target: self.slot.widget.id(),
            frame,
            paint: self
                .slot
                .widget
                .paint(self.external, &self.slot.local_state, &layout),
            surfaces: Vec::new(),
            diagnostics: layout.diagnostics.clone(),
            layout,
        }
    }

    fn default_probe_products(
        &mut self,
        frame: FrameIdentity,
        request: ProbeRequest,
    ) -> Vec<ProbeProduct> {
        let mut products = Vec::new();
        for kind in request.kinds {
            match kind {
                ProbeKind::Topology => {
                    let root = self.slot.widget.topology(self.external);
                    products.push(ProbeProduct::Topology(slipway_core::TopologyProbe {
                        traversal: root.traverse_depth_first(),
                        root,
                    }));
                }
                ProbeKind::State => {
                    products.push(ProbeProduct::State(slipway_core::StateProbe {
                        target: self.slot.widget.id(),
                        observations: self
                            .slot
                            .widget
                            .observe_state(self.external, &self.slot.local_state),
                    }));
                }
                ProbeKind::Paint => {
                    let packet = self.render_packet(frame.clone());
                    products.push(ProbeProduct::Paint(slipway_core::PaintProbe {
                        target: packet.target,
                        ops: packet.paint,
                    }));
                }
                ProbeKind::RenderPacket => {
                    products.push(ProbeProduct::RenderPacket(
                        self.render_packet(frame.clone()),
                    ));
                }
                ProbeKind::RenderEvidence => {
                    let packet = self.render_packet(frame.clone());
                    if let Ok(evidence) = self.render(packet) {
                        products.push(ProbeProduct::RenderEvidence(evidence));
                    }
                }
                ProbeKind::Event => {
                    let limit = request
                        .event_trace_limit
                        .unwrap_or(DEFAULT_EVENT_PROBE_TRACE_LIMIT);
                    let skip = self.backend_input_traces.len().saturating_sub(limit);
                    let selected_traces = self
                        .backend_input_traces
                        .iter()
                        .skip(skip)
                        .collect::<Vec<_>>();
                    for trace in &selected_traces {
                        products.push(ProbeProduct::Event(trace.event_probe()));
                    }
                    for diagnostic in backend_input_trace_equivalence_diagnostics(&selected_traces)
                    {
                        products.push(ProbeProduct::Diagnostic(diagnostic));
                    }
                }
                ProbeKind::Diagnostics => products.push(ProbeProduct::Diagnostic(runtime_diag(
                    self.slot.widget.id(),
                    "runtime-probe",
                    "runtime probe handled by assembled app state",
                ))),
                _ => {}
            }
        }
        products
    }

    fn render(&mut self, packet: RenderPacket) -> Result<RenderEvidence, RenderRefusal> {
        *self.debug_render_calls += 1;
        self.renderer.render_offscreen(packet)
    }

    fn handle_control(
        &mut self,
        request_id: String,
        frame: FrameIdentity,
        event: InputEvent,
        trace: bool,
    ) -> DebugReplyProduct
    where
        W::LocalState: Clone,
    {
        let event = self.event_with_runtime_layout_bounds(&frame, event);
        if trace {
            self.handle_traced_control(
                request_id,
                frame,
                event,
                DebugControlMode::SemanticDirect,
                None,
            )
        } else {
            self.handle_untraced_control(event)
        }
    }

    fn handle_physical_control(
        &mut self,
        request_id: String,
        frame: FrameIdentity,
        _operation: DebugPhysicalControl,
        _trace: bool,
    ) -> DebugReplyProduct
    where
        W::LocalState: Clone,
    {
        let _ = request_id;
        DebugReplyProduct::Error(slipway_debug_bridge::DebugFailure {
            code: "native-physical-control-required".to_string(),
            message: format!(
                "slipway.debug.physical_control cannot be satisfied by slipway-runtime for frame {}; physical success must come from a visible backend native input path and a backend-presented event trace",
                frame.frame_index
            ),
            dispatch_evidence: None,
        })
    }

    fn event_with_runtime_layout_bounds(
        &self,
        frame: &FrameIdentity,
        mut event: InputEvent,
    ) -> InputEvent {
        if let InputEvent::Pointer(pointer) = &mut event {
            if pointer.target_bounds.is_none() {
                pointer.target_bounds =
                    self.pointer_target_bounds_for_frame(frame, &pointer.target);
            }
        }
        event
    }

    fn pointer_target_bounds_for_frame(
        &self,
        frame: &FrameIdentity,
        target: &WidgetId,
    ) -> Option<TargetLocalRect> {
        let layout = self.slot.widget.layout(
            self.external,
            &self.slot.local_state,
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
        );
        let size = if *target == self.slot.widget.id() {
            layout.bounds.size
        } else {
            layout
                .child_placements
                .iter()
                .find(|placement| placement.child == *target)
                .map(|placement| placement.bounds.size)?
        };
        Some(TargetLocalRect::new(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size,
        }))
    }

    fn handle_untraced_control(&mut self, event: InputEvent) -> DebugReplyProduct {
        let declaration = slipway_core::declared_event_handling(
            &self.slot.widget,
            self.external,
            &self.slot.local_state,
            &event,
        );
        let raw_outcome =
            self.slot
                .widget
                .handle_event(self.external, &mut self.slot.local_state, event.clone());
        let outcome = slipway_core::apply_event_handling_declaration(declaration, raw_outcome);
        if outcome.handled {
            *self.revision += 1;
        }
        DebugReplyProduct::Diagnostics(outcome.diagnostics)
    }

    fn handle_traced_control(
        &mut self,
        request_id: String,
        frame: FrameIdentity,
        event: InputEvent,
        mode: DebugControlMode,
        dispatch_evidence: Option<slipway_core::DeclaredEventDispatchEvidence>,
    ) -> DebugReplyProduct
    where
        W::LocalState: Clone,
    {
        let revision_before = *self.revision;
        let declaration = slipway_core::declared_event_handling(
            &self.slot.widget,
            self.external,
            &self.slot.local_state,
            &event,
        );
        if mode == DebugControlMode::PhysicalEquivalent
            && !declaration.disposition.final_disposition.handled
        {
            let outcome: EventOutcome<W::AppMessage> =
                slipway_core::refuse_event_declared_unhandled(declaration);
            let result_identity = event_result_identity_from_outcome(&outcome, Vec::new());
            let reduction_stage =
                reduction_trace_stage(event.target().clone(), 0, self.message_reducer.is_some(), 0);
            if mode == DebugControlMode::PhysicalEquivalent {
                if let Some(evidence) = dispatch_evidence.clone() {
                    push_backend_input_trace(
                        self.backend_input_traces,
                        BackendInputTrace {
                            input: BackendInputEvent::declared(event.clone(), evidence),
                            handled: false,
                            revision_before: Some(revision_before),
                            revision_after: Some(*self.revision),
                            emitted_messages: Vec::new(),
                            local_state: self
                                .slot
                                .widget
                                .observe_state(self.external, &self.slot.local_state),
                            changes: Vec::new(),
                            diagnostics: outcome.diagnostics.clone(),
                        },
                    );
                }
            }
            return DebugReplyProduct::ControlTrace(
                DebugControlTrace::new(
                    request_id,
                    frame,
                    &event,
                    false,
                    revision_before,
                    *self.revision,
                    outcome.diagnostics,
                )
                .with_mode(mode)
                .with_dispatch_evidence(dispatch_evidence)
                .with_result_identity(result_identity)
                .with_messages(Vec::new())
                .with_reduction_stage(reduction_stage),
            );
        }
        let local_before = self.slot.local_state.clone();
        let raw_outcome =
            self.slot
                .widget
                .handle_event(self.external, &mut self.slot.local_state, event.clone());
        let outcome = if mode == DebugControlMode::PhysicalEquivalent {
            slipway_core::apply_physical_event_handling_declaration(declaration, raw_outcome)
        } else {
            slipway_core::apply_event_handling_declaration(declaration, raw_outcome)
        };
        if mode == DebugControlMode::PhysicalEquivalent
            && slipway_core::event_outcome_has_physical_declaration_mismatch(&outcome)
        {
            self.slot.local_state = local_before;
        }
        if outcome.handled {
            *self.revision += 1;
        }

        let reducer_available = self.message_reducer.is_some();
        let emitted_message_count = outcome.emitted_messages.len();
        let emitted_messages = emitted_message_evidence(&outcome.emitted_messages);
        let result_identity =
            event_result_identity_from_outcome(&outcome, emitted_messages.clone());
        let disposition = if reducer_available {
            MessageDisposition::Consumed
        } else {
            MessageDisposition::ReductionUnavailable
        };
        let messages = message_trace_entries(&outcome.emitted_messages, disposition);

        let mut reduced_message_count = 0usize;
        if let Some(message_reducer) = self.message_reducer.as_deref_mut() {
            let app_messages = outcome
                .emitted_messages
                .into_iter()
                .map(|message| message.message)
                .collect::<Vec<_>>();
            reduced_message_count = app_messages.len();
            if !app_messages.is_empty() {
                message_reducer(self.external, app_messages);
                *self.revision += 1;
            }
        }

        let revision_after = *self.revision;
        let reduction_stage = reduction_trace_stage(
            event.target().clone(),
            emitted_message_count,
            reducer_available,
            reduced_message_count,
        );
        if mode == DebugControlMode::PhysicalEquivalent {
            if let Some(evidence) = dispatch_evidence.clone() {
                push_backend_input_trace(
                    self.backend_input_traces,
                    BackendInputTrace {
                        input: BackendInputEvent::declared(event.clone(), evidence),
                        handled: outcome.handled,
                        revision_before: Some(revision_before),
                        revision_after: Some(revision_after),
                        emitted_messages,
                        local_state: self
                            .slot
                            .widget
                            .observe_state(self.external, &self.slot.local_state),
                        changes: outcome.changes.clone(),
                        diagnostics: outcome.diagnostics.clone(),
                    },
                );
            }
        }
        DebugReplyProduct::ControlTrace(
            DebugControlTrace::new(
                request_id,
                frame,
                &event,
                outcome.handled,
                revision_before,
                revision_after,
                outcome.diagnostics,
            )
            .with_mode(mode)
            .with_dispatch_evidence(dispatch_evidence)
            .with_result_identity(result_identity)
            .with_messages(messages)
            .with_reduction_stage(reduction_stage),
        )
    }
}

#[cfg(test)]
fn resolve_declared_physical_focus_control<F>(
    source: slipway_core::EvidenceSource,
    frame: FrameIdentity,
    focus_regions: &[FocusRegionDeclaration],
    selector: &DebugPhysicalControlDeclarationSelector,
    kind: DeclaredEventDispatchKind,
    text_edit_required: bool,
    build_event: F,
) -> (Option<InputEvent>, DeclaredEventDispatchEvidence)
where
    F: FnOnce(&FocusRegionDeclaration) -> InputEvent,
{
    let input_position = selector_target_local_position(&frame, selector);
    let candidate_regions = focus_regions
        .iter()
        .filter(|region| focus_region_matches_operation(region, text_edit_required))
        .filter(|region| focus_region_matches_selector(region, &input_position, selector))
        .map(|region| region.id.clone())
        .collect::<Vec<_>>();
    let selected_region = focus_regions.iter().find(|region| {
        focus_region_matches_operation(region, text_edit_required)
            && focus_region_matches_selector(region, &input_position, selector)
    });
    let generated_event = selected_region.map(build_event);
    let route = selected_region.map(route_for_focus_region);

    (
        generated_event.clone(),
        DeclaredEventDispatchEvidence {
            source,
            frame,
            kind,
            input_position,
            candidate_regions,
            selected_region: selected_region.map(|region| region.id.clone()),
            refusal_reason: selected_region
                .is_none()
                .then(|| focus_control_refusal_reason(selector, text_edit_required)),
            generated_event,
            route,
            capture_event: selected_region.is_some(),
            diagnostics: Vec::new(),
        },
    )
}

#[cfg(test)]
fn selector_target_local_position(
    frame: &FrameIdentity,
    selector: &DebugPhysicalControlDeclarationSelector,
) -> Option<Point> {
    match selector {
        DebugPhysicalControlDeclarationSelector::Position { position } => Some(Point {
            x: position.x - frame.viewport.origin.x,
            y: position.y - frame.viewport.origin.y,
        }),
        DebugPhysicalControlDeclarationSelector::Target { .. }
        | DebugPhysicalControlDeclarationSelector::Region { .. } => None,
    }
}

#[cfg(test)]
fn focus_region_matches_operation(
    region: &FocusRegionDeclaration,
    text_edit_required: bool,
) -> bool {
    region.enabled && (!text_edit_required || region.text_edit.is_some())
}

#[cfg(test)]
fn focus_region_matches_selector(
    region: &FocusRegionDeclaration,
    input_position: &Option<Point>,
    selector: &DebugPhysicalControlDeclarationSelector,
) -> bool {
    match selector {
        DebugPhysicalControlDeclarationSelector::Target { target } => &region.target == target,
        DebugPhysicalControlDeclarationSelector::Region { region: selected } => {
            &region.id == selected
        }
        DebugPhysicalControlDeclarationSelector::Position { .. } => input_position
            .as_ref()
            .is_some_and(|position| target_local_rect_contains_point(region.bounds, *position)),
    }
}

#[cfg(test)]
fn target_local_rect_contains_point(rect: TargetLocalRect, point: Point) -> bool {
    let raw = rect.into_rect();
    point.x >= raw.origin.x
        && point.y >= raw.origin.y
        && point.x <= raw.origin.x + raw.size.width
        && point.y <= raw.origin.y + raw.size.height
}

#[cfg(test)]
fn route_for_focus_region(region: &FocusRegionDeclaration) -> EventRoute {
    EventRoute {
        route_id: Some(region.id.as_str().to_string()),
        address: region.address.clone(),
        path: route_path_for_address(&region.target, &region.address),
        phase: EventRoutePhase::Target,
    }
}

#[cfg(test)]
fn route_path_for_address(
    target: &WidgetId,
    address: &Option<slipway_core::WidgetSlotAddress>,
) -> Vec<WidgetId> {
    address
        .as_ref()
        .map(|address| address.path.clone())
        .unwrap_or_else(|| vec![target.clone()])
}

#[cfg(test)]
fn focus_control_refusal_reason(
    selector: &DebugPhysicalControlDeclarationSelector,
    text_edit_required: bool,
) -> String {
    let declaration = if text_edit_required {
        "text-edit focus"
    } else {
        "focus"
    };
    format!(
        "no enabled {declaration} declaration matched {}",
        physical_selector_summary(selector)
    )
}

#[cfg(test)]
fn physical_selector_summary(selector: &DebugPhysicalControlDeclarationSelector) -> String {
    match selector {
        DebugPhysicalControlDeclarationSelector::Target { target } => {
            format!("target `{}`", target.as_str())
        }
        DebugPhysicalControlDeclarationSelector::Region { region } => {
            format!("region `{}`", region.as_str())
        }
        DebugPhysicalControlDeclarationSelector::Position { position } => {
            format!("position {},{}", position.x, position.y)
        }
    }
}

fn reduction_trace_stage(
    target: WidgetId,
    emitted_message_count: usize,
    reducer_available: bool,
    reduced_message_count: usize,
) -> DebugControlTraceStage {
    let detail = if reduced_message_count > 0 {
        format!("app reducer reduced {reduced_message_count} emitted message(s)")
    } else if emitted_message_count == 0 {
        "no emitted app messages required reduction".to_string()
    } else if reducer_available {
        format!("app reducer accepted no messages from {emitted_message_count} emission(s)")
    } else {
        format!("app reducer unavailable; {emitted_message_count} emitted message(s) not reduced")
    };

    DebugControlTraceStage::reduced("slipway-app-reducer", Some(target), detail)
}

fn message_trace_entries<M>(
    messages: &[EmittedMessage<M>],
    disposition: MessageDisposition,
) -> Vec<DebugMessageTraceEntry> {
    messages
        .iter()
        .map(|message| {
            DebugMessageTraceEntry::emitted(
                message.target.clone(),
                message.name.clone(),
                disposition,
            )
        })
        .collect()
}

impl<W> SlipwayDebugCommandHandler for RuntimeDebugOwner<'_, W>
where
    W: SlipwayAuthoredWidget
        + slipway_core::SlipwayViewDefinition
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy,
    W::LocalState: Clone,
{
    fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
        let request_id = command.request_id;
        match command.kind {
            DebugCommandKind::Status { .. } => DebugReplyProduct::Status(DebugStatus {
                admitted: true,
                detail: "Slipway runtime assembled app state is active".to_string(),
            }),
            DebugCommandKind::Probe { frame, request } => {
                DebugReplyProduct::Probes(self.default_probe_products(frame, request))
            }
            DebugCommandKind::Render { packet } => {
                let frame = packet.frame;
                let runtime_packet = self.render_packet(frame);
                match self.render(runtime_packet) {
                    Ok(evidence) => DebugReplyProduct::Render(RenderProduct::Evidence(evidence)),
                    Err(refusal) => DebugReplyProduct::Render(RenderProduct::Refusal(refusal)),
                }
            }
            DebugCommandKind::Control {
                frame,
                event,
                trace,
            } => self.handle_control(request_id, frame, event, trace),
            DebugCommandKind::PhysicalControl {
                frame,
                operation,
                trace,
            } => self.handle_physical_control(request_id, frame, operation, trace),
            DebugCommandKind::Resize { frame } => {
                *self.frame_index = frame.frame_index;
                DebugReplyProduct::Diagnostics(vec![runtime_diag(
                    self.slot.widget.id(),
                    "runtime-resize",
                    "runtime accepted resize frame",
                )])
            }
        }
    }
}

impl<W> SlipwayDebugCommandHandler for SlipwayRuntime<W>
where
    W: SlipwayAuthoredWidget
        + slipway_core::SlipwayViewDefinition
        + SlipwayEventRoutingPolicy
        + SlipwayEventDispositionPolicy,
    W::LocalState: Clone,
{
    fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
        let mut owner = RuntimeDebugOwner {
            external: &mut self.external,
            slot: &mut self.slot,
            renderer: &mut self.renderer,
            revision: &mut self.revision,
            frame_index: &mut self.frame_index,
            debug_render_calls: &mut self.debug_render_calls,
            backend_input_traces: &mut self.backend_input_traces,
            message_reducer: None,
        };
        owner.handle_debug_command(command)
    }
}

impl<A> SlipwayRuntime<SlipwayAppWidget<A>>
where
    A: SlipwayApp,
    SlipwayAppWidget<A>: SlipwayAuthoredWidget + slipway_core::SlipwayViewDefinition,
{
    pub fn from_app(
        app: A,
        external: <SlipwayAppWidget<A> as SlipwayWidgetTypes>::ExternalState,
    ) -> Self {
        Self::from_app_with_config(app, external, SlipwayRuntimeConfig::default())
    }

    pub fn from_app_with_config(
        app: A,
        external: <SlipwayAppWidget<A> as SlipwayWidgetTypes>::ExternalState,
        config: SlipwayRuntimeConfig,
    ) -> Self {
        Self::with_config(SlipwayAppWidget::new(app), external, config)
    }
}

pub fn admitted_debug_mcp_config() -> DebugMcpConfig {
    DebugMcpConfig::admitted()
}

pub fn no_debug_mcp_config() -> DebugMcpConfig {
    DebugMcpConfig::no_debug()
}

pub fn runtime_diag(target: WidgetId, code: &str, message: &str) -> Diagnostic {
    Diagnostic {
        target: Some(target),
        severity: DiagnosticSeverity::Info,
        code: code.to_string(),
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slipway_core::{
        Capability, CaretGeometryEvidence, CaretSet, ChildPlacement, Color, CommandEvent,
        CursorCapability, DeclaredEventDispatchKind, Diagnostic, EventOutcome, EventRoute,
        EventRoutePhase, FocusRegionDeclaration, HitRegionOrder, ImeCompositionPolicyDeclaration,
        InputEvent, KeyboardEvent, LayoutOutput, PaintOp, PaintOrderDeclaration, Point,
        PointerCaptureIntent, PointerEventKind, PresentationRegionId, Rect, ScrollAxes,
        ScrollConsumptionEvidence, ScrollConsumptionPolicy, ScrollDeltaConsumption,
        ScrollInputKind, ShapeDeclaration, ShapeKind, Size, SlipwayLogic, SlipwaySsot, SlipwayView,
        SlipwayViewDefinition, SlipwayWidgetTypes, StateObservation, TextBufferSnapshot,
        TextEditCommandDeclaration, TextEditEvent, TextEditKind, TextInputEvent, TextLineMode,
        TextSelectionPolicyDeclaration, TextStyle, TopologyNode, ViewDefinition,
        ViewDefinitionInput, WheelRouting, WidgetId,
    };
    use std::io::{BufRead, BufReader, Write};
    use std::net::TcpStream;
    use std::path::Path;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    fn test_rgb(red: u8, green: u8, blue: u8) -> slipway_core::Color {
        slipway_core::Color {
            red: f32::from(red) / 255.0,
            green: f32::from(green) / 255.0,
            blue: f32::from(blue) / 255.0,
            alpha: 1.0,
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct ProbeWidget;

    #[derive(Clone, Debug, PartialEq)]
    struct PhysicalProbeWidget;

    #[derive(Clone, Debug, PartialEq)]
    struct CloneCountingPhysicalProbeWidget;

    #[derive(Clone, Debug, PartialEq)]
    struct TextPhysicalProbeWidget;

    #[derive(Clone, Debug, PartialEq)]
    struct RuntimeInteractionDeclarationWidget {
        id: WidgetId,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct PhysicalMismatchWidget {
        id: WidgetId,
        declared_handled: bool,
        handler_handles: bool,
        declare_handled_only_before_first_press: bool,
        route_path_override: Option<Vec<WidgetId>>,
        empty_hit_route: bool,
        hit_bounds_outside_layout: bool,
        mutate_before_ignore: bool,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Local {
        count: u32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct PhysicalLocal {
        presses: u32,
    }

    #[derive(Debug, PartialEq)]
    struct CloneCountingPhysicalLocal {
        presses: u32,
    }

    static CLONE_COUNTING_PHYSICAL_LOCAL_CLONES: AtomicUsize = AtomicUsize::new(0);

    impl Clone for CloneCountingPhysicalLocal {
        fn clone(&self) -> Self {
            CLONE_COUNTING_PHYSICAL_LOCAL_CLONES.fetch_add(1, Ordering::SeqCst);
            Self {
                presses: self.presses,
            }
        }
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TextPhysicalLocal {
        text: String,
        focused: bool,
        key: Option<String>,
        command: Option<String>,
        scroll_y: f32,
    }

    #[derive(Clone, Debug, PartialEq)]
    enum Message {
        Counted,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TestApp {
        widgets: (AlphaChild, BetaChild),
    }

    #[derive(Clone, Debug, PartialEq)]
    struct TestExternal;

    #[derive(Clone, Debug, PartialEq)]
    struct TestAppLocal {
        initialized: bool,
    }

    #[derive(Clone, Debug, PartialEq)]
    enum TestMessage {
        ChildChanged,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct AlphaChild {
        initial_count: u32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BetaChild {
        initial_count: u32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct AlphaLocal {
        count: u32,
    }

    #[derive(Clone, Debug, PartialEq)]
    struct BetaLocal {
        count: u32,
    }

    macro_rules! impl_runtime_test_event_policy {
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

    impl_runtime_test_event_policy!(
        ProbeWidget,
        PhysicalProbeWidget,
        TextPhysicalProbeWidget,
        AlphaChild,
        BetaChild
    );

    impl SlipwayWidgetTypes for RuntimeInteractionDeclarationWidget {
        type ExternalState = ();
        type LocalState = TextPhysicalLocal;
        type AppMessage = Message;
    }

    impl SlipwaySsot for RuntimeInteractionDeclarationWidget {
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

    impl SlipwayLogic for RuntimeInteractionDeclarationWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            _local: &mut Self::LocalState,
            _event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            EventOutcome::ignored()
        }
    }

    impl SlipwayView for RuntimeInteractionDeclarationWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            TextPhysicalLocal {
                text: String::new(),
                focused: false,
                key: None,
                command: None,
                scroll_y: 0.0,
            }
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
            _local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayEventRoutingPolicy for RuntimeInteractionDeclarationWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            slipway_core::EventRoutingPolicyDeclaration {
                target: self.id.clone(),
                event_target: event.target().clone(),
                route: EventRoute {
                    route_id: None,
                    address: event.target_slot().cloned(),
                    path: vec![self.id.clone()],
                    phase: EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayEventDispositionPolicy for RuntimeInteractionDeclarationWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
            _route: &EventRoute,
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

    impl slipway_core::SlipwayTextBufferPolicy for RuntimeInteractionDeclarationWidget {
        fn text_buffer(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> TextBufferSnapshot {
            TextBufferSnapshot {
                target: self.id.clone(),
                text: local.text.clone(),
                revision: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayTextSelectionPolicy for RuntimeInteractionDeclarationWidget {
        fn text_selection(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
        ) -> TextSelectionPolicyDeclaration {
            let text_len = local.text.chars().count();
            TextSelectionPolicyDeclaration {
                target: self.id.clone(),
                selection: None,
                carets: CaretSet {
                    carets: vec![text_len],
                    primary: Some(text_len),
                },
                editable: true,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayImeCompositionPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayCaretGeometryPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayTextEditCommandPolicy for RuntimeInteractionDeclarationWidget {
        fn text_edit_commands(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<TextEditCommandDeclaration> {
            vec![
                TextEditCommandDeclaration {
                    command_id: "insert".to_string(),
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

    impl slipway_core::SlipwayTextInputVisualStylePolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayTextInputTypographyPolicy for RuntimeInteractionDeclarationWidget {
        fn text_input_typography(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> slipway_core::TextInputTypographyDeclaration {
            slipway_core::TextInputTypographyDeclaration::explicit(
                self.id.clone(),
                slipway_core::TextStyle::default().with_font_family("system-ui"),
            )
        }
    }

    impl slipway_core::SlipwayTextUndoRedoPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayTextFlowPolicy for RuntimeInteractionDeclarationWidget {
        fn text_flow_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> slipway_core::TextFlowPolicy {
            slipway_core::TextFlowPolicy {
                target: self.id.clone(),
                line_mode: TextLineMode::SingleLine,
                wrap: slipway_core::TextWrapMode::NoWrap,
                line_clamp: None,
                allow_ellipsis: false,
                baseline: None,
                caret_bounds: Vec::new(),
                viewport: None,
            }
        }
    }

    impl slipway_core::SlipwayTextMeasurementPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayTextMeasurementCachePolicy for RuntimeInteractionDeclarationWidget {
        fn text_measurement_cache_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> Vec<slipway_core::TextMeasurementCachePolicyDeclaration> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayCachedTextMeasurementPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayFocusTraversal for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwaySemantics for RuntimeInteractionDeclarationWidget {
        fn semantics(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
        ) -> Vec<slipway_core::SemanticNode> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayDebugEventTracePolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayContainerLayoutPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayChildConstraintPolicy for RuntimeInteractionDeclarationWidget {
        fn child_constraints(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: &LayoutInput,
        ) -> Vec<slipway_core::ChildConstraintPolicyDeclaration> {
            Vec::new()
        }
    }

    impl slipway_core::SlipwayLayoutInvalidationPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayLayoutEvidencePolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayScrollBehaviorPolicy for RuntimeInteractionDeclarationWidget {
        fn scroll_behavior_policy(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
            input: &LayoutInput,
        ) -> slipway_core::ScrollBehaviorPolicyDeclaration {
            slipway_core::ScrollBehaviorPolicyDeclaration {
                target: self.id.clone(),
                region_id: None,
                address: None,
                axes: ScrollAxes {
                    horizontal: false,
                    vertical: true,
                },
                extent: Size {
                    width: input.viewport.size.width,
                    height: input.viewport.size.height * 2.0,
                },
                viewport: input.viewport,
                content_bounds: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: input.viewport.size.width,
                        height: input.viewport.size.height * 2.0,
                    },
                }),
                offset: Point {
                    x: 0.0,
                    y: local.scroll_y,
                },
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

    impl slipway_core::SlipwayWheelRoutingPolicy for RuntimeInteractionDeclarationWidget {
        fn wheel_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _wheel: &slipway_core::WheelEvent,
        ) -> slipway_core::WheelRoutingPolicyDeclaration {
            slipway_core::WheelRoutingPolicyDeclaration {
                target: self.id.clone(),
                routing: WheelRouting::NearestScrollable,
                modifiers: None,
                diagnostics: Vec::new(),
            }
        }
    }

    impl slipway_core::SlipwayViewportObservationPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayVirtualCollectionPolicy for RuntimeInteractionDeclarationWidget {
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

    impl slipway_core::SlipwayHitTesting for RuntimeInteractionDeclarationWidget {
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

    impl PhysicalMismatchWidget {
        fn new(id: &str, declared_handled: bool, handler_handles: bool) -> Self {
            Self {
                id: WidgetId::from(id),
                declared_handled,
                handler_handles,
                declare_handled_only_before_first_press: false,
                route_path_override: None,
                empty_hit_route: false,
                hit_bounds_outside_layout: false,
                mutate_before_ignore: false,
            }
        }

        fn state_dependent(id: &str) -> Self {
            Self {
                id: WidgetId::from(id),
                declared_handled: true,
                handler_handles: true,
                declare_handled_only_before_first_press: true,
                route_path_override: None,
                empty_hit_route: false,
                hit_bounds_outside_layout: false,
                mutate_before_ignore: false,
            }
        }

        fn mutating_ignored(id: &str) -> Self {
            Self {
                id: WidgetId::from(id),
                declared_handled: true,
                handler_handles: false,
                declare_handled_only_before_first_press: false,
                route_path_override: None,
                empty_hit_route: false,
                hit_bounds_outside_layout: false,
                mutate_before_ignore: true,
            }
        }

        fn route_mismatch(id: &str) -> Self {
            Self {
                id: WidgetId::from(id),
                declared_handled: true,
                handler_handles: true,
                declare_handled_only_before_first_press: false,
                route_path_override: Some(vec![WidgetId::from("wrong-route-target")]),
                empty_hit_route: false,
                hit_bounds_outside_layout: false,
                mutate_before_ignore: false,
            }
        }

        fn invalid_hit_route(id: &str) -> Self {
            Self {
                id: WidgetId::from(id),
                declared_handled: true,
                handler_handles: true,
                declare_handled_only_before_first_press: false,
                route_path_override: None,
                empty_hit_route: true,
                hit_bounds_outside_layout: false,
                mutate_before_ignore: false,
            }
        }

        fn hit_bounds_outside_layout(id: &str) -> Self {
            Self {
                id: WidgetId::from(id),
                declared_handled: true,
                handler_handles: true,
                declare_handled_only_before_first_press: false,
                route_path_override: None,
                empty_hit_route: false,
                hit_bounds_outside_layout: true,
                mutate_before_ignore: false,
            }
        }
    }

    impl SlipwayWidgetTypes for ProbeWidget {
        type ExternalState = ();
        type LocalState = Local;
        type AppMessage = Message;
    }

    impl SlipwaySsot for ProbeWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("probe")
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

    impl SlipwayLogic for ProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Command(command) if command.command == "count" => {
                    local.count += 1;
                    EventOutcome::message(self.id(), "count", Message::Counted)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for ProbeWidget {
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
            local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Fill {
                shape: ShapeDeclaration {
                    id: Some(format!("count-{}", local.count)),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: local.count as f32 / 10.0,
                    green: 0.2,
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
                name: "count".to_string(),
                value: local.count.to_string(),
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
            let layout = self.layout(external, local, input.layout_input);
            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayWidgetTypes for PhysicalProbeWidget {
        type ExternalState = ();
        type LocalState = PhysicalLocal;
        type AppMessage = Message;
    }

    impl SlipwaySsot for PhysicalProbeWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("physical-probe")
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

    impl SlipwayLogic for PhysicalProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Pointer(pointer)
                    if pointer.target == self.id()
                        && matches!(pointer.kind, PointerEventKind::Press) =>
                {
                    local.presses += 1;
                    EventOutcome::message(self.id(), "press", Message::Counted)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for PhysicalProbeWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            PhysicalLocal { presses: 0 }
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
                    id: Some("physical-probe-body".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 0.1,
                    green: 0.1,
                    blue: 0.1,
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
                name: "presses".to_string(),
                value: local.presses.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for PhysicalProbeWidget {
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
                paint: self.paint(external, local, &layout),
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                hit_regions: vec![slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    PresentationRegionId::from("probe-hit"),
                    None,
                    layout.bounds,
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    Some("press".to_string()),
                    CursorCapability::Pointer,
                    true,
                    PointerCaptureIntent::OnPress,
                )],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayWidgetTypes for CloneCountingPhysicalProbeWidget {
        type ExternalState = ();
        type LocalState = CloneCountingPhysicalLocal;
        type AppMessage = Message;
    }

    impl SlipwaySsot for CloneCountingPhysicalProbeWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("clone-counting-physical-probe")
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

    impl SlipwayLogic for CloneCountingPhysicalProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Pointer(pointer)
                    if pointer.target == self.id()
                        && matches!(pointer.kind, PointerEventKind::Press) =>
                {
                    local.presses += 1;
                    EventOutcome::message(self.id(), "press", Message::Counted)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for CloneCountingPhysicalProbeWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            CloneCountingPhysicalLocal { presses: 0 }
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
                    id: Some("clone-counting-physical-probe-body".to_string()),
                    kind: ShapeKind::Rectangle,
                    bounds: layout.bounds.into_rect(),
                    path: None,
                    clip: None,
                },
                color: Color {
                    red: 0.1,
                    green: 0.1,
                    blue: 0.1,
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
                name: "presses".to_string(),
                value: local.presses.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for CloneCountingPhysicalProbeWidget {
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
                paint: self.paint(external, local, &layout),
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                hit_regions: vec![slipway_core::hit_region_from_pointer_capability(
                    self,
                    external,
                    local,
                    PresentationRegionId::from("clone-counting-probe-hit"),
                    None,
                    layout.bounds,
                    slipway_core::PointerEventCoordinateSpace::TargetLocal,
                    HitRegionOrder {
                        z_index: 0,
                        paint_order: 0,
                        traversal_order: 0,
                    },
                    Some("press".to_string()),
                    CursorCapability::Pointer,
                    true,
                    PointerCaptureIntent::OnPress,
                )],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayEventRoutingPolicy for CloneCountingPhysicalProbeWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            slipway_core::EventRoutingPolicyDeclaration {
                target: self.id(),
                event_target: event.target().clone(),
                route: EventRoute {
                    route_id: Some("clone-counting-press".to_string()),
                    address: event.target_slot().cloned(),
                    path: vec![self.id()],
                    phase: EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayEventDispositionPolicy for CloneCountingPhysicalProbeWidget {
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
                propagate: !handled,
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

    impl SlipwayWidgetTypes for PhysicalMismatchWidget {
        type ExternalState = ();
        type LocalState = PhysicalLocal;
        type AppMessage = Message;
    }

    impl SlipwaySsot for PhysicalMismatchWidget {
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

    impl SlipwayLogic for PhysicalMismatchWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            if self.handler_handles
                && matches!(event, InputEvent::Pointer(_))
                && event.target() == &self.id
            {
                local.presses += 1;
                EventOutcome::message(self.id(), "mismatch-press", Message::Counted)
            } else if self.mutate_before_ignore
                && matches!(event, InputEvent::Pointer(_))
                && event.target() == &self.id
            {
                local.presses += 1;
                EventOutcome::ignored()
            } else {
                EventOutcome::ignored()
            }
        }
    }

    impl SlipwayView for PhysicalMismatchWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            PhysicalLocal { presses: 0 }
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
                    id: Some("physical-mismatch-body".to_string()),
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
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![StateObservation {
                target: self.id(),
                slot: None,
                name: "presses".to_string(),
                value: local.presses.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for PhysicalMismatchWidget {
        fn view_definition(
            &self,
            external: &Self::ExternalState,
            local: &Self::LocalState,
            input: ViewDefinitionInput,
        ) -> ViewDefinition {
            let layout = self.layout(external, local, input.layout_input);
            let mut hit_region = slipway_core::hit_region_from_pointer_capability(
                self,
                external,
                local,
                PresentationRegionId::from("mismatch-hit"),
                None,
                layout.bounds,
                slipway_core::PointerEventCoordinateSpace::TargetLocal,
                HitRegionOrder {
                    z_index: 0,
                    paint_order: 0,
                    traversal_order: 0,
                },
                Some("mismatch-route".to_string()),
                CursorCapability::Pointer,
                true,
                PointerCaptureIntent::OnPress,
            );
            if self.hit_bounds_outside_layout {
                hit_region.bounds.size.width += 10.0;
            }

            ViewDefinition {
                target: self.id(),
                frame: input.frame,
                paint: self.paint(external, local, &layout),
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                hit_regions: vec![hit_region],
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayEventRoutingPolicy for PhysicalMismatchWidget {
        fn event_routing_policy(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            event: &InputEvent,
        ) -> slipway_core::EventRoutingPolicyDeclaration {
            let address = event.target_slot().cloned();
            let path = self.route_path_override.clone().unwrap_or_else(|| {
                if self.empty_hit_route {
                    Vec::new()
                } else {
                    address
                        .as_ref()
                        .map(|address| address.path.clone())
                        .unwrap_or_else(|| vec![self.id()])
                }
            });
            slipway_core::EventRoutingPolicyDeclaration {
                target: self.id(),
                event_target: event.target().clone(),
                route: EventRoute {
                    route_id: Some("mismatch-route".to_string()),
                    address,
                    path,
                    phase: EventRoutePhase::Target,
                },
                capture: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayEventDispositionPolicy for PhysicalMismatchWidget {
        fn event_disposition(
            &self,
            _external: &Self::ExternalState,
            local: &Self::LocalState,
            event: &InputEvent,
            route: &EventRoute,
        ) -> slipway_core::EventPropagationEvidence {
            let handled = if self.declare_handled_only_before_first_press {
                local.presses == 0
            } else {
                self.declared_handled
            };
            let disposition = slipway_core::EventDisposition {
                handled,
                propagate: !handled,
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

    impl SlipwayWidgetTypes for TextPhysicalProbeWidget {
        type ExternalState = ();
        type LocalState = TextPhysicalLocal;
        type AppMessage = Message;
    }

    impl SlipwaySsot for TextPhysicalProbeWidget {
        fn id(&self) -> WidgetId {
            WidgetId::from("text-probe")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![Capability::TextInput, Capability::Paint]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for TextPhysicalProbeWidget {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Text(text) if text.target == self.id() => {
                    local.text.push_str(&text.text);
                    EventOutcome::message(self.id(), "text", Message::Counted)
                }
                InputEvent::TextEdit(text_edit) if text_edit.target == self.id() => {
                    if let Some(text) = text_edit.text {
                        local.text = text;
                    }
                    EventOutcome::message(self.id(), "text_edit", Message::Counted)
                }
                InputEvent::Focus(focus) if focus.target == self.id() => {
                    local.focused = focus.focused;
                    EventOutcome::message(self.id(), "focus", Message::Counted)
                }
                InputEvent::Keyboard(keyboard) if keyboard.target == self.id() => {
                    local.key = Some(keyboard.key);
                    EventOutcome::message(self.id(), "keyboard", Message::Counted)
                }
                InputEvent::Command(command) if command.target == self.id() => {
                    local.command = Some(command.command);
                    EventOutcome::message(self.id(), "command", Message::Counted)
                }
                InputEvent::Scroll(scroll) if scroll.target == self.id() => {
                    local.scroll_y = scroll.offset_y;
                    EventOutcome::message(self.id(), "scroll", Message::Counted)
                }
                InputEvent::Wheel(wheel) if wheel.target == self.id() => {
                    local.scroll_y += wheel.delta_y;
                    EventOutcome::message(self.id(), "wheel", Message::Counted)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for TextPhysicalProbeWidget {
        fn initial_local_state(&self) -> Self::LocalState {
            TextPhysicalLocal {
                text: String::new(),
                focused: false,
                key: None,
                command: None,
                scroll_y: 0.0,
            }
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
                    id: Some("text-probe-body".to_string()),
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
            local: &Self::LocalState,
        ) -> Vec<StateObservation> {
            vec![
                StateObservation {
                    target: self.id(),
                    slot: None,
                    name: "text".to_string(),
                    value: local.text.clone(),
                },
                StateObservation {
                    target: self.id(),
                    slot: None,
                    name: "focused".to_string(),
                    value: local.focused.to_string(),
                },
                StateObservation {
                    target: self.id(),
                    slot: None,
                    name: "key".to_string(),
                    value: local.key.clone().unwrap_or_default(),
                },
                StateObservation {
                    target: self.id(),
                    slot: None,
                    name: "command".to_string(),
                    value: local.command.clone().unwrap_or_default(),
                },
                StateObservation {
                    target: self.id(),
                    slot: None,
                    name: "scroll_y".to_string(),
                    value: local.scroll_y.to_string(),
                },
            ]
        }
    }

    fn text_physical_local(text: String, scroll_y: f32) -> TextPhysicalLocal {
        TextPhysicalLocal {
            text,
            focused: false,
            key: None,
            command: None,
            scroll_y,
        }
    }

    fn layout_input_for_bounds(bounds: TargetLocalRect) -> LayoutInput {
        LayoutInput {
            viewport: bounds,
            constraints: LayoutConstraints {
                min: Size {
                    width: 0.0,
                    height: 0.0,
                },
                max: bounds.size,
            },
        }
    }

    fn text_physical_focus_region(
        target: WidgetId,
        region_id: &str,
        bounds: TargetLocalRect,
        text: String,
    ) -> FocusRegionDeclaration {
        let widget = RuntimeInteractionDeclarationWidget { id: target };
        let local = text_physical_local(text, 0.0);
        let input = layout_input_for_bounds(bounds);
        slipway_core::text_edit_focus_region_from_capability(
            &widget,
            &(),
            &local,
            PresentationRegionId::from(region_id),
            None,
            bounds,
            None,
            true,
            &input,
            None,
        )
    }

    fn text_physical_scroll_region(
        target: WidgetId,
        region_id: &str,
        bounds: TargetLocalRect,
        scroll_y: f32,
        evidence: Vec<ScrollConsumptionEvidence>,
    ) -> slipway_core::ScrollRegionDeclaration {
        let widget = RuntimeInteractionDeclarationWidget { id: target };
        let local = text_physical_local(String::new(), scroll_y);
        let layout = LayoutOutput {
            bounds,
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        };
        let mut region = slipway_core::scroll_region_from_scrollable_capability(
            &widget,
            &(),
            &local,
            &layout,
            Some(PresentationRegionId::from(region_id)),
            None,
            true,
        );
        region.evidence = evidence;
        region
    }

    impl SlipwayViewDefinition for TextPhysicalProbeWidget {
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
                paint: self.paint(external, local, &layout),
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: vec![
                    text_physical_focus_region(
                        WidgetId::from("decoy-text-probe"),
                        "decoy-focus",
                        layout.bounds,
                        String::new(),
                    ),
                    text_physical_focus_region(
                        self.id(),
                        "text-focus",
                        layout.bounds,
                        local.text.clone(),
                    ),
                ],
                scroll_regions: vec![
                    text_physical_scroll_region(
                        WidgetId::from("decoy-scroll-probe"),
                        "decoy-scroll",
                        layout.bounds,
                        0.0,
                        vec![ScrollConsumptionEvidence {
                            target: WidgetId::from("decoy-scroll-probe"),
                            region_id: Some(PresentationRegionId::from("decoy-scroll")),
                            input_kind: ScrollInputKind::Programmatic,
                            requested_delta: Point { x: 0.0, y: 0.0 },
                            consumed_delta: Point { x: 0.0, y: 0.0 },
                            remaining_delta: Point { x: 0.0, y: 0.0 },
                            consumption: ScrollDeltaConsumption::None,
                            source: slipway_core::EvidenceSource::debug_mcp("test"),
                            diagnostics: Vec::new(),
                        }],
                    ),
                    text_physical_scroll_region(
                        self.id(),
                        "text-scroll",
                        layout.bounds,
                        local.scroll_y,
                        vec![ScrollConsumptionEvidence {
                            target: self.id(),
                            region_id: Some(PresentationRegionId::from("text-scroll")),
                            input_kind: ScrollInputKind::Programmatic,
                            requested_delta: Point {
                                x: 0.0,
                                y: local.scroll_y,
                            },
                            consumed_delta: Point {
                                x: 0.0,
                                y: local.scroll_y,
                            },
                            remaining_delta: Point { x: 0.0, y: 0.0 },
                            consumption: ScrollDeltaConsumption::Complete,
                            source: slipway_core::EvidenceSource::debug_mcp("test"),
                            diagnostics: Vec::new(),
                        }],
                    ),
                ],
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
                layout,
            }
        }
    }

    impl SlipwayWidgetTypes for AlphaChild {
        type ExternalState = TestExternal;
        type LocalState = AlphaLocal;
        type AppMessage = TestMessage;
    }

    impl SlipwaySsot for AlphaChild {
        fn id(&self) -> WidgetId {
            WidgetId::from("alpha-child")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::CommandInput,
                Capability::Paint,
                Capability::StateObservation,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for AlphaChild {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Command(command) if command.command == "increment" => {
                    local.count += 1;
                    EventOutcome::message(self.id(), "increment", TestMessage::ChildChanged)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for AlphaChild {
        fn initial_local_state(&self) -> Self::LocalState {
            AlphaLocal {
                count: self.initial_count,
            }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: TargetLocalRect::new(Rect {
                    origin: Point { x: 0.0, y: 0.0 },
                    size: Size {
                        width: 24.0,
                        height: 16.0,
                    },
                }),
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
                content: format!("alpha:{}", local.count),
                color: Color {
                    red: 0.1,
                    green: 0.2,
                    blue: 0.3,
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
                target: self.id(),
                slot: None,
                name: "count".to_string(),
                value: local.count.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for AlphaChild {
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
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayWidgetTypes for BetaChild {
        type ExternalState = TestExternal;
        type LocalState = BetaLocal;
        type AppMessage = TestMessage;
    }

    impl SlipwaySsot for BetaChild {
        fn id(&self) -> WidgetId {
            WidgetId::from("beta-child")
        }

        fn capabilities(&self) -> Vec<Capability> {
            vec![
                Capability::CommandInput,
                Capability::Paint,
                Capability::StateObservation,
            ]
        }

        fn topology(&self, _external: &Self::ExternalState) -> TopologyNode {
            TopologyNode::leaf(self.id())
        }

        fn unsupported(&self) -> Vec<Diagnostic> {
            Vec::new()
        }
    }

    impl SlipwayLogic for BetaChild {
        fn handle_event(
            &self,
            _external: &Self::ExternalState,
            local: &mut Self::LocalState,
            event: InputEvent,
        ) -> EventOutcome<Self::AppMessage> {
            match event {
                InputEvent::Command(command) if command.command == "increment" => {
                    local.count += 10;
                    EventOutcome::message(self.id(), "increment", TestMessage::ChildChanged)
                }
                _ => EventOutcome::ignored(),
            }
        }
    }

    impl SlipwayView for BetaChild {
        fn initial_local_state(&self) -> Self::LocalState {
            BetaLocal {
                count: self.initial_count,
            }
        }

        fn layout(
            &self,
            _external: &Self::ExternalState,
            _local: &Self::LocalState,
            _input: LayoutInput,
        ) -> LayoutOutput {
            LayoutOutput {
                bounds: TargetLocalRect::new(Rect {
                    origin: Point { x: 28.0, y: 0.0 },
                    size: Size {
                        width: 24.0,
                        height: 16.0,
                    },
                }),
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
                content: format!("beta:{}", local.count),
                color: Color {
                    red: 0.4,
                    green: 0.2,
                    blue: 0.1,
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
                target: self.id(),
                slot: None,
                name: "count".to_string(),
                value: local.count.to_string(),
            }]
        }
    }

    impl SlipwayViewDefinition for BetaChild {
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
                paint_order: PaintOrderDeclaration::source_order(self.id()),
                hit_regions: Vec::new(),
                focus_regions: Vec::new(),
                scroll_regions: Vec::new(),
                semantic_slots: Vec::new(),
                probe_metadata: Vec::new(),
                diagnostics: Vec::new(),
            }
        }
    }

    impl SlipwayApp for TestApp {
        type ExternalState = TestExternal;
        type LocalState = TestAppLocal;
        type AppMessage = TestMessage;
        type Widgets = (AlphaChild, BetaChild);

        fn id(&self) -> WidgetId {
            WidgetId::from("test-app")
        }

        fn widgets(&self) -> &Self::Widgets {
            &self.widgets
        }

        fn initial_local_state(&self) -> Self::LocalState {
            TestAppLocal { initialized: true }
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
            local: &Self::LocalState,
            layout: &LayoutOutput,
        ) -> Vec<PaintOp> {
            vec![PaintOp::Text {
                bounds: layout.bounds.into_rect(),
                content: format!("app-initialized:{}", local.initialized),
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
                target: self.id(),
                slot: None,
                name: "initialized".to_string(),
                value: local.initialized.to_string(),
            }]
        }
    }

    fn frame(index: u64) -> FrameIdentity {
        FrameIdentity {
            surface_id: "runtime-test".to_string(),
            surface_instance_id: "instance".to_string(),
            revision: 1,
            frame_index: index,
            viewport: Rect {
                origin: Point { x: 0.0, y: 0.0 },
                size: Size {
                    width: 64.0,
                    height: 48.0,
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

    fn status_message(id: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.status","arguments":{{"frame":{}}}}}}}"#,
            id,
            frame_json(frame),
        )
    }

    fn current_status_message(id: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.status","arguments":{{"frame":"current"}}}}}}"#,
            id,
        )
    }

    fn control_message(id: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.control","arguments":{{"frame":{},"event":{{"type":"command","target":"probe","command":"count"}}}}}}}}"#,
            id,
            frame_json(frame),
        )
    }

    fn physical_pointer_message(id: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"pointer","phase":"press","position":{{"x":4.0,"y":4.0}},"button":"primary","device":"mouse"}}}}}}}}"#,
            id,
            frame_json(frame),
        )
    }

    fn current_physical_pointer_message(id: &str, x: f32, y: f32) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":"current","operation":{{"type":"pointer","phase":"press","position":{{"x":{},"y":{}}},"button":"primary","device":"mouse"}}}}}}}}"#,
            id, x, y,
        )
    }

    fn physical_text_message(id: &str, frame: &FrameIdentity, text: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"text","target":"text-probe","text":"{}"}}}}}}}}"#,
            id,
            frame_json(frame),
            text,
        )
    }

    fn physical_text_edit_message(id: &str, frame: &FrameIdentity, text: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"text_edit","target":"text-probe","edit_kind":"replace_selection","text":"{}"}}}}}}}}"#,
            id,
            frame_json(frame),
            text,
        )
    }

    fn physical_keyboard_message(id: &str, frame: &FrameIdentity, key: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"keyboard","target":"text-probe","key":"{}","phase":"press"}}}}}}}}"#,
            id,
            frame_json(frame),
            key,
        )
    }

    fn physical_command_message(
        id: &str,
        frame: &FrameIdentity,
        command: &str,
        payload_ref: &str,
    ) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"command","target":"text-probe","command":"{}","payload_ref":"{}"}}}}}}}}"#,
            id,
            frame_json(frame),
            command,
            payload_ref,
        )
    }

    fn unmatched_physical_pointer_message(id: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.physical_control","arguments":{{"frame":{},"operation":{{"type":"pointer","phase":"press","position":{{"x":400.0,"y":400.0}},"button":"primary","device":"mouse"}}}}}}}}"#,
            id,
            frame_json(frame),
        )
    }

    fn traced_app_control_message(id: &str, frame: &FrameIdentity, target: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.control","arguments":{{"frame":{},"trace":true,"event":{{"type":"command","target":"{}","command":"increment"}}}}}}}}"#,
            id,
            frame_json(frame),
            target,
        )
    }

    fn forged_render_message(id: &str, frame: &FrameIdentity) -> String {
        forged_render_tool_message(id, "slipway.debug.render", frame)
    }

    fn forged_render_tool_message(id: &str, tool: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"{}","arguments":{{"packet":{{"target":"forged-mcp-target","frame":{},"paint":[{{"id":"forged-paint"}}]}}}}}}}}"#,
            id,
            tool,
            frame_json(frame),
        )
    }

    fn current_screenshot_message(id: &str) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.screenshot","arguments":{{"packet":{{"target":"forged-mcp-target","frame":"current","paint":[{{"id":"forged-paint"}}]}}}}}}}}"#,
            id,
        )
    }

    fn test_app() -> TestApp {
        TestApp {
            widgets: (
                AlphaChild { initial_count: 3 },
                BetaChild { initial_count: 20 },
            ),
        }
    }

    fn increment_child(target: &str) -> InputEvent {
        InputEvent::Command(CommandEvent {
            target: WidgetId::from(target),
            target_slot: None,
            command: "increment".to_string(),
            payload_ref: None,
            source: None,
        })
    }

    fn count_probe() -> InputEvent {
        InputEvent::Command(CommandEvent {
            target: WidgetId::from("probe"),
            target_slot: None,
            command: "count".to_string(),
            payload_ref: None,
            source: None,
        })
    }

    fn ignored_probe() -> InputEvent {
        InputEvent::Command(CommandEvent {
            target: WidgetId::from("probe"),
            target_slot: None,
            command: "ignored".to_string(),
            payload_ref: None,
            source: None,
        })
    }

    fn mouse_pointer_details() -> slipway_core::PointerDetails {
        let mut buttons = slipway_core::PointerButtons::default();
        buttons.primary = true;
        slipway_core::PointerDetails {
            device: slipway_core::PointerDeviceKind::Mouse,
            buttons,
            ..slipway_core::PointerDetails::default()
        }
    }

    fn backend_presented_physical_press(
        runtime: &SlipwayRuntime<PhysicalProbeWidget>,
    ) -> BackendInputEvent {
        backend_presented_physical_press_for_frame(runtime, frame(35))
    }

    fn backend_presented_physical_press_for_frame(
        runtime: &SlipwayRuntime<PhysicalProbeWidget>,
        frame: FrameIdentity,
    ) -> BackendInputEvent {
        backend_presented_physical_press_for_frame_with_details(
            runtime,
            frame,
            mouse_pointer_details(),
        )
    }

    fn backend_presented_physical_press_for_frame_with_details(
        runtime: &SlipwayRuntime<PhysicalProbeWidget>,
        frame: FrameIdentity,
        details: slipway_core::PointerDetails,
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
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            slipway_core::EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.layout,
            &view.hit_regions,
            Point { x: 4.0, y: 4.0 },
            PointerEventKind::Press,
            Some(slipway_core::PointerButton::Primary),
            details,
            true,
        );
        BackendInputEvent::declared(
            dispatch.expect("backend press resolves hit region").input,
            evidence,
        )
    }

    fn backend_presented_clone_counting_physical_press(
        runtime: &SlipwayRuntime<CloneCountingPhysicalProbeWidget>,
    ) -> BackendInputEvent {
        let frame = frame(35);
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
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            slipway_core::EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.layout,
            &view.hit_regions,
            Point { x: 4.0, y: 4.0 },
            PointerEventKind::Press,
            Some(slipway_core::PointerButton::Primary),
            mouse_pointer_details(),
            true,
        );
        BackendInputEvent::declared(
            dispatch
                .expect("backend press resolves clone-counting hit region")
                .input,
            evidence,
        )
    }

    fn backend_presented_mismatch_press(
        runtime: &SlipwayRuntime<PhysicalMismatchWidget>,
        frame_index: u64,
    ) -> BackendInputEvent {
        let frame = frame(frame_index);
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
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let (dispatch, evidence) = slipway_core::resolve_declared_pointer_dispatch_with_evidence(
            slipway_core::EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.layout,
            &view.hit_regions,
            Point { x: 4.0, y: 4.0 },
            PointerEventKind::Press,
            Some(slipway_core::PointerButton::Primary),
            mouse_pointer_details(),
            true,
        );
        BackendInputEvent::declared(
            dispatch
                .expect("backend mismatch press resolves hit region")
                .input,
            evidence,
        )
    }

    fn backend_presented_physical_text_for_frame(
        runtime: &SlipwayRuntime<TextPhysicalProbeWidget>,
        frame: FrameIdentity,
        text: &str,
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
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let selector = DebugPhysicalControlDeclarationSelector::Target {
            target: WidgetId::from("text-probe"),
        };
        let (event, evidence) = resolve_declared_physical_focus_control(
            slipway_core::EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.focus_regions,
            &selector,
            DeclaredEventDispatchKind::Text,
            true,
            |region| {
                InputEvent::Text(TextInputEvent {
                    target: region.target.clone(),
                    target_slot: region.address.clone(),
                    text: text.to_string(),
                })
            },
        );
        BackendInputEvent::declared(
            event.expect("backend text resolves declared focus region"),
            evidence,
        )
    }

    fn backend_presented_physical_text_edit_for_frame(
        runtime: &SlipwayRuntime<TextPhysicalProbeWidget>,
        frame: FrameIdentity,
        text: &str,
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
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let selector = DebugPhysicalControlDeclarationSelector::Target {
            target: WidgetId::from("text-probe"),
        };
        let (event, evidence) = resolve_declared_physical_focus_control(
            slipway_core::EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.focus_regions,
            &selector,
            DeclaredEventDispatchKind::Text,
            true,
            |region| {
                InputEvent::TextEdit(TextEditEvent {
                    target: region.target.clone(),
                    target_slot: region.address.clone(),
                    kind: TextEditKind::ReplaceSelection,
                    text: Some(text.to_string()),
                    selection_before: None,
                    selection_after: None,
                })
            },
        );
        BackendInputEvent::declared(
            event.expect("backend text edit resolves declared focus region"),
            evidence,
        )
    }

    fn backend_presented_physical_keyboard_for_frame(
        runtime: &SlipwayRuntime<TextPhysicalProbeWidget>,
        frame: FrameIdentity,
        key: &str,
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
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let selector = DebugPhysicalControlDeclarationSelector::Target {
            target: WidgetId::from("text-probe"),
        };
        let (event, evidence) = resolve_declared_physical_focus_control(
            slipway_core::EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.focus_regions,
            &selector,
            DeclaredEventDispatchKind::Keyboard,
            true,
            |region| {
                InputEvent::Keyboard(KeyboardEvent {
                    target: region.target.clone(),
                    target_slot: region.address.clone(),
                    key: key.to_string(),
                    kind: slipway_core::KeyEventKind::Press,
                    modifiers: slipway_core::Modifiers::default(),
                    details: slipway_core::KeyboardDetails::default(),
                })
            },
        );
        BackendInputEvent::declared(
            event.expect("backend keyboard resolves declared focus region"),
            evidence,
        )
    }

    fn backend_presented_physical_command_for_frame(
        runtime: &SlipwayRuntime<TextPhysicalProbeWidget>,
        frame: FrameIdentity,
        command: &str,
        payload_ref: &str,
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
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let selector = DebugPhysicalControlDeclarationSelector::Target {
            target: WidgetId::from("text-probe"),
        };
        let (event, evidence) = resolve_declared_physical_focus_control(
            slipway_core::EvidenceSource::backend_presented("test-backend", "physical-input"),
            frame,
            &view.focus_regions,
            &selector,
            DeclaredEventDispatchKind::Command,
            true,
            |region| {
                InputEvent::Command(CommandEvent {
                    target: region.target.clone(),
                    target_slot: region.address.clone(),
                    command: command.to_string(),
                    payload_ref: Some(payload_ref.to_string()),
                    source: None,
                })
            },
        );
        BackendInputEvent::declared(
            event.expect("backend command resolves declared focus region"),
            evidence,
        )
    }

    fn probe_event_message(id: &str, frame: &FrameIdentity) -> String {
        format!(
            r#"{{"jsonrpc":"2.0","id":"{}","method":"tools/call","params":{{"name":"slipway.debug.probe","arguments":{{"frame":{},"kinds":["event"],"event_trace_limit":2}}}}}}"#,
            id,
            frame_json(frame),
        )
    }

    fn begin_pending(
        attachment: &SlipwayDebugMcpAttachment,
        message: String,
    ) -> slipway_debug_mcp::DebugMcpPendingToolCall {
        match attachment.begin_bridge_message(&message) {
            DebugMcpBridgeMessage::Pending(pending) => pending,
            DebugMcpBridgeMessage::Immediate(response) => {
                panic!("expected pending bridge call, got {response:?}")
            }
        }
    }

    fn response_tool_payload(response: &Value) -> Value {
        serde_json::from_str(
            response["result"]["content"][0]["text"]
                .as_str()
                .expect("tool result text"),
        )
        .expect("tool result text is JSON")
    }

    #[test]
    fn runtime_owns_local_state_and_applies_control() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());
        assert_eq!(runtime.local_state().count, 0);

        let outcome = runtime.apply_input_event(InputEvent::Command(CommandEvent {
            target: WidgetId::from("probe"),
            target_slot: None,
            command: "count".to_string(),
            payload_ref: None,
            source: None,
        }));

        assert!(outcome.handled);
        assert_eq!(runtime.local_state().count, 1);
    }

    #[test]
    fn physical_debug_control_requires_visible_native_backend() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "physical-press",
            frame(2),
            DebugPhysicalControl::Pointer {
                position: Point { x: 4.0, y: 4.0 },
                kind: PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
                pointer_is_pressed: true,
            },
        ));

        assert_eq!(runtime.local_state().presses, 0);
        assert!(runtime.last_backend_input_trace().is_none());
        let DebugReplyProduct::Error(error) = product else {
            panic!("runtime physical control must be refused without a visible native backend");
        };
        assert_eq!(error.code, "native-physical-control-required");
        assert!(error.dispatch_evidence.is_none());
    }

    #[test]
    fn backend_presented_physical_control_ingress_returns_control_trace() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let backend_input = backend_presented_physical_press(&runtime);
        let frame = backend_input
            .dispatch_evidence
            .as_ref()
            .expect("backend input carries evidence")
            .frame
            .clone();
        let mut app_message_batches = Vec::new();
        let product = runtime.handle_backend_presented_physical_control_with_app_reducer(
            DebugCommand::physical_control_with_trace(
                "backend-presented-physical-press",
                frame,
                DebugPhysicalControl::Pointer {
                    position: Point { x: 4.0, y: 4.0 },
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: mouse_pointer_details(),
                    pointer_is_pressed: true,
                },
            ),
            backend_input,
            &mut |_, messages: Vec<Message>| {
                app_message_batches.push(messages);
            },
        );

        assert_eq!(runtime.local_state().presses, 1);
        assert_eq!(app_message_batches, vec![vec![Message::Counted]]);
        assert_eq!(runtime.backend_input_traces().count(), 1);
        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("backend-presented physical ingress must return a control trace");
        };
        assert_eq!(trace.mode, DebugControlMode::PhysicalEquivalent);
        assert!(trace.handled);
        let evidence = trace
            .dispatch_evidence
            .as_ref()
            .expect("physical trace carries backend dispatch evidence");
        assert_eq!(
            evidence.source.label(),
            slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED
        );
        assert_eq!(
            evidence.selected_region,
            Some(PresentationRegionId::from("probe-hit"))
        );
        assert!(
            runtime
                .last_backend_input_trace()
                .expect("backend input trace recorded")
                .handled
        );
    }

    #[test]
    fn backend_presented_pointer_press_can_complete_native_focus_trace() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let frame = frame(39);
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
        let view = runtime.widget().visible_backend_view_definition(
            runtime.external(),
            runtime.local_state(),
            ViewDefinitionInput {
                frame: frame.clone(),
                layout_input,
            },
        );
        let region = view
            .focus_regions
            .iter()
            .find(|region| region.target == WidgetId::from("text-probe"))
            .expect("text probe focus region is declared");
        let position = Point { x: 4.0, y: 4.0 };
        let event = InputEvent::Focus(slipway_core::FocusEvent {
            target: region.target.clone(),
            target_slot: region.address.clone(),
            focused: true,
        });
        let evidence = slipway_core::declared_focus_text_dispatch_evidence(
            slipway_core::EvidenceSource::backend_presented("test-backend", "focused-input"),
            frame.clone(),
            &view.focus_regions,
            Some(region),
            DeclaredEventDispatchKind::Focus,
            Some(position),
            event.clone(),
        );
        let backend_input = BackendInputEvent::declared(event, evidence);

        let product = runtime.handle_backend_presented_physical_control_with_app_reducer(
            DebugCommand::physical_control_with_trace(
                "pointer-focus",
                frame,
                DebugPhysicalControl::Pointer {
                    position,
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: mouse_pointer_details(),
                    pointer_is_pressed: true,
                },
            ),
            backend_input,
            &mut |_, _messages: Vec<Message>| {},
        );

        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("pointer press over native text input should complete as focus trace");
        };
        assert!(trace.handled);
        assert_eq!(trace.event_summary, "focus:true");
        assert!(runtime.local_state().focused);
    }

    #[test]
    fn backend_presented_physical_control_ignores_view_contract_gate() {
        let mut runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::hit_bounds_outside_layout("mismatch-view-contract"),
            (),
        );
        let backend_input = backend_presented_mismatch_press(&runtime, 69);
        let frame = backend_input
            .dispatch_evidence
            .as_ref()
            .expect("backend input carries evidence")
            .frame
            .clone();

        let product = runtime.handle_backend_presented_physical_control_with_app_reducer(
            DebugCommand::physical_control_with_trace(
                "backend-presented-view-contract-delivered",
                frame,
                DebugPhysicalControl::Pointer {
                    position: Point { x: 4.0, y: 4.0 },
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: mouse_pointer_details(),
                    pointer_is_pressed: true,
                },
            ),
            backend_input,
            &mut |_, _messages: Vec<Message>| {},
        );

        assert_eq!(runtime.local_state().presses, 1);
        let DebugReplyProduct::ControlTrace(control_trace) = product else {
            panic!("backend-presented visible input must not be blocked by runtime evidence gates");
        };
        assert!(control_trace.handled);
        let trace = runtime
            .last_backend_input_trace()
            .expect("delivered visible input is traced");
        assert!(trace.handled);
    }

    #[test]
    fn backend_presented_physical_control_obeys_authored_route_without_evidence_gate() {
        let mut runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::route_mismatch("mismatch-route-policy"),
            (),
        );
        let mut backend_input = backend_presented_mismatch_press(&runtime, 70);
        let frame = backend_input
            .dispatch_evidence
            .as_ref()
            .expect("backend input carries evidence")
            .frame
            .clone();
        backend_input
            .dispatch_evidence
            .as_mut()
            .expect("backend helper attaches declared dispatch evidence")
            .route = Some(EventRoute {
            route_id: Some("forged-mismatch-route".to_string()),
            address: None,
            path: vec![WidgetId::from("forged-route-target")],
            phase: EventRoutePhase::Target,
        });

        let product = runtime.handle_backend_presented_physical_control_with_app_reducer(
            DebugCommand::physical_control_with_trace(
                "backend-presented-route-contract-delivered",
                frame,
                DebugPhysicalControl::Pointer {
                    position: Point { x: 4.0, y: 4.0 },
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: mouse_pointer_details(),
                    pointer_is_pressed: true,
                },
            ),
            backend_input,
            &mut |_, _messages: Vec<Message>| {},
        );

        assert_eq!(runtime.local_state().presses, 0);
        let DebugReplyProduct::Error(error) = product else {
            panic!(
                "unhandled backend-presented visible input must return an unhandled error, not an evidence-gate error"
            );
        };
        assert_eq!(error.code, "backend-physical-control-not-handled");
        let trace = runtime
            .last_backend_input_trace()
            .expect("delivered route input is traced");
        assert!(!trace.handled);
        assert!(!trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_ROUTE_MISMATCH
        }));
    }

    #[test]
    fn backend_presented_physical_control_refuses_unhandled_trace_with_dispatch_evidence() {
        let runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let backend_input = backend_presented_physical_press(&runtime);
        let frame = backend_input
            .dispatch_evidence
            .as_ref()
            .expect("backend input carries evidence")
            .frame
            .clone();
        let trace = BackendInputTrace {
            input: backend_input,
            handled: false,
            revision_before: Some(0),
            revision_after: Some(0),
            emitted_messages: Vec::new(),
            local_state: Vec::new(),
            changes: Vec::new(),
            diagnostics: Vec::new(),
        };

        let product = runtime.backend_presented_physical_control_product_from_trace(
            DebugCommand::physical_control_with_trace(
                "backend-presented-unhandled",
                frame,
                DebugPhysicalControl::Pointer {
                    position: Point { x: 4.0, y: 4.0 },
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: mouse_pointer_details(),
                    pointer_is_pressed: true,
                },
            ),
            &trace,
        );

        let DebugReplyProduct::Error(error) = product else {
            panic!("unhandled backend-presented trace must return an error");
        };
        assert_eq!(error.code, "backend-physical-control-not-handled");
        assert_eq!(
            error
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("probe-hit"))
        );
    }

    #[test]
    fn backend_presented_physical_text_delete_accepts_iced_result_mutation() {
        let target = WidgetId::from("text-probe");
        let operation = DebugPhysicalControl::TextEdit {
            selector: DebugPhysicalControlDeclarationSelector::Target {
                target: target.clone(),
            },
            kind: TextEditKind::DeleteBackward,
            text: None,
            selection_before: None,
            selection_after: None,
        };
        let event = InputEvent::TextEdit(TextEditEvent {
            target,
            target_slot: None,
            kind: TextEditKind::ReplaceSelection,
            text: Some("remaining".to_string()),
            selection_before: None,
            selection_after: None,
        });

        assert!(physical_control_operation_matches_backend_event(
            &operation, &event, None
        ));
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn unresolved_physical_debug_control_returns_resolver_evidence() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "physical-miss",
            frame(33),
            DebugPhysicalControl::Pointer {
                position: Point { x: 400.0, y: 400.0 },
                kind: PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
                pointer_is_pressed: true,
            },
        ));

        assert_eq!(runtime.local_state().presses, 0);
        let DebugReplyProduct::Error(error) = product else {
            panic!("expected unresolved physical control error");
        };
        assert_eq!(error.code, "physical-control-unresolved");
        let evidence = error
            .dispatch_evidence
            .expect("unresolved physical control keeps resolver evidence");
        assert_eq!(
            evidence.source.label(),
            slipway_core::EVIDENCE_SOURCE_DEBUG_MCP
        );
        assert_eq!(evidence.selected_region, None);
        assert!(evidence.candidate_regions.is_empty());
        assert!(evidence.generated_event.is_none());
        assert!(evidence.refusal_reason.is_some());
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn physical_text_control_resolves_text_focus_declaration_before_routing() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "physical-text",
            frame(40),
            DebugPhysicalControl::Text {
                selector: DebugPhysicalControlDeclarationSelector::Target {
                    target: WidgetId::from("text-probe"),
                },
                text: "abc".to_string(),
            },
        ));

        assert_eq!(runtime.local_state().text, "abc");
        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("expected physical text control trace");
        };
        assert_eq!(trace.mode, DebugControlMode::PhysicalEquivalent);
        assert_eq!(trace.routed_event_target, WidgetId::from("text-probe"));
        assert_eq!(trace.event_summary, "text:abc");
        assert!(trace.handled);
        let dispatch_evidence = trace
            .dispatch_evidence
            .as_ref()
            .expect("physical text trace carries resolver evidence");
        assert_eq!(dispatch_evidence.kind, DeclaredEventDispatchKind::Text);
        assert_eq!(
            dispatch_evidence.selected_region,
            Some(PresentationRegionId::from("text-focus"))
        );
        assert_eq!(
            dispatch_evidence.candidate_regions,
            vec![PresentationRegionId::from("text-focus")]
        );
        assert_eq!(
            dispatch_evidence
                .generated_event
                .as_ref()
                .map(InputEvent::target),
            Some(&WidgetId::from("text-probe"))
        );
        assert!(dispatch_evidence.capture_event);

        let backend_trace = runtime
            .last_backend_input_trace()
            .expect("physical text records backend-style trace");
        assert!(backend_trace.handled);
        assert_eq!(backend_trace.emitted_messages[0].name, "text");
        assert_eq!(
            backend_trace
                .input
                .dispatch_evidence
                .as_ref()
                .map(|evidence| evidence.kind),
            Some(DeclaredEventDispatchKind::Text)
        );
    }

    #[test]
    fn backend_presented_declared_physical_input_reports_handler_declaration_mismatches() {
        let mut ignored_runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::new("mismatch-ignored", true, false),
            (),
        );
        let ignored_input = backend_presented_mismatch_press(&ignored_runtime, 60);
        let ignored_outcome = ignored_runtime.apply_backend_input_event(ignored_input);

        assert!(!ignored_outcome.handled);
        assert_eq!(ignored_runtime.local_state().presses, 0);
        let ignored_trace = ignored_runtime
            .last_backend_input_trace()
            .expect("backend trace recorded");
        assert!(ignored_trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_ignored_declared_handled"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));
        assert_eq!(
            ignored_trace
                .input
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("mismatch-hit"))
        );

        let mut mutating_ignored_runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::mutating_ignored("mismatch-mutating-ignored"),
            (),
        );
        let mutating_ignored_input =
            backend_presented_mismatch_press(&mutating_ignored_runtime, 65);
        let mutating_ignored_outcome =
            mutating_ignored_runtime.apply_backend_input_event(mutating_ignored_input);

        assert!(!mutating_ignored_outcome.handled);
        assert_eq!(mutating_ignored_runtime.local_state().presses, 1);
        let mutating_ignored_trace = mutating_ignored_runtime
            .last_backend_input_trace()
            .expect("backend trace recorded");
        assert!(mutating_ignored_trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_ignored_declared_handled"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));

        let mut overhandled_runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::new("mismatch-overhandled", false, true),
            (),
        );
        let overhandled_input = backend_presented_mismatch_press(&overhandled_runtime, 61);
        let overhandled_outcome = overhandled_runtime.apply_backend_input_event(overhandled_input);

        assert!(!overhandled_outcome.handled);
        assert_eq!(overhandled_runtime.local_state().presses, 0);
        let overhandled_trace = overhandled_runtime
            .last_backend_input_trace()
            .expect("backend trace recorded");
        assert!(overhandled_trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_handled_declared_unhandled"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));
        assert_eq!(
            overhandled_trace
                .input
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("mismatch-hit"))
        );
    }

    #[test]
    fn backend_presented_physical_input_does_not_runtime_recheck_dispatch_route_mismatch() {
        let mut runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::route_mismatch("mismatch-route-policy"),
            (),
        );
        let mut input = backend_presented_mismatch_press(&runtime, 66);
        input
            .dispatch_evidence
            .as_mut()
            .expect("backend helper attaches declared dispatch evidence")
            .route = Some(EventRoute {
            route_id: Some("forged-mismatch-route".to_string()),
            address: None,
            path: vec![WidgetId::from("forged-route-target")],
            phase: EventRoutePhase::Target,
        });
        let outcome = runtime.apply_backend_input_event(input);

        assert!(!outcome.handled);
        assert_eq!(runtime.local_state().presses, 0);
        let trace = runtime.last_backend_input_trace().expect("trace recorded");
        assert!(!trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_ROUTE_MISMATCH
        }));
    }

    #[test]
    fn backend_presented_declared_physical_input_uses_pre_event_disposition() {
        let mut runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::state_dependent("state-dependent"),
            (),
        );
        let input = backend_presented_mismatch_press(&runtime, 64);
        let outcome = runtime.apply_backend_input_event(input);

        assert!(outcome.handled);
        assert_eq!(runtime.local_state().presses, 1);
        assert!(!outcome.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_handled_declared_unhandled"
        }));
        let trace = runtime.last_backend_input_trace().expect("trace recorded");
        assert!(trace.handled);
        assert!(!trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_handled_declared_unhandled"
        }));
        assert_eq!(
            trace
                .input
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("mismatch-hit"))
        );
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn mcp_physical_control_reports_handler_declaration_mismatches() {
        let mut ignored_runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::new("mismatch-ignored", true, false),
            (),
        );
        let ignored_product =
            ignored_runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
                "mcp-mismatch-ignored",
                frame(62),
                DebugPhysicalControl::Pointer {
                    position: Point { x: 4.0, y: 4.0 },
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: slipway_core::PointerDetails::default(),
                    pointer_is_pressed: true,
                },
            ));
        let DebugReplyProduct::ControlTrace(ignored_trace) = ignored_product else {
            panic!("expected ignored mismatch physical trace");
        };
        assert!(!ignored_trace.handled);
        assert_eq!(ignored_runtime.local_state().presses, 0);
        assert!(ignored_trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_ignored_declared_handled"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));
        assert_eq!(
            ignored_trace
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("mismatch-hit"))
        );

        let mut mutating_ignored_runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::mutating_ignored("mcp-mismatch-mutating-ignored"),
            (),
        );
        let mutating_ignored_product = mutating_ignored_runtime.handle_debug_command(
            DebugCommand::physical_control_with_trace(
                "mcp-mismatch-mutating-ignored",
                frame(165),
                DebugPhysicalControl::Pointer {
                    position: Point { x: 4.0, y: 4.0 },
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: slipway_core::PointerDetails::default(),
                    pointer_is_pressed: true,
                },
            ),
        );
        let DebugReplyProduct::ControlTrace(mutating_ignored_trace) = mutating_ignored_product
        else {
            panic!("expected mutating ignored mismatch physical trace");
        };
        assert!(!mutating_ignored_trace.handled);
        assert_eq!(mutating_ignored_runtime.local_state().presses, 0);
        assert!(mutating_ignored_trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_ignored_declared_handled"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));

        let mut overhandled_runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::new("mismatch-overhandled", false, true),
            (),
        );
        let overhandled_product =
            overhandled_runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
                "mcp-mismatch-overhandled",
                frame(63),
                DebugPhysicalControl::Pointer {
                    position: Point { x: 4.0, y: 4.0 },
                    kind: PointerEventKind::Press,
                    button: Some(slipway_core::PointerButton::Primary),
                    details: slipway_core::PointerDetails::default(),
                    pointer_is_pressed: true,
                },
            ));
        let DebugReplyProduct::ControlTrace(overhandled_trace) = overhandled_product else {
            panic!("expected overhandled mismatch physical trace");
        };
        assert!(!overhandled_trace.handled);
        assert_eq!(overhandled_runtime.local_state().presses, 0);
        assert!(overhandled_trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_handled_declared_unhandled"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));
        assert_eq!(
            overhandled_trace
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("mismatch-hit"))
        );
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn mcp_physical_control_refuses_policy_route_without_target_before_handler() {
        let mut runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::route_mismatch("mismatch-route-policy"),
            (),
        );
        let product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "mcp-policy-route",
            frame(67),
            DebugPhysicalControl::Pointer {
                position: Point { x: 4.0, y: 4.0 },
                kind: PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
                pointer_is_pressed: true,
            },
        ));

        let DebugReplyProduct::Diagnostics(diagnostics) = product else {
            panic!("expected policy-derived route contract diagnostics");
        };
        assert_eq!(runtime.local_state().presses, 0);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "view_contract.hit_route_target_missing"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn mcp_physical_control_refuses_blocking_view_contract_before_handler() {
        let mut runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::invalid_hit_route("invalid-hit-route"),
            (),
        );
        let product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "mcp-invalid-view",
            frame(68),
            DebugPhysicalControl::Pointer {
                position: Point { x: 4.0, y: 4.0 },
                kind: PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
                pointer_is_pressed: true,
            },
        ));

        let DebugReplyProduct::Diagnostics(diagnostics) = product else {
            panic!("expected view contract diagnostics");
        };
        assert_eq!(runtime.local_state().presses, 0);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "view_contract.hit_route_empty"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn mcp_physical_control_uses_pre_event_disposition() {
        let mut runtime = SlipwayRuntime::new(
            PhysicalMismatchWidget::state_dependent("state-dependent"),
            (),
        );
        let product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "mcp-state-dependent",
            frame(65),
            DebugPhysicalControl::Pointer {
                position: Point { x: 4.0, y: 4.0 },
                kind: PointerEventKind::Press,
                button: Some(slipway_core::PointerButton::Primary),
                details: slipway_core::PointerDetails::default(),
                pointer_is_pressed: true,
            },
        ));

        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("expected state-dependent physical trace");
        };
        assert!(trace.handled);
        assert_eq!(runtime.local_state().presses, 1);
        assert!(!trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_handled_declared_unhandled"
        }));
        assert_eq!(
            trace
                .dispatch_evidence
                .as_ref()
                .and_then(|evidence| evidence.selected_region.as_ref()),
            Some(&PresentationRegionId::from("mismatch-hit"))
        );
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn physical_keyboard_and_command_select_later_text_focus_declaration() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let keyboard_product =
            runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
                "physical-keyboard",
                frame(45),
                DebugPhysicalControl::Keyboard {
                    selector: DebugPhysicalControlDeclarationSelector::Target {
                        target: WidgetId::from("text-probe"),
                    },
                    key: "Enter".to_string(),
                    kind: slipway_core::KeyEventKind::Press,
                    modifiers: slipway_core::Modifiers::default(),
                    details: slipway_core::KeyboardDetails::default(),
                },
            ));

        assert_eq!(runtime.local_state().key.as_deref(), Some("Enter"));
        let DebugReplyProduct::ControlTrace(keyboard_trace) = keyboard_product else {
            panic!("expected physical keyboard control trace");
        };
        assert_eq!(keyboard_trace.mode, DebugControlMode::PhysicalEquivalent);
        let keyboard_evidence = keyboard_trace
            .dispatch_evidence
            .as_ref()
            .expect("keyboard trace carries resolver evidence");
        assert_eq!(keyboard_evidence.kind, DeclaredEventDispatchKind::Keyboard);
        assert_eq!(
            keyboard_evidence.selected_region,
            Some(PresentationRegionId::from("text-focus"))
        );
        assert_eq!(
            keyboard_evidence.candidate_regions,
            vec![PresentationRegionId::from("text-focus")]
        );
        assert_eq!(
            keyboard_trace.routed_event_target,
            WidgetId::from("text-probe")
        );
        assert!(keyboard_trace.handled);

        let command_product =
            runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
                "physical-command",
                frame(46),
                DebugPhysicalControl::Command {
                    selector: DebugPhysicalControlDeclarationSelector::Region {
                        region: PresentationRegionId::from("text-focus"),
                    },
                    command: "submit".to_string(),
                    payload_ref: Some("payload-1".to_string()),
                },
            ));

        assert_eq!(runtime.local_state().command.as_deref(), Some("submit"));
        let DebugReplyProduct::ControlTrace(command_trace) = command_product else {
            panic!("expected physical command control trace");
        };
        assert_eq!(command_trace.mode, DebugControlMode::PhysicalEquivalent);
        let command_evidence = command_trace
            .dispatch_evidence
            .as_ref()
            .expect("command trace carries resolver evidence");
        assert_eq!(command_evidence.kind, DeclaredEventDispatchKind::Command);
        assert_eq!(
            command_evidence.selected_region,
            Some(PresentationRegionId::from("text-focus"))
        );
        assert_eq!(
            command_trace.routed_event_target,
            WidgetId::from("text-probe")
        );
        assert!(command_trace.handled);
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn physical_scroll_control_selects_later_scroll_declaration_before_routing() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "physical-scroll",
            frame(47),
            DebugPhysicalControl::Scroll {
                selector: DebugPhysicalControlDeclarationSelector::Target {
                    target: WidgetId::from("text-probe"),
                },
                offset_x: 0.0,
                offset_y: 24.0,
            },
        ));

        assert_eq!(runtime.local_state().scroll_y, 24.0);
        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("expected physical scroll control trace");
        };
        assert_eq!(trace.mode, DebugControlMode::PhysicalEquivalent);
        assert_eq!(trace.routed_event_target, WidgetId::from("text-probe"));
        assert_eq!(trace.event_summary, "scroll:text-scroll:0,24");
        assert!(trace.handled);
        let evidence = trace
            .dispatch_evidence
            .as_ref()
            .expect("scroll trace carries resolver evidence");
        assert_eq!(evidence.kind, DeclaredEventDispatchKind::Scroll);
        assert_eq!(
            evidence.selected_region,
            Some(PresentationRegionId::from("text-scroll"))
        );
        assert_eq!(
            evidence.candidate_regions,
            vec![PresentationRegionId::from("text-scroll")]
        );

        let backend_trace = runtime
            .last_backend_input_trace()
            .expect("physical scroll records backend-style trace");
        assert_eq!(backend_trace.emitted_messages[0].name, "scroll");
        assert_eq!(
            backend_trace
                .input
                .dispatch_evidence
                .as_ref()
                .map(|dispatch| dispatch.selected_region.clone()),
            Some(Some(PresentationRegionId::from("text-scroll")))
        );
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn physical_wheel_control_preserves_existing_scroll_region_resolution() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "physical-wheel",
            frame(48),
            DebugPhysicalControl::Wheel {
                position: Point { x: 4.0, y: 4.0 },
                delta_x: 0.0,
                delta_y: 7.0,
            },
        ));

        assert_eq!(runtime.local_state().scroll_y, 7.0);
        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("expected physical wheel control trace");
        };
        assert_eq!(trace.mode, DebugControlMode::PhysicalEquivalent);
        assert_eq!(trace.routed_event_target, WidgetId::from("text-probe"));
        assert_eq!(trace.event_summary, "wheel:0,7");
        assert!(trace.handled);
        let evidence = trace
            .dispatch_evidence
            .as_ref()
            .expect("wheel trace carries resolver evidence");
        assert_eq!(evidence.kind, DeclaredEventDispatchKind::Wheel);
        assert_eq!(
            evidence.selected_region,
            Some(PresentationRegionId::from("text-scroll"))
        );
        assert_eq!(trace.messages[0].name, "wheel");
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control no longer resolves declarations or mutates state"]
    fn unresolved_physical_text_and_focus_refuse_without_mutating_state() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let text_product = runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
            "physical-text-miss",
            frame(41),
            DebugPhysicalControl::Text {
                selector: DebugPhysicalControlDeclarationSelector::Target {
                    target: WidgetId::from("missing"),
                },
                text: "abc".to_string(),
            },
        ));

        assert_eq!(runtime.local_state().text, "");
        let DebugReplyProduct::Error(text_error) = text_product else {
            panic!("expected unresolved text physical control error");
        };
        assert_eq!(text_error.code, "physical-control-unresolved");
        let text_evidence = text_error
            .dispatch_evidence
            .expect("unresolved text keeps resolver evidence");
        assert_eq!(text_evidence.kind, DeclaredEventDispatchKind::Text);
        assert_eq!(text_evidence.selected_region, None);
        assert!(text_evidence.generated_event.is_none());
        assert!(!text_evidence.capture_event);
        assert!(
            text_evidence
                .refusal_reason
                .as_deref()
                .expect("text refusal reason")
                .contains("text-edit focus")
        );

        let focus_product =
            runtime.handle_debug_command(DebugCommand::physical_control_with_trace(
                "physical-focus-miss",
                frame(42),
                DebugPhysicalControl::Focus {
                    selector: DebugPhysicalControlDeclarationSelector::Region {
                        region: PresentationRegionId::from("missing-focus"),
                    },
                    focused: true,
                },
            ));

        assert!(!runtime.local_state().focused);
        let DebugReplyProduct::Error(focus_error) = focus_product else {
            panic!("expected unresolved focus physical control error");
        };
        assert_eq!(focus_error.code, "physical-control-unresolved");
        let focus_evidence = focus_error
            .dispatch_evidence
            .expect("unresolved focus keeps resolver evidence");
        assert_eq!(focus_evidence.kind, DeclaredEventDispatchKind::Focus);
        assert_eq!(focus_evidence.selected_region, None);
        assert!(focus_evidence.generated_event.is_none());
        assert!(!focus_evidence.capture_event);
        assert!(
            focus_evidence
                .refusal_reason
                .as_deref()
                .expect("focus refusal reason")
                .contains("focus")
        );
    }

    #[test]
    fn backend_input_event_probe_preserves_declared_dispatch_evidence() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let backend_input = backend_presented_physical_press(&runtime);

        let outcome = runtime.apply_backend_input_event(backend_input.clone());

        assert!(outcome.handled);
        assert_eq!(runtime.local_state().presses, 1);
        assert_eq!(runtime.last_backend_input_event(), Some(&backend_input));

        let product = runtime.handle_debug_command(DebugCommand::probe(
            "backend-event-probe",
            frame(36),
            ProbeRequest {
                target: None,
                kinds: vec![ProbeKind::Event],
                event_trace_limit: None,
            },
        ));
        let DebugReplyProduct::Probes(products) = product else {
            panic!("expected probe products");
        };
        let event = products
            .iter()
            .find_map(|product| match product {
                ProbeProduct::Event(event) => Some(event),
                _ => None,
            })
            .expect("event probe");
        let evidence = event
            .dispatch_evidence
            .as_ref()
            .expect("backend event probe carries dispatch evidence");
        assert_eq!(
            evidence.source.label(),
            slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED
        );
        assert_eq!(
            evidence.selected_region,
            Some(PresentationRegionId::from("probe-hit"))
        );
        assert_eq!(
            evidence.generated_event.as_ref().map(InputEvent::target),
            Some(&WidgetId::from("physical-probe"))
        );

        assert_eq!(event.handled, Some(true));
        assert_eq!(event.emitted_messages.len(), 1);
        assert_eq!(
            event.emitted_messages[0].target,
            WidgetId::from("physical-probe")
        );
        assert_eq!(event.emitted_messages[0].name, "press");
        assert!(event.local_state.is_empty());
        assert!(matches!(
            (event.revision_before, event.revision_after),
            (Some(before), Some(after)) if after > before
        ));
    }

    #[test]
    fn backend_input_event_trace_preserves_messages_after_app_reducer_consumes_them() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let backend_input = backend_presented_physical_press(&runtime);
        let mut app_message_batches = Vec::new();

        let report = runtime.apply_backend_input_event_with_app_reducer(
            backend_input,
            &mut |_external, app_messages| {
                app_message_batches.push(app_messages.len());
            },
        );

        assert!(report.handled);
        assert_eq!(report.emitted_messages, 1);
        assert_eq!(report.applied_messages, 1);
        assert_eq!(app_message_batches, vec![1]);
        let trace = runtime
            .last_backend_input_trace()
            .expect("backend input trace recorded");
        assert!(trace.handled);
        assert_eq!(trace.emitted_messages.len(), 1);
        assert_eq!(
            trace.emitted_messages[0].target,
            WidgetId::from("physical-probe")
        );
        assert_eq!(trace.emitted_messages[0].name, "press");
        assert!(matches!(
            (trace.revision_before, trace.revision_after),
            (Some(before), Some(after)) if after > before
        ));
    }

    #[test]
    fn ordinary_backend_trace_change_values_are_compacted_but_shape_is_preserved() {
        let change = slipway_core::ChangeEvidence {
            target: WidgetId::from("trace-target"),
            slot: Some(WidgetSlotAddress {
                widget: WidgetId::from("trace-target"),
                ordinal: 3,
                path: vec![WidgetId::from("root"), WidgetId::from("trace-target")],
            }),
            field: "large-field".to_string(),
            before: Some("large-before-value".repeat(128)),
            after: Some("large-after-value".repeat(128)),
        };

        let compact = compact_backend_trace_changes(std::slice::from_ref(&change));

        assert_eq!(compact.len(), 1);
        assert_eq!(compact[0].target, change.target);
        assert_eq!(compact[0].slot, change.slot);
        assert_eq!(compact[0].field, change.field);
        assert_eq!(compact[0].before.as_deref(), Some("<redacted>"));
        assert_eq!(compact[0].after.as_deref(), Some("<redacted>"));
        assert_eq!(
            ChangeShapeIdentity::from(&compact[0]),
            ChangeShapeIdentity {
                target: WidgetId::from("trace-target"),
                slot: Some(WidgetSlotAddress {
                    widget: WidgetId::from("trace-target"),
                    ordinal: 3,
                    path: vec![WidgetId::from("root"), WidgetId::from("trace-target")],
                }),
                field: "large-field".to_string(),
                before_present: true,
                after_present: true,
            }
        );
    }

    #[test]
    fn visible_backend_input_does_not_clone_local_state_for_rollback() {
        CLONE_COUNTING_PHYSICAL_LOCAL_CLONES.store(0, Ordering::SeqCst);
        let mut runtime = SlipwayRuntime::new(CloneCountingPhysicalProbeWidget, ());
        let backend_input = backend_presented_clone_counting_physical_press(&runtime);

        let outcome = runtime.apply_backend_input_event(backend_input);

        assert!(outcome.handled);
        assert_eq!(runtime.local_state().presses, 1);
        assert_eq!(
            CLONE_COUNTING_PHYSICAL_LOCAL_CLONES.load(Ordering::SeqCst),
            0,
            "visible backend input must not clone local state for rollback"
        );
    }

    #[test]
    fn direct_backend_input_reaches_handler_without_runtime_evidence_gate() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());
        let backend_input = BackendInputEvent::direct(count_probe());

        let outcome = runtime.apply_backend_input_event(backend_input.clone());

        assert!(outcome.handled);
        assert_eq!(runtime.local_state().count, 1);
        assert!(!outcome.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_MISSING
        }));
        let trace = runtime
            .last_backend_input_trace()
            .expect("backend input trace recorded");
        assert_eq!(trace.input, backend_input);
        assert!(trace.handled);
        assert_eq!(trace.emitted_messages.len(), 1);
        assert!(trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "backend_input.dispatch_evidence_missing"
                && diagnostic.severity == DiagnosticSeverity::Error
        }));
    }

    #[test]
    fn backend_input_event_probe_defaults_to_latest_backend_trace() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let first = backend_presented_physical_press(&runtime);
        runtime.apply_backend_input_event(first);
        let second = backend_presented_physical_press(&runtime);
        runtime.apply_backend_input_event(second.clone());

        assert_eq!(runtime.backend_input_traces().count(), 2);
        assert_eq!(runtime.last_backend_input_event(), Some(&second));

        let product = runtime.handle_debug_command(DebugCommand::probe(
            "backend-event-probe-log",
            frame(38),
            ProbeRequest {
                target: None,
                kinds: vec![ProbeKind::Event],
                event_trace_limit: None,
            },
        ));
        let DebugReplyProduct::Probes(products) = product else {
            panic!("expected probe products");
        };
        let events = products
            .iter()
            .filter_map(|product| match product {
                ProbeProduct::Event(event) => Some(event),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0].dispatch_evidence.as_ref(),
            second.dispatch_evidence.as_ref()
        );
        assert!(events[0].local_state.is_empty());
    }

    #[test]
    fn backend_input_event_probe_respects_explicit_trace_limit() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let first = backend_presented_physical_press(&runtime);
        runtime.apply_backend_input_event(first.clone());
        let second = backend_presented_physical_press(&runtime);
        runtime.apply_backend_input_event(second.clone());

        let product = runtime.handle_debug_command(DebugCommand::probe(
            "backend-event-probe-log",
            frame(38),
            ProbeRequest {
                target: None,
                kinds: vec![ProbeKind::Event],
                event_trace_limit: Some(2),
            },
        ));
        let DebugReplyProduct::Probes(products) = product else {
            panic!("expected probe products");
        };
        let events = products
            .iter()
            .filter_map(|product| match product {
                ProbeProduct::Event(event) => Some(event),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].dispatch_evidence.as_ref(),
            first.dispatch_evidence.as_ref()
        );
        assert_eq!(
            events[1].dispatch_evidence.as_ref(),
            second.dispatch_evidence.as_ref()
        );
        assert!(events[0].local_state.is_empty());
        assert!(events[1].local_state.is_empty());
    }

    #[test]
    fn assembly_from_widget_and_external_state_creates_runtime_and_attachment() {
        let mut app = SlipwayAssembledApp::new(ProbeWidget, ());

        assert_eq!(app.runtime.local_state().count, 0);
        assert!(app.debug_mcp.server().config().allow_control);
        assert!(app.debug_mcp.server().config().allow_render);

        let pending = begin_pending(&app.debug_mcp, status_message("status", &frame(1)));
        assert!(
            pending
                .try_finish()
                .expect("pending finish should not fail")
                .is_none()
        );

        app.runtime
            .drain_debug_once()
            .expect("drain succeeds")
            .expect("reply produced");
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("status response");

        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(payload.contains(r#""product_kind":"status""#));
    }

    #[test]
    fn command_submitted_through_attachment_mutates_same_runtime_state() {
        let mut app = SlipwayAssembledApp::new(ProbeWidget, ());
        let pending = begin_pending(&app.debug_mcp, control_message("control", &frame(1)));

        app.runtime
            .drain_debug_once()
            .expect("drain succeeds")
            .expect("reply produced");
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("control response");

        assert_eq!(app.runtime.local_state().count, 1);
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(payload.contains(r#""product_kind":"diagnostics""#));
    }

    #[test]
    fn runtime_mcp_attachment_request_mutates_same_runtime_state() {
        let mut app = SlipwayAssembledApp::new(ProbeWidget, ());
        let handle = app
            .debug_mcp
            .submit_runtime_request(control_message("runtime-control", &frame(30)))
            .expect("runtime MCP request queued");

        let drained = app
            .runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("control returns response");
        let response = handle
            .recv()
            .expect("transport response arrives")
            .expect("control response sent");

        assert_eq!(app.runtime.local_state().count, 1);
        assert_eq!(drained, response);
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(payload.contains(r#""product_kind":"diagnostics""#));
    }

    #[test]
    fn runtime_mcp_physical_control_refuses_without_visible_native_backend() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(physical_pointer_message(
                "runtime-native-required",
                &frame(32),
            ))
            .expect("runtime MCP physical request queued");

        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical control refusal returns response");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("physical control response sent");

        assert_eq!(runtime.local_state().presses, 0);
        assert!(runtime.last_backend_input_trace().is_none());
        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["tool"], "slipway.debug.physical_control");
        assert_eq!(payload["bridge_method"], "control");
        assert_eq!(payload["product_kind"], "error");
        assert_eq!(
            payload["product"]["code"],
            "native-physical-control-required"
        );
        assert!(payload["product"]["dispatch_evidence"].is_null());
    }

    #[test]
    fn runtime_native_mcp_physical_control_yields_pending_bridge_lease() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(physical_pointer_message(
                "native-driver-pending",
                &frame(32),
            ))
            .expect("runtime MCP physical request queued");

        let pending = runtime
            .take_pending_native_mcp_call()
            .expect("native MCP take succeeds")
            .expect("physical control becomes a pending bridge call");
        let lease = runtime
            .take_debug_command_lease()
            .expect("debug bridge take succeeds")
            .expect("physical control command is leased to the native driver");

        match lease.command().kind {
            DebugCommandKind::PhysicalControl { .. } => {}
            ref other => panic!("expected physical control lease, got {other:?}"),
        }
        assert_eq!(runtime.local_state().presses, 0);
        assert!(runtime.last_backend_input_trace().is_none());

        lease
            .complete(DebugReplyProduct::Error(DebugFailure {
                code: "native-driver-test-response".to_string(),
                message: "native driver owns physical completion".to_string(),
                dispatch_evidence: None,
            }))
            .expect("native driver can complete the leased command");
        let response = pending
            .try_finish_and_respond()
            .expect("pending MCP response can be finished")
            .expect("response is sent");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("physical control response sent");

        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["tool"], "slipway.debug.physical_control");
        assert_eq!(payload["product"]["code"], "native-driver-test-response");
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_mcp_physical_control_returns_resolver_evidence_json() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(physical_pointer_message("runtime-physical", &frame(32)))
            .expect("runtime MCP physical request queued");

        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical control trace returns response");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("physical control response sent");

        assert_eq!(runtime.local_state().presses, 1);
        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["tool"], "slipway.debug.physical_control");
        assert_eq!(payload["bridge_method"], "control");
        assert_eq!(payload["product_kind"], "control_trace");
        assert_eq!(payload["product"]["mode"], "physical_equivalent");
        assert_eq!(payload["product"]["physical_equivalent"], true);
        assert_eq!(payload["product"]["routed_event_target"], "physical-probe");
        assert_eq!(
            payload["product"]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_DEBUG_MCP
        );
        assert_eq!(payload["product"]["dispatch_evidence"]["kind"], "pointer");
        assert_eq!(
            payload["product"]["dispatch_evidence"]["selected_region"],
            "probe-hit"
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["candidate_regions"][0],
            "probe-hit"
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["generated_event"]["target"],
            "physical-probe"
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["generated_event"]["summary"],
            "pointer:Press"
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["route"]["path"][0],
            "physical-probe"
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["capture_event"],
            true
        );
        assert_eq!(payload["product"]["result_identity"]["handled"], true);
        assert_eq!(
            payload["product"]["result_identity"]["emitted_messages"][0]["name"],
            "press"
        );
        let trace = runtime
            .last_backend_input_trace()
            .expect("MCP physical control records backend-style trace evidence");
        assert_eq!(trace.handled, true);
        assert_eq!(trace.emitted_messages.len(), 1);
        assert_eq!(trace.emitted_messages[0].name, "press");
        assert_eq!(
            trace
                .input
                .dispatch_evidence
                .as_ref()
                .map(|evidence| evidence.source.label()),
            Some(slipway_core::EVIDENCE_SOURCE_DEBUG_MCP)
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_mcp_physical_control_and_backend_input_share_event_probe_surface() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(32);
        let physical_handle = client
            .submit(physical_pointer_message("runtime-physical", &shared_frame))
            .expect("runtime MCP physical request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical control returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical control response sent");
        assert_eq!(physical_response, physical_transport_response);
        let physical_payload = response_tool_payload(&physical_transport_response);

        let backend_input = backend_presented_physical_press_for_frame(&runtime, shared_frame);
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);
        assert_eq!(runtime.local_state().presses, 2);

        let probe_handle = client
            .submit(probe_event_message("runtime-event-probe", &frame(39)))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");

        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_DEBUG_MCP
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED
        );
        assert_eq!(
            events[0]["dispatch_evidence"]["selected_region"],
            "probe-hit"
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["selected_region"],
            "probe-hit"
        );
        assert_eq!(
            events[0]["dispatch_identity"],
            events[1]["dispatch_identity"]
        );
        assert_eq!(
            events[0]["dispatch_identity"]["selected_region"],
            "probe-hit"
        );
        assert_eq!(
            physical_payload["product"]["result_identity"],
            events[0]["result_identity"]
        );
        assert_eq!(events[0]["result_identity"], events[1]["result_identity"]);
        assert_eq!(events[0]["result_identity"]["handled"], true);
        assert_eq!(
            events[0]["result_identity"]["emitted_messages"][0]["name"],
            "press"
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic["diagnostic"]["code"]
                    == "event_equivalence.identity_match"),
            "expected equivalence match diagnostic, got {diagnostics:?}"
        );
        assert_eq!(events[0]["handled"], true);
        assert_eq!(events[1]["handled"], true);
        assert_eq!(events[0]["emitted_messages"][0]["name"], "press");
        assert_eq!(events[1]["emitted_messages"][0]["name"], "press");
        assert_eq!(events[0]["local_state"][0]["value"], "1");
        assert_eq!(events[1]["local_state"][0]["value"], "2");
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_event_probe_warns_when_mcp_and_backend_physical_identity_diverge() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(41);
        let physical_handle = client
            .submit(physical_pointer_message(
                "runtime-physical-divergence",
                &shared_frame,
            ))
            .expect("runtime MCP physical request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical control returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical control response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input = backend_presented_physical_press_for_frame_with_details(
            &runtime,
            shared_frame,
            slipway_core::PointerDetails::default(),
        );
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-event-divergence-probe",
                &frame(42),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");

        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_DEBUG_MCP
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED
        );
        assert_ne!(
            events[0]["dispatch_identity"], events[1]["dispatch_identity"],
            "MCP mouse input and backend unknown-device input must not be collapsed into the same physical identity"
        );
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                    && diagnostic["diagnostic"]["severity"] == "Warning"
            }),
            "expected dispatch mismatch diagnostic, got {diagnostics:?}"
        );
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_event_probe_warns_when_same_dispatch_has_different_result_identity() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(44);
        let physical_handle = client
            .submit(physical_pointer_message(
                "runtime-physical-result-divergence",
                &shared_frame,
            ))
            .expect("runtime MCP physical request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical control returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical control response sent");
        assert_eq!(physical_response, physical_transport_response);

        let mcp_trace = runtime
            .last_backend_input_trace()
            .expect("MCP physical control records trace")
            .clone();
        let mut backend_input = mcp_trace.input.clone();
        let evidence = backend_input
            .dispatch_evidence
            .as_mut()
            .expect("MCP trace carries dispatch evidence");
        evidence.source =
            slipway_core::EvidenceSource::backend_presented("test-backend", "physical-input");
        runtime.record_backend_input_trace(BackendInputTrace {
            input: backend_input,
            handled: false,
            revision_before: None,
            revision_after: None,
            emitted_messages: Vec::new(),
            local_state: Vec::new(),
            changes: Vec::new(),
            diagnostics: Vec::new(),
        });

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-result-divergence-probe",
                &frame(45),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");

        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]["dispatch_identity"], events[1]["dispatch_identity"],
            "source labels must not make otherwise identical physical dispatches appear different"
        );
        assert_ne!(
            events[0]["result_identity"], events[1]["result_identity"],
            "handled/messages/diagnostic result shape must remain visible"
        );
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic["diagnostic"]["code"] == "event_equivalence.result_identity_mismatch"
                    && diagnostic["diagnostic"]["severity"] == "Warning"
            }),
            "expected result mismatch diagnostic, got {diagnostics:?}"
        );
    }

    #[test]
    fn runtime_delivers_backend_input_with_forged_dispatch_evidence_without_runtime_gate() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let mut backend_input = backend_presented_physical_press_for_frame(&runtime, frame(46));
        let evidence = backend_input
            .dispatch_evidence
            .as_mut()
            .expect("backend input carries dispatch evidence");
        evidence.source = slipway_core::EvidenceSource::debug_mcp("forged-backend-input");

        let outcome = runtime.apply_backend_input_event(backend_input);

        assert!(outcome.handled);
        assert_eq!(runtime.local_state().presses, 1);
        assert!(!outcome.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_SOURCE_MISMATCH
        }));
        let trace = runtime
            .last_backend_input_trace()
            .expect("backend input is recorded for evidence");
        assert!(trace.handled);
        assert_eq!(trace.emitted_messages.len(), 1);
        assert!(!trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == slipway_core::BACKEND_INPUT_DISPATCH_EVIDENCE_SOURCE_MISMATCH
        }));
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_mcp_physical_text_event_probe_uses_backend_trace_surface() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let physical_handle = client
            .submit(physical_text_message(
                "runtime-physical-text",
                &frame(43),
                "abc",
            ))
            .expect("runtime MCP physical text request queued");

        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical text returns response");
        let transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical text response sent");

        assert_eq!(runtime.local_state().text, "abc");
        assert_eq!(response, transport_response);
        let physical_payload = response_tool_payload(&transport_response);
        assert_eq!(physical_payload["product"]["mode"], "physical_equivalent");
        assert_eq!(
            physical_payload["product"]["dispatch_evidence"]["kind"],
            "text"
        );

        let probe_handle = client
            .submit(probe_event_message("runtime-text-event-probe", &frame(44)))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");

        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        let events = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        assert_eq!(events.len(), 1);
        assert_eq!(
            events[0]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_DEBUG_MCP
        );
        assert_eq!(events[0]["dispatch_evidence"]["kind"], "text");
        assert_eq!(
            events[0]["dispatch_evidence"]["selected_region"],
            "text-focus"
        );
        assert_eq!(
            events[0]["dispatch_evidence"]["generated_event"]["target"],
            "text-probe"
        );
        assert_eq!(
            events[0]["dispatch_evidence"]["generated_event"]["text"]["text"],
            "abc"
        );
        assert_eq!(events[0]["handled"], true);
        assert_eq!(events[0]["emitted_messages"][0]["name"], "text");
        let text_state = events[0]["local_state"]
            .as_array()
            .expect("local state array")
            .iter()
            .find(|state| state["name"] == "text")
            .expect("text state");
        assert_eq!(text_state["value"], "abc");
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_mcp_and_backend_physical_text_share_event_identity() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(47);
        let physical_handle = client
            .submit(physical_text_message(
                "runtime-text-equivalence",
                &shared_frame,
                "abc",
            ))
            .expect("runtime MCP physical text request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical text returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical text response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input =
            backend_presented_physical_text_for_frame(&runtime, shared_frame, "abc");
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-text-equivalence-probe",
                &frame(48),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        assert_eq!(response, transport_response);

        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_DEBUG_MCP
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED
        );
        assert_eq!(events[0]["dispatch_evidence"]["kind"], "text");
        assert_eq!(events[1]["dispatch_evidence"]["kind"], "text");
        assert_eq!(
            events[0]["dispatch_identity"],
            events[1]["dispatch_identity"]
        );
        assert_eq!(events[0]["result_identity"], events[1]["result_identity"]);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.identity_match"
        }));
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                || diagnostic["diagnostic"]["code"] == "event_equivalence.result_identity_mismatch"
        }));
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_mcp_and_backend_physical_text_edit_share_event_identity() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(147);
        let physical_handle = client
            .submit(physical_text_edit_message(
                "runtime-text-edit-equivalence",
                &shared_frame,
                "abc",
            ))
            .expect("runtime MCP physical text edit request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical text edit returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical text edit response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input =
            backend_presented_physical_text_edit_for_frame(&runtime, shared_frame, "abc");
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-text-edit-equivalence-probe",
                &frame(148),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        assert_eq!(response, transport_response);

        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["dispatch_evidence"]["kind"], "text");
        assert_eq!(events[1]["dispatch_evidence"]["kind"], "text");
        assert_eq!(
            events[0]["dispatch_evidence"]["generated_event"]["text_edit"]["kind"],
            "ReplaceSelection"
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["generated_event"]["text_edit"]["kind"],
            "ReplaceSelection"
        );
        assert_eq!(
            events[0]["dispatch_identity"],
            events[1]["dispatch_identity"]
        );
        assert_eq!(events[0]["result_identity"], events[1]["result_identity"]);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.identity_match"
        }));
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                || diagnostic["diagnostic"]["code"] == "event_equivalence.result_identity_mismatch"
        }));
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_event_probe_warns_when_text_and_text_edit_shape_diverge() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(149);
        let physical_handle = client
            .submit(physical_text_message(
                "runtime-text-vs-edit-divergence",
                &shared_frame,
                "abc",
            ))
            .expect("runtime MCP physical text request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical text returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical text response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input =
            backend_presented_physical_text_edit_for_frame(&runtime, shared_frame, "abc");
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-text-vs-edit-divergence-probe",
                &frame(150),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        assert_eq!(response, transport_response);

        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0]["dispatch_evidence"]["generated_event"]["text"]["text"],
            "abc"
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["generated_event"]["text_edit"]["kind"],
            "ReplaceSelection"
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                && diagnostic["diagnostic"]["severity"] == "Warning"
        }));
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_event_probe_warns_when_text_content_identity_diverges() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(49);
        let physical_handle = client
            .submit(physical_text_message(
                "runtime-text-divergence",
                &shared_frame,
                "abc",
            ))
            .expect("runtime MCP physical text request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical text returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical text response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input =
            backend_presented_physical_text_for_frame(&runtime, shared_frame, "xyz");
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-text-divergence-probe",
                &frame(50),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        assert_eq!(response, transport_response);

        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_ne!(
            events[0]["dispatch_identity"], events[1]["dispatch_identity"],
            "different text input must remain visible in dispatch identity"
        );
        assert_eq!(
            events[0]["dispatch_evidence"]["generated_event"]["text"]["text"],
            "abc"
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["generated_event"]["text"]["text"],
            "xyz"
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                && diagnostic["diagnostic"]["severity"] == "Warning"
        }));
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_mcp_and_backend_physical_keyboard_share_event_identity() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(51);
        let physical_handle = client
            .submit(physical_keyboard_message(
                "runtime-keyboard-equivalence",
                &shared_frame,
                "Enter",
            ))
            .expect("runtime MCP physical keyboard request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical keyboard returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical keyboard response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input =
            backend_presented_physical_keyboard_for_frame(&runtime, shared_frame, "Enter");
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-keyboard-equivalence-probe",
                &frame(52),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        assert_eq!(response, transport_response);

        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["dispatch_evidence"]["kind"], "keyboard");
        assert_eq!(events[1]["dispatch_evidence"]["kind"], "keyboard");
        assert_eq!(
            events[0]["dispatch_identity"],
            events[1]["dispatch_identity"]
        );
        assert_eq!(events[0]["result_identity"], events[1]["result_identity"]);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.identity_match"
        }));
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                || diagnostic["diagnostic"]["code"] == "event_equivalence.result_identity_mismatch"
        }));
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_event_probe_warns_when_keyboard_key_identity_diverges() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(53);
        let physical_handle = client
            .submit(physical_keyboard_message(
                "runtime-keyboard-divergence",
                &shared_frame,
                "Enter",
            ))
            .expect("runtime MCP physical keyboard request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical keyboard returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical keyboard response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input =
            backend_presented_physical_keyboard_for_frame(&runtime, shared_frame, "Escape");
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-keyboard-divergence-probe",
                &frame(54),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        assert_eq!(response, transport_response);

        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_ne!(
            events[0]["dispatch_identity"], events[1]["dispatch_identity"],
            "different keyboard key must remain visible in dispatch identity"
        );
        assert_eq!(
            events[0]["dispatch_evidence"]["generated_event"]["keyboard"]["key"],
            "Enter"
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["generated_event"]["keyboard"]["key"],
            "Escape"
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                && diagnostic["diagnostic"]["severity"] == "Warning"
        }));
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_mcp_and_backend_physical_command_share_event_identity() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(55);
        let physical_handle = client
            .submit(physical_command_message(
                "runtime-command-equivalence",
                &shared_frame,
                "submit",
                "payload-1",
            ))
            .expect("runtime MCP physical command request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical command returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical command response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input = backend_presented_physical_command_for_frame(
            &runtime,
            shared_frame,
            "submit",
            "payload-1",
        );
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-command-equivalence-probe",
                &frame(56),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        assert_eq!(response, transport_response);

        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0]["dispatch_evidence"]["kind"], "command");
        assert_eq!(events[1]["dispatch_evidence"]["kind"], "command");
        assert_eq!(
            events[0]["dispatch_identity"],
            events[1]["dispatch_identity"]
        );
        assert_eq!(events[0]["result_identity"], events[1]["result_identity"]);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.identity_match"
        }));
        assert!(!diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                || diagnostic["diagnostic"]["code"] == "event_equivalence.result_identity_mismatch"
        }));
    }

    #[test]
    #[ignore = "obsolete: MCP physical_control must not synthesize backend-presented input inside runtime"]
    fn runtime_event_probe_warns_when_command_identity_diverges() {
        let mut runtime = SlipwayRuntime::new(TextPhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let shared_frame = frame(57);
        let physical_handle = client
            .submit(physical_command_message(
                "runtime-command-divergence",
                &shared_frame,
                "submit",
                "payload-1",
            ))
            .expect("runtime MCP physical command request queued");

        let physical_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical command returns response");
        let physical_transport_response = physical_handle
            .recv()
            .expect("transport response arrives")
            .expect("physical command response sent");
        assert_eq!(physical_response, physical_transport_response);

        let backend_input = backend_presented_physical_command_for_frame(
            &runtime,
            shared_frame,
            "cancel",
            "payload-1",
        );
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let probe_handle = client
            .submit(probe_event_message(
                "runtime-command-divergence-probe",
                &frame(58),
            ))
            .expect("runtime MCP event probe queued");
        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = probe_handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");
        assert_eq!(response, transport_response);

        let payload = response_tool_payload(&transport_response);
        let products = payload["product"]["products"]
            .as_array()
            .expect("probe products array");
        let events = products
            .iter()
            .filter(|product| product["kind"] == "event")
            .collect::<Vec<_>>();
        let diagnostics = products
            .iter()
            .filter(|product| product["kind"] == "diagnostic")
            .collect::<Vec<_>>();

        assert_eq!(events.len(), 2);
        assert_ne!(
            events[0]["dispatch_identity"], events[1]["dispatch_identity"],
            "different command must remain visible in dispatch identity"
        );
        assert_eq!(
            events[0]["dispatch_evidence"]["generated_event"]["command"]["command"],
            "submit"
        );
        assert_eq!(
            events[1]["dispatch_evidence"]["generated_event"]["command"]["command"],
            "cancel"
        );
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic["diagnostic"]["code"] == "event_equivalence.dispatch_identity_mismatch"
                && diagnostic["diagnostic"]["severity"] == "Warning"
        }));
    }

    #[test]
    #[ignore = "obsolete: runtime-level unresolved physical_control is now a native-backend-required refusal"]
    fn runtime_mcp_unresolved_physical_control_returns_refusal_evidence_json() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(unmatched_physical_pointer_message(
                "runtime-physical-miss",
                &frame(34),
            ))
            .expect("runtime MCP physical miss request queued");

        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical miss returns response");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("physical miss response sent");

        assert_eq!(runtime.local_state().presses, 0);
        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["tool"], "slipway.debug.physical_control");
        assert_eq!(payload["product_kind"], "error");
        assert_eq!(payload["product"]["code"], "physical-control-unresolved");
        assert_eq!(
            payload["product"]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_DEBUG_MCP
        );
        assert_eq!(payload["product"]["dispatch_evidence"]["kind"], "pointer");
        assert!(payload["product"]["dispatch_evidence"]["selected_region"].is_null());
        assert!(payload["product"]["dispatch_evidence"]["generated_event"].is_null());
        assert!(
            payload["product"]["dispatch_evidence"]["refusal_reason"]
                .as_str()
                .expect("refusal reason")
                .contains("no enabled hit region")
        );
    }

    #[test]
    fn runtime_mcp_event_probe_returns_backend_dispatch_evidence_json() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        let backend_input = backend_presented_physical_press(&runtime);
        let outcome = runtime.apply_backend_input_event(backend_input);
        assert!(outcome.handled);

        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(probe_event_message("runtime-event-probe", &frame(37)))
            .expect("runtime MCP event probe queued");

        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("event probe returns response");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("event probe response sent");

        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["tool"], "slipway.debug.probe");
        assert_eq!(payload["product_kind"], "probes");
        assert_eq!(payload["product"]["products"][0]["kind"], "event");
        assert_eq!(
            payload["product"]["products"][0]["dispatch_evidence"]["source"]["label"],
            slipway_core::EVIDENCE_SOURCE_BACKEND_PRESENTED
        );
        assert_eq!(
            payload["product"]["products"][0]["dispatch_evidence"]["selected_region"],
            "probe-hit"
        );
        assert_eq!(
            payload["product"]["products"][0]["dispatch_evidence"]["generated_event"]["target"],
            "physical-probe"
        );
        assert_eq!(payload["product"]["products"][0]["handled"], true);
        assert!(
            payload["product"]["products"][0]["revision_after"]
                .as_u64()
                .expect("revision after")
                > payload["product"]["products"][0]["revision_before"]
                    .as_u64()
                    .expect("revision before")
        );
        assert_eq!(
            payload["product"]["products"][0]["emitted_messages"][0]["target"],
            "physical-probe"
        );
        assert_eq!(
            payload["product"]["products"][0]["emitted_messages"][0]["name"],
            "press"
        );
    }

    #[test]
    fn runtime_mcp_current_frame_resolves_from_live_runtime_viewport() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());
        runtime.record_presented_viewport(Rect {
            origin: Point { x: 10.0, y: 12.0 },
            size: Size {
                width: 640.0,
                height: 360.0,
            },
        });
        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(current_status_message("runtime-current"))
            .expect("runtime MCP request queued");

        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("status returns response");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("status response sent");

        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["frame"]["viewport"]["origin"]["x"], 10.0);
        assert_eq!(payload["frame"]["viewport"]["origin"]["y"], 12.0);
        assert_eq!(payload["frame"]["viewport"]["size"]["width"], 640.0);
        assert_eq!(payload["frame"]["viewport"]["size"]["height"], 360.0);
    }

    #[test]
    #[ignore = "obsolete: runtime physical_control current-frame success belongs to native backend injection"]
    fn runtime_mcp_physical_control_current_frame_uses_live_viewport() {
        let mut runtime = SlipwayRuntime::new(PhysicalProbeWidget, ());
        runtime.record_presented_viewport(Rect {
            origin: Point { x: 0.0, y: 0.0 },
            size: Size {
                width: 640.0,
                height: 360.0,
            },
        });
        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(current_physical_pointer_message(
                "runtime-physical-current",
                4.0,
                4.0,
            ))
            .expect("runtime MCP physical request queued");

        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("physical control returns response");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("physical control response sent");

        assert_eq!(response, transport_response);
        assert_eq!(runtime.local_state().presses, 1);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["product_kind"], "control_trace");
        assert_eq!(
            payload["product"]["dispatch_evidence"]["selected_region"],
            "probe-hit"
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["input_position"]["x"],
            4.0
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["input_position"]["y"],
            4.0
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["frame"]["viewport"]["origin"]["x"],
            0.0
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["frame"]["viewport"]["origin"]["y"],
            0.0
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["frame"]["viewport"]["size"]["width"],
            640.0
        );
        assert_eq!(
            payload["product"]["dispatch_evidence"]["frame"]["viewport"]["size"]["height"],
            360.0
        );
    }

    #[test]
    fn runtime_mcp_screenshot_alias_resolves_current_frame_and_renders() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());
        runtime.record_presented_viewport(Rect {
            origin: Point { x: 14.0, y: 16.0 },
            size: Size {
                width: 512.0,
                height: 288.0,
            },
        });
        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(current_screenshot_message("runtime-screenshot-current"))
            .expect("runtime MCP request queued");

        let response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("screenshot returns response");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("screenshot response sent");

        assert_eq!(response, transport_response);
        assert_eq!(runtime.debug_render_calls(), 1);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["tool"], "slipway.debug.screenshot");
        assert_eq!(payload["bridge_method"], "render");
        assert_eq!(payload["product_kind"], "render_evidence");
        assert_eq!(payload["frame"]["viewport"]["origin"]["x"], 14.0);
        assert_eq!(payload["frame"]["viewport"]["origin"]["y"], 16.0);
        assert_eq!(payload["frame"]["viewport"]["size"]["width"], 512.0);
        assert_eq!(payload["frame"]["viewport"]["size"]["height"], 288.0);
        assert_eq!(payload["product"]["frame"], payload["frame"]);
        assert_eq!(payload["product"]["width"], 512);
        assert_eq!(payload["product"]["height"], 288);
        assert!(
            payload["product"]["artifact_ref"]
                .as_str()
                .expect("screenshot artifact ref")
                .len()
                > 0
        );
        let artifact_path = payload["product"]["artifact_path"]
            .as_str()
            .expect("screenshot artifact path");
        assert!(artifact_path.ends_with(".png"));
        assert!(
            Path::new(artifact_path).is_file(),
            "screenshot artifact path should exist: {artifact_path}"
        );
    }

    #[test]
    fn runtime_mcp_control_trace_applies_app_reducer() {
        let mut runtime = SlipwayRuntime::from_app(test_app(), TestExternal);
        let client = runtime.runtime_mcp_client_clone();
        let handle = client
            .submit(traced_app_control_message(
                "runtime-app-trace",
                &frame(31),
                "beta-child",
            ))
            .expect("runtime MCP request queued");
        let mut reduced_messages = 0usize;

        let response = runtime
            .drain_runtime_mcp_once_with_app_reducer(
                &mut |_external: &mut TestExternal, messages: Vec<TestMessage>| {
                    reduced_messages += messages.len();
                },
            )
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("control trace returns response");
        let transport_response = handle
            .recv()
            .expect("transport response arrives")
            .expect("control trace response sent");

        assert_eq!(runtime.local_state().widgets.1.count, 30);
        assert_eq!(reduced_messages, 1);
        assert_eq!(response, transport_response);
        let payload = response_tool_payload(&transport_response);
        assert_eq!(payload["tool"], "slipway.debug.control");
        assert_eq!(payload["bridge_method"], "control");
        assert_eq!(payload["product_kind"], "control_trace");
        assert_eq!(payload["frame"], payload["product"]["frame"]);
        assert_eq!(payload["product"]["request_id"], "runtime-app-trace");
        assert_eq!(payload["product"]["routed_event_target"], "beta-child");
        assert_eq!(payload["product"]["event_summary"], "command:increment");
        assert_eq!(payload["product"]["handled"], true);
        assert_eq!(payload["product"]["revision_before"], 1);
        assert_eq!(payload["product"]["revision_after"], 3);
        let stages = payload["product"]["stages"]
            .as_array()
            .expect("trace stages");
        assert_eq!(
            stages
                .iter()
                .map(|stage| stage["stage"].as_str().expect("stage name"))
                .collect::<Vec<_>>(),
            vec!["generated", "routed", "consumed", "reduced"]
        );
        assert_eq!(stages[0]["target"], "beta-child");
        assert_eq!(stages[1]["target"], "beta-child");
        assert_eq!(stages[2]["target"], "beta-child");
        assert_eq!(stages[3]["actor"], "slipway-app-reducer");
        assert_eq!(stages[3]["target"], "beta-child");
        let messages = payload["product"]["messages"]
            .as_array()
            .expect("trace messages");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["source"], "beta-child");
        assert_eq!(messages[0]["name"], "increment");
        assert_eq!(messages[0]["disposition"], "Consumed");
    }

    #[test]
    fn denied_runtime_mcp_status_control_and_screenshot_do_not_touch_live_state() {
        let mut runtime =
            SlipwayRuntime::with_config(ProbeWidget, (), SlipwayRuntimeConfig::no_debug());
        assert_eq!(runtime.debug_mcp_server().config(), &no_debug_mcp_config());
        let client = runtime.runtime_mcp_client_clone();

        let status_handle = client
            .submit(current_status_message("denied-status"))
            .expect("denied status request queued");
        let status_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("status denial returns response");
        let transport_status_response = status_handle
            .recv()
            .expect("transport response arrives")
            .expect("status denial response sent");

        assert_eq!(status_response, transport_status_response);
        assert_eq!(runtime.local_state().count, 0);
        assert_eq!(runtime.debug_render_calls(), 0);
        let status_payload = response_tool_payload(&transport_status_response);
        assert_eq!(status_payload["tool"], "slipway.debug.status");
        assert_eq!(status_payload["bridge_method"], "status");
        assert_eq!(status_payload["admitted"], false);
        assert_eq!(status_payload["refused"], true);
        assert_eq!(status_payload["product_kind"], "refusal");
        assert_eq!(status_payload["refusal"]["code"], "status-denied");

        let control_handle = client
            .submit(control_message("denied-control", &frame(33)))
            .expect("denied control request queued");
        let control_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("control denial returns response");
        let transport_control_response = control_handle
            .recv()
            .expect("transport response arrives")
            .expect("control denial response sent");

        assert_eq!(control_response, transport_control_response);
        assert_eq!(runtime.local_state().count, 0);
        assert_eq!(runtime.debug_render_calls(), 0);
        let control_payload = response_tool_payload(&transport_control_response);
        assert_eq!(control_payload["tool"], "slipway.debug.control");
        assert_eq!(control_payload["admitted"], false);
        assert_eq!(control_payload["refused"], true);
        assert_eq!(control_payload["product_kind"], "refusal");
        assert_eq!(control_payload["refusal"]["code"], "control-denied");

        let screenshot_handle = client
            .submit(current_screenshot_message("denied-screenshot"))
            .expect("denied screenshot request queued");
        let screenshot_response = runtime
            .drain_runtime_mcp_once()
            .expect("runtime MCP drain succeeds")
            .expect("request drained")
            .expect("screenshot denial returns response");
        let transport_screenshot_response = screenshot_handle
            .recv()
            .expect("transport response arrives")
            .expect("screenshot denial response sent");

        assert_eq!(screenshot_response, transport_screenshot_response);
        assert_eq!(runtime.local_state().count, 0);
        assert_eq!(runtime.debug_render_calls(), 0);
        let screenshot_payload = response_tool_payload(&transport_screenshot_response);
        assert_eq!(screenshot_payload["tool"], "slipway.debug.screenshot");
        assert_eq!(screenshot_payload["bridge_method"], "render");
        assert_eq!(screenshot_payload["admitted"], false);
        assert_eq!(screenshot_payload["refused"], true);
        assert_eq!(screenshot_payload["product_kind"], "refusal");
        assert_eq!(screenshot_payload["refusal"]["code"], "render-denied");
    }

    #[test]
    fn runtime_tcp_transport_wakes_and_mutates_same_runtime() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());
        let transport = runtime
            .start_debug_mcp_transport()
            .expect("runtime TCP transport starts");

        assert!(transport.local_addr().ip().is_loopback());
        assert_ne!(transport.local_addr().port(), 0);

        let mut stream =
            TcpStream::connect(transport.local_addr()).expect("connect to runtime MCP transport");
        stream
            .set_read_timeout(Some(Duration::from_secs(2)))
            .expect("set read timeout");
        writeln!(stream, "{}", control_message("tcp-control", &frame(32)))
            .expect("write JSON-RPC line");
        stream.flush().expect("flush JSON-RPC line");

        assert!(transport.wake_receiver().recv());
        let responses = runtime
            .drain_runtime_mcp_pending()
            .expect("runtime MCP drain succeeds");

        assert_eq!(runtime.local_state().count, 1);
        assert_eq!(responses.len(), 1);

        let mut reader = BufReader::new(stream);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .expect("read JSON-RPC response line");
        let response: Value = serde_json::from_str(response_line.trim()).expect("response is JSON");
        assert_eq!(response, responses[0]);
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        assert!(payload.contains(r#""product_kind":"diagnostics""#));
    }

    #[test]
    fn debug_bridge_budgeted_drain_stops_after_limit() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());
        let first = runtime
            .bridge_client_clone()
            .submit(DebugCommand::status("budget-status-1", frame(40)))
            .expect("first debug command queued");
        let second = runtime
            .bridge_client_clone()
            .submit(DebugCommand::status("budget-status-2", frame(41)))
            .expect("second debug command queued");

        let replies = runtime
            .drain_debug_pending_budgeted(1)
            .expect("budgeted drain succeeds");

        assert_eq!(replies.len(), 1);
        assert!(
            first
                .try_recv()
                .expect("first reply channel readable")
                .is_some()
        );
        assert!(
            second
                .try_recv()
                .expect("second reply channel readable")
                .is_none()
        );

        let replies = runtime
            .drain_debug_pending_budgeted(1)
            .expect("second budgeted drain succeeds");
        assert_eq!(replies.len(), 1);
        assert!(
            second
                .try_recv()
                .expect("second reply channel readable")
                .is_some()
        );
    }

    #[test]
    fn runtime_mcp_budgeted_drain_stops_after_limit() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());
        let client = runtime.runtime_mcp_client_clone();
        let first = client
            .submit(control_message("budget-mcp-1", &frame(42)))
            .expect("first runtime MCP request queued");
        let second = client
            .submit(control_message("budget-mcp-2", &frame(43)))
            .expect("second runtime MCP request queued");
        let mut budget = SlipwayRuntimeDrainBudget::default();
        budget.debug_bridge = 0;
        budget.runtime_mcp = 1;
        budget.mcp_pending_debug_bridge = 1;

        let report = runtime
            .drain_live_debug_turn_with_app_reducer(budget, &mut |_external, _messages| {})
            .expect("budgeted UI turn drain succeeds");

        assert_eq!(
            report,
            SlipwayRuntimeDrainReport {
                debug_replies_drained: 0,
                runtime_mcp_replies_drained: 1,
            }
        );
        assert_eq!(runtime.local_state().count, 1);
        assert!(
            first
                .try_recv()
                .expect("first runtime response readable")
                .is_some()
        );
        assert!(
            second
                .try_recv()
                .expect("second runtime response readable")
                .is_none()
        );

        let report = runtime
            .drain_live_debug_turn_with_app_reducer(budget, &mut |_external, _messages| {})
            .expect("second budgeted UI turn drain succeeds");
        assert_eq!(report.runtime_mcp_replies_drained, 1);
        assert_eq!(runtime.local_state().count, 2);
        assert!(
            second
                .try_recv()
                .expect("second runtime response readable")
                .is_some()
        );
    }

    #[test]
    fn untraced_debug_control_remains_diagnostics_only() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());

        let product = runtime.handle_debug_command(DebugCommand::control(
            "control",
            frame(20),
            count_probe(),
        ));

        assert_eq!(runtime.local_state().count, 1);
        match product {
            DebugReplyProduct::Diagnostics(diagnostics) => {
                assert!(diagnostics.is_empty());
            }
            other => panic!("expected diagnostics-only control product, got {other:?}"),
        }
    }

    #[test]
    fn traced_generic_debug_control_marks_messages_reduction_unavailable() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());

        let product = runtime.handle_debug_command(DebugCommand::control_with_trace(
            "generic-trace",
            frame(21),
            count_probe(),
        ));

        assert_eq!(runtime.local_state().count, 1);
        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("expected control trace");
        };
        assert_eq!(trace.request_id, "generic-trace");
        assert_eq!(trace.routed_event_target, WidgetId::from("probe"));
        assert_eq!(trace.event_summary, "command:count");
        assert!(trace.handled);
        assert_eq!(trace.revision_before, 1);
        assert_eq!(trace.revision_after, 2);
        assert!(trace.diagnostics.is_empty());
        assert_eq!(trace.stages.len(), 4);
        assert_eq!(
            trace.stages[0].stage,
            slipway_debug_bridge::DebugControlTraceStageKind::Generated
        );
        assert_eq!(
            trace.stages[1].stage,
            slipway_debug_bridge::DebugControlTraceStageKind::Routed
        );
        assert_eq!(
            trace.stages[2].stage,
            slipway_debug_bridge::DebugControlTraceStageKind::Consumed
        );
        assert_eq!(
            trace.stages[3].stage,
            slipway_debug_bridge::DebugControlTraceStageKind::Reduced
        );
        assert_eq!(trace.stages[3].actor, "slipway-app-reducer");
        assert!(trace.stages[3].detail.contains("app reducer unavailable"));
        assert_eq!(trace.messages.len(), 1);
        assert_eq!(trace.messages[0].source, WidgetId::from("probe"));
        assert_eq!(trace.messages[0].name, "count");
        assert_eq!(
            trace.messages[0].disposition,
            MessageDisposition::ReductionUnavailable
        );
    }

    #[test]
    fn traced_debug_control_records_ignored_stage_when_widget_ignores_event() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());

        let product = runtime.handle_debug_command(DebugCommand::control_with_trace(
            "ignored-trace",
            frame(24),
            ignored_probe(),
        ));

        assert_eq!(runtime.local_state().count, 0);
        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("expected control trace");
        };
        assert_eq!(trace.request_id, "ignored-trace");
        assert_eq!(trace.routed_event_target, WidgetId::from("probe"));
        assert_eq!(trace.event_summary, "command:ignored");
        assert!(!trace.handled);
        assert_eq!(trace.revision_before, 1);
        assert_eq!(trace.revision_after, 1);
        assert_eq!(trace.messages.len(), 0);
        assert_eq!(trace.stages.len(), 4);
        assert_eq!(
            trace.stages[2].stage,
            slipway_debug_bridge::DebugControlTraceStageKind::Ignored
        );
        assert_eq!(
            trace.stages[3].stage,
            slipway_debug_bridge::DebugControlTraceStageKind::Reduced
        );
        assert!(trace.stages[3].detail.contains("no emitted app messages"));
        assert!(trace.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "event_declaration.handler_ignored_declared_handled"
        }));
    }

    #[test]
    fn traced_app_debug_control_marks_messages_consumed_when_reducer_runs() {
        let mut runtime = SlipwayRuntime::from_app(test_app(), TestExternal);
        let mut reduced_messages = 0usize;

        let product = runtime.handle_debug_command_with_app_reducer(
            DebugCommand::control_with_trace(
                "app-trace",
                frame(22),
                increment_child("alpha-child"),
            ),
            &mut |_external: &mut TestExternal, messages: Vec<TestMessage>| {
                reduced_messages += messages.len();
            },
        );

        assert_eq!(runtime.local_state().widgets.0.count, 4);
        assert_eq!(runtime.local_state().widgets.1.count, 20);
        assert_eq!(reduced_messages, 1);
        let DebugReplyProduct::ControlTrace(trace) = product else {
            panic!("expected control trace");
        };
        assert_eq!(trace.request_id, "app-trace");
        assert_eq!(trace.routed_event_target, WidgetId::from("alpha-child"));
        assert_eq!(trace.revision_before, 1);
        assert_eq!(trace.revision_after, 3);
        assert_eq!(trace.stages.len(), 4);
        assert_eq!(
            trace.stages[2].stage,
            slipway_debug_bridge::DebugControlTraceStageKind::Consumed
        );
        assert_eq!(
            trace.stages[3].stage,
            slipway_debug_bridge::DebugControlTraceStageKind::Reduced
        );
        assert!(
            trace.stages[3]
                .detail
                .contains("app reducer reduced 1 emitted message")
        );
        assert_eq!(trace.messages.len(), 1);
        assert_eq!(trace.messages[0].source, WidgetId::from("alpha-child"));
        assert_eq!(trace.messages[0].name, "increment");
        assert_eq!(trace.messages[0].disposition, MessageDisposition::Consumed);
    }

    #[test]
    fn bridged_traced_app_control_can_drain_with_reducer_hook() {
        let mut runtime = SlipwayRuntime::from_app(test_app(), TestExternal);
        let handle = runtime
            .bridge_client_clone()
            .submit(DebugCommand::control_with_trace(
                "bridge-app-trace",
                frame(23),
                increment_child("beta-child"),
            ))
            .expect("trace command queued");
        let mut reduced_messages = 0usize;

        runtime
            .drain_debug_once_with_app_reducer(
                &mut |_external: &mut TestExternal, messages: Vec<TestMessage>| {
                    reduced_messages += messages.len();
                },
            )
            .expect("drain succeeds")
            .expect("reply produced");
        let reply = handle
            .try_recv()
            .expect("reply channel readable")
            .expect("reply available");

        assert_eq!(runtime.local_state().widgets.1.count, 30);
        assert_eq!(reduced_messages, 1);
        let DebugReplyProduct::ControlTrace(trace) = reply.product else {
            panic!("expected control trace reply");
        };
        assert_eq!(trace.request_id, "bridge-app-trace");
        assert_eq!(trace.messages[0].disposition, MessageDisposition::Consumed);
        assert_eq!(trace.revision_after, 3);
    }

    #[test]
    fn ordinary_runtime_input_event_does_not_increment_debug_render_calls() {
        let mut runtime = SlipwayRuntime::new(ProbeWidget, ());
        assert_eq!(runtime.debug_render_calls(), 0);

        runtime.apply_input_event(InputEvent::Command(CommandEvent {
            target: WidgetId::from("probe"),
            target_slot: None,
            command: "count".to_string(),
            payload_ref: None,
            source: None,
        }));
        assert_eq!(runtime.debug_render_calls(), 0);
    }

    #[test]
    fn runtime_config_exposes_platform_ime_allow_policy() {
        let config = SlipwayRuntimeConfig::admitted_debug().with_platform_ime_always_allowed();
        assert_eq!(config.ime_policy, SlipwayImePolicy::AlwaysAllowed);
        assert!(config.ime_policy.keeps_platform_ime_allowed());

        let config = SlipwayRuntimeConfig::admitted_debug()
            .with_ime_policy(SlipwayImePolicy::BackendRequested);
        assert_eq!(config.ime_policy, SlipwayImePolicy::BackendRequested);
        assert!(!config.ime_policy.keeps_platform_ime_allowed());
    }

    #[test]
    fn render_request_through_attachment_returns_real_evidence() {
        let mut app = SlipwayAssembledApp::new(ProbeWidget, ());
        let pending = begin_pending(&app.debug_mcp, forged_render_message("render", &frame(2)));

        app.runtime
            .drain_debug_once()
            .expect("drain succeeds")
            .expect("reply produced");
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("render response");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");

        assert_eq!(app.runtime.debug_render_calls(), 1);
        assert!(payload.contains(r#""product_kind":"render_evidence""#));
        assert!(payload.contains(r#""label":"canonical_offscreen""#));
        assert!(payload.contains(r#""provider_id":"slipway-debug-renderer.cpu.v1""#));
        assert!(!payload.contains("backend_presented"));
        assert!(payload.contains(r#""artifact_ref":"#));
        assert!(payload.contains(r#""pixel_hash":"#));
    }

    #[test]
    fn mcp_supplied_render_packet_paint_is_ignored_and_rebuilt_from_runtime_state() {
        let mut app = SlipwayAssembledApp::new(ProbeWidget, ());
        app.runtime
            .apply_input_event(InputEvent::Command(CommandEvent {
                target: WidgetId::from("probe"),
                target_slot: None,
                command: "count".to_string(),
                payload_ref: None,
                source: None,
            }));

        let pending = begin_pending(
            &app.debug_mcp,
            forged_render_message("forged-render", &frame(3)),
        );
        app.runtime
            .drain_debug_once()
            .expect("drain succeeds")
            .expect("reply produced");
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("render response");
        let payload = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");

        assert!(payload.contains(r#""target":"probe""#));
        assert!(!payload.contains("forged-mcp-target"));
        assert!(payload.contains(r#""product_kind":"render_evidence""#));
        assert!(payload.contains(r#""label":"canonical_offscreen""#));
        assert!(!payload.contains("backend_presented"));
    }

    #[test]
    fn from_app_initializes_app_and_child_local_state() {
        let runtime: SlipwayAppRuntime<TestApp> =
            SlipwayRuntime::from_app(test_app(), TestExternal);
        let local = runtime.local_state();

        assert_eq!(local.app, TestAppLocal { initialized: true });
        assert_eq!(local.widgets.0, AlphaLocal { count: 3 });
        assert_eq!(local.widgets.1, BetaLocal { count: 20 });
    }

    #[test]
    fn targeted_control_input_mutates_child_through_app_widget() {
        let mut runtime = SlipwayRuntime::from_app(test_app(), TestExternal);

        let outcome = runtime.apply_input_event(increment_child("beta-child"));

        assert!(outcome.handled);
        assert_eq!(runtime.local_state().widgets.0.count, 3);
        assert_eq!(runtime.local_state().widgets.1.count, 30);
    }

    #[test]
    fn debug_probe_sees_app_topology_and_child_state() {
        let mut runtime = SlipwayRuntime::from_app(test_app(), TestExternal);
        runtime.apply_input_event(increment_child("alpha-child"));
        runtime.apply_input_event(increment_child("beta-child"));

        let product = runtime.handle_debug_command(DebugCommand::probe(
            "app-probe",
            frame(4),
            ProbeRequest {
                target: None,
                kinds: vec![ProbeKind::Topology, ProbeKind::State],
                event_trace_limit: None,
            },
        ));

        let DebugReplyProduct::Probes(products) = product else {
            panic!("expected probe products");
        };
        let topology = products
            .iter()
            .find_map(|product| match product {
                ProbeProduct::Topology(topology) => Some(topology),
                _ => None,
            })
            .expect("topology probe");
        let state = products
            .iter()
            .find_map(|product| match product {
                ProbeProduct::State(state) => Some(state),
                _ => None,
            })
            .expect("state probe");

        assert_eq!(topology.root.id, WidgetId::from("test-app"));
        assert_eq!(topology.root.children.len(), 2);
        assert_eq!(topology.root.children[0].id, WidgetId::from("alpha-child"));
        assert_eq!(topology.root.children[1].id, WidgetId::from("beta-child"));
        assert!(state.observations.contains(&StateObservation {
            target: WidgetId::from("alpha-child"),
            slot: topology.root.children[0].local_state_slot.clone(),
            name: "count".to_string(),
            value: "4".to_string(),
        }));
        assert!(state.observations.contains(&StateObservation {
            target: WidgetId::from("beta-child"),
            slot: topology.root.children[1].local_state_slot.clone(),
            name: "count".to_string(),
            value: "30".to_string(),
        }));
    }

    #[test]
    fn app_runtime_render_packet_uses_runtime_owned_state() {
        let mut runtime = SlipwayRuntime::from_app(test_app(), TestExternal);
        runtime.apply_input_event(increment_child("beta-child"));

        let packet = runtime.render_packet_for_frame(frame(5));
        let painted_text: Vec<_> = packet
            .paint
            .iter()
            .filter_map(|op| match op {
                PaintOp::Text { content, .. } => Some(content.as_str()),
                _ => None,
            })
            .collect();

        assert_eq!(packet.target, WidgetId::from("test-app"));
        assert_eq!(packet.layout.child_placements.len(), 2);
        assert!(painted_text.contains(&"app-initialized:true"));
        assert!(painted_text.contains(&"alpha:3"));
        assert!(painted_text.contains(&"beta:30"));
    }
}
