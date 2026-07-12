# Backend API Map

Slipway backends mechanically lift authored widgets into the selected host
framework. They do not own the UI body.

Select backends through the facade crate:

```toml
slipway = { git = "https://github.com/shim9610/slipway.git", features = ["iced"] }
```

or:

```toml
slipway = { git = "https://github.com/shim9610/slipway.git", features = ["egui"] }
```

## Backend-Neutral Rule

Core authoring code should not contain iced or egui types. Backend-specific
types belong in backend crates or explicit backend-native wrapper code.

Switching backend is not an automatic translation promise. It is a typed repair
workflow: missing backend-specific contracts should produce compile errors or
unsupported diagnostics.

For LLM workers: a compile error caused by a missing backend contract is not a
reason to import internals or bypass the backend. Either implement the required
backend-specific wrapper contract or report the missing public API/backend gap.

## iced Backend

Main crate:

- `slipway-backend-iced`

Important public concepts:

- `run_slipway_iced_runtime_app` - run an authored Slipway app on iced.
- `SlipwayIcedAuthoredChildren` - exposes authored children to the iced
  adapter.
- `SlipwayIcedWidgetListVisitor` - visitor used by the iced backend to lift
  child widgets.
- `SlipwayIcedBackendWidget` - root/runtime backend gate.
- `SlipwayIcedBackendChildWidget` - child backend gate.
- `SlipwayIcedNativeWidget<N>` - backend-specific wrapper for an already-owned
  iced native widget/provider.

The iced backend expects widgets to satisfy the core view, logic, event routing,
event disposition, and child traversal contracts needed for visible backend
interaction.

## egui Backend

Main crate:

- `slipway-backend-egui`

Important public concepts:

- `run_slipway_egui_runtime_app_with_default_bridge` - run an authored Slipway
  app on egui with the default debug bridge.
- `SlipwayEguiAuthoredChildren` - exposes authored children to the egui
  adapter.
- `SlipwayEguiWidgetListVisitor` - visitor used by the egui backend to lift
  child widgets.
- `SlipwayEguiBackendWidget` - root/runtime backend gate.
- `SlipwayEguiBackendChildWidget` - child backend gate.
- `SlipwayEguiNativeWidget<N>` - backend-specific wrapper for an already-owned
  egui native widget/provider.

egui native wrapper code is not portable to iced. That is intentional.

The egui root gate additionally requires font-resolution evidence from the
root widget. For a plain facade app this is already satisfied:
`SlipwayAppWidget` delegates to `SlipwayApp::resolve_app_font`, whose
default refuses honestly (`app-font-resolution-refused`) so the backend
falls back to its own fonts. Override that hook only to declare a real
font source (see [IME and Korean text input](ime.md)). Do not write a root
wrapper just to satisfy the font bound.

## Text Wrap and Alignment

Both visible backends lay out every `PaintOp::Text` the same way. This is
the whole per-op text presentation contract — nothing here requires
reading backend source.

**Wrapping is declared per op: `TextStyle.wrap`.** The default
(`TextWrap::Word`, when you declare nothing) is word wrap at the declared
rect width — byte-identical to the historical hardcoded behavior.
`TextStyle::with_wrap(TextWrap::None)` — or the `.no_wrap()` convenience —
opts the op out of soft wrapping: the text lays out as ONE line (an
explicit `\n` still breaks lines in both modes) and clips at the rect
edge around the declared horizontal alignment anchor. Both visible
backends and the canonical debug renderer honor the declaration (iced
maps it to its native `Wrapping`; egui turns the layout job's wrap width
off). The two modes are the honest common subset of the backends' native
options; glyph-level wrapping is deliberately not exposed. Consequences
for authors:

- default: text wider than `bounds` wraps to further lines inside the
  rect; text taller than `bounds` is clipped to the rect;
- `TextWrap::None`: a CJK header, tab, or badge label stays on one line —
  size the rect for the content or accept the clip;
- do not estimate glyph or character widths to predict wrapping — declare
  the wrap mode you mean, and measure real text through
  `project_text_metrics` (below) when geometry must depend on it.

**Alignment is declared, not hand-computed.** `TextStyle.align_x`
(`Start`/`Center`/`End`) and `TextStyle.align_y` (`Top`/`Center`/`Bottom`)
anchor the laid-out text WITHIN the op's `bounds`, honored by both visible
backends (iced maps them to its native text alignment plus the matching
anchor point; egui maps them to the galley `halign` plus the same anchor
rule) and by the canonical debug renderer, so offscreen render evidence
shows the declared anchoring. Unspecified alignment is `Start`/`Top` —
byte-identical to the historical top-left anchoring, so existing apps
present unchanged.

`TextStyle::centered()` is the button-label pattern: declare the FULL
control rect as the text op's `bounds` and let the backend center inside
it. Never shrink the rect with hand-computed insets from estimated
character widths — the guess drifts off-center wherever it is wrong. The
reference example's roaming overlay titlebar models the pattern.

Order of operations: wrapping happens first, at the rect width per the
declared mode; alignment then anchors the wrapped block (and,
horizontally, each wrapped line) within the rect. Alignment moves text
only inside `bounds` — clipping to the rect is unchanged.

**Paint-text measurement is projected, never estimated.** When authored
geometry must depend on laid-out text size — a badge sized to its label, a
center-ellipsized header — override `SlipwayApp::project_text_metrics`
(or `SlipwayLogic::project_text_metrics` on a hand-rolled root widget).
The runtime invokes the hook with the presenting backend's REAL
text-metric provider on the same cadence as the `project_frame_viewport`
viewport projection: iced measures through its own paragraph layout,
egui through its own galley layout — the identical pipelines that draw
`PaintOp::Text`, constructed with the identical style mapping, so the
measured size equals the presented pixels. In the hook, build a
`TextMeasurementRequest` with the SAME `TextStyle` your paint op declares
(`available_bounds: None` for the intrinsic size, `Some(rect)` to measure
wrapped layout at that rect), match the `TextMeasurementReceipt`, and
write a valid receipt's `facts.measured_size` into your external state;
`paint`, `view_definition`, and `handle_event` then read it like any
other state. Two rules keep the channel honest: measure with the exact
style the op paints, and treat `Invalid`/`Unsupported` receipts as "no
measurement" (paint without the measured element) — never fall back to
estimated character-width ratios. The reference example's input-card
window badge models the full pattern (`ssot::MeasuredLabel`,
`communication.rs::project_text_metrics`, the badge paint in `view.rs`).
View code stays pure: measurement happens only in the runtime-invoked
hook, which is a sanctioned external-state writer alongside
`project_frame_viewport` and the app reducer.

## Keyboard Delivery

Declaring `Capability::KeyboardInput` with a plain (non-text-edit) focus
region admits, but delivery is backend-split:

- **iced delivers keyboard events to text-edit focus regions only.** Real
  keyboard input routes exclusively through the focused text-edit region;
  a focused plain focus region receives nothing. Debug-MCP physical
  keyboard control has the same shape — it first focuses the selector
  region's native text widget and refuses with
  `native-physical-control-text-focus-widget-unavailable` when the region
  is not backed by one. A keyboard handler reachable only through a plain
  focus region (for example Escape-to-close on a modal) never runs on
  iced and cannot be exercised by physical-control test coverage there.
- **egui delivers keyboard events to whichever declared focus region is
  focused**, plain or text-edit — click the region (or use the focus
  physical control) first, then key events route to it.

Admission states this at pre-flight: `KeyboardInput` with only plain focus
regions draws the
`view_contract.keyboard_capability_plain_focus_delivery_limited` warning
(evidence, not blocking). If keyboard handling must work on every visible
backend, declare a text-edit focus region
(`text_edit_focus_region_from_capability`); otherwise treat the handler as
egui-only and provide a pointer path for the same action (the modal-close
case: a close-button hit region).

## Visible Backend Rules

Visible backends must:

- keep authored children as separate backend child widgets where possible;
- call `visible_backend_view_definition` for visible presentation boundaries;
- validate view and dispatch evidence before mutating widget state;
- preserve `BackendInputEvent` evidence for physical input and validate that
  the evidence backend id matches the selected backend;
- preserve the same `FrameIdentity` between MCP/debug physical-control
  commands and backend-presented evidence. Matching viewport alone is not
  enough evidence;
- refuse blocking contract diagnostics instead of silently applying behavior;
- keep canonical/offscreen rendering out of the visible hot path.

Backend-presented physical evidence is backend-specific. An iced visible
runtime must reject egui/test backend provenance, and an egui visible runtime
must reject iced/test backend provenance.

Visible backend success must be proven through the same backend path the user
will exercise. Semantic runtime control or fabricated JSON evidence is not a
substitute for backend-presented input traces.

## Backend-Native Escape Hatch

If you already own an iced or egui widget, wrap it through that backend's native
wrapper path. The wrapper is still a Slipway child and must expose enough
identity, layout, event, and debug evidence for Slipway to inspect where it
sits in the app.

This is an explicit backend-specific escape hatch. It is not a cross-backend
visual parity guarantee and it is not a way to smuggle undeclared behavior into
the neutral authoring API. If a task requires Slipway parity evidence, the
wrapper must expose the relevant bounds, event regions, state, snapshots, or
unsupported diagnostics through its backend-specific contract.

Scroll repair is also backend-owned defense, not an authoring shortcut. If a
visible backend crops or disables invalid scroll geometry, that keeps the
window stable but should be treated as diagnostic evidence that the authored
scroll declaration needs repair.
