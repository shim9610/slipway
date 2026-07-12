# slipway-example-admission — internal stress fixture

**This crate is NOT the authoring template.** Do not copy it to write a
Slipway app. The designated copy source is
[`crates/slipway-example-authored`](../slipway-example-authored/) — the
facade-based five-file reference example. This crate was demoted from
example duty by the LLM-ergonomics audit (2026-07-11, findings
LE-H2/LE-H6; `docs/agents/audits/2026-07-11-llm-ergonomics-audit.md`)
because it deliberately contradicts the documented authoring rules: a
single 7,000+-line `main.rs` with direct `slipway_core` /
`slipway_runtime` / backend-crate imports instead of the
`slipway::prelude` facade and the five-file split.

## Real role (why it stays)

* **Admission stress harness** — nine widget kinds exercising action,
  segment, text-edit, toggle, slider, list scroll, movable overlay,
  overlay-stack z-order, and triple-nested-scroll admission paths in one
  app.
* **Load-bearing regression fixture** — its 45 tests pin declaration and
  dispatch behavior; step-packet procedures under `docs/agents/steps/`
  reference them.
* **Debug-MCP live-verification target** — drivers and step packets
  launch `slipway-example-admission --iced|--egui` by name.

Because drivers and recorded procedures reference this crate by name and
shape, do not rename, move, or restructure it; keep its tests and launch
behavior stable.

## If you are authoring an app

Read `docs/public/llm-entry.md` and
`docs/public/quickstart-authoring.md`, then copy
`crates/slipway-example-authored`.
