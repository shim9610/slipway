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
