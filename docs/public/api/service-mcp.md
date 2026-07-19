# Service MCP

Service MCP is the release-intended MCP surface for an app's own operations.
It is separate from Debug MCP.

Use Debug MCP when an LLM must inspect layout, evidence, screenshots, probes,
or physical-control diagnostics. Use Service MCP when the shipped app should
intentionally expose a tool such as "create report", "select city", "export
snapshot", or "run domain query".

## Contract

Service MCP exposes only tools declared by the app author.

The runtime does not invent business operations and does not expose backend
internals. A service tool runs on the Slipway runtime owner, receives JSON
arguments, and returns JSON content plus optional typed app messages. Those
messages are applied through the same app reducer used by visible backend
events.

```rust
use slipway::prelude::*;
use serde_json::json;

struct ServiceTools;

impl SlipwayServiceMcpHandler<AppState, AppMessage> for ServiceTools {
    fn tools(&self, _state: &AppState) -> Vec<SlipwayServiceMcpToolDefinition> {
        vec![SlipwayServiceMcpToolDefinition::new(
            "app.city.select",
            "Select the active city.",
            json!({
                "type": "object",
                "properties": {
                    "city": { "type": "string" }
                },
                "required": ["city"]
            }),
        )]
    }

    fn call(
        &mut self,
        _state: &mut AppState,
        call: SlipwayServiceMcpToolCall,
    ) -> SlipwayServiceMcpToolResult<AppMessage> {
        let city = call.arguments["city"].as_str().unwrap_or_default().to_string();
        SlipwayServiceMcpToolResult::ok_with_messages(
            json!({ "accepted": true, "city": city }),
            vec![AppMessage::SelectCity(city)],
        )
    }
}
```

Attach the handler before starting the service transport:

```rust
use slipway::backend_iced::SlipwayIcedRuntimeApp;

let mut assembled = SlipwayAssembledApp::from_app(app, state)
    .with_service_mcp_handler(ServiceTools);
let service = assembled.runtime.start_service_mcp_transport()?;
let iced_app = SlipwayIcedRuntimeApp::new(assembled, reducer)
    .with_service_mcp_transport(service);
```

The exact backend runner type differs between iced and egui, but the service
handler contract is backend-independent.

The service API uses `serde_json::Value` for tool schemas, arguments, and
content. Add `serde_json` in your app if you want the `json!` helper.

## Boundaries

- Service MCP is opt-in. No handler means no service tools.
- Service tools must not use the `slipway.debug.*` or `slipway.internal.*`
  namespaces.
- Service MCP is not proof that pointer, wheel, keyboard, IME, or screenshot
  paths work. Use Debug MCP for those diagnostics.
- Service tools should emit typed app messages for state changes unless they
  are intentionally app-global service state mutations.
- The runtime uses bounded channels and request-scoped draining. It does not
  record all frames and does not share widget ownership with the transport
  thread.
