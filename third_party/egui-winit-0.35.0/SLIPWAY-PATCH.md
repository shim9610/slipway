# Slipway Patch

- Package: `egui-winit` 0.35.0
- Source: `C:/Users/sim96/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/egui-winit-0.35.0`
- Upstream commit: `6f15dc0e16b26edce1fc2a05212eaf7e749c1d05`
- Step: 223 debug MCP completion trio

Changed files:

- `Cargo.toml` and `Cargo.toml.orig`: add the empty `slipway_debug` feature.
- `src/lib.rs`: add request-scoped pre-take debug input plans, notices, origin
  spans, and shared native/debug input normalizers.

The remaining package files are copied unchanged from the registry source.
