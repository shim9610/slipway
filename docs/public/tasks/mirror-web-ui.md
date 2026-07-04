# Task Guide: Mirror A Web UI

This guide is for an LLM worker asked to mirror a web UI, such as a Svelte app,
with Slipway.

Before reading source code, read [Quickstart for app authors](../quickstart-authoring.md).
The mirroring worker should not need to inspect Slipway internals. If a required
operation is missing from public docs, report `PUBLIC_DOC_GAP` and name the
missing operation.

## Goal

The goal is not automatic conversion. The worker reads the source UI, then
authors an equivalent Slipway app with explicit state, logic, view declarations,
and backend evidence.

## Required Steps

1. Inspect the source web app.
2. Identify component boundaries, state, props, events, layout, overflow, and
   responsive behavior.
3. Create the Slipway file map:

   ```text
   ssot.rs
   internal_logic.rs
   communication.rs
   view.rs
   app_runner.rs
   ```

4. Map each meaningful web component to a Slipway widget identity.
5. Implement app messages and reducers in the communication file.
6. Implement widget-local interaction in the internal logic file.
7. Implement layout, paint, hit, focus, scroll, text, and paint-order
   declarations in the view file.
8. Run the selected backend through the public facade/backend feature.
9. Use MCP/debug tools from the running backend window to inspect status,
   view/probe evidence, screenshot or
   render evidence, physical-control behavior, resize, and scroll.
10. Fix authoring gaps and repeat.

## Do Not Weaken The Goal

If the task asks for visual or interaction parity, do not replace it with a
smaller demo. A reduced app is useful only when the user explicitly asks for a
minimal API experiment.

## What To Compare

Compare at least:

- initial layout at the target viewport;
- resized layout at small and large viewports;
- scrollable regions;
- hover, selected, active, disabled, and focused states when present;
- text input behavior;
- popovers, overlays, and modal layer order;
- chart or custom-rendered regions;
- table/list row activation and sorting;
- source-state changes and displayed state.

## Report Gaps Precisely

Classify remaining gaps as:

- authoring error;
- missing public API contract;
- backend adapter bug;
- debug/MCP evidence gap;
- source UI ambiguity.

Do not report "done" because the app compiles. The app must be inspected
through the selected backend and debug evidence.
