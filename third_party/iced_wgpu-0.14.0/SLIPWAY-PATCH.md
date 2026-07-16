# Slipway Patch Provenance

- Package: `iced_wgpu 0.14.0`
- Registry source: `C:/Users/sim96/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/iced_wgpu-0.14.0`
- Crate archive SHA-256: `ff144a999b0ca0f8a10257934500060240825c42e950ec0ebee9c8ae30561c13`
- Upstream VCS commit: `3997291f318a8bc06fa522f5579836fb3feb94df`
- Changed upstream file: `src/window/compositor.rs`
- Added provenance file: `SLIPWAY-PATCH.md`

Step 223 adds a one-shot direct-capture hook that copies the acquired wgpu
surface texture after visible rendering and before presentation. The ordinary
presentation path remains unchanged and performs no capture work when no request
is armed.
