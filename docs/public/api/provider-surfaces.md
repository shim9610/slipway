# Provider Surfaces

Provider surfaces let a user-owned renderer or backend-native widget sit inside
the Slipway widget tree.

Slipway does not implement a renderer, GPU runtime, shader system, canvas
engine, command encoder, or device owner.

## Use Cases

Use provider surfaces when you already have:

- a canvas renderer;
- a chart or plot renderer;
- a media renderer;
- a wgpu/Vulkan/custom GPU renderer;
- an iced or egui native widget that should be mounted as one Slipway child.

## Core Idea

The provider remains user-owned. Slipway provides typed slots for:

- stable identity;
- placement and bounds;
- provider kind;
- hit-test policy;
- snapshot policy;
- debug evidence;
- backend-specific insertion.

## Backend-Specific Wrappers

Provider insertion is backend-specific:

- iced uses iced provider/native wrapper traits;
- egui uses egui provider/native wrapper traits.

An iced provider wrapper is not expected to compile on egui, and an egui
provider wrapper is not expected to compile on iced.

Provider/native wrappers are explicit escape hatches. Slipway can mount them,
route declared evidence around them, and ask them for debug/snapshot/probe data
when their trait contract provides it. Slipway does not automatically infer
their internal drawing, hit testing, focus behavior, or parity with another
backend.

For a parity-sensitive task, the provider must expose its own declared bounds,
event regions, snapshot/probe output, or unsupported diagnostic. Otherwise the
correct result is a backend-specific unsupported gap, not a silent success.

Provider geometry is target-local unless a backend-specific wrapper explicitly
states otherwise. `ProviderSurfaceRequest.bounds`, dirty regions,
`ProviderHitTestEvidence.point`, and provider snapshot bounds are interpreted in
the local coordinate space of the Slipway widget that owns the provider. Backend
adapters may map that geometry into root/window/render-pass coordinates, but a
provider should not guess those backend coordinates itself.

Provider hit evidence does not replace declared Slipway hit/focus/scroll
regions. The wrapper should expose a declared Slipway region for the provider
slot, then use provider-specific hit evidence to explain what happened inside
that slot. If the provider cannot produce that mapping, return an unsupported
diagnostic rather than claiming parity.

## GPU Split Contract

For live visible GPU insertion, prefer a split-phase shape:

```text
prepare(&mut provider, request) -> PreparedFrame
paint(&provider, prepared_frame, backend_render_pass)
```

This avoids hiding mutable renderer access behind a project-owned lock.

If an existing renderer requires `&mut self` during the backend paint callback
but the backend only exposes shared callback resources, the live visible path
should report unsupported until a backend-specific exclusive boundary exists.

Request-scoped debug/offscreen rendering may still own or borrow the provider
mutably for that one request.
