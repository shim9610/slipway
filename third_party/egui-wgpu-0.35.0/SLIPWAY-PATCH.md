# Slipway patch

- Package: `egui-wgpu 0.35.0`
- Source: `C:/Users/sim96/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-wgpu-0.35.0`
- Upstream commit: `6f15dc0e16b26edce1fc2a05212eaf7e749c1d05`
- Upstream path: `crates/egui-wgpu`
- Copied source-tree SHA-256: `eb7321e3d7f74a3587c3987218c15ac0bd2e93ce1dd36f508fb271f011d058d2`

Step 223 adds a request-only direct capture API in `src/winit.rs`. The API renders
once to the acquired surface texture, copies that same texture to a staging buffer,
presents it, restores ordinary surface usage, and reports bounded capture events.
It does not use egui's `CaptureState` or screenshot event path.

Changed package files:

- `src/winit.rs`: direct capture types, painter sibling, readback helpers, and tests.
- `SLIPWAY-PATCH.md`: source identity and patch purpose.

`Cargo.toml`, `Cargo.toml.orig`, and `Cargo.lock` remain byte-identical to the
registry package because Agent 15 was forbidden to edit manifests and lockfiles.
The manifest owner must declare the empty `slipway_debug` feature used by the
source-level gates before workspace integration.
