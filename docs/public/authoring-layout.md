# Authoring Layout

Slipway makes UI work inspectable by forcing the major responsibilities to stay
separate. The file split is for humans and LLMs; the exact module names may
vary, but every authored app should have the same roles.

## File Map

```text
src/
  ssot.rs
  internal_logic.rs
  communication.rs
  view.rs
  app_runner.rs
```

## `ssot.rs`

Owns stable source-of-truth declarations.

Put here:

- widget and app ids;
- app state and stable semantic data;
- capability declarations;
- topology names;
- source-derived constants.

Do not put here:

- backend framework types;
- reducers;
- widget-local mutation;
- paint bodies;
- MCP transport logic.

## `internal_logic.rs`

Owns behavior inside one widget instance.

Put here:

- event handlers for a widget;
- widget-local state transitions;
- emitted messages;
- local validation and change evidence.

Do not put here:

- sibling state reads or writes;
- app-wide reducers;
- backend drawing callbacks.

## `communication.rs`

Owns app-level coordination.

Put here:

- app message enums;
- parent reducers;
- child output to app message mapping;
- app state projection into child inputs;
- inter-widget communication policy.

The intended flow is:

```text
child event -> typed child/app message -> parent reducer -> projected child input
```

## `view.rs`

Owns view declarations and presentation-local state.

Put here:

- widget local-state structs when they are presentation/internal state;
- layout declarations;
- paint declarations;
- hit, focus, scroll, text, and command declarations;
- paint order, overflow, resize, and responsive declarations;
- state observation hooks for debug requests.

Do not put app reducers or sibling communication here.

## `app_runner.rs`

Owns assembly and launch.

Put here:

- app composition;
- runtime construction;
- backend selection;
- default debug MCP attachment;
- feature gates for debug/release behavior.

Do not put widget semantics or paint bodies here.

## Why This Split Matters

The split prevents a user-side LLM from hiding behavior in one convenient root
object. It also gives debug tools useful names: a failed click, scroll, resize,
or text edit can be tied to a widget id, route, state field, and source file
role.
