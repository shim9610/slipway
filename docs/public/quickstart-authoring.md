# Quickstart For App Authors

Use this page when you are writing a Slipway app. This is the authoring path,
not an architecture investigation path.

If this page and the linked public docs do not tell you how to continue, report
`PUBLIC_DOC_GAP` with the missing operation. Do not inspect private planning
docs, old evaluation crates, or repository history to guess the intended API.

## 1. Add Slipway

Prefer `cargo add`:

```powershell
cargo add slipway --git https://github.com/shim9610/slipway.git --features iced
```

Use `--features egui` for the egui backend. Use `--features all-backends` only
when the task explicitly needs both backend adapters.

## 2. Import The Facade

Application code should import the public facade:

```rust
use slipway::prelude::*;
```

Do not depend on `slipway-core`, `slipway-runtime`, `slipway-backend-iced`, or
`slipway-backend-egui` directly unless you are extending Slipway itself.

## 3. Split The App Before Coding Widgets

Create this file map first:

```text
src/
  ssot.rs
  internal_logic.rs
  communication.rs
  view.rs
  app_runner.rs
```

Each file has one job:

- `ssot.rs`: app data, ids, stable semantic source state.
- `internal_logic.rs`: behavior inside one widget instance.
- `communication.rs`: app messages, reducers, and widget-to-widget
  coordination.
- `view.rs`: layout, paint, hit, focus, scroll, text, order, overflow, and
  responsive declarations.
- `app_runner.rs`: runtime assembly, backend selection, and debug startup.

Do not start with one large root surface. If the source UI has a sidebar,
filters, cards, chart, table, modal, or scroll area, those should become
separate authored widget identities unless there is a clear reason not to.

## 4. Run An Authored App

For iced:

```rust
use slipway::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = SlipwayRuntimeConfig::admitted_debug()
        .with_platform_ime_always_allowed();

    run_slipway_iced_runtime_app_with_config(
        SlipwayAppWidget::new(MyApp::new()),
        MyExternalState::default(),
        apply_messages,
        config,
    )?;
    Ok(())
}
```

Use `with_platform_ime_always_allowed()` for Korean/Hangul text input on iced.
See [IME and Korean text input](api/ime.md).

For egui:

```rust
use slipway::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = SlipwayRuntime::from_app(MyApp::new(), MyExternalState::default());
    run_slipway_egui_runtime_app_with_default_bridge(
        "My Slipway app",
        runtime,
        apply_messages,
    )?;
    Ok(())
}
```

`apply_messages` is the app reducer. It receives messages emitted by widget
logic and updates the external app state:

```rust
fn apply_messages(state: &mut MyExternalState, messages: Vec<MyMessage>) {
    for message in messages {
        // Update app state here.
    }
}
```

## 5. Declare Interaction, Do Not Only Paint It

A painted shape is not interactive by itself. The view must also declare the
matching contract:

- clickable or hoverable region: `HitRegionDeclaration`;
- focusable or text-edit region: `FocusRegionDeclaration`;
- wheel or overflow region: `ScrollRegionDeclaration`;
- text editing support: text-edit declarations and command support;
- overlay or modal ordering: `PaintOrderDeclaration`;
- child placement: `ChildLayoutPlan` and `ParentLocalRect`;
- target-owned geometry: `TargetLocalRect`.

If an element should react to pointer, wheel, focus, keyboard, or text input,
missing declarations are an authoring bug or a missing public API contract.
Do not replace them with direct state mutation.

## 6. Use MCP From The Running Backend

Debug MCP is attached by the standard debug runtime path. The running window
title includes the loopback address, for example:

```text
Slipway Backend Iced - Iced MCP: 127.0.0.1:52883
```

Send line-delimited JSON-RPC requests to that address. Common calls:

```json
{"jsonrpc":"2.0","id":"status-1","method":"tools/call","params":{"name":"slipway.debug.status","arguments":{"frame":"current"}}}
```

```json
{"jsonrpc":"2.0","id":"shot-1","method":"tools/call","params":{"name":"slipway.debug.screenshot","arguments":{"frame":"current"}}}
```

```json
{"jsonrpc":"2.0","id":"click-1","method":"tools/call","params":{"name":"slipway.debug.physical_control","arguments":{"frame":"current","operation":{"type":"pointer","phase":"press","position":{"x":120.0,"y":80.0},"button":"primary","device":"mouse"}}}}
```

For visual parity work, always compare the current visible frame identity:

```text
surface_id + surface_instance_id + revision + frame_index + viewport
```

If MCP returns success but the visible backend does not change in the same way,
that is a framework/debug evidence gap or backend adapter bug, not proof of
success.

## 7. When Mirroring A Web UI

Use the source web UI only to derive component boundaries, state, layout,
overflow, responsive behavior, and interaction states. Slipway is not a CSS
engine or automatic converter.

For each source component, write down:

- the widget id and repeated slot identity;
- external state input;
- widget-local state;
- emitted messages;
- layout bounds and overflow policy;
- paint order;
- hit/focus/scroll/text declarations;
- MCP checks for resize, scroll, click, focus, and text input.

Then implement the smallest missing declarations needed for that source UI.
Do not implement a general browser layout engine, CSS parser, or one-off
canvas surface for the whole page.
