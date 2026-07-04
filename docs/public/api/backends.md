# Backend API Map

Slipway backends mechanically lift authored widgets into the selected host
framework. They do not own the UI body.

## Backend-Neutral Rule

Core authoring code should not contain iced or egui types. Backend-specific
types belong in backend crates or explicit backend-native wrapper code.

Switching backend is not an automatic translation promise. It is a typed repair
workflow: missing backend-specific contracts should produce compile errors or
unsupported diagnostics.

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
- preserve `BackendInputEvent` evidence for physical input;
- refuse blocking contract diagnostics instead of silently applying behavior;
- keep canonical/offscreen rendering out of the visible hot path.

## Backend-Native Escape Hatch

If you already own an iced or egui widget, wrap it through that backend's native
wrapper path. The wrapper is still a Slipway child and must expose enough
identity, layout, event, and debug evidence for Slipway to inspect where it
sits in the app.
