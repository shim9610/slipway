# Slipway patch

This directory is copied from the crates.io `eframe` 0.35.0 package at upstream
commit `6f15dc0e16b26edce1fc2a05212eaf7e749c1d05` (`crates/eframe`).

The `slipway_debug` feature forwards request-scoped input and direct acquired-
surface capture events through eframe's existing native winit/wgpu ownership
path. Ordinary input, repaint, presentation, and teardown behavior remains the
upstream path when the feature is disabled or no request is armed.
