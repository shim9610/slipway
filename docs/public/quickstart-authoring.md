# Quickstart For App Authors

Use this page when you are writing a Slipway app. This is the authoring path,
not an architecture investigation path.

If the public docs do not tell you how to continue, report `PUBLIC_DOC_GAP`;
do not guess the API from private notes, old crates, or repository history.

## 1. Add Slipway

Prefer `cargo add`:

```powershell
cargo add slipway --git https://github.com/shim9610/slipway.git --features iced
```

Use `--features egui` for egui, or `--features all-backends` only when the
task genuinely needs both backend adapters.

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

This is exactly the module map of the reference example
`crates/slipway-example-authored` — the designated copy source; start
from it (every pattern site there carries a `PATTERN:` comment). Each
file has one job, shown concretely in the example:

- `ssot.rs`: app data, ids, capabilities, design tokens, and the shared
  geometry constants that paint, hit, and pointer math MUST agree on.
- `internal_logic.rs`: behavior inside one widget instance (the example's
  four `SlipwayLogic` impls).
- `communication.rs`: app messages, reducers, and widget-to-widget
  coordination (`ShowcaseMessage`, `apply_messages`, the `SlipwayApp`
  composition).
- `view.rs`: layout, paint, hit, focus, scroll, text, order, overflow,
  and responsive declarations, plus `reserved_policy_defaults!`.
- `app_runner.rs`: runtime assembly, backend selection, and debug startup
  (`--iced`/`--egui` driving the same authored modules).

Do not start with one large root surface: sidebars, filters, cards, charts,
tables, modals, and scroll areas become separate authored widget identities
unless there is a clear reason not to.

## 4. Run An Authored App

`SlipwayAppWidget::new(app)` alone is the supported root. The adapter
already carries every root policy the backends require (its font bound is
satisfied by `SlipwayApp::resolve_app_font`, below) — a hand-written
delegation wrapper around it is never needed. The reference example runs
exactly this bare root on both backends
(`crates/slipway-example-authored/src/app_runner.rs`); the ~160-line
`AdmissionRuntimeAppWidget` wrapper in the internal admission fixture is
the pre-Step-209 historical idiom, kept only as back-compat proof — do not
copy it.

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

The egui root's font bound is satisfied by `SlipwayApp::resolve_app_font`,
whose default refuses honestly (`app-font-resolution-refused`) and the
backend falls back to its own fonts; override it only to declare a real
font source ([IME and Korean text input](api/ime.md)).

`apply_messages` is the app reducer: it receives widget-emitted messages and
updates the external app state:

```rust
fn apply_messages(state: &mut MyExternalState, messages: Vec<MyMessage>) {
    for message in messages {
        // Update app state here.
    }
}
```

## 5. Declare Interaction, Do Not Only Paint It

A painted shape is not interactive by itself. The view must also declare the
matching contract, using the prelude helper that constructs it:

- clickable or hoverable region: `HitRegionDeclaration`
  (`hit_region_from_pointer_capability`);
- focusable or text-edit region: `FocusRegionDeclaration`
  (`focus_region_from_focus_capability`, or
  `text_edit_focus_region_from_capability` for text input);
- wheel or overflow region: `ScrollRegionDeclaration`
  (`scroll_region_from_scrollable_capability`, or the `_with_order` variant
  when regions can overlap — see
  [Routing and scroll](api/routing-and-scroll.md));
- text editing support: text-edit declarations and command support;
- overlay or modal ordering: `PaintOrderDeclaration`;
- child placement: `ChildLayoutPlan` and `ParentLocalRect`;
- target-owned geometry: `TargetLocalRect`.

Then ask the scrolling question explicitly, for every widget and for the
composed app: does any content exceed its container or the window? A card
column taller than the window counts. If yes, declare a covering scroll
region (`scroll_region_from_scrollable_capability`, `_with_order` when
regions can overlap — [Routing and scroll](api/routing-and-scroll.md));
for a page taller than the window, use the app-level page-scroll pattern
(`SlipwayApp::app_scroll_regions`, modeled at the `PATTERN:` site in
`crates/slipway-example-authored/src/communication.rs`). Painted content
with no covering scroll region draws the admission advisory
`view_contract.content_overflow_without_scroll_region` (warning,
non-blocking) naming this fix; clipping the overflow is fine only when it
is intentional.

The helpers require capability-bundle trait bounds. Implement the
LOAD-BEARING traits by hand and cover every RESERVED bound with one macro
call, `reserved_policy_defaults!(MyWidget);` — do not write real logic in
RESERVED traits, it is a silent no-op (see
[Trait surface](api/trait-surface.md)).

Validate before launching a window: run
`view_definition_contract_diagnostics_for_capabilities` in a unit test and
assert no blocking diagnostics. Every refusal code is cataloged with its
trigger and fix in the [Diagnostics catalog](api/diagnostics.md).

If an element should react to input, a missing declaration is an authoring
bug or a missing public API contract; do not replace it with direct state
mutation.

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

The full tool surface is in [Debug MCP](api/debug-mcp.md).

For visual parity work, always compare the current visible frame identity:
`surface_id + surface_instance_id + revision + frame_index + viewport`.

If MCP returns success but the visible backend does not change in the same
way, that is an evidence gap or backend adapter bug, not proof of success.

## 7. When Mirroring A Web UI

Use the source web UI only to derive component boundaries, state, layout,
overflow, responsiveness, and interaction states; Slipway is not a CSS engine.

For each source component, write down:

- the widget id and repeated slot identity;
- external state input;
- widget-local state;
- emitted messages;
- layout bounds and overflow policy;
- paint order;
- hit/focus/scroll/text declarations;
- MCP checks for resize, scroll, click, focus, and text input.

Then implement the smallest missing declarations for that source UI; do not
build a general browser layout engine, CSS parser, or whole-page canvas.
