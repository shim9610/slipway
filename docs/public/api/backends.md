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
