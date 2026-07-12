//! # Slipway reference example — the authoring template
//!
//! This crate is the designated COPY SOURCE for authoring a Slipway app
//! (LLM-ergonomics roadmap Phase 4, findings LE-H2/LE-H6). It models every
//! cross-cutting rule in `docs/public/llm-contract-checklist.md`:
//!
//! * **Facade first rule** — the four authored modules import
//!   `slipway::prelude::*` and nothing else; only `app_runner.rs` (the
//!   backend boundary) additionally reaches the facade's backend-adapter
//!   surface, and only behind its feature gates.
//! * **Five-file shape** (`docs/public/authoring-layout.md`,
//!   `docs/agents/authoring-file-boundaries.md`):
//!
//!   | Role | Module |
//!   |------|--------|
//!   | SSOT: ids, app state, capabilities, tokens, shared geometry constants | [`ssot`] |
//!   | Widget-internal logic: per-widget event handling, typed messages | [`internal_logic`] |
//!   | Inter-widget/app communication: messages, reducer, app layout plan | [`communication`] |
//!   | View and internal state: layout, paint, hit/focus/scroll/text declarations | [`view`] |
//!   | App runner/bootstrap: assembly, backend selection, adapter glue | [`app_runner`] |
//!
//! * **Patterns demonstrated** (each pattern site carries a `PATTERN:`
//!   comment with its docs link and failure mode):
//!   1. scrollable list + row hit regions (`view::note_list_declarations`),
//!   2. plain focus region (`focus_region_from_focus_capability`, list),
//!   3. single-line text input (`text_edit_focus_region_from_capability`),
//!   4. movable overlays: pointer-opaque + wheel-transparent layers, drag
//!      with `PointerCaptureIntent::DuringDrag`, BOTH drag patterns — the
//!      clamped panel and the roaming panel whose declared overflow
//!      allowance is the whole LIVE WINDOW
//!      (`ssot::overlay_overflow_bounds`, fed by the projected
//!      `ShowcaseState::viewport`),
//!   5. nested scrolling with default `NearestScrollable` routing,
//!      at-limit chaining, `_with_order` overlap resolution, declared
//!      scroll-indicator modes, and row SELECTION inside the scrolled
//!      Visible inner panel (per-row hit regions re-declared under BOTH
//!      scroll offsets — `ssot::nested_inner_row_rect_in_card`),
//!   6. `reserved_policy_defaults!` for every RESERVED bound (`view`),
//!   7. pre-flight admission validation in tests (`tests`),
//!   8. the app-level PAGE scroll region
//!      (`SlipwayApp::app_scroll_regions` + the app `handle_event`,
//!      `communication.rs`): the card column FILLS the window
//!      (responsive width in `layout_plan`), and when it exceeds the
//!      window the page region scrolls it — wheel over dead space, card
//!      regions chain to it at their limits, Auto indicator appears
//!      exactly on overflow,
//!   9. platform-truth projection
//!      (`SlipwayApp::project_frame_viewport`): the live window size as
//!      external state, so window-derived geometry agrees across paint,
//!      declarations, and handlers.
//!
//! Run it with `cargo run -p slipway-example-authored -- --iced` or
//! `-- --egui`; the same authored modules drive both backends.

mod app_runner;
mod communication;
mod internal_logic;
mod ssot;
mod view;

#[cfg(test)]
mod tests;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    app_runner::run()
}
