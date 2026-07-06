use std::io::{self, BufRead, Write};

use crossbeam_channel::{Receiver, Sender, TryRecvError, TrySendError, bounded};
use serde_json::{Value, json};
use slipway_core::{
    CommandEvent, DeclaredEventDispatchEvidence, DeclaredEventDispatchKind, Diagnostic,
    EvidenceSource, FocusEvent, FrameIdentity, InputEvent, KeyEventKind, KeyLocation,
    KeyboardDetails, KeyboardEvent, LayoutOutput, Modifiers, Point, PointerButton, PointerButtons,
    PointerDetails, PointerDeviceKind, PointerEvent, PointerEventKind, PresentationRegionId,
    ProbeKind, ProbeProduct, ProbeRequest, Rect, RenderPacket, Size, TargetLocalRect, TextEditKind,
    TextInputEvent, TextSelectionRange, WheelEvent, WidgetId, WidgetSlotAddress,
};
use slipway_debug_bridge::{
    DebugBridgeClient, DebugBridgeError, DebugCommand, DebugFailure, DebugPhysicalControl,
    DebugPhysicalControlDeclarationSelector, DebugReply, DebugReplyProduct, DebugRequestHandle,
    DebugStatus, McpProbeMethod, RenderProduct, SlipwayDebugCommandHandler,
};

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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DebugMcpConfig {
    pub allow_status: bool,
    pub allow_probe: bool,
    pub allow_render: bool,
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
    response_tx: Sender<Option<Value>>,
}

pub struct DebugMcpRuntimeResponseHandle {
    response_rx: Receiver<Option<Value>>,
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
        self.response_tx
            .try_send(response)
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
            Ok(response) => Ok(Some(response)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(TryRecvError::Disconnected) => {
                Err(DebugMcpRuntimeTransportError::ResponseQueueDisconnected)
            }
        }
    }

    pub fn recv(&self) -> Result<Option<Value>, DebugMcpRuntimeTransportError> {
        self.response_rx
            .recv()
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
        let reply = match self.handle.try_recv()? {
            Some(reply) => reply,
            None => return Ok(None),
        };

        Ok(Some(json_rpc_result(
            self.id.clone(),
            tool_result(reply_payload(&self.tool, self.method, reply, true)),
        )))
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
            "tools/call" => Some(self.handle_tools_call(id, message.get("params"), handler)),
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
            "tools/call" => self.begin_bridge_tools_call(id, message.get("params"), bridge),
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
            TOOL_RENDER => self.call_render(TOOL_RENDER, &request_id, arguments, handler),
            TOOL_SCREENSHOT => self.call_render(TOOL_SCREENSHOT, &request_id, arguments, handler),
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
            TOOL_RENDER => {
                self.begin_bridge_render(TOOL_RENDER, id.clone(), &request_id, arguments, bridge)
            }
            TOOL_SCREENSHOT => self.begin_bridge_render(
                TOOL_SCREENSHOT,
                id.clone(),
                &request_id,
                arguments,
                bridge,
            ),
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
        tool: &'static str,
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
                    tool,
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
            tool,
            McpProbeMethod::Render,
            DebugCommand::render(request_id, packet),
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
        tool: &str,
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
                tool,
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
            tool,
            McpProbeMethod::Render,
            handler_reply(handler, command),
            true,
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
        render_tool_schema(
            TOOL_SCREENSHOT,
            "Alias slipway.debug.render for explicit screenshot capture requests.",
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
            "Request a viewport resize for a frame identity.",
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
    let product = handler.handle_debug_command(command);
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
        DebugReplyProduct::Diagnostics(diagnostics) => (
            "diagnostics",
            json!({
                "count": diagnostics.len(),
                "diagnostics": diagnostics_summary(diagnostics),
            }),
        ),
        DebugReplyProduct::ControlTrace(trace) => ("control_trace", control_trace_summary(trace)),
        DebugReplyProduct::Error(error) => ("error", failure_summary(error)),
    }
}

fn status_summary(status: &DebugStatus) -> Value {
    json!({
        "admitted": status.admitted,
        "detail": status.detail,
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
            allow_control: true,
            allow_resize: false,
        })
    }

    fn render_server() -> DebugMcpServer {
        DebugMcpServer::new(DebugMcpConfig {
            allow_status: true,
            allow_probe: false,
            allow_render: true,
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
        for name in [TOOL_RENDER, TOOL_SCREENSHOT] {
            let schema = tools
                .iter()
                .find(|tool| tool["name"] == name)
                .expect("render tool exists")
                .get("inputSchema")
                .expect("schema exists");
            assert_eq!(schema["required"], json!(["frame"]));
            assert_eq!(schema["properties"]["target"]["type"], "string");
        }
    }

    #[test]
    fn denied_status_probe_render_control_and_resize_return_refusal_content() {
        let server = DebugMcpServer::new(DebugMcpConfig::default());
        let mut handler = FakeHandler::default();

        for (tool, expected_code) in [
            (TOOL_STATUS, "status-denied"),
            (TOOL_PROBE, "probe-denied"),
            (TOOL_RENDER, "render-denied"),
            (TOOL_SCREENSHOT, "render-denied"),
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
            (TOOL_SCREENSHOT, "render-denied"),
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
    fn admitted_screenshot_alias_uses_render_command_and_reports_alias_tool() {
        let server = render_server();
        let mut handler = FakeHandler::default();

        let response = server
            .handle_message(
                call_request(json!("shot-direct"), TOOL_SCREENSHOT, render_arguments()),
                &mut handler,
            )
            .expect("tools/call has response");
        let payload = tool_payload(&response);

        assert_eq!(handler.calls.len(), 1);
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Render { packet } => {
                assert_eq!(packet.frame, frame());
            }
            other => panic!("expected render command, got {other:?}"),
        }
        assert_eq!(payload["tool"], TOOL_SCREENSHOT);
        assert_eq!(payload["bridge_method"], "render");
        assert_eq!(payload["request_id"], "shot-direct");
        assert_eq!(payload["admitted"], true);
        assert_eq!(payload["refused"], false);
        assert_eq!(payload["product_kind"], "render_refusal");
    }

    #[test]
    fn admitted_render_and_screenshot_accept_frame_only_and_default_target() {
        for tool in [TOOL_RENDER, TOOL_SCREENSHOT] {
            let server = render_server();
            let mut handler = FakeHandler::default();

            let response = server
                .handle_message(
                    call_request(json!(tool), tool, render_arguments_without_target()),
                    &mut handler,
                )
                .expect("tools/call has response");
            let payload = tool_payload(&response);

            assert_eq!(handler.calls.len(), 1);
            match &handler.calls[0].kind {
                slipway_debug_bridge::DebugCommandKind::Render { packet } => {
                    assert_eq!(packet.frame, frame());
                    assert_eq!(packet.target, WidgetId::from("instance"));
                }
                other => panic!("expected render command, got {other:?}"),
            }
            assert_eq!(payload["tool"], tool);
            assert_eq!(payload["admitted"], true);
        }
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
    fn bridged_screenshot_alias_uses_render_method_and_payload_shape() {
        let server = render_server();
        let (client, runtime) = bounded_debug_bridge(1);
        let message =
            call_request(json!("shot-bridge"), TOOL_SCREENSHOT, render_arguments()).to_string();

        let pending = match server.begin_bridge_message(&message, &client) {
            DebugMcpBridgeMessage::Pending(pending) => pending,
            DebugMcpBridgeMessage::Immediate(response) => {
                panic!("admitted screenshot should be pending, got {response:?}")
            }
        };

        assert_eq!(pending.tool_name(), TOOL_SCREENSHOT);
        assert_eq!(pending.method(), McpProbeMethod::Render);
        assert_eq!(pending.request_id(), "shot-bridge");

        let mut handler = FakeHandler::default();
        runtime
            .drain_one(&mut handler)
            .expect("runtime drains")
            .expect("screenshot alias reply generated");
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Render { packet } => {
                assert_eq!(packet.frame, frame());
            }
            other => panic!("expected render command, got {other:?}"),
        }
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("reply is available after app drain");
        let payload = tool_payload(&response);

        assert_eq!(payload["tool"], TOOL_SCREENSHOT);
        assert_eq!(payload["bridge_method"], "render");
        assert_eq!(payload["product_kind"], "render_refusal");
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
            .expect("screenshot alias reply generated");
        match &handler.calls[0].kind {
            slipway_debug_bridge::DebugCommandKind::Render { packet } => {
                assert_eq!(packet.frame, frame());
                assert_eq!(packet.target, WidgetId::from("instance"));
            }
            other => panic!("expected render command, got {other:?}"),
        }
        let response = pending
            .try_finish()
            .expect("pending finish should not fail")
            .expect("reply is available after app drain");
        let payload = tool_payload(&response);

        assert_eq!(payload["tool"], TOOL_SCREENSHOT);
        assert_eq!(payload["bridge_method"], "render");
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
