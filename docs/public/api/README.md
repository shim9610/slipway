# Public API Guide

The API guide is split by responsibility so an LLM worker can read only the
part needed for the current task.

Most users depend on the facade crate:

```powershell
cargo add slipway --git https://github.com/shim9610/slipway.git --features iced
```

Backend choice is the feature split. Do not add separate `slipway-core`,
`slipway-runtime`, or backend crate dependencies unless you are deliberately
working on the Slipway crates themselves.

LLM workers should read [LLM contract checklist](../llm-contract-checklist.md)
before choosing an API surface. If a type is not reachable from
`slipway::prelude::*`, first decide whether the task is ordinary app authoring,
backend-native wrapping, provider insertion, or a public API gap.

## Files

- [Core API](core.md) - backend-neutral identity, state, logic, view,
  geometry, declarations, and event evidence.
- [Routing and scroll](routing-and-scroll.md) - scroll region declaration,
  `HitRegionOrder`, wheel routing modes, chaining, and wheel-transparent
  overlays.
- [Diagnostics catalog](diagnostics.md) - every admission/runtime diagnostic
  code with trigger and fix, plus the pre-flight validation call.
- [Trait surface](trait-surface.md) - load-bearing vs RESERVED status for
  every policy trait and capability bundle; `reserved_policy_defaults!`.
- [Backend API](backends.md) - iced/egui adapter gates, backend-specific
  wrappers, visible backend rules, and backend switching expectations.
- [IME and Korean text input](ime.md) - platform IME policy, Hangul input
  expectations, text-input visual/typography token contracts, and debug
  checklist.
- [Debug MCP](debug-mcp.md) - request-scoped debug tools, physical-control
  meaning, and frame identity.
- [Provider surfaces](provider-surfaces.md) - canvas, plot, media, GPU, and
  already-owned renderer insertion.

## How To Choose

- Unsure whether an API is allowed: read
  [LLM contract checklist](../llm-contract-checklist.md).
- Writing normal widgets or app state: read [Core API](core.md).
- Declaring scrolling, overlapping regions, or overlays: read
  [Routing and scroll](routing-and-scroll.md).
- Admission refused the view, or a diagnostic code needs reading: read
  [Diagnostics catalog](diagnostics.md).
- A trait bound looks unused, or a bundle error triages RESERVED bounds:
  read [Trait surface](trait-surface.md).
- Running on iced or egui: read [Backend API](backends.md).
- Enabling Korean/Hangul text input: read [IME and Korean text input](ime.md).
- Testing with debug/control/screenshot/probe evidence: read
  [Debug MCP](debug-mcp.md).
- Inserting an existing chart, canvas, or GPU renderer: read
  [Provider surfaces](provider-surfaces.md).

If a task touches more than one area, read the files in that order.

## Finding The Full Contract

When these pages end and a contract detail is still missing, the sanctioned
next step is the API reference itself — not private project notes, not old
evaluation crates, not git history:

1. **Rustdoc:** `cargo doc -p slipway-core --no-deps --open` builds the full
   reference for the backend-neutral surface (`-p slipway` for the facade).
2. **Grep-then-ranged-read of the source of truth:** the entire
   backend-neutral contract lives in one file,
   `crates/slipway-core/src/lib.rs`. Every load-bearing name in these docs
   is a greppable identifier there: diagnostic codes
   (`view_contract.ambiguous_wheel_overlap`), helper names
   (`scroll_region_from_scrollable_capability_with_order`), trait names,
   and struct fields. Grep the symbol, then read the surrounding range;
   do not read the file end to end.
3. **The RESERVED convention:** `grep "RESERVED"` in that file enumerates
   the declared-ahead-of-consumption surface; see
   [Trait surface](trait-surface.md).

If neither rustdoc nor the `slipway-core` source answers the question, the
operation is genuinely undocumented: report `PUBLIC_DOC_GAP` with the
missing operation, per the
[LLM contract checklist](../llm-contract-checklist.md). Reading
`docs/agents` or evaluation-crate history remains out of bounds for app
authoring.
