# IME And Korean Text Input

Slipway does not implement a Korean IME. The operating system, winit, and the
selected backend own composition, candidate windows, language switching, and
committed text events.

Slipway provides the runtime policy that lets a backend window keep platform IME
input allowed when the app needs native text editing.

## Iced Runtime Policy

For Korean/Hangul text input on iced, prefer an explicit runtime config:

```rust
use slipway::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config = SlipwayRuntimeConfig::admitted_debug()
        .with_platform_ime_always_allowed();

    run_slipway_iced_runtime_app_with_config(
        SlipwayAppWidget::new(MyApp::new()),
        MyExternalState::default(),
        apply_messages,
        config,
    )?;

    Ok(())
}
```

`with_platform_ime_always_allowed()` maps to
`SlipwayImePolicy::AlwaysAllowed`. The iced native runner calls
`set_ime_allowed(true)` on the winit window. When iced temporarily reports
`InputMethod::Disabled`, the runner keeps platform IME allowed and does not
clear preedit or focused-text IME state. This avoids interrupting active Windows
Hangul composition between preedit and commit events.

For iced, Slipway draws an inline preedit overlay only while the backend reports
active composition text. The overlay uses the explicit
`TextInputVisualStyleDeclaration::preedit_color` from the active text-edit
declaration, the explicit `TextInputTypographyDeclaration`, and the IME cursor
rectangle. It does not paint a popup background box. Slipway still leaves
composition semantics to the platform IME and backend text widget; committed
text is routed through normal text-edit events.

On Windows, the iced native runner may load an available Korean-capable system
font as a compatibility fallback. This is not the application style contract.
Applications that need reliable Korean/CJK text should declare their font token
and source explicitly.

Use the default `SlipwayImePolicy::BackendRequested` only when the backend
should enable IME strictly from focused text widgets.

The application author must create an explicit design-token or theme module for
text input visuals and typography, then implement both
`SlipwayTextInputVisualStylePolicy` and `SlipwayTextInputTypographyPolicy` on
every widget that declares text input. Without those policies, the widget does
not satisfy `SlipwayTextInputCapability`.

The style policy converts the author's tokens into
`TextInputVisualStyleDeclaration`, which carries value, placeholder, preedit,
selection, background, border, and icon colors plus border metrics. The
typography policy converts the author's tokens into
`TextInputTypographyDeclaration`, which carries `TextStyle` and may carry a font
source. `SlipwayFontResolutionPolicy` resolves that source for the selected
backend. At the app root, `SlipwayAppWidget` provides that policy by
delegating to `SlipwayApp::resolve_app_font`; the default refuses honestly
(`app-font-resolution-refused`) — override it only when the app declares a
loadable font source, and never claim a resolution that was not validated.

Widget local state should only choose variants such as focused, disabled,
selected, invalid, read-only, compact, or large. Use the builder-style override
helpers such as `TextStyle::with_font_size(...)` and
`TextInputTypographyDeclaration::with_font_size(...)` when a local variant only
changes one or two fields; do not duplicate a whole token just to tweak one
size.

## What This Guarantees

- Slipway exposes a public policy for keeping the platform IME allowed.
- The iced native runner applies that policy at window creation.
- The iced native runner does not toggle platform IME association while Hangul
  composition is active.
- The iced native runner displays active preedit text at the backend cursor
  rectangle with the active text input's explicit `preedit_color`, font, and
  size, and no separate popup background.
- Text input widgets must provide explicit visual and typography policies.
- egui text edits use the declared typography for native `TextEdit` font
  selection and declared font installation evidence.
- Focused native iced text inputs can receive `Ime::Preedit` and `Ime::Commit`
  events from winit when the OS sends them.
- MCP text controls can still verify committed text routing through the same
  declared text-edit target.

## What This Does Not Guarantee

- Slipway does not synthesize Hangul composition itself.
- Slipway does not replace the OS language switcher or IME candidate UI.
- If the OS/winit window never emits `Ime::Commit`, Slipway cannot infer Korean
  syllables from raw key strokes without becoming a custom IME.
- Native block selection, caret painting, and candidate windows remain backend
  owned unless exposed later as debug evidence.

## Debug Checklist

If Hangul input does not work:

1. Run with `SlipwayRuntimeConfig::admitted_debug().with_platform_ime_always_allowed()`.
2. Focus a declared text-edit region backed by a native text input.
3. Confirm MCP `text_edit replace_buffer` works for the same target.
4. Check whether the backend receives `Ime::Commit` events. If it receives only
   key events such as `HangulMode`, the failure is in OS/winit IME delivery, not
   in Slipway text-buffer routing.
