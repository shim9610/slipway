use std::io::{self, BufRead, Write};

use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use serde_json::{Value, json};
use slipway_core::{
    CommandEvent, DeclaredEventDispatchEvidence, DeclaredEventDispatchKind, Diagnostic,
    EvidenceSource, FocusEvent, FrameIdentity, InputEvent, KeyEventKind, KeyLocation,
    KeyboardDetails, KeyboardEvent, LayoutOutput, Modifiers, Point, PointerButton, PointerButtons,
    PointerDetails, PointerDeviceKind, PointerEvent, PointerEventKind, PresentationRegionId,
    ProbeKind, ProbeProduct, ProbeRequest, Rect, RenderPacket, Size, TargetLocalRect,
    TextCompositionPhase, TextEditKind, TextInputEvent, TextSelectionRange, WheelEvent, WidgetId,
    WidgetSlotAddress,
};
use slipway_debug_bridge::{
    CompositionPhaseProvenance, DebugBridgeClient, DebugBridgeError, DebugCommand,
    DebugCompositionIngressObservation, DebugFailure, DebugPhysicalControl,
    DebugPhysicalControlDeclarationSelector, DebugReply, DebugReplyProduct, DebugRequestHandle,
    DebugStatus, DebugTextCompositionUpdate, McpProbeMethod, PresentedAlphaMode,
    PresentedCapturePath, PresentedScreenshotAdmission, PresentedScreenshotProduct,
    PresentedScreenshotRefusal, PresentedScreenshotRequest, PresentedScreenshotSelector,
    PresentedSurfaceFormat, PresentedTransferFunction, RenderProduct, SlipwayDebugCommandHandler,
    validate_presented_screenshot_product,
};
#[cfg(test)]
use slipway_debug_bridge::{PRESENTED_PIXELS_PASS_ID, PresentedPixels};
use slipway_debug_renderer::{DebugPngArtifactError, write_debug_rgba8_png_artifact};

const PROTOCOL_VERSION: &str = "2025-11-25";
const SERVER_NAME: &str = "slipway-debug-mcp";
const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

const TOOL_STATUS: &str = "slipway.debug.status";
const TOOL_PROBE: &str = "slipway.debug.probe";
const TOOL_RENDER: &str = "slipway.debug.render";
const TOOL_SCREENSHOT: &str = "slipway.debug.screenshot";
const TOOL_CONTROL: &str = "slipway.debug.control";
const TOOL_PHYSICAL_CONTROL: &str = "slipway.debug.physical_control";
const TOOL_RESIZE: &str = "slipway.debug.resize";
const EVENT_TRACE_LIMIT_FIELDS: &[&str] = &[
    "event_trace_limit",
    "eventTraceLimit",
    "event_limit",
    "eventLimit",
];
const PRESENTED_ARTIFACT_PROVIDER_ID: &str = "slipway-debug-renderer.presented.v1";
const MAX_COMPOSITION_UPDATES: usize = 16;
const MAX_COMPOSITION_UTF8_BYTES: usize = 65_536;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuntimeScreenshotSelector {
    Exact,
    Current,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScreenshotParseOrigin {
    Direct,
    RuntimeNormalized(RuntimeScreenshotSelector),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugMcpConfig {
    pub allow_status: bool,
    pub allow_probe: bool,
    pub allow_render: bool,
    pub allow_screenshot: bool,
    pub allow_control: bool,
    pub allow_resize: bool,
}

impl DebugMcpConfig {
    pub fn no_debug() -> Self {
        Self::default()
    }

    pub fn admitted() -> Self {
        Self {
            allow_status: true,
            allow_probe: true,
            allow_render: true,
            allow_screenshot: true,
            allow_control: true,
            allow_resize: true,
        }
    }
}

impl Default for DebugMcpConfig {
    fn default() -> Self {
        Self {
            allow_status: false,
            allow_probe: false,
            allow_render: false,
            allow_screenshot: false,
            allow_control: false,
            allow_resize: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugMcpServer {
    config: DebugMcpConfig,
}

pub enum DebugMcpBridgeMessage {
    Immediate(Option<Value>),
    Pending(DebugMcpPendingToolCall),
}

pub struct DebugMcpPendingToolCall {
    id: Option<Value>,
    tool: String,
    method: McpProbeMethod,
    handle: DebugRequestHandle,
}

#[derive(Clone)]
pub struct DebugMcpRuntimeClient {
    request_tx: Sender<DebugMcpRuntimeRequest>,
}

pub struct DebugMcpRuntimeEndpoint {
    request_rx: Receiver<DebugMcpRuntimeRequest>,
}

pub struct DebugMcpRuntimeRequest {
    request: String,
    response_tx: Sender<DebugMcpResponseWork>,
}

pub struct DebugMcpRuntimeResponseHandle {
    response_rx: Receiver<DebugMcpResponseWork>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DebugMcpResponseWork {
    Ready(Option<Value>),
    PresentedScreenshot {
        rpc_id: Option<Value>,
        tool: String,
        method: McpProbeMethod,
        reply: DebugReply,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DebugMcpRuntimeTransportError {
    RequestQueueFull,
    RequestQueueDisconnected,
    ResponseQueueEmpty,
    ResponseQueueFull,
    ResponseQueueDisconnected,
}

pub fn bounded_runtime_mcp(capacity: usize) -> (DebugMcpRuntimeClient, DebugMcpRuntimeEndpoint) {
    let (request_tx, request_rx) = bounded(capacity.max(1));
    (
        DebugMcpRuntimeClient { request_tx },
        DebugMcpRuntimeEndpoint { request_rx },
    )
}

impl DebugMcpRuntimeClient {
    pub fn submit(
        &self,
        request: impl Into<String>,
    ) -> Result<DebugMcpRuntimeResponseHandle, DebugMcpRuntimeTransportError> {
        let (response_tx, response_rx) = bounded(1);
        let envelope = DebugMcpRuntimeRequest {
            request: request.into(),
            response_tx,
        };

        self.request_tx
            .try_send(envelope)
            .map_err(|error| match error {
                TrySendError::Full(_) => DebugMcpRuntimeTransportError::RequestQueueFull,
                TrySendError::Disconnected(_) => {
                    DebugMcpRuntimeTransportError::RequestQueueDisconnected
                }
            })?;

        Ok(DebugMcpRuntimeResponseHandle { response_rx })
    }
}

impl DebugMcpRuntimeEndpoint {
    pub fn try_recv(
        &self,
    ) -> Result<Option<DebugMcpRuntimeRequest>, DebugMcpRuntimeTransportError> {
        match self.request_rx.try_recv() {
            Ok(request) => Ok(Some(request)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err(DebugMcpRuntimeTransportError::RequestQueueDisconnected)
            }
        }
    }
}

impl DebugMcpRuntimeRequest {
    pub fn request(&self) -> &str {
        &self.request
    }

    pub fn into_request(self) -> String {
        self.request
    }

    pub fn respond(self, response: Option<Value>) -> Result<(), DebugMcpRuntimeTransportError> {
        self.respond_work(DebugMcpResponseWork::Ready(response))
    }

    pub fn respond_work(
        self,
        work: DebugMcpResponseWork,
    ) -> Result<(), DebugMcpRuntimeTransportError> {
        self.response_tx
            .try_send(work)
            .map_err(|error| match error {
                TrySendError::Full(_) => DebugMcpRuntimeTransportError::ResponseQueueFull,
                TrySendError::Disconnected(_) => {
                    DebugMcpRuntimeTransportError::ResponseQueueDisconnected
                }
            })
    }
}

impl DebugMcpRuntimeResponseHandle {
    pub fn try_recv(&self) -> Result<Option<Option<Value>>, DebugMcpRuntimeTransportError> {
        match self.response_rx.try_recv() {
            Ok(work) => Ok(Some(finalize_response_work(work))),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err(DebugMcpRuntimeTransportError::ResponseQueueDisconnected)
            }
        }
    }

    pub fn recv(&self) -> Result<Option<Value>, DebugMcpRuntimeTransportError> {
        self.response_rx
            .recv()
            .map(finalize_response_work)
            .map_err(|_| DebugMcpRuntimeTransportError::ResponseQueueDisconnected)
    }
}

impl DebugMcpPendingToolCall {
    pub fn id(&self) -> Option<&Value> {
        self.id.as_ref()
    }

    pub fn tool_name(&self) -> &str {
        &self.tool
    }

    pub fn method(&self) -> McpProbeMethod {
        self.method
    }

    pub fn request_id(&self) -> &str {
        self.handle.request_id()
    }

    pub fn try_finish(&self) -> Result<Option<Value>, DebugBridgeError> {
        Ok(self.try_finish_work()?.and_then(finalize_response_work))
    }

    pub fn try_finish_work(&self) -> Result<Option<DebugMcpResponseWork>, DebugBridgeError> {
        let reply = match self.handle.try_recv()? {
            Some(reply) => reply,
            None => return Ok(None),
        };

        if self.method == McpProbeMethod::Screenshot {
            Ok(Some(DebugMcpResponseWork::PresentedScreenshot {
                rpc_id: self.id.clone(),
                tool: self.tool.clone(),
                method: self.method,
                reply,
            }))
        } else {
            Ok(Some(DebugMcpResponseWork::Ready(Some(json_rpc_result(
                self.id.clone(),
                tool_result(reply_payload(&self.tool, self.method, reply, true)),
            )))))
        }
    }
}

impl DebugMcpServer {
    pub fn new(config: DebugMcpConfig) -> Self {
        Self { config }
    }

    pub fn config(&self) -> &DebugMcpConfig {
        &self.config
    }

    pub fn handle_json_rpc<H>(
        &self,
        request: &str,
        handler: &mut H,
    ) -> Result<Option<Value>, serde_json::Error>
    where
        H: SlipwayDebugCommandHandler,
    {
        let message = serde_json::from_str::<Value>(request)?;
        Ok(self.handle_message(message, handler))
    }

    pub fn handle_message<H>(&self, message: Value, handler: &mut H) -> Option<Value>
    where
        H: SlipwayDebugCommandHandler,
    {
        self.handle_message_with_origin(message, ScreenshotParseOrigin::Direct, handler)
    }

    fn handle_message_with_origin<H>(
        &self,
        message: Value,
        origin: ScreenshotParseOrigin,
        handler: &mut H,
    ) -> Option<Value>
    where
        H: SlipwayDebugCommandHandler,
    {
        let id = message.get("id").cloned();
        let method = match message.get("method").and_then(Value::as_str) {
            Some(method) => method,
            None => return Some(json_rpc_error(id, -32600, "missing JSON-RPC method")),
        };

        match method {
            "notifications/initialized" => None,
            "initialize" => Some(json_rpc_result(
                id,
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "serverInfo": {
                        "name": SERVER_NAME,
                        "version": SERVER_VERSION,
                    },
                    "capabilities": {
                        "tools": {},
                    },
                }),
            )),
            "ping" => Some(json_rpc_result(id, json!({}))),
            "tools/list" => Some(json_rpc_result(id, json!({ "tools": default_tools() }))),
            "tools/call" => {
                Some(self.handle_tools_call(id, message.get("params"), origin, handler))
            }
            _ => Some(json_rpc_error(
                id,
                -32601,
                format!("unsupported JSON-RPC method `{method}`"),
            )),
        }
    }

    pub fn begin_bridge_message(
        &self,
        request: &str,
        bridge: &DebugBridgeClient,
    ) -> DebugMcpBridgeMessage {
        match serde_json::from_str::<Value>(request) {
            Ok(message) => self.begin_bridge_value(message, bridge),
            Err(error) => DebugMcpBridgeMessage::Immediate(Some(json_rpc_error(
                None,
                -32700,
                format!("invalid JSON-RPC message: {error}"),
            ))),
        }
    }

    pub fn begin_bridge_value(
        &self,
        message: Value,
        bridge: &DebugBridgeClient,
    ) -> DebugMcpBridgeMessage {
        self.begin_bridge_value_with_origin(message, ScreenshotParseOrigin::Direct, bridge)
    }

    pub fn begin_runtime_bridge_value(
        &self,
        message: Value,
        screenshot: Option<RuntimeScreenshotSelector>,
        bridge: &DebugBridgeClient,
    ) -> DebugMcpBridgeMessage {
        let origin = screenshot.map_or(ScreenshotParseOrigin::Direct, |selector| {
            ScreenshotParseOrigin::RuntimeNormalized(selector)
        });
        self.begin_bridge_value_with_origin(message, origin, bridge)
    }

    fn begin_bridge_value_with_origin(
        &self,
        message: Value,
        origin: ScreenshotParseOrigin,
        bridge: &DebugBridgeClient,
    ) -> DebugMcpBridgeMessage {
        let id = message.get("id").cloned();
        let method = match message.get("method").and_then(Value::as_str) {
            Some(method) => method,
            None => {
                return DebugMcpBridgeMessage::Immediate(Some(json_rpc_error(
                    id,
                    -32600,
                    "missing JSON-RPC method",
                )));
            }
        };

        match method {
            "notifications/initialized" => DebugMcpBridgeMessage::Immediate(None),
            "initialize" => DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "serverInfo": {
                        "name": SERVER_NAME,
                        "version": SERVER_VERSION,
                    },
                    "capabilities": {
                        "tools": {},
                    },
                }),
            ))),
            "ping" => DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(id, json!({})))),
            "tools/list" => DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                json!({ "tools": default_tools() }),
            ))),
            "tools/call" => self.begin_bridge_tools_call(id, message.get("params"), origin, bridge),
            _ => DebugMcpBridgeMessage::Immediate(Some(json_rpc_error(
                id,
                -32601,
                format!("unsupported JSON-RPC method `{method}`"),
            ))),
        }
    }

    fn handle_tools_call<H>(
        &self,
        id: Option<Value>,
        params: Option<&Value>,
        origin: ScreenshotParseOrigin,
        handler: &mut H,
    ) -> Value
    where
        H: SlipwayDebugCommandHandler,
    {
        let params = match params.and_then(Value::as_object) {
            Some(params) => params,
            None => return json_rpc_error(id, -32602, "tools/call params must be an object"),
        };
        let tool = match params.get("name").and_then(Value::as_str) {
            Some(name) => name,
            None => return json_rpc_error(id, -32602, "tools/call params.name must be a string"),
        };
        let arguments = params.get("arguments").unwrap_or(&Value::Null);
        let request_id = request_id_string(id.as_ref());

        let result = match tool {
            TOOL_STATUS => self.call_status(&request_id, arguments, handler),
            TOOL_PROBE => self.call_probe(&request_id, arguments, handler),
            TOOL_RENDER => self.call_render(&request_id, arguments, handler),
            TOOL_SCREENSHOT => self.call_screenshot(&request_id, arguments, origin, handler),
            TOOL_CONTROL => self.call_control(&request_id, arguments, handler),
            TOOL_PHYSICAL_CONTROL => self.call_physical_control(&request_id, arguments, handler),
            TOOL_RESIZE => self.call_resize(&request_id, arguments, handler),
            _ => Err(RpcError::invalid_params(format!(
                "unknown debug tool `{tool}`"
            ))),
        };

        match result {
            Ok(result) => json_rpc_result(id, result),
            Err(error) => json_rpc_error(id, error.code, error.message),
        }
    }

    fn begin_bridge_tools_call(
        &self,
        id: Option<Value>,
        params: Option<&Value>,
        origin: ScreenshotParseOrigin,
        bridge: &DebugBridgeClient,
    ) -> DebugMcpBridgeMessage {
        let params = match params.and_then(Value::as_object) {
            Some(params) => params,
            None => {
                return DebugMcpBridgeMessage::Immediate(Some(json_rpc_error(
                    id,
                    -32602,
                    "tools/call params must be an object",
                )));
            }
        };
        let tool = match params.get("name").and_then(Value::as_str) {
            Some(name) => name,
            None => {
                return DebugMcpBridgeMessage::Immediate(Some(json_rpc_error(
                    id,
                    -32602,
                    "tools/call params.name must be a string",
                )));
            }
        };
        let arguments = params.get("arguments").unwrap_or(&Value::Null);
        let request_id = request_id_string(id.as_ref());

        let pending = match tool {
            TOOL_STATUS => self.begin_bridge_status(id.clone(), &request_id, arguments, bridge),
            TOOL_PROBE => self.begin_bridge_probe(id.clone(), &request_id, arguments, bridge),
            TOOL_RENDER => self.begin_bridge_render(id.clone(), &request_id, arguments, bridge),
            TOOL_SCREENSHOT => {
                self.begin_bridge_screenshot(id.clone(), &request_id, arguments, origin, bridge)
            }
            TOOL_CONTROL => self.begin_bridge_control(id.clone(), &request_id, arguments, bridge),
            TOOL_PHYSICAL_CONTROL => {
                self.begin_bridge_physical_control(id.clone(), &request_id, arguments, bridge)
            }
            TOOL_RESIZE => self.begin_bridge_resize(id.clone(), &request_id, arguments, bridge),
            _ => {
                return DebugMcpBridgeMessage::Immediate(Some(json_rpc_error(
                    id,
                    -32602,
                    format!("unknown debug tool `{tool}`"),
                )));
            }
        };

        match pending {
            Ok(message) => message,
            Err(error) => DebugMcpBridgeMessage::Immediate(Some(json_rpc_error(
                id,
                error.code,
                error.message,
            ))),
        }
    }

    fn begin_bridge_status(
        &self,
        id: Option<Value>,
        request_id: &str,
        arguments: &Value,
        bridge: &DebugBridgeClient,
    ) -> Result<DebugMcpBridgeMessage, RpcError> {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_status {
            return Ok(DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                tool_result(refusal_payload(
                    TOOL_STATUS,
                    McpProbeMethod::Status,
                    request_id,
                    &frame,
                    "status-denied",
                    "debug status is not admitted by this server configuration",
                )),
            ))));
        }

        Ok(submit_bridge_tool(
            id,
            TOOL_STATUS,
            McpProbeMethod::Status,
            DebugCommand::status(request_id, frame),
            bridge,
        ))
    }

    fn begin_bridge_probe(
        &self,
        id: Option<Value>,
        request_id: &str,
        arguments: &Value,
        bridge: &DebugBridgeClient,
    ) -> Result<DebugMcpBridgeMessage, RpcError> {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_probe {
            return Ok(DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                tool_result(refusal_payload(
                    TOOL_PROBE,
                    McpProbeMethod::Probe,
                    request_id,
                    &frame,
                    "probe-denied",
                    "debug probe is not admitted by this server configuration",
                )),
            ))));
        }

        let request = parse_probe_request(arguments)?;
        Ok(submit_bridge_tool(
            id,
            TOOL_PROBE,
            McpProbeMethod::Probe,
            DebugCommand::probe(request_id, frame, request),
            bridge,
        ))
    }

    fn begin_bridge_render(
        &self,
        id: Option<Value>,
        request_id: &str,
        arguments: &Value,
        bridge: &DebugBridgeClient,
    ) -> Result<DebugMcpBridgeMessage, RpcError> {
        let frame = parse_render_frame(arguments)?;
        if !self.config.allow_render {
            return Ok(DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                tool_result(refusal_payload(
                    TOOL_RENDER,
                    McpProbeMethod::Render,
                    request_id,
                    &frame,
                    "render-denied",
                    "debug render is not admitted by this server configuration",
                )),
            ))));
        }

        let packet = parse_render_packet(arguments)?;
        Ok(submit_bridge_tool(
            id,
            TOOL_RENDER,
            McpProbeMethod::Render,
            DebugCommand::render(request_id, packet),
            bridge,
        ))
    }

    fn begin_bridge_screenshot(
        &self,
        id: Option<Value>,
        request_id: &str,
        arguments: &Value,
        origin: ScreenshotParseOrigin,
        bridge: &DebugBridgeClient,
    ) -> Result<DebugMcpBridgeMessage, RpcError> {
        let request = parse_presented_screenshot_request(arguments, origin)?;
        if !self.config.allow_screenshot {
            return Ok(DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                tool_result(screenshot_refusal_payload(
                    request_id,
                    &request.selector,
                    "screenshot-denied",
                    "presented screenshot capture is not admitted by this server configuration",
                )),
            ))));
        }

        if optional_string_field(arguments, "target")?.is_some_and(|target| {
            target
                != request
                    .selector
                    .correlation_frame()
                    .surface_instance_id
                    .as_str()
        }) {
            return Ok(DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                tool_result(screenshot_refusal_payload(
                    request_id,
                    &request.selector,
                    "screenshot-target-unsupported",
                    "presented screenshot captures only the root surface instance",
                )),
            ))));
        }

        Ok(submit_bridge_tool(
            id,
            TOOL_SCREENSHOT,
            McpProbeMethod::Screenshot,
            DebugCommand::screenshot(request_id, request),
            bridge,
        ))
    }

    fn begin_bridge_control(
        &self,
        id: Option<Value>,
        request_id: &str,
        arguments: &Value,
        bridge: &DebugBridgeClient,
    ) -> Result<DebugMcpBridgeMessage, RpcError> {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_control {
            return Ok(DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                tool_result(refusal_payload(
                    TOOL_CONTROL,
                    McpProbeMethod::Control,
                    request_id,
                    &frame,
                    "control-denied",
                    "debug control is not admitted by this server configuration",
                )),
            ))));
        }

        let event = parse_input_event(required_object_field(arguments, "event")?)?;
        let trace = parse_control_trace_flag(arguments)?;
        let command = if trace {
            DebugCommand::control_with_trace(request_id, frame, event)
        } else {
            DebugCommand::control(request_id, frame, event)
        };
        Ok(submit_bridge_tool(
            id,
            TOOL_CONTROL,
            McpProbeMethod::Control,
            command,
            bridge,
        ))
    }

    fn begin_bridge_physical_control(
        &self,
        id: Option<Value>,
        request_id: &str,
        arguments: &Value,
        bridge: &DebugBridgeClient,
    ) -> Result<DebugMcpBridgeMessage, RpcError> {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_control {
            return Ok(DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                tool_result(refusal_payload(
                    TOOL_PHYSICAL_CONTROL,
                    McpProbeMethod::Control,
                    request_id,
                    &frame,
                    "control-denied",
                    "physical debug control is not admitted by this server configuration",
                )),
            ))));
        }

        let operation = parse_physical_control(required_object_field(arguments, "operation")?)?;
        let command = DebugCommand::physical_control_with_trace(request_id, frame, operation);
        Ok(submit_bridge_tool(
            id,
            TOOL_PHYSICAL_CONTROL,
            McpProbeMethod::Control,
            command,
            bridge,
        ))
    }

    fn begin_bridge_resize(
        &self,
        id: Option<Value>,
        request_id: &str,
        arguments: &Value,
        bridge: &DebugBridgeClient,
    ) -> Result<DebugMcpBridgeMessage, RpcError> {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_resize {
            return Ok(DebugMcpBridgeMessage::Immediate(Some(json_rpc_result(
                id,
                tool_result(refusal_payload(
                    TOOL_RESIZE,
                    McpProbeMethod::Resize,
                    request_id,
                    &frame,
                    "resize-denied",
                    "debug resize is not admitted by this server configuration",
                )),
            ))));
        }

        Ok(submit_bridge_tool(
            id,
            TOOL_RESIZE,
            McpProbeMethod::Resize,
            DebugCommand::resize(request_id, frame),
            bridge,
        ))
    }

    fn call_status<H>(
        &self,
        request_id: &str,
        arguments: &Value,
        handler: &mut H,
    ) -> Result<Value, RpcError>
    where
        H: SlipwayDebugCommandHandler,
    {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_status {
            return Ok(tool_result(refusal_payload(
                TOOL_STATUS,
                McpProbeMethod::Status,
                request_id,
                &frame,
                "status-denied",
                "debug status is not admitted by this server configuration",
            )));
        }

        let command = DebugCommand::status(request_id, frame.clone());
        Ok(tool_result(reply_payload(
            TOOL_STATUS,
            McpProbeMethod::Status,
            handler_reply(handler, command),
            true,
        )))
    }

    fn call_probe<H>(
        &self,
        request_id: &str,
        arguments: &Value,
        handler: &mut H,
    ) -> Result<Value, RpcError>
    where
        H: SlipwayDebugCommandHandler,
    {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_probe {
            return Ok(tool_result(refusal_payload(
                TOOL_PROBE,
                McpProbeMethod::Probe,
                request_id,
                &frame,
                "probe-denied",
                "debug probe is not admitted by this server configuration",
            )));
        }

        let request = parse_probe_request(arguments)?;
        let command = DebugCommand::probe(request_id, frame, request);
        Ok(tool_result(reply_payload(
            TOOL_PROBE,
            McpProbeMethod::Probe,
            handler_reply(handler, command),
            true,
        )))
    }

    fn call_render<H>(
        &self,
        request_id: &str,
        arguments: &Value,
        handler: &mut H,
    ) -> Result<Value, RpcError>
    where
        H: SlipwayDebugCommandHandler,
    {
        let frame = parse_render_frame(arguments)?;
        if !self.config.allow_render {
            return Ok(tool_result(refusal_payload(
                TOOL_RENDER,
                McpProbeMethod::Render,
                request_id,
                &frame,
                "render-denied",
                "debug render is not admitted by this server configuration",
            )));
        }

        let packet = parse_render_packet(arguments)?;
        let command = DebugCommand::render(request_id, packet);
        Ok(tool_result(reply_payload(
            TOOL_RENDER,
            McpProbeMethod::Render,
            handler_reply(handler, command),
            true,
        )))
    }

    fn call_screenshot<H>(
        &self,
        request_id: &str,
        arguments: &Value,
        origin: ScreenshotParseOrigin,
        handler: &mut H,
    ) -> Result<Value, RpcError>
    where
        H: SlipwayDebugCommandHandler,
    {
        let request = parse_presented_screenshot_request(arguments, origin)?;
        if !self.config.allow_screenshot {
            return Ok(tool_result(screenshot_refusal_payload(
                request_id,
                &request.selector,
                "screenshot-denied",
                "presented screenshot capture is not admitted by this server configuration",
            )));
        }
        if optional_string_field(arguments, "target")?.is_some_and(|target| {
            target
                != request
                    .selector
                    .correlation_frame()
                    .surface_instance_id
                    .as_str()
        }) {
            return Ok(tool_result(screenshot_refusal_payload(
                request_id,
                &request.selector,
                "screenshot-target-unsupported",
                "presented screenshot captures only the root surface instance",
            )));
        }

        let reply = handler_reply(handler, DebugCommand::screenshot(request_id, request));
        Ok(tool_result(finalize_screenshot_reply_payload(
            TOOL_SCREENSHOT,
            McpProbeMethod::Screenshot,
            reply,
        )))
    }

    fn call_control<H>(
        &self,
        request_id: &str,
        arguments: &Value,
        handler: &mut H,
    ) -> Result<Value, RpcError>
    where
        H: SlipwayDebugCommandHandler,
    {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_control {
            return Ok(tool_result(refusal_payload(
                TOOL_CONTROL,
                McpProbeMethod::Control,
                request_id,
                &frame,
                "control-denied",
                "debug control is not admitted by this server configuration",
            )));
        }

        let event = parse_input_event(required_object_field(arguments, "event")?)?;
        let trace = parse_control_trace_flag(arguments)?;
        let command = if trace {
            DebugCommand::control_with_trace(request_id, frame, event)
        } else {
            DebugCommand::control(request_id, frame, event)
        };
        Ok(tool_result(reply_payload(
            TOOL_CONTROL,
            McpProbeMethod::Control,
            handler_reply(handler, command),
            true,
        )))
    }

    fn call_physical_control<H>(
        &self,
        request_id: &str,
        arguments: &Value,
        handler: &mut H,
    ) -> Result<Value, RpcError>
    where
        H: SlipwayDebugCommandHandler,
    {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_control {
            return Ok(tool_result(refusal_payload(
                TOOL_PHYSICAL_CONTROL,
                McpProbeMethod::Control,
                request_id,
                &frame,
                "control-denied",
                "physical debug control is not admitted by this server configuration",
            )));
        }

        let operation = parse_physical_control(required_object_field(arguments, "operation")?)?;
        Ok(tool_result(reply_payload(
            TOOL_PHYSICAL_CONTROL,
            McpProbeMethod::Control,
            handler_reply(
                handler,
                DebugCommand::physical_control_with_trace(request_id, frame, operation),
            ),
            true,
        )))
    }

    fn call_resize<H>(
        &self,
        request_id: &str,
        arguments: &Value,
        handler: &mut H,
    ) -> Result<Value, RpcError>
    where
        H: SlipwayDebugCommandHandler,
    {
        let frame = parse_frame_from_arguments(arguments)?;
        if !self.config.allow_resize {
            return Ok(tool_result(refusal_payload(
                TOOL_RESIZE,
                McpProbeMethod::Resize,
                request_id,
                &frame,
                "resize-denied",
                "debug resize is not admitted by this server configuration",
            )));
        }

        let command = DebugCommand::resize(request_id, frame);
        Ok(tool_result(reply_payload(
            TOOL_RESIZE,
            McpProbeMethod::Resize,
            handler_reply(handler, command),
            true,
        )))
    }
}

pub fn run_stdio<R, W, H>(
    reader: R,
    writer: &mut W,
    server: &DebugMcpServer,
    handler: &mut H,
) -> std::io::Result<()>
where
    R: BufRead,
    W: Write,
    H: SlipwayDebugCommandHandler,
{
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = match server.handle_json_rpc(&line, handler) {
            Ok(response) => response,
            Err(error) => Some(json_rpc_error(
                None,
                -32700,
                format!("invalid JSON-RPC message: {error}"),
            )),
        };

        if let Some(response) = response {
            serde_json::to_writer(&mut *writer, &response)?;
            writer.write_all(b"\n")?;
        }
    }
    writer.flush()
}

pub fn run_runtime_stdio<R, W>(
    reader: R,
    writer: &mut W,
    client: &DebugMcpRuntimeClient,
) -> io::Result<()>
where
    R: BufRead,
    W: Write,
{
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }

        let response = client
            .submit(line)
            .map_err(runtime_transport_io_error)?
            .recv()
            .map_err(runtime_transport_io_error)?;

        if let Some(response) = response {
            serde_json::to_writer(&mut *writer, &response)?;
            writer.write_all(b"\n")?;
            writer.flush()?;
        }
    }

    Ok(())
}

fn runtime_transport_io_error(error: DebugMcpRuntimeTransportError) -> io::Error {
    io::Error::other(format!("runtime MCP transport error: {error:?}"))
}

fn submit_bridge_tool(
    id: Option<Value>,
    tool: &'static str,
    method: McpProbeMethod,
    command: DebugCommand,
    bridge: &DebugBridgeClient,
) -> DebugMcpBridgeMessage {
    match bridge.submit(command) {
        Ok(handle) => DebugMcpBridgeMessage::Pending(DebugMcpPendingToolCall {
            id,
            tool: tool.to_string(),
            method,
            handle,
        }),
        Err(error) => DebugMcpBridgeMessage::Immediate(Some(json_rpc_error(
            id,
            -32000,
            format!("debug bridge submit failed: {error:?}"),
        ))),
    }
}

fn default_tools() -> Vec<Value> {
    vec![
        tool_schema(TOOL_STATUS, "Read the current Slipway debug status."),
        tool_schema(TOOL_PROBE, "Request explicit Slipway debug probe products."),
        render_tool_schema(
            TOOL_RENDER,
            "Request offscreen render evidence through the app runtime.",
        ),
        screenshot_tool_schema(
            TOOL_SCREENSHOT,
            "Capture the root window pixels directly from the next acquired surface texture presentation.",
        ),
        tool_schema(
            TOOL_CONTROL,
            "Submit an input/control event to the app runtime.",
        ),
        tool_schema(
            TOOL_PHYSICAL_CONTROL,
            "Resolve a physical-equivalent debug operation through declared hit/scroll/focus routing.",
        ),
        tool_schema(
            TOOL_RESIZE,
            "Request a viewport resize for a frame identity. No backend currently performs native resizes; the runtime refuses with `resize-unsupported` and the viewport is unchanged.",
        ),
    ]
}

fn tool_schema(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {
                "frame": frame_schema(),
            },
            "required": ["frame"],
            "additionalProperties": true,
        },
    })
}

fn render_tool_schema(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {
                "frame": frame_schema(),
                "target": { "type": "string" },
            },
            "required": ["frame"],
            "additionalProperties": true,
        },
    })
}

fn frame_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "surface_id": { "type": "string" },
            "surface_instance_id": { "type": "string" },
            "revision": { "type": "integer", "minimum": 0 },
            "frame_index": { "type": "integer", "minimum": 0 },
            "viewport": { "type": "object" },
        },
        "required": [
            "surface_id",
            "surface_instance_id",
            "revision",
            "frame_index",
            "viewport"
        ],
        "additionalProperties": true,
    })
}

fn handler_reply<H>(handler: &mut H, command: DebugCommand) -> DebugReply
where
    H: SlipwayDebugCommandHandler,
{
    let request_id = command.request_id.clone();
    let frame = command.frame_identity().clone();
    let screenshot_admission = command.screenshot_admission();
    let mut product = handler.handle_debug_command(command);
    if let DebugReplyProduct::Screenshot(screenshot) = &product {
        product = match screenshot_admission {
            Some(admitted) => validate_presented_screenshot_product(&frame, admitted, screenshot)
                .map_or_else(DebugReplyProduct::Error, |_| product),
            None => DebugReplyProduct::Error(DebugFailure {
                code: "screenshot-admission-missing".to_string(),
                message: "screenshot product returned for a non-screenshot command".to_string(),
                dispatch_evidence: None,
            }),
        };
    }
    DebugReply {
        request_id,
        frame,
        product,
    }
}

fn json_rpc_result(id: Option<Value>, result: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "result": result,
    })
}

fn json_rpc_error(id: Option<Value>, code: i64, message: impl Into<String>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id.unwrap_or(Value::Null),
        "error": {
            "code": code,
            "message": message.into(),
        },
    })
}

fn tool_result(payload: Value) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": payload.to_string(),
            }
        ],
    })
}

fn screenshot_tool_schema(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": {
                "frame": {
                    "oneOf": [
                        frame_schema(),
                        { "type": "string", "enum": ["current", "last"] }
                    ]
                },
                "target": { "type": "string" },
            },
            "additionalProperties": true,
        },
    })
}

fn finalize_response_work(work: DebugMcpResponseWork) -> Option<Value> {
    match work {
        DebugMcpResponseWork::Ready(response) => response,
        DebugMcpResponseWork::PresentedScreenshot {
            rpc_id,
            tool,
            method,
            reply,
        } => Some(json_rpc_result(
            rpc_id,
            tool_result(finalize_screenshot_reply_payload(&tool, method, reply)),
        )),
    }
}

fn finalize_screenshot_reply_payload(
    tool: &str,
    method: McpProbeMethod,
    mut reply: DebugReply,
) -> Value {
    if let DebugReplyProduct::Screenshot(product) = &reply.product {
        let admitted = match product {
            PresentedScreenshotProduct::Captured(pixels) => pixels.selector.admission(),
            PresentedScreenshotProduct::Refusal(refusal) => refusal.selector.admission(),
        };
        if let Err(failure) = validate_presented_screenshot_product(&reply.frame, admitted, product)
        {
            reply.product = DebugReplyProduct::Error(failure);
        }
    }
    let receipt = match &reply.product {
        DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Captured(pixels)) => {
            match write_debug_rgba8_png_artifact(
                PRESENTED_ARTIFACT_PROVIDER_ID,
                &pixels.captured_frame.surface_instance_id,
                &pixels.captured_frame,
                pixels.width,
                pixels.height,
                &pixels.bytes,
            ) {
                Ok(receipt) => Some(receipt),
                Err(error) => {
                    let invalid_rgba = matches!(
                        error,
                        DebugPngArtifactError::ZeroSize
                            | DebugPngArtifactError::ByteLengthOverflow
                            | DebugPngArtifactError::ByteLengthMismatch { .. }
                    );
                    reply.product = DebugReplyProduct::Screenshot(
                        PresentedScreenshotProduct::Refusal(PresentedScreenshotRefusal {
                            selector: pixels.selector.clone(),
                            captured_frame: Some(pixels.captured_frame.clone()),
                            backend_id: pixels.source.backend_id.clone(),
                            code: if invalid_rgba {
                                "screenshot-artifact-invalid-rgba".to_string()
                            } else {
                                "screenshot-artifact-write-failed".to_string()
                            },
                            reason: format!(
                                "presented screenshot artifact finalization failed: {error:?}"
                            ),
                            diagnostics: pixels.diagnostics.clone(),
                        }),
                    );
                    None
                }
            }
        }
        _ => None,
    };

    let (product_kind, product, refused) = match &reply.product {
        DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Captured(pixels)) => {
            let receipt = receipt.expect("captured screenshot artifact was finalized above");
            (
                "presented_screenshot",
                json!({
                    "admission": screenshot_admission_name(pixels.selector.admission()),
                    "requested_frame": frame_json(pixels.selector.correlation_frame()),
                    "selector": screenshot_selector_json(&pixels.selector),
                    "captured_frame": frame_json(&pixels.captured_frame),
                    "source": evidence_source_summary(&pixels.source),
                    "capture_path": presented_capture_path_name(pixels.capture_path),
                    "source_format": presented_surface_format_name(pixels.source_format),
                    "transfer": presented_transfer_name(pixels.transfer),
                    "alpha": presented_alpha_name(pixels.alpha),
                    "width": pixels.width,
                    "height": pixels.height,
                    "artifact_ref": receipt.artifact_ref,
                    "artifact_path": receipt.artifact_path,
                    "pixel_hash": receipt.pixel_hash,
                    "diagnostics": diagnostics_summary(&pixels.diagnostics),
                }),
                false,
            )
        }
        DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Refusal(refusal)) => (
            "screenshot_refusal",
            json!({
                "admission": screenshot_admission_name(refusal.selector.admission()),
                "requested_frame": frame_json(refusal.selector.correlation_frame()),
                "selector": screenshot_selector_json(&refusal.selector),
                "captured_frame": refusal.captured_frame.as_ref().map(frame_json),
                "backend_id": refusal.backend_id,
                "code": refusal.code,
                "reason": refusal.reason,
                "diagnostics": diagnostics_summary(&refusal.diagnostics),
            }),
            true,
        ),
        _ => return reply_payload(tool, method, reply, true),
    };

    json!({
        "method": "tools/call",
        "tool": tool,
        "bridge_method": bridge_method_name(method),
        "request_id": reply.request_id,
        "frame": frame_json(&reply.frame),
        "admitted": true,
        "refused": refused,
        "product_kind": product_kind,
        "product": product,
    })
}

fn presented_capture_path_name(path: PresentedCapturePath) -> &'static str {
    match path {
        PresentedCapturePath::DirectAcquiredSurfaceTextureCopy => {
            "direct_acquired_surface_texture_copy"
        }
    }
}

fn presented_surface_format_name(format: PresentedSurfaceFormat) -> &'static str {
    match format {
        PresentedSurfaceFormat::Rgba8Unorm => "rgba8_unorm",
        PresentedSurfaceFormat::Rgba8UnormSrgb => "rgba8_unorm_srgb",
        PresentedSurfaceFormat::Bgra8Unorm => "bgra8_unorm",
        PresentedSurfaceFormat::Bgra8UnormSrgb => "bgra8_unorm_srgb",
    }
}

fn presented_transfer_name(transfer: PresentedTransferFunction) -> &'static str {
    match transfer {
        PresentedTransferFunction::Linear => "linear",
        PresentedTransferFunction::Srgb => "srgb",
    }
}

fn presented_alpha_name(alpha: PresentedAlphaMode) -> &'static str {
    match alpha {
        PresentedAlphaMode::Opaque => "opaque",
        PresentedAlphaMode::Premultiplied => "premultiplied",
    }
}

fn screenshot_admission_name(admission: PresentedScreenshotAdmission) -> &'static str {
    match admission {
        PresentedScreenshotAdmission::Exact => "exact",
        PresentedScreenshotAdmission::Current => "current",
    }
}

fn screenshot_selector_json(selector: &PresentedScreenshotSelector) -> Value {
    match selector {
        PresentedScreenshotSelector::Exact { expected_frame } => json!({
            "kind": "exact",
            "expected_frame": frame_json(expected_frame),
        }),
        PresentedScreenshotSelector::Current { request_context } => json!({
            "kind": "current",
            "request_context": frame_json(request_context),
        }),
    }
}

fn refusal_payload(
    tool: &str,
    method: McpProbeMethod,
    request_id: &str,
    frame: &FrameIdentity,
    code: &str,
    message: &str,
) -> Value {
    json!({
        "method": "tools/call",
        "tool": tool,
        "bridge_method": bridge_method_name(method),
        "request_id": request_id,
        "frame": frame_json(frame),
        "admitted": false,
        "refused": true,
        "product_kind": "refusal",
        "refusal": {
            "code": code,
            "message": message,
        },
    })
}

fn screenshot_refusal_payload(
    request_id: &str,
    selector: &PresentedScreenshotSelector,
    code: &str,
    message: &str,
) -> Value {
    json!({
        "method": "tools/call",
        "tool": TOOL_SCREENSHOT,
        "bridge_method": bridge_method_name(McpProbeMethod::Screenshot),
        "request_id": request_id,
        "frame": frame_json(selector.correlation_frame()),
        "admitted": false,
        "refused": true,
        "product_kind": "refusal",
        "admission": screenshot_admission_name(selector.admission()),
        "requested_frame": frame_json(selector.correlation_frame()),
        "selector": screenshot_selector_json(selector),
        "captured_frame": Value::Null,
        "refusal": {
            "code": code,
            "message": message,
        },
    })
}

fn reply_payload(tool: &str, method: McpProbeMethod, reply: DebugReply, admitted: bool) -> Value {
    let (product_kind, product) = product_summary(&reply.product);
    json!({
        "method": "tools/call",
        "tool": tool,
        "bridge_method": bridge_method_name(method),
        "request_id": reply.request_id,
        "frame": frame_json(&reply.frame),
        "admitted": admitted,
        "refused": false,
        "product_kind": product_kind,
        "product": product,
    })
}

fn product_summary(product: &DebugReplyProduct) -> (&'static str, Value) {
    match product {
        DebugReplyProduct::Status(status) => ("status", status_summary(status)),
        DebugReplyProduct::Probes(products) => (
            "probes",
            json!({
                "count": products.len(),
                "kinds": products
                    .iter()
                    .map(|product| format!("{product:?}"))
                    .collect::<Vec<_>>(),
                "products": products
                    .iter()
                    .map(probe_product_summary)
                    .collect::<Vec<_>>(),
            }),
        ),
        DebugReplyProduct::Render(RenderProduct::Evidence(evidence)) => (
            "render_evidence",
            json!({
                "target": evidence.target.as_str(),
                "frame": frame_json(&evidence.frame),
                "source": evidence_source_summary(&evidence.source),
                "provider_id": evidence.provider_id,
                "artifact_ref": evidence.artifact_ref,
                "artifact_path": evidence.artifact_path,
                "pixel_hash": evidence.pixel_hash,
                "width": evidence.width,
                "height": evidence.height,
                "diagnostics": diagnostics_summary(&evidence.diagnostics),
            }),
        ),
        DebugReplyProduct::Render(RenderProduct::Refusal(refusal)) => (
            "render_refusal",
            json!({
                "target": refusal.target.as_ref().map(WidgetId::as_str),
                "frame": frame_json(&refusal.frame),
                "source": refusal.source.as_ref().map(evidence_source_summary),
                "provider_id": refusal.provider_id,
                "reason": refusal.reason,
                "diagnostics": diagnostics_summary(&refusal.diagnostics),
            }),
        ),
        DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Captured(pixels)) => (
            "presented_screenshot_unfinalized",
            json!({
                "admission": screenshot_admission_name(pixels.selector.admission()),
                "requested_frame": frame_json(pixels.selector.correlation_frame()),
                "selector": screenshot_selector_json(&pixels.selector),
                "captured_frame": frame_json(&pixels.captured_frame),
                "source": evidence_source_summary(&pixels.source),
                "capture_path": presented_capture_path_name(pixels.capture_path),
                "source_format": presented_surface_format_name(pixels.source_format),
                "transfer": presented_transfer_name(pixels.transfer),
                "alpha": presented_alpha_name(pixels.alpha),
                "width": pixels.width,
                "height": pixels.height,
                "diagnostics": diagnostics_summary(&pixels.diagnostics),
            }),
        ),
        DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Refusal(refusal)) => (
            "screenshot_refusal",
            json!({
                "admission": screenshot_admission_name(refusal.selector.admission()),
                "requested_frame": frame_json(refusal.selector.correlation_frame()),
                "selector": screenshot_selector_json(&refusal.selector),
                "captured_frame": refusal.captured_frame.as_ref().map(frame_json),
                "backend_id": refusal.backend_id,
                "code": refusal.code,
                "reason": refusal.reason,
                "diagnostics": diagnostics_summary(&refusal.diagnostics),
            }),
        ),
        DebugReplyProduct::Diagnostics(diagnostics) => (
            "diagnostics",
            json!({
                "count": diagnostics.len(),
                "diagnostics": diagnostics_summary(diagnostics),
            }),
        ),
        DebugReplyProduct::ControlTrace(trace) => ("control_trace", control_trace_summary(trace)),
        DebugReplyProduct::CompositionTrace(trace) => {
            ("text_composition_trace", composition_trace_summary(trace))
        }
        DebugReplyProduct::Error(error) => ("error", failure_summary(error)),
    }
}

fn composition_trace_summary(trace: &slipway_debug_bridge::DebugCompositionTrace) -> Value {
    json!({
        "request_id": trace.request_id,
        "frame": frame_json(&trace.frame),
        "backend_id": trace.backend_id,
        "target": trace.target.as_str(),
        "selected_region": trace.selected_region.as_str(),
        "focused_before": trace.focused_before,
        "focused_after": trace.focused_after,
        "phases": trace.phases.iter().map(|phase| json!({
            "phase": composition_phase_name(phase.phase),
            "backend_event": phase.backend_event,
            "provenance": match phase.provenance {
                CompositionPhaseProvenance::Native => json!({ "kind": "native" }),
                CompositionPhaseProvenance::Derived { from } => json!({
                    "kind": "derived",
                    "from": composition_phase_name(from),
                }),
            },
            "event": {
                "target": phase.event.target.as_str(),
                "target_slot": phase.event.target_slot.as_ref().map(widget_slot_json),
                "phase": composition_phase_name(phase.event.phase),
                "preedit_text": phase.event.preedit_text,
                "cursor_range": phase.event.cursor_range.as_ref().map(text_selection_range_json),
            },
            "ingress_observation": composition_ingress_summary(phase.ingress_observation),
            "dispatch_evidence": dispatch_evidence_summary(&phase.dispatch_evidence),
            "app_handled": phase.app_handled,
            "result_identity": phase.result_identity.as_ref().map(result_identity_summary),
        })).collect::<Vec<_>>(),
        "commit_mutation": trace.commit_mutation.as_ref().map(|mutation| json!({
            "trace": control_trace_summary(&mutation.trace),
            "before": mutation.before,
            "after": mutation.after,
        })),
        "completed": trace.completed,
        "failure": trace.failure.as_ref().map(failure_summary),
    })
}

fn composition_ingress_summary(observation: DebugCompositionIngressObservation) -> Value {
    match observation {
        DebugCompositionIngressObservation::IcedQueueSlice { sequence_index } => {
            json!({ "kind": "iced_queue_slice", "sequence_index": sequence_index })
        }
        DebugCompositionIngressObservation::EguiRawInputSpan { event_index } => {
            json!({ "kind": "egui_raw_input_span", "event_index": event_index })
        }
        DebugCompositionIngressObservation::Derived {
            from_sequence_index,
        } => json!({
            "kind": "derived",
            "from_sequence_index": from_sequence_index,
        }),
    }
}

fn composition_phase_name(phase: TextCompositionPhase) -> &'static str {
    match phase {
        TextCompositionPhase::Start => "start",
        TextCompositionPhase::Update => "update",
        TextCompositionPhase::Commit => "commit",
        TextCompositionPhase::End => "end",
        TextCompositionPhase::Cancel => "cancel",
    }
}

fn status_summary(status: &DebugStatus) -> Value {
    json!({
        "admitted": status.admitted,
        "detail": status.detail,
        "revision": status.revision,
        "backend_id": status.backend_id,
        "trace_buffer_depth": status.trace_buffer_depth,
        "trace_buffer_capacity": status.trace_buffer_capacity,
        "refused_debug_replies": status.refused_debug_replies,
        "unhandled_backend_input_traces": status.unhandled_backend_input_traces,
    })
}

fn failure_summary(error: &DebugFailure) -> Value {
    json!({
        "code": error.code,
        "message": error.message,
        "dispatch_evidence": error
            .dispatch_evidence
            .as_ref()
            .map(dispatch_evidence_summary),
    })
}

fn evidence_source_summary(source: &EvidenceSource) -> Value {
    json!({
        "label": source.label.as_str(),
        "backend_id": source.backend_id.as_deref(),
        "provider_id": source.provider_id.as_deref(),
        "pass_id": source.pass_id.as_deref(),
    })
}

fn diagnostics_summary(diagnostics: &[Diagnostic]) -> Vec<Value> {
    diagnostics
        .iter()
        .map(|diagnostic| {
            json!({
                "target": diagnostic.target.as_ref().map(WidgetId::as_str),
                "severity": format!("{:?}", diagnostic.severity),
                "code": diagnostic.code,
                "message": diagnostic.message,
            })
        })
        .collect()
}

fn control_trace_summary(trace: &slipway_debug_bridge::DebugControlTrace) -> Value {
    json!({
        "request_id": trace.request_id.as_str(),
        "frame": frame_json(&trace.frame),
        "mode": control_trace_mode_name(trace.mode),
        "physical_equivalent": matches!(
            trace.mode,
            slipway_debug_bridge::DebugControlMode::PhysicalEquivalent
        ),
        "dispatch_evidence": trace
            .dispatch_evidence
            .as_ref()
            .map(dispatch_evidence_summary),
        "dispatch_identity": trace
            .dispatch_evidence
            .as_ref()
            .map(|evidence| dispatch_identity_summary(&evidence.dispatch_identity())),
        "result_identity": trace
            .result_identity
            .as_ref()
            .map(result_identity_summary),
        "routed_event_target": trace.routed_event_target.as_str(),
        "event_summary": trace.event_summary.as_str(),
        "handled": trace.handled,
        "revision_before": trace.revision_before,
        "revision_after": trace.revision_after,
        "stages": trace
            .stages
            .iter()
            .map(control_trace_stage_summary)
            .collect::<Vec<_>>(),
        "messages": trace
            .messages
            .iter()
            .map(|message| {
                json!({
                    "source": message.source.as_str(),
                    "name": message.name.as_str(),
                    "disposition": format!("{:?}", message.disposition),
                })
            })
            .collect::<Vec<_>>(),
        "diagnostics": diagnostics_summary(&trace.diagnostics),
    })
}

fn dispatch_evidence_summary(evidence: &DeclaredEventDispatchEvidence) -> Value {
    json!({
        "source": evidence_source_summary(&evidence.source),
        "frame": frame_json(&evidence.frame),
        "kind": declared_dispatch_kind_name(evidence.kind),
        "input_position": evidence.input_position.map(point_json),
        "input_position_space": evidence.input_position_space.map(|space| match space {
            slipway_core::DispatchPositionSpace::Content => "content",
            slipway_core::DispatchPositionSpace::Viewport => "viewport",
        }),
        "candidate_regions": evidence
            .candidate_regions
            .iter()
            .map(|region| region.as_str())
            .collect::<Vec<_>>(),
        "selected_region": evidence.selected_region.as_ref().map(|region| region.as_str()),
        "refusal_reason": evidence.refusal_reason.as_deref(),
        "generated_event": evidence.generated_event.as_ref().map(generated_event_summary),
        "route": evidence.route.as_ref().map(event_route_summary),
        "capture_event": evidence.capture_event,
        "diagnostics": diagnostics_summary(&evidence.diagnostics),
    })
}

fn dispatch_identity_summary(identity: &slipway_core::DeclaredEventDispatchIdentity) -> Value {
    json!({
        "frame": frame_json(&identity.frame),
        "kind": declared_dispatch_kind_name(identity.kind),
        "input_position": identity.input_position.map(point_json),
        "candidate_regions": identity
            .candidate_regions
            .iter()
            .map(|region| region.as_str())
            .collect::<Vec<_>>(),
        "selected_region": identity.selected_region.as_ref().map(|region| region.as_str()),
        "generated_event": identity.generated_event.as_ref().map(generated_event_summary),
        "route": identity.route.as_ref().map(event_route_summary),
        "capture_event": identity.capture_event,
    })
}

fn result_identity_summary(identity: &slipway_core::EventResultIdentity) -> Value {
    json!({
        "handled": identity.handled,
        "emitted_messages": identity
            .emitted_messages
            .iter()
            .map(|message| {
                json!({
                    "target": message.target.as_str(),
                    "name": message.name.as_str(),
                })
            })
            .collect::<Vec<_>>(),
        "change_shapes": identity
            .change_shapes
            .iter()
            .map(|change| {
                json!({
                    "target": change.target.as_str(),
                    "slot": change.slot.as_ref().map(widget_slot_json),
                    "field": change.field.as_str(),
                    "before_present": change.before_present,
                    "after_present": change.after_present,
                })
            })
            .collect::<Vec<_>>(),
        "diagnostics": identity
            .diagnostics
            .iter()
            .map(|diagnostic| {
                json!({
                    "target": diagnostic.target.as_ref().map(|target| target.as_str()),
                    "severity": format!("{:?}", diagnostic.severity),
                    "code": diagnostic.code.as_str(),
                })
            })
            .collect::<Vec<_>>(),
    })
}

fn probe_product_summary(product: &ProbeProduct) -> Value {
    match product {
        ProbeProduct::Event(event) => json!({
            "kind": "event",
            "routed_target": event.routed_target.as_str(),
            "event": generated_event_summary(&event.event),
            "dispatch_evidence": event
                .dispatch_evidence
                .as_ref()
                .map(dispatch_evidence_summary),
            "dispatch_identity": event
                .dispatch_identity
                .as_ref()
                .map(dispatch_identity_summary),
            "result_identity": result_identity_summary(&event.result_identity),
            "handled": event.handled,
            "revision_before": event.revision_before,
            "revision_after": event.revision_after,
            "emitted_messages": event
                .emitted_messages
                .iter()
                .map(|message| {
                    json!({
                        "target": message.target.as_str(),
                        "name": message.name,
                    })
                })
                .collect::<Vec<_>>(),
            "local_state": event
                .local_state
                .iter()
                .map(state_observation_summary)
                .collect::<Vec<_>>(),
            "changes": event
                .changes
                .iter()
                .map(change_evidence_summary)
                .collect::<Vec<_>>(),
            "diagnostics": diagnostics_summary(&event.diagnostics),
        }),
        ProbeProduct::Diagnostic(diagnostic) => json!({
            "kind": "diagnostic",
            "diagnostic": diagnostics_summary(std::slice::from_ref(diagnostic))
                .into_iter()
                .next(),
        }),
        ProbeProduct::RenderEvidence(evidence) => json!({
            "kind": "render_evidence",
            "frame": frame_json(&evidence.frame),
            "source": evidence_source_summary(&evidence.source),
            "artifact_ref": evidence.artifact_ref,
            "artifact_path": evidence.artifact_path,
            "pixel_hash": evidence.pixel_hash,
            "width": evidence.width,
            "height": evidence.height,
        }),
        ProbeProduct::RenderPacket(packet) => json!({
            "kind": "render_packet",
            "target": packet.target.as_str(),
            "frame": frame_json(&packet.frame),
            "paint_ops": packet.paint.len(),
            "diagnostics": diagnostics_summary(&packet.diagnostics),
        }),
        ProbeProduct::ViewDefinition(view) => json!({
            "kind": "view_definition",
            "target": view.target.as_str(),
            "frame": frame_json(&view.frame),
            "hit_regions": view.hit_regions.len(),
            "focus_regions": view.focus_regions.len(),
            "scroll_regions": view.scroll_regions.len(),
            "diagnostics": diagnostics_summary(&view.diagnostics),
        }),
        ProbeProduct::State(state) => json!({
            "kind": "state",
            "target": state.target.as_str(),
            "observations": state
                .observations
                .iter()
                .map(state_observation_summary)
                .collect::<Vec<_>>(),
        }),
        ProbeProduct::Topology(topology) => json!({
            "kind": "topology",
            "root": topology.root.id.as_str(),
            "traversal_count": topology.traversal.order.len(),
        }),
        ProbeProduct::Change(change) => json!({
            "kind": "change",
            "target": change.target.as_str(),
            "changes": change
                .changes
                .iter()
                .map(change_evidence_summary)
                .collect::<Vec<_>>(),
        }),
        ProbeProduct::Paint(paint) => json!({
            "kind": "paint",
            "target": paint.target.as_str(),
            "ops": paint.ops.len(),
        }),
        ProbeProduct::Semantics(semantics) => json!({
            "kind": "semantics",
            "root": semantics.root.as_str(),
            "nodes": semantics.nodes.len(),
        }),
        ProbeProduct::Surface(surface) => json!({
            "kind": "surface",
            "target": surface.target.as_str(),
            "surfaces": surface.surfaces.len(),
        }),
        ProbeProduct::LayoutIntent(layout) => json!({
            "kind": "layout_intent",
            "target": layout.target.as_str(),
            "has_intrinsic_size": layout.intrinsic_size.is_some(),
            "has_size_policy": layout.size_policy.is_some(),
            "has_overflow_policy": layout.overflow_policy.is_some(),
            "has_scroll_policy": layout.scroll.is_some(),
        }),
        ProbeProduct::DispatchGraph(probe) => json!({
            "kind": "dispatch_graph",
            "target": probe.target.as_str(),
            "frame": frame_json(&probe.frame),
            "graph": dispatch_graph_summary(&probe.graph),
        }),
    }
}

fn dispatch_graph_summary(graph: &slipway_core::DispatchGraph) -> Value {
    json!({
        "target": graph.target.as_str(),
        "nodes": graph
            .nodes
            .iter()
            .map(dispatch_graph_node_summary)
            .collect::<Vec<_>>(),
        "edges": graph
            .edges
            .iter()
            .map(dispatch_graph_edge_summary)
            .collect::<Vec<_>>(),
    })
}

fn dispatch_graph_node_summary(node: &slipway_core::DispatchGraphNode) -> Value {
    json!({
        "id": node.id.as_str(),
        "kind": dispatch_graph_node_kind_name(node.kind),
        "target": node.target.as_str(),
        "address": node.address.as_ref().map(widget_slot_json),
        "bounds": rect_json(&node.bounds),
        "order": {
            "z_index": node.order.z_index,
            "paint_order": node.order.paint_order,
            "traversal_order": node.order.traversal_order,
        },
        "enabled": node.enabled,
        "capture": node.capture.map(|capture| format!("{capture:?}")),
        "consumes_wheel": node.consumes_wheel,
        "blocks_pointer": node.blocks_pointer,
        "blocks_wheel": node.blocks_wheel,
    })
}

fn dispatch_graph_edge_summary(edge: &slipway_core::DispatchGraphEdge) -> Value {
    json!({
        "kind": dispatch_graph_edge_kind_name(edge.kind),
        "channel": dispatch_graph_channel_name(edge.channel),
        "from": edge.from.as_str(),
        "to": edge.to.as_str(),
    })
}

fn dispatch_graph_node_kind_name(kind: slipway_core::DispatchGraphNodeKind) -> &'static str {
    match kind {
        slipway_core::DispatchGraphNodeKind::Hit => "hit",
        slipway_core::DispatchGraphNodeKind::Focus => "focus",
        slipway_core::DispatchGraphNodeKind::Scroll => "scroll",
        slipway_core::DispatchGraphNodeKind::Occlusion => "occlusion",
    }
}

fn dispatch_graph_edge_kind_name(kind: slipway_core::DispatchGraphEdgeKind) -> &'static str {
    match kind {
        slipway_core::DispatchGraphEdgeKind::HitOrder => "hit_order",
        slipway_core::DispatchGraphEdgeKind::Occlusion => "occlusion",
        slipway_core::DispatchGraphEdgeKind::Capture => "capture",
        slipway_core::DispatchGraphEdgeKind::Chaining => "chaining",
        slipway_core::DispatchGraphEdgeKind::FocusRoute => "focus_route",
    }
}

fn dispatch_graph_channel_name(channel: slipway_core::DispatchGraphChannel) -> &'static str {
    match channel {
        slipway_core::DispatchGraphChannel::Pointer => "pointer",
        slipway_core::DispatchGraphChannel::Wheel => "wheel",
        slipway_core::DispatchGraphChannel::FocusRouted => "focus_routed",
    }
}

fn state_observation_summary(observation: &slipway_core::StateObservation) -> Value {
    json!({
        "target": observation.target.as_str(),
        "slot": observation.slot.as_ref().map(widget_slot_json),
        "name": observation.name,
        "value": observation.value,
    })
}

fn change_evidence_summary(change: &slipway_core::ChangeEvidence) -> Value {
    json!({
        "target": change.target.as_str(),
        "slot": change.slot.as_ref().map(widget_slot_json),
        "field": change.field,
        "before": change.before,
        "after": change.after,
    })
}

fn declared_dispatch_kind_name(kind: DeclaredEventDispatchKind) -> &'static str {
    match kind {
        DeclaredEventDispatchKind::Pointer => "pointer",
        DeclaredEventDispatchKind::Wheel => "wheel",
        DeclaredEventDispatchKind::Scroll => "scroll",
        DeclaredEventDispatchKind::Focus => "focus",
        DeclaredEventDispatchKind::Keyboard => "keyboard",
        DeclaredEventDispatchKind::Text => "text",
        DeclaredEventDispatchKind::Command => "command",
    }
}

fn generated_event_summary(event: &InputEvent) -> Value {
    let mut summary = json!({
        "target": event.target().as_str(),
        "target_slot": event.target_slot().map(widget_slot_json),
        "summary": input_event_summary(event),
    });
    match event {
        InputEvent::Pointer(pointer) => {
            summary["pointer"] = json!({
                "position": point_json(pointer.position),
                "target_bounds": pointer
                    .target_bounds
                    .map(|bounds| rect_json(&bounds.into_rect())),
                "kind": format!("{:?}", pointer.kind),
                "button": pointer.button.map(|button| format!("{button:?}")),
                "details": pointer_details_json(&pointer.details),
            });
        }
        InputEvent::Keyboard(keyboard) => {
            summary["keyboard"] = json!({
                "key": keyboard.key,
                "kind": format!("{:?}", keyboard.kind),
                "modifiers": modifiers_json(keyboard.modifiers),
                "details": {
                    "logical_key": keyboard.details.logical_key,
                    "physical_key": keyboard.details.physical_key,
                    "text": keyboard.details.text,
                    "repeat": keyboard.details.repeat,
                    "location": format!("{:?}", keyboard.details.location),
                },
            });
        }
        InputEvent::Text(text) => {
            summary["text"] = json!({
                "text": text.text,
            });
        }
        InputEvent::TextEdit(text_edit) => {
            summary["text_edit"] = json!({
                "kind": format!("{:?}", text_edit.kind),
                "text": text_edit.text,
                "selection_before": text_edit.selection_before.as_ref().map(text_selection_range_json),
                "selection_after": text_edit.selection_after.as_ref().map(text_selection_range_json),
            });
        }
        InputEvent::TextComposition(text_composition) => {
            summary["text_composition"] = json!({
                "phase": format!("{:?}", text_composition.phase),
                "preedit_text": text_composition.preedit_text,
                "cursor_range": text_composition.cursor_range.as_ref().map(text_selection_range_json),
            });
        }
        InputEvent::Selection(selection) => {
            summary["selection"] = json!({
                "mode": format!("{:?}", selection.state.mode),
                "ranges": selection
                    .state
                    .ranges
                    .iter()
                    .map(text_selection_range_json)
                    .collect::<Vec<_>>(),
            });
        }
        InputEvent::Wheel(wheel) => {
            summary["wheel"] = json!({
                "delta_x": wheel.delta_x,
                "delta_y": wheel.delta_y,
            });
        }
        InputEvent::Scroll(scroll) => {
            summary["scroll"] = json!({
                "region_id": scroll.region_id.as_str(),
                "offset_x": scroll.offset_x,
                "offset_y": scroll.offset_y,
                "viewport": rect_json(&scroll.viewport.into_rect()),
                "content_bounds": rect_json(&scroll.content_bounds.into_rect()),
            });
        }
        InputEvent::Focus(focus) => {
            summary["focus"] = json!({
                "focused": focus.focused,
            });
        }
        InputEvent::Command(command) => {
            summary["command"] = json!({
                "command": command.command,
                "payload_ref": command.payload_ref,
                "source": command.source.as_ref().map(|source| source.as_str()),
            });
        }
        InputEvent::Clipboard(clipboard) => {
            summary["clipboard"] = json!({
                "kind": format!("{:?}", clipboard.kind),
                "formats": clipboard.formats,
                "payload_ref": clipboard.payload_ref,
            });
        }
        InputEvent::DragDrop(drag_drop) => {
            summary["drag_drop"] = json!({
                "phase": format!("{:?}", drag_drop.phase),
                "position": point_json(drag_drop.position),
                "payloads": drag_drop
                    .payloads
                    .iter()
                    .map(|payload| json!({
                        "format": payload.format,
                        "payload_ref": payload.payload_ref,
                        "size_bytes": payload.size_bytes,
                    }))
                    .collect::<Vec<_>>(),
            });
        }
        InputEvent::File(file) => {
            summary["file"] = json!({
                "files": file
                    .files
                    .iter()
                    .map(|file| json!({
                        "name": file.name,
                        "mime_type": file.mime_type,
                        "size_bytes": file.size_bytes,
                        "payload_ref": file.payload_ref,
                    }))
                    .collect::<Vec<_>>(),
            });
        }
    }
    summary
}

fn pointer_details_json(details: &PointerDetails) -> Value {
    json!({
        "pointer_id": details.pointer_id,
        "device": format!("{:?}", details.device),
        "buttons": {
            "primary": details.buttons.primary,
            "secondary": details.buttons.secondary,
            "auxiliary": details.buttons.auxiliary,
        },
        "modifiers": modifiers_json(details.modifiers),
        "pressure": details.pressure,
        "tilt_x": details.tilt_x,
        "tilt_y": details.tilt_y,
        "twist": details.twist,
    })
}

fn modifiers_json(modifiers: Modifiers) -> Value {
    json!({
        "shift": modifiers.shift,
        "control": modifiers.control,
        "alt": modifiers.alt,
        "meta": modifiers.meta,
    })
}

fn text_selection_range_json(range: &slipway_core::TextSelectionRange) -> Value {
    json!({
        "anchor": range.anchor,
        "focus": range.focus,
    })
}

fn input_event_summary(event: &InputEvent) -> String {
    match event {
        InputEvent::Pointer(pointer) => format!("pointer:{:?}", pointer.kind),
        InputEvent::Keyboard(keyboard) => format!("keyboard:{:?}", keyboard.kind),
        InputEvent::Text(_) => "text".to_string(),
        InputEvent::TextEdit(text_edit) => format!("text_edit:{:?}", text_edit.kind),
        InputEvent::TextComposition(text_composition) => {
            format!("text_composition:{:?}", text_composition.phase)
        }
        InputEvent::Selection(_) => "selection".to_string(),
        InputEvent::Wheel(_) => "wheel".to_string(),
        InputEvent::Scroll(_) => "scroll".to_string(),
        InputEvent::Focus(focus) => format!("focus:{}", focus.focused),
        InputEvent::Command(command) => format!("command:{}", command.command),
        InputEvent::Clipboard(clipboard) => format!("clipboard:{:?}", clipboard.kind),
        InputEvent::DragDrop(drag_drop) => format!("drag_drop:{:?}", drag_drop.phase),
        InputEvent::File(file) => format!("file:{}", file.files.len()),
    }
}

fn event_route_summary(route: &slipway_core::EventRoute) -> Value {
    json!({
        "route_id": route.route_id.as_deref(),
        "address": route.address.as_ref().map(widget_slot_json),
        "path": route.path.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
        "phase": format!("{:?}", route.phase),
    })
}

fn widget_slot_json(slot: &WidgetSlotAddress) -> Value {
    json!({
        "widget": slot.widget.as_str(),
        "path": slot.path.iter().map(|id| id.as_str()).collect::<Vec<_>>(),
        "ordinal": slot.ordinal,
    })
}

fn control_trace_mode_name(mode: slipway_debug_bridge::DebugControlMode) -> &'static str {
    match mode {
        slipway_debug_bridge::DebugControlMode::SemanticDirect => "semantic_direct",
        slipway_debug_bridge::DebugControlMode::PhysicalEquivalent => "physical_equivalent",
    }
}

fn control_trace_stage_summary(stage: &slipway_debug_bridge::DebugControlTraceStage) -> Value {
    json!({
        "stage": control_trace_stage_name(stage.stage),
        "actor": stage.actor.as_str(),
        "target": stage.target.as_ref().map(WidgetId::as_str),
        "detail": stage.detail.as_str(),
    })
}

fn control_trace_stage_name(
    stage: slipway_debug_bridge::DebugControlTraceStageKind,
) -> &'static str {
    match stage {
        slipway_debug_bridge::DebugControlTraceStageKind::Generated => "generated",
        slipway_debug_bridge::DebugControlTraceStageKind::Routed => "routed",
        slipway_debug_bridge::DebugControlTraceStageKind::Consumed => "consumed",
        slipway_debug_bridge::DebugControlTraceStageKind::Ignored => "ignored",
        slipway_debug_bridge::DebugControlTraceStageKind::Reduced => "reduced",
    }
}

fn frame_json(frame: &FrameIdentity) -> Value {
    json!({
        "surface_id": frame.surface_id,
        "surface_instance_id": frame.surface_instance_id,
        "revision": frame.revision,
        "frame_index": frame.frame_index,
        "viewport": rect_json(&frame.viewport),
    })
}

fn rect_json(rect: &Rect) -> Value {
    json!({
        "origin": {
            "x": rect.origin.x,
            "y": rect.origin.y,
        },
        "size": {
            "width": rect.size.width,
            "height": rect.size.height,
        },
    })
}

fn point_json(point: Point) -> Value {
    json!({
        "x": point.x,
        "y": point.y,
    })
}

fn bridge_method_name(method: McpProbeMethod) -> &'static str {
    match method {
        McpProbeMethod::Status => "status",
        McpProbeMethod::Probe => "probe",
        McpProbeMethod::Render => "render",
        McpProbeMethod::Screenshot => "screenshot",
        McpProbeMethod::Control => "control",
        McpProbeMethod::Resize => "resize",
    }
}

fn request_id_string(id: Option<&Value>) -> String {
    match id {
        Some(Value::String(value)) => value.clone(),
        Some(Value::Number(value)) => value.to_string(),
        Some(other) => other.to_string(),
        None => "notification".to_string(),
    }
}

fn parse_frame_from_arguments(arguments: &Value) -> Result<FrameIdentity, RpcError> {
    parse_frame(required_object_field(arguments, "frame")?)
}

fn parse_presented_screenshot_request(
    arguments: &Value,
    origin: ScreenshotParseOrigin,
) -> Result<PresentedScreenshotRequest, RpcError> {
    let object = arguments
        .as_object()
        .ok_or_else(|| RpcError::invalid_params("tool arguments must be an object"))?;
    if object.contains_key("_slipway_frame_admission") {
        return Err(RpcError::invalid_params(
            "field `_slipway_frame_admission` is not supported",
        ));
    }
    let frame = parse_frame(required_object_field(arguments, "frame")?)?;
    let selector = match origin {
        ScreenshotParseOrigin::Direct
        | ScreenshotParseOrigin::RuntimeNormalized(RuntimeScreenshotSelector::Exact) => {
            PresentedScreenshotSelector::Exact {
                expected_frame: frame,
            }
        }
        ScreenshotParseOrigin::RuntimeNormalized(RuntimeScreenshotSelector::Current) => {
            PresentedScreenshotSelector::Current {
                request_context: frame,
            }
        }
    };
    Ok(PresentedScreenshotRequest { selector })
}

fn parse_render_frame(arguments: &Value) -> Result<FrameIdentity, RpcError> {
    if let Ok(frame) = parse_frame_from_arguments(arguments) {
        return Ok(frame);
    }
    let packet = required_object_field(arguments, "packet")?;
    parse_frame(required_object_field(packet, "frame")?)
}

fn parse_render_packet(arguments: &Value) -> Result<RenderPacket, RpcError> {
    let packet = arguments.get("packet").unwrap_or(arguments);
    let frame = if let Some(frame) = packet.get("frame") {
        parse_frame(frame)?
    } else {
        parse_frame_from_arguments(arguments)?
    };
    let target = optional_string_field(packet, "target")?
        .map(parse_widget_id)
        .unwrap_or_else(|| parse_widget_id(&frame.surface_instance_id));
    Ok(RenderPacket {
        target,
        frame: frame.clone(),
        layout: LayoutOutput {
            bounds: TargetLocalRect::new(frame.viewport),
            child_placements: Vec::new(),
            diagnostics: Vec::new(),
        },
        paint: Vec::new(),
        surfaces: Vec::new(),
        diagnostics: Vec::new(),
    })
}

fn parse_probe_request(arguments: &Value) -> Result<ProbeRequest, RpcError> {
    let target = optional_string_field(arguments, "target")?.map(parse_widget_id);
    let event_trace_limit =
        optional_usize_alias_field(arguments, EVENT_TRACE_LIMIT_FIELDS, "event trace limit")?;
    let kinds_value = required_array_field(arguments, "kinds")?;
    let mut kinds = Vec::with_capacity(kinds_value.len());
    for kind in kinds_value {
        let kind = kind
            .as_str()
            .ok_or_else(|| RpcError::invalid_params("probe kinds must be strings"))?;
        kinds.push(parse_probe_kind(kind)?);
    }
    Ok(ProbeRequest {
        target,
        kinds,
        event_trace_limit,
    })
}

fn parse_control_trace_flag(arguments: &Value) -> Result<bool, RpcError> {
    Ok(optional_bool_field(arguments, "trace")?.unwrap_or(false))
}

fn parse_probe_kind(kind: &str) -> Result<ProbeKind, RpcError> {
    match kind {
        "topology" | "Topology" => Ok(ProbeKind::Topology),
        "state" | "State" => Ok(ProbeKind::State),
        "event" | "Event" => Ok(ProbeKind::Event),
        "change" | "Change" => Ok(ProbeKind::Change),
        "diagnostics" | "Diagnostics" => Ok(ProbeKind::Diagnostics),
        "paint" | "Paint" => Ok(ProbeKind::Paint),
        "semantics" | "Semantics" => Ok(ProbeKind::Semantics),
        "surface" | "Surface" => Ok(ProbeKind::Surface),
        "layout_intent" | "layoutIntent" | "LayoutIntent" => Ok(ProbeKind::LayoutIntent),
        "view_definition" | "viewDefinition" | "ViewDefinition" => Ok(ProbeKind::ViewDefinition),
        "render_packet" | "renderPacket" | "RenderPacket" => Ok(ProbeKind::RenderPacket),
        "render_evidence" | "renderEvidence" | "RenderEvidence" => Ok(ProbeKind::RenderEvidence),
        "dispatch_graph" | "dispatchGraph" | "DispatchGraph" => Ok(ProbeKind::DispatchGraph),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported probe kind `{kind}`"
        ))),
    }
}

fn parse_input_event(event: &Value) -> Result<InputEvent, RpcError> {
    let kind =
        required_string_field(event, "type").or_else(|_| required_string_field(event, "kind"))?;
    let target = parse_widget_id(required_string_field(event, "target")?);
    let target_slot = parse_optional_widget_slot_address(event)?;
    match kind {
        "command" | "Command" => Ok(InputEvent::Command(CommandEvent {
            target,
            target_slot,
            command: required_string_field(event, "command")?.to_string(),
            payload_ref: optional_string_field(event, "payload_ref")?.map(str::to_string),
            source: optional_string_field(event, "source")?.map(parse_widget_id),
        })),
        "text" | "Text" => Ok(InputEvent::Text(TextInputEvent {
            target,
            target_slot,
            text: required_string_field(event, "text")?.to_string(),
        })),
        "wheel" | "Wheel" => Ok(InputEvent::Wheel(WheelEvent {
            target,
            target_slot,
            region_id: None,
            delta_x: optional_f32_field(event, "delta_x")?.unwrap_or(0.0),
            delta_y: optional_f32_field(event, "delta_y")?.unwrap_or(0.0),
        })),
        "focus" | "Focus" => Ok(InputEvent::Focus(FocusEvent {
            target,
            target_slot,
            focused: optional_bool_field(event, "focused")?.unwrap_or(true),
        })),
        "keyboard" | "Keyboard" => Ok(InputEvent::Keyboard(KeyboardEvent {
            target,
            target_slot,
            key: required_string_field(event, "key")?.to_string(),
            kind: parse_key_event_kind(optional_string_field(event, "phase")?.unwrap_or("press"))?,
            modifiers: parse_modifiers(event.get("modifiers")),
            details: KeyboardDetails {
                logical_key: optional_string_field(event, "logical_key")?.map(str::to_string),
                physical_key: optional_string_field(event, "physical_key")?.map(str::to_string),
                text: optional_string_field(event, "text")?.map(str::to_string),
                repeat: optional_bool_field(event, "repeat")?.unwrap_or(false),
                location: parse_key_location(
                    optional_string_field(event, "location")?.unwrap_or("unknown"),
                )?,
            },
        })),
        "pointer" | "Pointer" => Ok(InputEvent::Pointer(PointerEvent {
            target,
            target_slot,
            position: parse_point(required_object_field(event, "position")?)?,
            target_bounds: None,
            kind: parse_pointer_event_kind(
                optional_string_field(event, "phase")?.unwrap_or("move"),
            )?,
            button: optional_string_field(event, "button")?
                .map(parse_pointer_button)
                .transpose()?,
            details: PointerDetails {
                pointer_id: optional_u64_field(event, "pointer_id")?,
                device: parse_pointer_device(
                    optional_string_field(event, "device")?.unwrap_or("unknown"),
                )?,
                buttons: parse_pointer_buttons(event.get("buttons")),
                modifiers: parse_modifiers(event.get("modifiers")),
                pressure: optional_f32_field(event, "pressure")?,
                tilt_x: optional_f32_field(event, "tilt_x")?,
                tilt_y: optional_f32_field(event, "tilt_y")?,
                twist: optional_f32_field(event, "twist")?,
            },
        })),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported input event type `{kind}`"
        ))),
    }
}

fn parse_physical_control(operation: &Value) -> Result<DebugPhysicalControl, RpcError> {
    let kind = required_string_field(operation, "type")
        .or_else(|_| required_string_field(operation, "kind"))?;
    match kind {
        "pointer" | "Pointer" => {
            let pointer_kind = parse_pointer_event_kind(
                optional_string_field(operation, "phase")?.unwrap_or("press"),
            )?;
            let button = optional_string_field(operation, "button")?
                .map(parse_pointer_button)
                .transpose()?;
            let buttons = operation
                .get("buttons")
                .map(|buttons| parse_pointer_buttons(Some(buttons)))
                .unwrap_or_else(|| pointer_buttons_from_button(button));
            Ok(DebugPhysicalControl::Pointer {
                position: parse_point(required_object_field(operation, "position")?)?,
                kind: pointer_kind,
                button,
                details: PointerDetails {
                    pointer_id: optional_u64_field(operation, "pointer_id")?,
                    device: parse_pointer_device(
                        optional_string_field(operation, "device")?.unwrap_or("mouse"),
                    )?,
                    buttons,
                    modifiers: parse_modifiers(operation.get("modifiers")),
                    pressure: optional_f32_field(operation, "pressure")?,
                    tilt_x: optional_f32_field(operation, "tilt_x")?,
                    tilt_y: optional_f32_field(operation, "tilt_y")?,
                    twist: optional_f32_field(operation, "twist")?,
                },
                pointer_is_pressed: optional_bool_field(operation, "pointer_is_pressed")?
                    .unwrap_or_else(|| {
                        matches!(
                            pointer_kind,
                            PointerEventKind::Press
                                | PointerEventKind::Move
                                | PointerEventKind::Release
                        )
                    }),
            })
        }
        "wheel" | "Wheel" => Ok(DebugPhysicalControl::Wheel {
            position: parse_point(required_object_field(operation, "position")?)?,
            delta_x: optional_f32_field(operation, "delta_x")?.unwrap_or(0.0),
            delta_y: optional_f32_field(operation, "delta_y")?.unwrap_or(0.0),
        }),
        "focus" | "Focus" => Ok(DebugPhysicalControl::Focus {
            selector: parse_physical_control_selector(operation)?,
            focused: optional_bool_field(operation, "focused")?.unwrap_or(true),
        }),
        "text" | "Text" => Ok(DebugPhysicalControl::Text {
            selector: parse_physical_control_selector(operation)?,
            text: required_string_field(operation, "text")?.to_string(),
        }),
        "text_edit" | "TextEdit" | "textEdit" => Ok(DebugPhysicalControl::TextEdit {
            selector: parse_physical_control_selector(operation)?,
            kind: parse_text_edit_kind(
                optional_string_field(operation, "edit_kind")?.unwrap_or("replace_selection"),
            )?,
            text: optional_string_field(operation, "text")?.map(str::to_string),
            selection_before: optional_text_selection_range(operation, "selection_before")?,
            selection_after: optional_text_selection_range(operation, "selection_after")?,
        }),
        "text_composition" | "TextComposition" | "textComposition" => {
            let selector = parse_physical_control_selector(operation)?;
            let updates_value = required_array_field(operation, "updates")?;
            if updates_value.is_empty() || updates_value.len() > MAX_COMPOSITION_UPDATES {
                return Err(RpcError::invalid_params(format!(
                    "text composition requires 1 through {MAX_COMPOSITION_UPDATES} updates"
                )));
            }

            let commit = required_string_field(operation, "commit")?;
            if commit.is_empty() {
                return Err(RpcError::invalid_params(
                    "text composition commit must be non-empty",
                ));
            }
            let mut total_bytes = commit.len();
            let mut updates = Vec::with_capacity(updates_value.len());
            for (index, update) in updates_value.iter().enumerate() {
                if !update.is_object() {
                    return Err(RpcError::invalid_params(format!(
                        "text composition updates[{index}] must be an object"
                    )));
                }
                let preedit_text = required_string_field(update, "preedit_text")?;
                if preedit_text.is_empty() {
                    return Err(RpcError::invalid_params(format!(
                        "text composition updates[{index}].preedit_text must be non-empty"
                    )));
                }
                total_bytes = total_bytes.checked_add(preedit_text.len()).ok_or_else(|| {
                    RpcError::invalid_params("text composition UTF-8 byte count overflowed")
                })?;
                if total_bytes > MAX_COMPOSITION_UTF8_BYTES {
                    return Err(RpcError::invalid_params(format!(
                        "text composition exceeds {MAX_COMPOSITION_UTF8_BYTES} total UTF-8 bytes"
                    )));
                }
                let cursor_range = optional_text_selection_range(update, "cursor_range")?;
                if let Some(range) = &cursor_range {
                    let scalar_count = preedit_text.chars().count();
                    if range.anchor > scalar_count || range.focus > scalar_count {
                        return Err(RpcError::invalid_params(format!(
                            "text composition updates[{index}].cursor_range must use Unicode scalar indices within the preedit text"
                        )));
                    }
                }
                updates.push(DebugTextCompositionUpdate {
                    preedit_text: preedit_text.to_string(),
                    cursor_range,
                });
            }

            Ok(DebugPhysicalControl::TextComposition {
                selector,
                updates,
                commit: commit.to_string(),
            })
        }
        "keyboard" | "Keyboard" => Ok(DebugPhysicalControl::Keyboard {
            selector: parse_physical_control_selector(operation)?,
            key: required_string_field(operation, "key")?.to_string(),
            kind: parse_key_event_kind(
                optional_string_field(operation, "phase")?.unwrap_or("press"),
            )?,
            modifiers: parse_modifiers(operation.get("modifiers")),
            details: KeyboardDetails {
                logical_key: optional_string_field(operation, "logical_key")?.map(str::to_string),
                physical_key: optional_string_field(operation, "physical_key")?.map(str::to_string),
                text: optional_string_field(operation, "text")?.map(str::to_string),
                repeat: optional_bool_field(operation, "repeat")?.unwrap_or(false),
                location: parse_key_location(
                    optional_string_field(operation, "location")?.unwrap_or("unknown"),
                )?,
            },
        }),
        "command" | "Command" => Ok(DebugPhysicalControl::Command {
            selector: parse_physical_control_selector(operation)?,
            command: required_string_field(operation, "command")?.to_string(),
            payload_ref: optional_string_field(operation, "payload_ref")?.map(str::to_string),
        }),
        "scroll" | "Scroll" => Ok(DebugPhysicalControl::Scroll {
            selector: parse_physical_control_selector(operation)?,
            offset_x: optional_f32_field(operation, "offset_x")?.unwrap_or(0.0),
            offset_y: optional_f32_field(operation, "offset_y")?.unwrap_or(0.0),
        }),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported physical control operation `{kind}`"
        ))),
    }
}

fn parse_physical_control_selector(
    operation: &Value,
) -> Result<DebugPhysicalControlDeclarationSelector, RpcError> {
    let target = optional_string_field(operation, "target")?;
    let region = optional_string_field(operation, "region")?;
    let position = operation.get("position");
    let selector_count = usize::from(target.is_some())
        + usize::from(region.is_some())
        + usize::from(position.is_some());

    if selector_count != 1 {
        return Err(RpcError::invalid_params(
            "physical control operation must include exactly one of `target`, `region`, or `position`",
        ));
    }

    if let Some(target) = target {
        return Ok(DebugPhysicalControlDeclarationSelector::Target {
            target: parse_widget_id(target),
        });
    }

    if let Some(region) = region {
        return Ok(DebugPhysicalControlDeclarationSelector::Region {
            region: PresentationRegionId::from(region),
        });
    }

    let position = position.expect("selector count guarantees position is present");
    if !position.is_object() {
        return Err(RpcError::invalid_params(
            "field `position` must be an object",
        ));
    }
    Ok(DebugPhysicalControlDeclarationSelector::Position {
        position: parse_point(position)?,
    })
}

fn parse_frame(value: &Value) -> Result<FrameIdentity, RpcError> {
    Ok(FrameIdentity {
        surface_id: required_string_field(value, "surface_id")?.to_string(),
        surface_instance_id: required_string_field(value, "surface_instance_id")?.to_string(),
        revision: required_u64_field(value, "revision")?,
        frame_index: required_u64_field(value, "frame_index")?,
        viewport: parse_rect(required_object_field(value, "viewport")?)?,
    })
}

fn parse_rect(value: &Value) -> Result<Rect, RpcError> {
    Ok(Rect {
        origin: parse_point(required_object_field(value, "origin")?)?,
        size: parse_size(required_object_field(value, "size")?)?,
    })
}

fn parse_point(value: &Value) -> Result<Point, RpcError> {
    Ok(Point {
        x: required_f32_field(value, "x")?,
        y: required_f32_field(value, "y")?,
    })
}

fn parse_size(value: &Value) -> Result<Size, RpcError> {
    Ok(Size {
        width: required_f32_field(value, "width")?,
        height: required_f32_field(value, "height")?,
    })
}

fn parse_widget_id(value: &str) -> WidgetId {
    WidgetId::from(value.to_string())
}

fn parse_optional_widget_slot_address(
    value: &Value,
) -> Result<Option<WidgetSlotAddress>, RpcError> {
    let Some(slot) = value.get("target_slot") else {
        return Ok(None);
    };
    if !slot.is_object() {
        return Err(RpcError::invalid_params(
            "field `target_slot` must be an object",
        ));
    }
    let widget = parse_widget_id(required_string_field(slot, "widget")?);
    let path = required_array_field(slot, "path")?
        .iter()
        .map(|entry| {
            entry
                .as_str()
                .map(parse_widget_id)
                .ok_or_else(|| RpcError::invalid_params("field `target_slot.path` must be strings"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let ordinal = required_u64_field(slot, "ordinal")?;
    let ordinal = usize::try_from(ordinal).map_err(|_| {
        RpcError::invalid_params("field `target_slot.ordinal` is too large for this platform")
    })?;

    Ok(Some(WidgetSlotAddress {
        widget,
        path,
        ordinal,
    }))
}

fn parse_key_event_kind(value: &str) -> Result<KeyEventKind, RpcError> {
    match value {
        "press" | "Press" => Ok(KeyEventKind::Press),
        "release" | "Release" => Ok(KeyEventKind::Release),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported keyboard phase `{value}`"
        ))),
    }
}

fn parse_key_location(value: &str) -> Result<KeyLocation, RpcError> {
    match value {
        "standard" | "Standard" => Ok(KeyLocation::Standard),
        "left" | "Left" => Ok(KeyLocation::Left),
        "right" | "Right" => Ok(KeyLocation::Right),
        "numpad" | "Numpad" => Ok(KeyLocation::Numpad),
        "unknown" | "Unknown" => Ok(KeyLocation::Unknown),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported keyboard location `{value}`"
        ))),
    }
}

fn parse_pointer_event_kind(value: &str) -> Result<PointerEventKind, RpcError> {
    match value {
        "move" | "Move" => Ok(PointerEventKind::Move),
        "press" | "Press" => Ok(PointerEventKind::Press),
        "release" | "Release" => Ok(PointerEventKind::Release),
        "enter" | "Enter" => Ok(PointerEventKind::Enter),
        "leave" | "Leave" => Ok(PointerEventKind::Leave),
        "cancel" | "Cancel" => Ok(PointerEventKind::Cancel),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported pointer phase `{value}`"
        ))),
    }
}

fn parse_text_edit_kind(value: &str) -> Result<TextEditKind, RpcError> {
    match value {
        "insert_text" | "insertText" | "InsertText" => Ok(TextEditKind::InsertText),
        "delete_backward" | "deleteBackward" | "DeleteBackward" => Ok(TextEditKind::DeleteBackward),
        "delete_forward" | "deleteForward" | "DeleteForward" => Ok(TextEditKind::DeleteForward),
        "move_caret" | "moveCaret" | "MoveCaret" => Ok(TextEditKind::MoveCaret),
        "replace_selection" | "replaceSelection" | "ReplaceSelection" => {
            Ok(TextEditKind::ReplaceSelection)
        }
        "replace_buffer" | "replaceBuffer" | "ReplaceBuffer" => Ok(TextEditKind::ReplaceBuffer),
        "unknown" | "Unknown" => Ok(TextEditKind::Unknown),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported text edit kind `{value}`"
        ))),
    }
}

fn optional_text_selection_range(
    value: &Value,
    field: &str,
) -> Result<Option<TextSelectionRange>, RpcError> {
    let Some(range) = value.get(field) else {
        return Ok(None);
    };
    if !range.is_object() {
        return Err(RpcError::invalid_params(format!(
            "field `{field}` must be an object"
        )));
    }
    Ok(Some(TextSelectionRange {
        anchor: required_u64_field(range, "anchor")? as usize,
        focus: required_u64_field(range, "focus")? as usize,
    }))
}

fn parse_pointer_button(value: &str) -> Result<PointerButton, RpcError> {
    match value {
        "primary" | "Primary" => Ok(PointerButton::Primary),
        "secondary" | "Secondary" => Ok(PointerButton::Secondary),
        "auxiliary" | "Auxiliary" => Ok(PointerButton::Auxiliary),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported pointer button `{value}`"
        ))),
    }
}

fn parse_pointer_device(value: &str) -> Result<PointerDeviceKind, RpcError> {
    match value {
        "unknown" | "Unknown" => Ok(PointerDeviceKind::Unknown),
        "mouse" | "Mouse" => Ok(PointerDeviceKind::Mouse),
        "touch" | "Touch" => Ok(PointerDeviceKind::Touch),
        "pen" | "Pen" => Ok(PointerDeviceKind::Pen),
        _ => Err(RpcError::invalid_params(format!(
            "unsupported pointer device `{value}`"
        ))),
    }
}

fn parse_modifiers(value: Option<&Value>) -> Modifiers {
    let Some(value) = value else {
        return Modifiers::default();
    };
    Modifiers {
        shift: value.get("shift").and_then(Value::as_bool).unwrap_or(false),
        control: value
            .get("control")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        alt: value.get("alt").and_then(Value::as_bool).unwrap_or(false),
        meta: value.get("meta").and_then(Value::as_bool).unwrap_or(false),
    }
}

fn parse_pointer_buttons(value: Option<&Value>) -> PointerButtons {
    let Some(value) = value else {
        return PointerButtons::default();
    };
    PointerButtons {
        primary: value
            .get("primary")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        secondary: value
            .get("secondary")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        auxiliary: value
            .get("auxiliary")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    }
}

fn pointer_buttons_from_button(button: Option<PointerButton>) -> PointerButtons {
    let mut buttons = PointerButtons::default();
    match button {
        Some(PointerButton::Primary) => buttons.primary = true,
        Some(PointerButton::Secondary) => buttons.secondary = true,
        Some(PointerButton::Auxiliary) => buttons.auxiliary = true,
        None => {}
    }
    buttons
}

fn required_object_field<'a>(value: &'a Value, field: &str) -> Result<&'a Value, RpcError> {
    let field_value = value
        .get(field)
        .ok_or_else(|| RpcError::invalid_params(format!("missing required field `{field}`")))?;
    if field_value.is_object() {
        Ok(field_value)
    } else {
        Err(RpcError::invalid_params(format!(
            "field `{field}` must be an object"
        )))
    }
}

fn required_array_field<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>, RpcError> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| RpcError::invalid_params(format!("field `{field}` must be an array")))
}

fn required_string_field<'a>(value: &'a Value, field: &str) -> Result<&'a str, RpcError> {
    value
        .get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params(format!("field `{field}` must be a string")))
}

fn optional_string_field<'a>(value: &'a Value, field: &str) -> Result<Option<&'a str>, RpcError> {
    match value.get(field) {
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(RpcError::invalid_params(format!(
            "field `{field}` must be a string"
        ))),
        None => Ok(None),
    }
}

fn required_u64_field(value: &Value, field: &str) -> Result<u64, RpcError> {
    value
        .get(field)
        .and_then(Value::as_u64)
        .ok_or_else(|| RpcError::invalid_params(format!("field `{field}` must be an integer")))
}

fn optional_u64_field(value: &Value, field: &str) -> Result<Option<u64>, RpcError> {
    match value.get(field) {
        Some(value) => value
            .as_u64()
            .map(Some)
            .ok_or_else(|| RpcError::invalid_params(format!("field `{field}` must be an integer"))),
        None => Ok(None),
    }
}

fn optional_usize_alias_field(
    value: &Value,
    fields: &[&str],
    label: &str,
) -> Result<Option<usize>, RpcError> {
    let mut seen: Option<(usize, &str)> = None;
    for &field in fields {
        if value.get(field).is_none() {
            continue;
        }
        let raw =
            optional_u64_field(value, field)?.expect("field presence was checked before parsing");
        let parsed = usize::try_from(raw).map_err(|_| {
            RpcError::invalid_params(format!("field `{field}` is too large for usize"))
        })?;
        if let Some((existing, existing_field)) = seen {
            if existing != parsed {
                return Err(RpcError::invalid_params(format!(
                    "conflicting {label} aliases `{existing_field}` and `{field}`"
                )));
            }
        } else {
            seen = Some((parsed, field));
        }
    }
    Ok(seen.map(|(value, _field)| value))
}

fn required_f32_field(value: &Value, field: &str) -> Result<f32, RpcError> {
    value
        .get(field)
        .and_then(Value::as_f64)
        .map(|value| value as f32)
        .ok_or_else(|| RpcError::invalid_params(format!("field `{field}` must be a number")))
}

fn optional_f32_field(value: &Value, field: &str) -> Result<Option<f32>, RpcError> {
    match value.get(field) {
        Some(value) => value
            .as_f64()
            .map(|value| Some(value as f32))
            .ok_or_else(|| RpcError::invalid_params(format!("field `{field}` must be a number"))),
        None => Ok(None),
    }
}

fn optional_bool_field(value: &Value, field: &str) -> Result<Option<bool>, RpcError> {
    match value.get(field) {
        Some(value) => value
            .as_bool()
            .map(Some)
            .ok_or_else(|| RpcError::invalid_params(format!("field `{field}` must be a boolean"))),
        None => Ok(None),
    }
}

#[derive(Debug, Eq, PartialEq)]
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;
    use slipway_core::{DiagnosticSeverity, ProbeProduct};
    use slipway_debug_bridge::bounded_debug_bridge;

    fn frame_json_value() -> Value {
        json!({
            "surface_id": "surface",
            "surface_instance_id": "instance",
            "revision": 7,
            "frame_index": 11,
            "viewport": {
                "origin": { "x": 0.0, "y": 0.0 },
                "size": { "width": 320.0, "height": 180.0 }
            }
        })
    }

    fn frame() -> FrameIdentity {
        parse_frame(&frame_json_value()).expect("test frame parses")
    }

    #[test]
    fn dispatch_graph_probe_kind_parses_and_serializes_nodes_and_edges() {
        assert_eq!(
            parse_probe_kind("dispatch_graph").expect("snake case parses"),
            ProbeKind::DispatchGraph
        );
        assert_eq!(
            parse_probe_kind("dispatchGraph").expect("camel case parses"),
            ProbeKind::DispatchGraph
        );

        let frame = frame();
        let probe = slipway_core::DispatchGraphProbe {
            target: slipway_core::WidgetId::from("graph-root"),
            frame: frame.clone(),
            graph: slipway_core::DispatchGraph {
                target: slipway_core::WidgetId::from("graph-root"),
                nodes: vec![slipway_core::DispatchGraphNode {
                    id: "hit-a".to_string(),
                    kind: slipway_core::DispatchGraphNodeKind::Hit,
                    target: slipway_core::WidgetId::from("graph-root"),
                    address: None,
                    bounds: frame.viewport,
                    order: slipway_core::HitRegionOrder::default(),
                    enabled: true,
                    capture: Some(slipway_core::PointerCaptureIntent::DuringDrag),
                    consumes_wheel: None,
                    blocks_pointer: None,
                    blocks_wheel: None,
                }],
                edges: vec![slipway_core::DispatchGraphEdge {
                    kind: slipway_core::DispatchGraphEdgeKind::Occlusion,
                    channel: slipway_core::DispatchGraphChannel::Wheel,
                    from: "occlusion:graph-root:10:0:0:0".to_string(),
                    to: "scroll-root".to_string(),
                }],
            },
        };

        let summary = probe_product_summary(&ProbeProduct::DispatchGraph(probe));
        assert_eq!(summary["kind"], "dispatch_graph");
        assert_eq!(summary["target"], "graph-root");
        assert_eq!(summary["frame"]["frame_index"], 11);
        assert_eq!(summary["graph"]["nodes"][0]["id"], "hit-a");
        assert_eq!(summary["graph"]["nodes"][0]["kind"], "hit");
        assert_eq!(summary["graph"]["nodes"][0]["capture"], "DuringDrag");
        assert_eq!(summary["graph"]["edges"][0]["kind"], "occlusion");
        assert_eq!(summary["graph"]["edges"][0]["channel"], "wheel");
        assert_eq!(
            summary["graph"]["edges"][0]["from"],
            "occlusion:graph-root:10:0:0:0"
        );
        assert_eq!(summary["graph"]["edges"][0]["to"], "scroll-root");
    }

    fn request(id: Value, method: &str, params: Value) -> Value {
        json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        })
    }

    fn call_request(id: Value, name: &str, arguments: Value) -> Value {
        request(
            id,
            "tools/call",
            json!({
                "name": name,
                "arguments": arguments,
            }),
        )
    }

    fn control_arguments(trace: Option<bool>) -> Value {
        let mut arguments = json!({
            "frame": frame_json_value(),
            "event": {
                "type": "command",
                "target": "widget",
                "command": "activate"
            }
        });
        if let Some(trace) = trace {
            arguments["trace"] = json!(trace);
        }
        arguments
    }

    fn physical_control_arguments() -> Value {
        json!({
            "frame": frame_json_value(),
            "operation": {
                "type": "pointer",
                "phase": "press",
                "position": { "x": 8.0, "y": 9.0 },
                "button": "primary",
                "device": "mouse"
            }
        })
    }

    fn physical_text_control_arguments() -> Value {
        json!({
            "frame": frame_json_value(),
            "operation": {
                "type": "text",
                "target": "search",
                "text": "abc"
            }
        })
    }

    fn render_arguments() -> Value {
        json!({
            "packet": {
                "target": "widget",
                "frame": frame_json_value(),
                "paint": []
            }
        })
    }

    fn render_arguments_without_target() -> Value {
        json!({
            "frame": frame_json_value()
        })
    }

    fn control_server() -> DebugMcpServer {
        DebugMcpServer::new(DebugMcpConfig {
            allow_status: true,
            allow_probe: false,
            allow_render: false,
            allow_screenshot: false,
            allow_control: true,
            allow_resize: false,
        })
    }

    fn render_server() -> DebugMcpServer {
        DebugMcpServer::new(DebugMcpConfig {
            allow_status: true,
            allow_probe: false,
            allow_render: true,
            allow_screenshot: true,
            allow_control: false,
            allow_resize: false,
        })
    }

    fn tool_payload(response: &Value) -> Value {
        let text = response["result"]["content"][0]["text"]
            .as_str()
            .expect("tool result text");
        serde_json::from_str(text).expect("tool result text is structured JSON")
    }

    #[derive(Default)]
    struct FakeHandler {
        calls: Vec<DebugCommand>,
    }

    impl SlipwayDebugCommandHandler for FakeHandler {
        fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
            let request_id = command.request_id.clone();
            self.calls.push(command.clone());
            match command.kind {
                slipway_debug_bridge::DebugCommandKind::Status { .. } => {
                    DebugReplyProduct::Status(DebugStatus {
                        admitted: true,
                        detail: "fake handler ready".to_string(),
                        revision: 3,
                        backend_id: Some("fake-backend".to_string()),
                        trace_buffer_depth: 1,
                        trace_buffer_capacity: 32,
                        refused_debug_replies: 2,
                        unhandled_backend_input_traces: 4,
                    })
                }
                slipway_debug_bridge::DebugCommandKind::Probe { request, .. } => {
                    DebugReplyProduct::Probes(
                        request
                            .kinds
                            .into_iter()
                            .map(|kind| {
                                ProbeProduct::Diagnostic(Diagnostic {
                                    target: None,
                                    severity: DiagnosticSeverity::Info,
                                    code: format!("{kind:?}"),
                                    message: "probe observed".to_string(),
                                })
                            })
                            .collect(),
                    )
                }
                slipway_debug_bridge::DebugCommandKind::Render { packet } => {
                    DebugReplyProduct::Render(RenderProduct::Refusal(slipway_core::RenderRefusal {
                        target: Some(packet.target),
                        frame: packet.frame,
                        source: Some(EvidenceSource::canonical_offscreen("fake-handler")),
                        provider_id: Some("fake-handler".to_string()),
                        reason: "fake handler has no renderer".to_string(),
                        diagnostics: Vec::new(),
                    }))
                }
                slipway_debug_bridge::DebugCommandKind::Screenshot { request } => {
                    DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Refusal(
                        PresentedScreenshotRefusal {
                            selector: request.selector,
                            captured_frame: None,
                            backend_id: Some("fake-backend".to_string()),
                            code: "screenshot-no-window".to_string(),
                            reason: "fake handler has no visible window".to_string(),
                            diagnostics: Vec::new(),
                        },
                    ))
                }
                slipway_debug_bridge::DebugCommandKind::Control {
                    frame,
                    event,
                    trace,
                } => {
                    if trace {
                        DebugReplyProduct::ControlTrace(
                            slipway_debug_bridge::DebugControlTrace::new(
                                request_id,
                                frame,
                                &event,
                                true,
                                7,
                                8,
                                vec![Diagnostic {
                                    target: Some(event.target().clone()),
                                    severity: DiagnosticSeverity::Info,
                                    code: "traced".to_string(),
                                    message: "trace captured by fake handler".to_string(),
                                }],
                            )
                            .with_messages(vec![
                                slipway_debug_bridge::DebugMessageTraceEntry::emitted(
                                    event.target().clone(),
                                    "fake-message",
                                    slipway_debug_bridge::MessageDisposition::Consumed,
                                ),
                            ])
                            .with_result_identity(slipway_core::EventResultIdentity {
                                handled: Some(true),
                                emitted_messages: vec![slipway_core::EmittedMessageEvidence {
                                    target: event.target().clone(),
                                    name: "fake-message".to_string(),
                                }],
                                change_shapes: Vec::new(),
                                diagnostics: vec![slipway_core::DiagnosticIdentity {
                                    target: Some(event.target().clone()),
                                    severity: DiagnosticSeverity::Info,
                                    code: "traced".to_string(),
                                }],
                            })
                            .with_reduction_stage(
                                slipway_debug_bridge::DebugControlTraceStage::reduced(
                                    "fake-handler",
                                    Some(event.target().clone()),
                                    "fake handler reduced one emitted message",
                                ),
                            ),
                        )
                    } else {
                        DebugReplyProduct::Diagnostics(vec![Diagnostic {
                            target: None,
                            severity: DiagnosticSeverity::Info,
                            code: "received".to_string(),
                            message: "command reached fake handler".to_string(),
                        }])
                    }
                }
                slipway_debug_bridge::DebugCommandKind::PhysicalControl { .. } => {
                    DebugReplyProduct::Diagnostics(vec![Diagnostic {
                        target: None,
                        severity: DiagnosticSeverity::Info,
                        code: "received".to_string(),
                        message: "physical control command reached fake handler".to_string(),
                    }])
                }
                slipway_debug_bridge::DebugCommandKind::Resize { .. } => {
                    DebugReplyProduct::Diagnostics(vec![Diagnostic {
                        target: None,
                        severity: DiagnosticSeverity::Info,
                        code: "received".to_string(),
                        message: "command reached fake handler".to_string(),
                    }])
                }
            }
        }
    }

    #[test]
    fn initialize_result_contains_protocol_server_info_and_tools_capability() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let mut handler = FakeHandler::default();
        let response =
            server.handle_message(request(json!(1), "initialize", json!({})), &mut handler);

        let response = response.expect("initialize has response");
        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(response["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(response["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn begin_bridge_initialize_returns_immediate_initialize_response() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let (client, _runtime) = bounded_debug_bridge(1);
        let message = request(json!(1), "initialize", json!({})).to_string();

        let response = match server.begin_bridge_message(&message, &client) {
            DebugMcpBridgeMessage::Immediate(Some(response)) => response,
            DebugMcpBridgeMessage::Immediate(None) => panic!("initialize must respond"),
            DebugMcpBridgeMessage::Pending(_) => panic!("initialize must not submit to bridge"),
        };

        assert_eq!(response["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(response["result"]["serverInfo"]["name"], SERVER_NAME);
        assert!(response["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_returns_default_debug_tools() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let mut handler = FakeHandler::default();
        let response = server
            .handle_message(
                request(json!("tools"), "tools/list", json!({})),
                &mut handler,
            )
            .expect("tools/list has response");

        let tools = response["result"]["tools"].as_array().expect("tools array");
        let names = tools
            .iter()
            .map(|tool| tool["name"].as_str().expect("tool name"))
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                TOOL_STATUS,
                TOOL_PROBE,
                TOOL_RENDER,
                TOOL_SCREENSHOT,
                TOOL_CONTROL,
                TOOL_PHYSICAL_CONTROL,
                TOOL_RESIZE
            ]
        );
        assert!(tools.iter().all(|tool| tool["inputSchema"].is_object()));
    }

    #[test]
    fn tools_list_render_and_screenshot_target_is_optional() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let mut handler = FakeHandler::default();
        let response = server
            .handle_message(
                request(json!("tools"), "tools/list", json!({})),
                &mut handler,
            )
            .expect("tools/list has response");

        let tools = response["result"]["tools"].as_array().expect("tools array");
        let render = tools
            .iter()
            .find(|tool| tool["name"] == TOOL_RENDER)
            .expect("render tool exists")["inputSchema"]
            .clone();
        assert_eq!(render["required"], json!(["frame"]));
        assert_eq!(render["properties"]["target"]["type"], "string");

        let screenshot = tools
            .iter()
            .find(|tool| tool["name"] == TOOL_SCREENSHOT)
            .expect("screenshot tool exists")["inputSchema"]
            .clone();
        assert!(screenshot.get("required").is_none());
        assert_eq!(screenshot["properties"]["target"]["type"], "string");
        assert_eq!(
            screenshot["properties"]["frame"]["oneOf"][1]["enum"],
            json!(["current", "last"])
        );
        let schema_text = screenshot.to_string();
        assert!(!schema_text.contains("_slipway_frame_admission"));
        assert!(!schema_text.contains("continu"));
    }

    #[test]
    fn screenshot_parser_enforces_out_of_band_origin() {
        let exact = parse_presented_screenshot_request(
            &json!({ "frame": frame_json_value() }),
            ScreenshotParseOrigin::Direct,
        )
        .expect("direct concrete frame parses");
        assert_eq!(
            exact.selector,
            PresentedScreenshotSelector::Exact {
                expected_frame: frame()
            }
        );

        for frame_value in [None, Some(json!("current")), Some(json!("last"))] {
            let mut arguments = json!({});
            if let Some(frame_value) = frame_value {
                arguments["frame"] = frame_value;
            }
            assert!(
                parse_presented_screenshot_request(&arguments, ScreenshotParseOrigin::Direct)
                    .is_err()
            );
        }

        let forged = json!({
            "frame": frame_json_value(),
            "_slipway_frame_admission": "current"
        });
        assert!(
            parse_presented_screenshot_request(&forged, ScreenshotParseOrigin::Direct).is_err()
        );

        let current = parse_presented_screenshot_request(
            &json!({ "frame": frame_json_value() }),
            ScreenshotParseOrigin::RuntimeNormalized(RuntimeScreenshotSelector::Current),
        )
        .expect("trusted runtime origin parses");
        assert_eq!(
            current.selector,
            PresentedScreenshotSelector::Current {
                request_context: frame()
            }
        );
    }

    #[test]
    fn screenshot_sync_async_refusals_preserve_identical_selectors() {
        let vectors = [
            (ScreenshotParseOrigin::Direct, "exact", "expected_frame"),
            (
                ScreenshotParseOrigin::RuntimeNormalized(RuntimeScreenshotSelector::Current),
                "current",
                "request_context",
            ),
        ];

        for (origin, admission, selector_field) in vectors {
            for (config, target, expected_code) in [
                (DebugMcpConfig::default(), None, "screenshot-denied"),
                (
                    DebugMcpConfig::admitted(),
                    Some("not-the-root"),
                    "screenshot-target-unsupported",
                ),
            ] {
                let server = DebugMcpServer::new(config);
                let mut arguments = json!({ "frame": frame_json_value() });
                if let Some(target) = target {
                    arguments["target"] = json!(target);
                }
                let message = call_request(json!("selector-vector"), TOOL_SCREENSHOT, arguments);
                let mut handler = FakeHandler::default();
                let sync = server
                    .handle_message_with_origin(message.clone(), origin, &mut handler)
                    .expect("sync refusal responds");
                assert!(handler.calls.is_empty());

                let (client, runtime) = bounded_debug_bridge(1);
                let asynchronous =
                    match server.begin_bridge_value_with_origin(message, origin, &client) {
                        DebugMcpBridgeMessage::Immediate(Some(response)) => response,
                        DebugMcpBridgeMessage::Immediate(None) => panic!("refusal must respond"),
                        DebugMcpBridgeMessage::Pending(_) => panic!("refusal must not lease"),
                    };
                assert!(runtime.take_one().expect("bridge readable").is_none());

                let sync = tool_payload(&sync);
                let asynchronous = tool_payload(&asynchronous);
                assert_eq!(sync, asynchronous);
                assert_eq!(sync["admission"], admission);
                assert_eq!(sync["selector"]["kind"], admission);
                assert!(sync["selector"].get(selector_field).is_some());
                assert_eq!(sync["requested_frame"], frame_json_value());
                assert!(sync["captured_frame"].is_null());
                assert_eq!(sync["refusal"]["code"], expected_code);
            }
        }
    }

    #[test]
    fn screenshot_sync_async_malformed_vectors_match() {
        let server = DebugMcpServer::new(DebugMcpConfig::admitted());
        for arguments in [
            json!({}),
            json!({ "frame": "current" }),
            json!({
                "frame": frame_json_value(),
                "_slipway_frame_admission": "current"
            }),
        ] {
            let message = call_request(json!("malformed-vector"), TOOL_SCREENSHOT, arguments);
            let mut handler = FakeHandler::default();
            let sync = server
                .handle_message(message.clone(), &mut handler)
                .expect("sync parse error responds");
            let (client, runtime) = bounded_debug_bridge(1);
            let asynchronous = match server.begin_bridge_value(message, &client) {
                DebugMcpBridgeMessage::Immediate(Some(response)) => response,
                DebugMcpBridgeMessage::Immediate(None) => panic!("parse error must respond"),
                DebugMcpBridgeMessage::Pending(_) => panic!("parse error must not lease"),
            };
            assert_eq!(sync["error"], asynchronous["error"]);
            assert!(handler.calls.is_empty());
            assert!(runtime.take_one().expect("bridge readable").is_none());
        }
    }

    #[test]
    fn synchronous_handler_retains_admitted_screenshot_origin() {
        struct WrongAdmissionHandler;

        impl SlipwayDebugCommandHandler for WrongAdmissionHandler {
            fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
                let correlation = command.frame_identity().clone();
                DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Refusal(
                    PresentedScreenshotRefusal {
                        selector: PresentedScreenshotSelector::Exact {
                            expected_frame: correlation,
                        },
                        captured_frame: None,
                        backend_id: None,
                        code: "wrong-admission".to_string(),
                        reason: "test handler changed selector admission".to_string(),
                        diagnostics: Vec::new(),
                    },
                ))
            }
        }

        let server = DebugMcpServer::new(DebugMcpConfig::admitted());
        let message = call_request(
            json!("handler-origin"),
            TOOL_SCREENSHOT,
            json!({ "frame": frame_json_value() }),
        );
        let response = server
            .handle_message_with_origin(
                message,
                ScreenshotParseOrigin::RuntimeNormalized(RuntimeScreenshotSelector::Current),
                &mut WrongAdmissionHandler,
            )
            .expect("synchronous call responds");
        let payload = tool_payload(&response);
        assert_eq!(payload["product_kind"], "error");
        assert_eq!(payload["product"]["code"], "screenshot-admission-mismatch");
    }

    #[test]
    fn tools_list_describes_screenshot_and_resize_honestly() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let mut handler = FakeHandler::default();
        let response = server
            .handle_message(
                request(json!("tools"), "tools/list", json!({})),
                &mut handler,
            )
            .expect("tools/list has response");

        let tools = response["result"]["tools"].as_array().expect("tools array");
        let screenshot = tools
            .iter()
            .find(|tool| tool["name"] == TOOL_SCREENSHOT)
            .expect("screenshot tool exists");
        let description = screenshot["description"]
            .as_str()
            .expect("screenshot description");
        assert!(
            description.contains("acquired surface texture"),
            "screenshot description must name direct presented capture: {description}"
        );
        assert!(
            !description.contains("offscreen"),
            "screenshot description must stay distinct from canonical render: {description}"
        );

        let resize = tools
            .iter()
            .find(|tool| tool["name"] == TOOL_RESIZE)
            .expect("resize tool exists");
        let description = resize["description"].as_str().expect("resize description");
        assert!(
            description.contains("resize-unsupported"),
            "resize description must state the refusal contract: {description}"
        );
    }

    #[test]
    fn denied_status_probe_render_control_and_resize_return_refusal_content() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let mut handler = FakeHandler::default();

        for (tool, expected_code) in [
            (TOOL_STATUS, "status-denied"),
            (TOOL_PROBE, "probe-denied"),
            (TOOL_RENDER, "render-denied"),
            (TOOL_SCREENSHOT, "screenshot-denied"),
            (TOOL_CONTROL, "control-denied"),
            (TOOL_PHYSICAL_CONTROL, "control-denied"),
            (TOOL_RESIZE, "resize-denied"),
        ] {
            let response = server
                .handle_message(
                    call_request(json!(tool), tool, json!({ "frame": frame_json_value() })),
                    &mut handler,
                )
                .expect("tools/call has response");
            let payload = tool_payload(&response);

            assert_eq!(payload["tool"], tool);
            assert_eq!(payload["admitted"], false);
            assert_eq!(payload["refused"], true);
            assert_eq!(payload["product_kind"], "refusal");
            assert_eq!(payload["refusal"]["code"], expected_code);
        }

        assert!(handler.calls.is_empty());
    }

    #[test]
    fn begin_bridge_refused_status_probe_render_control_and_resize_do_not_touch_bridge() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let (client, runtime) = bounded_debug_bridge(3);

        for (tool, expected_code) in [
            (TOOL_STATUS, "status-denied"),
            (TOOL_PROBE, "probe-denied"),
            (TOOL_RENDER, "render-denied"),
            (TOOL_SCREENSHOT, "screenshot-denied"),
            (TOOL_CONTROL, "control-denied"),
            (TOOL_PHYSICAL_CONTROL, "control-denied"),
            (TOOL_RESIZE, "resize-denied"),
        ] {
            let message =
                call_request(json!(tool), tool, json!({ "frame": frame_json_value() })).to_string();
            let response = match server.begin_bridge_message(&message, &client) {
                DebugMcpBridgeMessage::Immediate(Some(response)) => response,
                DebugMcpBridgeMessage::Immediate(None) => {
                    panic!("refused tools/call must return a response")
                }
                DebugMcpBridgeMessage::Pending(_) => panic!("refused call must not be pending"),
            };
            let payload = tool_payload(&response);

            assert_eq!(payload["tool"], tool);
            assert_eq!(payload["admitted"], false);
            assert_eq!(payload["refused"], true);
            assert_eq!(payload["product_kind"], "refusal");
            assert_eq!(payload["refusal"]["code"], expected_code);
        }

        let mut handler = FakeHandler::default();
        let drained = runtime.drain_one(&mut handler).expect("drain succeeds");
        assert!(drained.is_none());
        assert!(handler.calls.is_empty());
    }

    #[test]
    fn admitted_screenshot_uses_distinct_command_and_method() {
        let server = DebugMcpServer::new(DebugMcpConfig::admitted());
        let mut handler = FakeHandler::default();

        let response = server
            .handle_message(
                call_request(
                    json!("shot-direct"),
                    TOOL_SCREENSHOT,
                    render_arguments_without_target(),
                ),
                &mut handler,
            )
            .expect("tools/call has response");
        let payload = tool_payload(&response);

        assert_eq!(handler.calls.len(), 1);
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Screenshot { request } => {
                assert_eq!(
                    request.selector,
                    PresentedScreenshotSelector::Exact {
                        expected_frame: frame()
                    }
                );
            }
            other => panic!("expected screenshot command, got {other:?}"),
        }
        assert_eq!(payload["tool"], TOOL_SCREENSHOT);
        assert_eq!(payload["bridge_method"], "screenshot");
        assert_eq!(payload["request_id"], "shot-direct");
        assert_eq!(payload["admitted"], true);
        assert_eq!(payload["refused"], true);
        assert_eq!(payload["product_kind"], "screenshot_refusal");
        assert_eq!(payload["product"]["admission"], "exact");
        assert_eq!(payload["product"]["selector"]["kind"], "exact");
        assert_eq!(
            payload["product"]["selector"]["expected_frame"],
            frame_json_value()
        );
        assert!(payload["product"]["captured_frame"].is_null());
    }

    #[test]
    fn admitted_screenshot_refuses_non_root_target_before_handler() {
        let server = render_server();
        let mut handler = FakeHandler::default();
        let response = server
            .handle_message(
                call_request(
                    json!("shot-target"),
                    TOOL_SCREENSHOT,
                    json!({ "frame": frame_json_value(), "target": "widget" }),
                ),
                &mut handler,
            )
            .expect("tools/call has response");
        let payload = tool_payload(&response);

        assert!(handler.calls.is_empty());
        assert_eq!(payload["refusal"]["code"], "screenshot-target-unsupported");
    }

    #[test]
    fn admitted_render_preserves_explicit_target_and_rejects_bad_target() {
        let server = render_server();
        let mut handler = FakeHandler::default();

        server
            .handle_message(
                call_request(json!("explicit"), TOOL_RENDER, render_arguments()),
                &mut handler,
            )
            .expect("explicit target call has response");
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Render { packet } => {
                assert_eq!(packet.target, WidgetId::from("widget"));
            }
            other => panic!("expected render command, got {other:?}"),
        }

        let response = server
            .handle_message(
                call_request(
                    json!("bad-target"),
                    TOOL_RENDER,
                    json!({ "frame": frame_json_value(), "target": 123 }),
                ),
                &mut handler,
            )
            .expect("bad target returns response");
        assert_eq!(response["error"]["code"], -32602);
        assert_eq!(
            response["error"]["message"],
            "field `target` must be a string"
        );
    }

    #[test]
    fn bridged_screenshot_uses_screenshot_method_and_payload_shape() {
        let server = render_server();
        let (client, runtime) = bounded_debug_bridge(1);
        let message = call_request(
            json!("shot-bridge"),
            TOOL_SCREENSHOT,
            render_arguments_without_target(),
        )
        .to_string();

        let pending = match server.begin_bridge_message(&message, &client) {
            DebugMcpBridgeMessage::Pending(pending) => pending,
            DebugMcpBridgeMessage::Immediate(response) => {
                panic!("admitted screenshot should be pending, got {response:?}")
            }
        };

        assert_eq!(pending.tool_name(), TOOL_SCREENSHOT);
        assert_eq!(pending.method(), McpProbeMethod::Screenshot);
        assert_eq!(pending.request_id(), "shot-bridge");

        let mut handler = FakeHandler::default();
        runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("screenshot reply generated");
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Screenshot { request } => {
                assert_eq!(
                    request.selector,
                    PresentedScreenshotSelector::Exact {
                        expected_frame: frame()
                    }
                );
            }
            other => panic!("expected screenshot command, got {other:?}"),
        }
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("reply is available after app drain");
        let payload = tool_payload(&response);

        assert_eq!(payload["tool"], TOOL_SCREENSHOT);
        assert_eq!(payload["bridge_method"], "screenshot");
        assert_eq!(payload["product_kind"], "screenshot_refusal");
    }

    #[test]
    fn bridged_screenshot_accepts_frame_only_and_default_target() {
        let server = render_server();
        let (client, runtime) = bounded_debug_bridge(1);
        let message = call_request(
            json!("shot-frame-only"),
            TOOL_SCREENSHOT,
            render_arguments_without_target(),
        )
        .to_string();

        let pending = match server.begin_bridge_message(&message, &client) {
            DebugMcpBridgeMessage::Pending(pending) => pending,
            DebugMcpBridgeMessage::Immediate(response) => {
                panic!("admitted screenshot should be pending, got {response:?}")
            }
        };

        let mut handler = FakeHandler::default();
        runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("screenshot reply generated");
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Screenshot { request } => {
                assert_eq!(
                    request.selector,
                    PresentedScreenshotSelector::Exact {
                        expected_frame: frame()
                    }
                );
            }
            other => panic!("expected screenshot command, got {other:?}"),
        }
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("reply is available after app drain");
        let payload = tool_payload(&response);

        assert_eq!(payload["tool"], TOOL_SCREENSHOT);
        assert_eq!(payload["bridge_method"], "screenshot");
    }

    #[test]
    fn presented_screenshot_finalizer_writes_artifact_without_serializing_raw_bytes() {
        struct CapturedHandler;

        impl SlipwayDebugCommandHandler for CapturedHandler {
            fn handle_debug_command(&mut self, command: DebugCommand) -> DebugReplyProduct {
                let slipway_debug_bridge::DebugCommandKind::Screenshot { request } = command.kind
                else {
                    panic!("captured handler only accepts screenshot commands")
                };
                let mut captured_frame = request.selector.correlation_frame().clone();
                captured_frame.frame_index += 1;
                DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Captured(
                    PresentedPixels {
                        selector: request.selector,
                        captured_frame,
                        source: EvidenceSource::backend_presented(
                            "test-backend",
                            PRESENTED_PIXELS_PASS_ID,
                        ),
                        capture_path: PresentedCapturePath::DirectAcquiredSurfaceTextureCopy,
                        source_format: PresentedSurfaceFormat::Rgba8UnormSrgb,
                        transfer: PresentedTransferFunction::Srgb,
                        alpha: PresentedAlphaMode::Opaque,
                        width: 1,
                        height: 1,
                        bytes: std::sync::Arc::from([17, 34, 51, 255]),
                        diagnostics: Vec::new(),
                    },
                ))
            }
        }

        let server = render_server();
        let mut handler = CapturedHandler;
        let response = server
            .handle_message(
                call_request(
                    json!("shot-artifact"),
                    TOOL_SCREENSHOT,
                    render_arguments_without_target(),
                ),
                &mut handler,
            )
            .expect("screenshot responds");
        let payload = tool_payload(&response);
        assert_eq!(payload["product_kind"], "presented_screenshot");
        assert_eq!(payload["bridge_method"], "screenshot");
        assert_eq!(payload["product"]["admission"], "exact");
        assert_eq!(payload["product"]["selector"]["kind"], "exact");
        assert_eq!(payload["product"]["width"], 1);
        assert!(
            payload["product"]["artifact_ref"]
                .as_str()
                .expect("artifact ref")
                .starts_with("slipway-debug-renderer://")
        );
        let artifact_path = payload["product"]["artifact_path"]
            .as_str()
            .expect("artifact path");
        assert!(std::path::Path::new(artifact_path).is_file());
        assert!(!payload.to_string().contains("\"bytes\""));
        let _ = std::fs::remove_file(artifact_path);
    }

    #[test]
    fn admitted_status_reaches_handler_and_returns_status_product() {
        let server = DebugMcpServer::new(DebugMcpConfig::admitted());
        let mut handler = FakeHandler::default();

        let response = server
            .handle_message(
                call_request(
                    json!("status-1"),
                    TOOL_STATUS,
                    json!({ "frame": frame_json_value() }),
                ),
                &mut handler,
            )
            .expect("tools/call has response");
        let payload = tool_payload(&response);

        assert_eq!(handler.calls.len(), 1);
        assert_eq!(payload["tool"], TOOL_STATUS);
        assert_eq!(payload["request_id"], "status-1");
        assert_eq!(payload["admitted"], true);
        assert_eq!(payload["product_kind"], "status");
        assert_eq!(payload["product"]["detail"], "fake handler ready");
        assert_eq!(payload["product"]["revision"], 3);
        assert_eq!(payload["product"]["backend_id"], "fake-backend");
        assert_eq!(payload["product"]["trace_buffer_depth"], 1);
        assert_eq!(payload["product"]["trace_buffer_capacity"], 32);
        assert_eq!(payload["product"]["refused_debug_replies"], 2);
        assert_eq!(payload["product"]["unhandled_backend_input_traces"], 4);
        assert_eq!(payload["frame"], frame_json(&frame()));
    }

    fn admitted_probe_event_limit_call(arguments: Value) -> Result<Option<usize>, Value> {
        let server = DebugMcpServer::new(DebugMcpConfig::admitted());
        let mut handler = FakeHandler::default();
        let response = server
            .handle_message(
                call_request(json!("probe-limit"), TOOL_PROBE, arguments),
                &mut handler,
            )
            .expect("tools/call has response");
        if response.get("error").is_some() {
            return Err(response);
        }
        assert_eq!(handler.calls.len(), 1);
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Probe { request, .. } => {
                Ok(request.event_trace_limit)
            }
            other => panic!("expected probe command, got {other:?}"),
        }
    }

    #[test]
    fn admitted_probe_event_trace_limit_absent_means_runtime_default() {
        let limit = admitted_probe_event_limit_call(json!({
            "frame": frame_json_value(),
            "kinds": ["event"]
        }))
        .expect("probe accepted");

        assert_eq!(limit, None);
    }

    #[test]
    fn admitted_probe_event_trace_limit_accepts_canonical_and_aliases() {
        let canonical = admitted_probe_event_limit_call(json!({
            "frame": frame_json_value(),
            "kinds": ["event"],
            "event_trace_limit": 2
        }))
        .expect("canonical limit accepted");
        let camel = admitted_probe_event_limit_call(json!({
            "frame": frame_json_value(),
            "kinds": ["event"],
            "eventTraceLimit": 3
        }))
        .expect("camel alias accepted");
        let short = admitted_probe_event_limit_call(json!({
            "frame": frame_json_value(),
            "kinds": ["event"],
            "eventLimit": 4
        }))
        .expect("short alias accepted");

        assert_eq!(canonical, Some(2));
        assert_eq!(camel, Some(3));
        assert_eq!(short, Some(4));
    }

    #[test]
    fn admitted_probe_event_trace_limit_accepts_identical_aliases() {
        let limit = admitted_probe_event_limit_call(json!({
            "frame": frame_json_value(),
            "kinds": ["event"],
            "event_trace_limit": 5,
            "eventLimit": 5
        }))
        .expect("identical aliases accepted");

        assert_eq!(limit, Some(5));
    }

    #[test]
    fn admitted_probe_event_trace_limit_rejects_conflicting_aliases() {
        let response = admitted_probe_event_limit_call(json!({
            "frame": frame_json_value(),
            "kinds": ["event"],
            "event_trace_limit": 5,
            "eventLimit": 6
        }))
        .expect_err("conflicting aliases are invalid");

        assert_eq!(response["error"]["code"], -32602);
        assert!(
            response["error"]["message"]
                .as_str()
                .expect("message")
                .contains("conflicting event trace limit aliases")
        );
    }

    #[test]
    fn admitted_probe_event_trace_limit_rejects_invalid_values() {
        for invalid in [json!(-1), json!(1.5), json!("2"), Value::Null, json!(1e40)] {
            let response = admitted_probe_event_limit_call(json!({
                "frame": frame_json_value(),
                "kinds": ["event"],
                "event_trace_limit": invalid
            }))
            .expect_err("invalid limit is rejected");

            assert_eq!(response["error"]["code"], -32602);
            assert!(
                response["error"]["message"]
                    .as_str()
                    .expect("message")
                    .contains("event_trace_limit")
            );
        }
    }

    #[test]
    fn admitted_bridge_status_drains_and_finishes_as_direct_status_response() {
        let server = DebugMcpServer::new(DebugMcpConfig::admitted());
        let (client, runtime) = bounded_debug_bridge(1);
        let message = call_request(
            json!("status-bridge"),
            TOOL_STATUS,
            json!({ "frame": frame_json_value() }),
        );

        let pending = match server.begin_bridge_message(&message.to_string(), &client) {
            DebugMcpBridgeMessage::Pending(pending) => pending,
            DebugMcpBridgeMessage::Immediate(response) => {
                panic!("admitted status should be pending, got {response:?}")
            }
        };

        assert_eq!(pending.id(), Some(&json!("status-bridge")));
        assert_eq!(pending.tool_name(), TOOL_STATUS);
        assert_eq!(pending.method(), McpProbeMethod::Status);
        assert_eq!(pending.request_id(), "status-bridge");
        assert!(
            pending
                .try_finish()
                .expect("pending finish should not fail")
                .is_none()
        );

        let mut bridge_handler = FakeHandler::default();
        runtime
            .drain_one(&mut bridge_handler)
            .expect("runtime drains")
            .expect("status reply generated");

        let bridge_response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("reply is available after app drain");

        let mut direct_handler = FakeHandler::default();
        let direct_response = server
            .handle_message(message, &mut direct_handler)
            .expect("direct status response");

        assert_eq!(bridge_handler.calls.len(), 1);
        assert_eq!(direct_handler.calls.len(), 1);
        assert_eq!(bridge_response, direct_response);
    }

    #[test]
    fn admitted_control_without_trace_stays_diagnostics_product() {
        let server = control_server();
        let mut handler = FakeHandler::default();

        for (request_id, trace) in [("control", None), ("control-false", Some(false))] {
            let response = server
                .handle_message(
                    call_request(json!(request_id), TOOL_CONTROL, control_arguments(trace)),
                    &mut handler,
                )
                .expect("control response");
            let payload = tool_payload(&response);

            match &handler.calls.last().expect("handler call").kind {
                slipway_debug_bridge::DebugCommandKind::Control { trace, .. } => {
                    assert!(!trace);
                }
                other => panic!("expected control command, got {other:?}"),
            }
            assert_eq!(payload["tool"], TOOL_CONTROL);
            assert_eq!(payload["request_id"], request_id);
            assert_eq!(payload["product_kind"], "diagnostics");
            assert_eq!(payload["product"]["count"], 1);
        }
        assert_eq!(handler.calls.len(), 2);
    }

    #[test]
    fn admitted_control_with_trace_returns_structured_trace_json() {
        let server = control_server();
        let mut handler = FakeHandler::default();

        let response = server
            .handle_message(
                call_request(
                    json!("control-trace"),
                    TOOL_CONTROL,
                    control_arguments(Some(true)),
                ),
                &mut handler,
            )
            .expect("control trace response");
        let payload = tool_payload(&response);

        assert_eq!(handler.calls.len(), 1);
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Control { trace, .. } => {
                assert!(*trace);
            }
            other => panic!("expected control command, got {other:?}"),
        }
        assert_eq!(payload["product_kind"], "control_trace");
        assert_eq!(payload["product"]["request_id"], "control-trace");
        assert_eq!(payload["product"]["mode"], "semantic_direct");
        assert_eq!(payload["product"]["physical_equivalent"], false);
        assert_eq!(payload["product"]["routed_event_target"], "widget");
        assert_eq!(payload["product"]["event_summary"], "command:activate");
        assert_eq!(payload["product"]["handled"], true);
        assert_eq!(payload["product"]["revision_before"], 7);
        assert_eq!(payload["product"]["revision_after"], 8);
        let stages = payload["product"]["stages"]
            .as_array()
            .expect("control trace stages");
        assert_eq!(stages.len(), 4);
        assert_eq!(stages[0]["stage"], "generated");
        assert_eq!(stages[0]["actor"], "slipway-debug-control");
        assert_eq!(stages[0]["target"], "widget");
        assert_eq!(stages[1]["stage"], "routed");
        assert_eq!(stages[1]["actor"], "slipway-runtime");
        assert_eq!(stages[1]["target"], "widget");
        assert_eq!(stages[2]["stage"], "consumed");
        assert_eq!(stages[2]["actor"], "slipway-widget");
        assert_eq!(stages[2]["target"], "widget");
        assert_eq!(stages[3]["stage"], "reduced");
        assert_eq!(stages[3]["actor"], "fake-handler");
        assert_eq!(stages[3]["target"], "widget");
        assert_eq!(payload["product"]["messages"][0]["source"], "widget");
        assert_eq!(payload["product"]["messages"][0]["name"], "fake-message");
        assert_eq!(payload["product"]["messages"][0]["disposition"], "Consumed");
        assert_eq!(payload["product"]["result_identity"]["handled"], true);
        assert_eq!(
            payload["product"]["result_identity"]["emitted_messages"][0]["name"],
            "fake-message"
        );
        assert_eq!(
            payload["product"]["result_identity"]["diagnostics"][0]["code"],
            "traced"
        );
        assert_eq!(payload["product"]["diagnostics"][0]["code"], "traced");
    }

    #[test]
    fn admitted_physical_control_submits_physical_command() {
        let server = control_server();
        let mut handler = FakeHandler::default();

        let response = server
            .handle_message(
                call_request(
                    json!("physical-control"),
                    TOOL_PHYSICAL_CONTROL,
                    physical_control_arguments(),
                ),
                &mut handler,
            )
            .expect("physical control response");
        let payload = tool_payload(&response);

        assert_eq!(handler.calls.len(), 1);
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::PhysicalControl {
                operation, trace, ..
            } => {
                assert!(*trace);
                match operation {
                    DebugPhysicalControl::Pointer { position, kind, .. } => {
                        assert_eq!(*position, Point { x: 8.0, y: 9.0 });
                        assert_eq!(*kind, PointerEventKind::Press);
                    }
                    other => panic!("expected pointer physical control, got {other:?}"),
                }
            }
            other => panic!("expected physical control command, got {other:?}"),
        }
        assert_eq!(payload["tool"], TOOL_PHYSICAL_CONTROL);
        assert_eq!(payload["bridge_method"], "control");
        assert_eq!(payload["product_kind"], "diagnostics");
    }

    #[test]
    fn admitted_physical_text_control_submits_declaration_resolved_operation() {
        let server = control_server();
        let mut handler = FakeHandler::default();

        let response = server
            .handle_message(
                call_request(
                    json!("physical-text"),
                    TOOL_PHYSICAL_CONTROL,
                    physical_text_control_arguments(),
                ),
                &mut handler,
            )
            .expect("physical text response");
        let payload = tool_payload(&response);

        assert_eq!(handler.calls.len(), 1);
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::PhysicalControl {
                operation, trace, ..
            } => {
                assert!(*trace);
                match operation {
                    DebugPhysicalControl::Text { selector, text } => {
                        assert_eq!(text, "abc");
                        assert_eq!(
                            selector,
                            &DebugPhysicalControlDeclarationSelector::Target {
                                target: WidgetId::from("search")
                            }
                        );
                    }
                    other => panic!("expected text physical control, got {other:?}"),
                }
            }
            other => panic!("expected physical control command, got {other:?}"),
        }
        assert_eq!(payload["tool"], TOOL_PHYSICAL_CONTROL);
        assert_eq!(payload["bridge_method"], "control");
        assert_eq!(payload["product_kind"], "diagnostics");
    }

    #[test]
    fn bridged_control_with_trace_submits_traced_command_and_finishes_trace_json() {
        let server = control_server();
        let (client, runtime) = bounded_debug_bridge(1);
        let message = call_request(
            json!("bridge-control-trace"),
            TOOL_CONTROL,
            control_arguments(Some(true)),
        )
        .to_string();

        let pending = match server.begin_bridge_message(&message, &client) {
            DebugMcpBridgeMessage::Pending(pending) => pending,
            DebugMcpBridgeMessage::Immediate(response) => {
                panic!("admitted traced control should be pending, got {response:?}")
            }
        };
        let mut handler = FakeHandler::default();
        runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("control trace reply generated");
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("reply is available after drain");
        let payload = tool_payload(&response);

        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Control { trace, .. } => assert!(*trace),
            other => panic!("expected control command, got {other:?}"),
        }
        assert_eq!(payload["product_kind"], "control_trace");
        assert_eq!(payload["product"]["request_id"], "bridge-control-trace");
        assert_eq!(payload["product"]["messages"][0]["disposition"], "Consumed");
        assert_eq!(
            payload["product"]["result_identity"]["emitted_messages"][0]["name"],
            "fake-message"
        );
    }

    #[test]
    fn pending_bridge_status_finish_before_app_drain_returns_none() {
        let server = DebugMcpServer::new(DebugMcpConfig::admitted());
        let (client, _runtime) = bounded_debug_bridge(1);
        let message = call_request(
            json!("status-pending"),
            TOOL_STATUS,
            json!({ "frame": frame_json_value() }),
        )
        .to_string();

        let pending = match server.begin_bridge_message(&message, &client) {
            DebugMcpBridgeMessage::Pending(pending) => pending,
            DebugMcpBridgeMessage::Immediate(response) => {
                panic!("admitted status should be pending, got {response:?}")
            }
        };

        let response = pending.try_finish().expect("nonblocking finish succeeds");
        assert!(response.is_none());
    }

    #[test]
    fn stdio_in_memory_io_emits_newline_delimited_json_rpc_responses_only() {
        let input = [
            request(json!(1), "initialize", json!({})).to_string(),
            json!({
                "jsonrpc": "2.0",
                "method": "notifications/initialized"
            })
            .to_string(),
            request(json!(2), "ping", json!({})).to_string(),
            call_request(
                json!(3),
                TOOL_STATUS,
                json!({ "frame": frame_json_value() }),
            )
            .to_string(),
        ]
        .join("\n");
        let reader = Cursor::new(format!("{input}\n"));
        let mut output = Vec::new();
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let mut handler = FakeHandler::default();

        run_stdio(reader, &mut output, &server, &mut handler).expect("stdio loop succeeds");

        let output = String::from_utf8(output).expect("utf8 output");
        let lines = output.lines().collect::<Vec<_>>();
        assert_eq!(lines.len(), 3);
        for line in lines {
            let value: Value = serde_json::from_str(line).expect("each stdout line is JSON-RPC");
            assert_eq!(value["jsonrpc"], "2.0");
            assert!(value.get("result").is_some() || value.get("error").is_some());
        }
    }

    #[test]
    fn runtime_mcp_transport_requires_app_endpoint_response() {
        let (client, endpoint) = bounded_runtime_mcp(1);
        let handle = client
            .submit(request(json!("ping"), "ping", json!({})).to_string())
            .expect("request queued");

        assert!(handle.try_recv().expect("reply channel readable").is_none());

        let runtime_request = endpoint
            .try_recv()
            .expect("endpoint readable")
            .expect("request available");
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let (bridge_client, _bridge_runtime) = bounded_debug_bridge(1);
        let response = match server.begin_bridge_message(runtime_request.request(), &bridge_client)
        {
            DebugMcpBridgeMessage::Immediate(response) => response,
            DebugMcpBridgeMessage::Pending(_) => panic!("ping must not submit to bridge"),
        };
        runtime_request
            .respond(response)
            .expect("endpoint responds");

        let response = handle
            .recv()
            .expect("transport receives")
            .expect("ping returns response");
        assert_eq!(response["result"], json!({}));
    }

    #[test]
    fn runtime_mcp_transport_is_bounded() {
        let (client, _endpoint) = bounded_runtime_mcp(1);
        let _first = client.submit("one").expect("first request queued");
        let second = client.submit("two").err().expect("second request rejected");
        assert_eq!(second, DebugMcpRuntimeTransportError::RequestQueueFull);
    }

    #[test]
    fn enabled_but_idle_runtime_transport_creates_no_response_work() {
        let (_client, endpoint) = bounded_runtime_mcp(1);
        assert!(
            endpoint
                .try_recv()
                .expect("idle endpoint is readable")
                .is_none()
        );
    }

    #[test]
    fn presented_screenshot_response_work_finalizes_on_receiving_thread() {
        let (client, endpoint) = bounded_runtime_mcp(1);
        let handle = client
            .submit("deferred screenshot")
            .expect("request queued");
        let request = endpoint
            .try_recv()
            .expect("endpoint readable")
            .expect("request available");
        let mut requested_frame = frame();
        requested_frame.surface_instance_id = format!("connection-thread-{}", std::process::id());
        let mut captured_frame = requested_frame.clone();
        captured_frame.frame_index += 1;
        let reply = DebugReply {
            request_id: "deferred-screenshot".to_string(),
            frame: requested_frame.clone(),
            product: DebugReplyProduct::Screenshot(PresentedScreenshotProduct::Captured(
                PresentedPixels {
                    selector: PresentedScreenshotSelector::Exact {
                        expected_frame: requested_frame,
                    },
                    captured_frame,
                    source: EvidenceSource::backend_presented(
                        "test-backend",
                        PRESENTED_PIXELS_PASS_ID,
                    ),
                    capture_path: PresentedCapturePath::DirectAcquiredSurfaceTextureCopy,
                    source_format: PresentedSurfaceFormat::Rgba8Unorm,
                    transfer: PresentedTransferFunction::Linear,
                    alpha: PresentedAlphaMode::Opaque,
                    width: 1,
                    height: 1,
                    bytes: std::sync::Arc::from([4, 3, 2, 255]),
                    diagnostics: Vec::new(),
                },
            )),
        };
        request
            .respond_work(DebugMcpResponseWork::PresentedScreenshot {
                rpc_id: Some(json!("deferred-screenshot")),
                tool: TOOL_SCREENSHOT.to_string(),
                method: McpProbeMethod::Screenshot,
                reply,
            })
            .expect("response work queued without finalizing");

        let response = std::thread::spawn(move || handle.recv())
            .join()
            .expect("connection thread joins")
            .expect("response work finalizes")
            .expect("screenshot returns response");
        let payload = tool_payload(&response);
        let artifact_path = payload["product"]["artifact_path"]
            .as_str()
            .expect("artifact path");
        assert!(std::path::Path::new(artifact_path).is_file());
        assert_eq!(payload["product_kind"], "presented_screenshot");
        let _ = std::fs::remove_file(artifact_path);
    }

    #[test]
    fn text_composition_parser_accepts_bounded_multibyte_scalar_ranges() {
        let operation = json!({
            "type": "text_composition",
            "target": "editor",
            "updates": [
                {
                    "preedit_text": "한a",
                    "cursor_range": { "anchor": 1, "focus": 2 }
                }
            ],
            "commit": "한"
        });
        let parsed = parse_physical_control(&operation).expect("valid composition parses");
        let DebugPhysicalControl::TextComposition {
            selector,
            updates,
            commit,
        } = parsed
        else {
            panic!("expected text composition operation")
        };
        assert_eq!(
            selector,
            DebugPhysicalControlDeclarationSelector::Target {
                target: WidgetId::from("editor")
            }
        );
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].cursor_range.as_ref().expect("range").focus, 2);
        assert_eq!(commit, "한");
    }

    #[test]
    fn text_composition_serializer_preserves_end_and_derived_provenance() {
        let event = slipway_core::TextCompositionEvent {
            target: WidgetId::from("editor"),
            target_slot: None,
            phase: TextCompositionPhase::End,
            preedit_text: String::new(),
            cursor_range: None,
        };
        let dispatch_evidence = DeclaredEventDispatchEvidence {
            source: EvidenceSource::backend_presented(
                "egui",
                slipway_debug_bridge::DEBUG_COMPOSITION_PASS_ID,
            ),
            frame: frame(),
            kind: DeclaredEventDispatchKind::Text,
            input_position: None,
            input_position_space: None,
            candidate_regions: vec![PresentationRegionId::from("editor-region")],
            selected_region: Some(PresentationRegionId::from("editor-region")),
            refusal_reason: None,
            generated_event: Some(InputEvent::TextComposition(event.clone())),
            route: None,
            capture_event: false,
            diagnostics: Vec::new(),
        };
        let trace = slipway_debug_bridge::DebugCompositionTrace {
            request_id: "composition-json".to_string(),
            frame: frame(),
            backend_id: "egui".to_string(),
            target: WidgetId::from("editor"),
            selected_region: PresentationRegionId::from("editor-region"),
            focused_before: true,
            focused_after: true,
            phases: vec![slipway_debug_bridge::DebugCompositionPhaseTrace {
                phase: TextCompositionPhase::End,
                backend_event: "derived-end".to_string(),
                provenance: CompositionPhaseProvenance::Derived {
                    from: TextCompositionPhase::Commit,
                },
                event,
                ingress_observation: DebugCompositionIngressObservation::Derived {
                    from_sequence_index: 0,
                },
                dispatch_evidence,
                app_handled: false,
                result_identity: Some(slipway_core::EventResultIdentity {
                    handled: Some(false),
                    emitted_messages: Vec::new(),
                    change_shapes: Vec::new(),
                    diagnostics: Vec::new(),
                }),
            }],
            commit_mutation: None,
            completed: false,
            failure: None,
        };

        let json = composition_trace_summary(&trace);
        assert_eq!(json["phases"][0]["phase"], "end");
        assert_eq!(json["phases"][0]["provenance"]["kind"], "derived");
        assert_eq!(json["phases"][0]["provenance"]["from"], "commit");
    }

    #[test]
    fn text_composition_parser_rejects_update_byte_and_scalar_bounds_before_leasing() {
        let server = DebugMcpServer::new(DebugMcpConfig::admitted());
        let (client, runtime) = bounded_debug_bridge(1);
        let invalid = call_request(
            json!("composition-invalid"),
            TOOL_PHYSICAL_CONTROL,
            json!({
                "frame": frame_json_value(),
                "operation": {
                    "type": "text_composition",
                    "target": "editor",
                    "updates": [{
                        "preedit_text": "한",
                        "cursor_range": { "anchor": 0, "focus": 3 }
                    }],
                    "commit": "한"
                }
            }),
        )
        .to_string();
        let response = match server.begin_bridge_message(&invalid, &client) {
            DebugMcpBridgeMessage::Immediate(Some(response)) => response,
            DebugMcpBridgeMessage::Immediate(None) => panic!("invalid params must respond"),
            DebugMcpBridgeMessage::Pending(_) => panic!("invalid composition must not lease"),
        };
        assert_eq!(response["error"]["code"], -32602);
        assert!(runtime.take_one().expect("bridge readable").is_none());

        let too_many = (0..=MAX_COMPOSITION_UPDATES)
            .map(|_| json!({ "preedit_text": "x" }))
            .collect::<Vec<_>>();
        assert!(
            parse_physical_control(&json!({
                "type": "text_composition",
                "target": "editor",
                "updates": too_many,
                "commit": "x"
            }))
            .is_err()
        );
        assert!(
            parse_physical_control(&json!({
                "type": "text_composition",
                "target": "editor",
                "updates": [{ "preedit_text": "x".repeat(MAX_COMPOSITION_UTF8_BYTES) }],
                "commit": "x"
            }))
            .is_err()
        );
    }

    #[test]
    fn unknown_tool_and_malformed_arguments_are_handled() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let mut handler = FakeHandler::default();

        let unknown = server
            .handle_message(
                call_request(json!("bad-tool"), "slipway.debug.unknown", json!({})),
                &mut handler,
            )
            .expect("unknown tool has response");
        assert_eq!(unknown["error"]["code"], -32602);
        assert!(
            unknown["error"]["message"]
                .as_str()
                .expect("message")
                .contains("unknown debug tool")
        );

        let malformed = server
            .handle_message(
                call_request(
                    json!("bad-args"),
                    TOOL_STATUS,
                    json!({ "frame": "not-object" }),
                ),
                &mut handler,
            )
            .expect("malformed call has response");
        assert_eq!(malformed["error"]["code"], -32602);
        assert!(
            malformed["error"]["message"]
                .as_str()
                .expect("message")
                .contains("frame")
        );
        assert!(handler.calls.is_empty());
    }
}
