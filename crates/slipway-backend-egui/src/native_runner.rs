use super::*;
use eframe::egui_winit::{SlipwayDebugInputEvent, SlipwayDebugKeyEvent};
use winit::dpi::PhysicalPosition;
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase};
use winit::event_loop::{ControlFlow, EventLoop};
use winit::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey, PhysicalKey};

pub(super) fn egui_events_for_native_physical_operation(
    operation: &DebugPhysicalControl,
    pixels_per_point: f32,
) -> Result<NativePhysicalControlPlan, NativePhysicalControlUnsupported> {
    match operation {
        DebugPhysicalControl::Pointer {
            position,
            kind,
            button,
            ..
        } => {
            let position = physical_position(position, pixels_per_point);
            let mut events = Vec::new();
            match kind {
                PointerEventKind::Move => {
                    events.push(SlipwayDebugInputEvent::CursorMoved { position });
                }
                PointerEventKind::Press | PointerEventKind::Release => {
                    events.push(SlipwayDebugInputEvent::CursorMoved { position });
                    events.push(SlipwayDebugInputEvent::MouseButton {
                        state: if matches!(kind, PointerEventKind::Press) {
                            ElementState::Pressed
                        } else {
                            ElementState::Released
                        },
                        button: button.map(winit_mouse_button).unwrap_or(MouseButton::Left),
                    });
                }
                PointerEventKind::Cancel => events.push(SlipwayDebugInputEvent::PointerGone),
                PointerEventKind::Enter | PointerEventKind::Leave => {
                    return Err(NativePhysicalControlUnsupported::new(
                        "native-physical-control-pointer-hover-unsupported",
                        "egui native runner derives pointer enter/leave from pointer movement; request a pointer move to the target position instead",
                    ));
                }
            }
            Ok(NativePhysicalControlPlan::Input(events))
        }
        DebugPhysicalControl::Wheel {
            position,
            delta_x,
            delta_y,
        } => Ok(NativePhysicalControlPlan::Input(vec![
            SlipwayDebugInputEvent::CursorMoved {
                position: physical_position(position, pixels_per_point),
            },
            SlipwayDebugInputEvent::MouseWheel {
                delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(
                    f64::from(*delta_x * pixels_per_point),
                    f64::from(*delta_y * pixels_per_point),
                )),
                phase: TouchPhase::Moved,
            },
        ])),
        DebugPhysicalControl::Text { text, .. } => Ok(NativePhysicalControlPlan::Input(vec![
            SlipwayDebugInputEvent::Keyboard(SlipwayDebugKeyEvent {
                physical_key: PhysicalKey::Unidentified(
                    winit::keyboard::NativeKeyCode::Unidentified,
                ),
                logical_key: Key::Character(text.clone().into()),
                text: Some(text.clone()),
                location: KeyLocation::Standard,
                state: ElementState::Pressed,
                repeat: false,
            }),
        ])),
        DebugPhysicalControl::Keyboard {
            key,
            kind,
            modifiers,
            details,
            ..
        } => {
            let Some((physical_key, logical_key)) = winit_key_for_native(key) else {
                return Err(NativePhysicalControlUnsupported::new(
                    "native-physical-control-key-unsupported",
                    "egui native runner only supports keyboard keys accepted by egui::Key::from_name",
                ));
            };
            Ok(NativePhysicalControlPlan::Input(vec![
                SlipwayDebugInputEvent::Modifiers(winit_modifiers(*modifiers)),
                SlipwayDebugInputEvent::Keyboard(SlipwayDebugKeyEvent {
                    physical_key,
                    logical_key,
                    text: details.text.clone(),
                    location: winit_key_location(details.location),
                    state: if matches!(kind, KeyEventKind::Press) {
                        ElementState::Pressed
                    } else {
                        ElementState::Released
                    },
                    repeat: details.repeat,
                }),
            ]))
        }
        DebugPhysicalControl::Command {
            command,
            payload_ref,
            ..
        } => {
            if payload_ref.is_some() {
                return Err(NativePhysicalControlUnsupported::new(
                    "native-physical-control-command-payload-unsupported",
                    "egui native command events do not carry Slipway payload_ref data through RawInput",
                ));
            }
            match command.as_str() {
                "copy" => Ok(NativePhysicalControlPlan::Input(vec![
                    SlipwayDebugInputEvent::Copy,
                ])),
                "cut" => Ok(NativePhysicalControlPlan::Input(vec![
                    SlipwayDebugInputEvent::Cut,
                ])),
                _ => Err(NativePhysicalControlUnsupported::new(
                    "native-physical-control-command-unsupported",
                    "egui RawInput exposes native copy/cut command events here; arbitrary Slipway commands require a separate command surface seam",
                )),
            }
        }
        DebugPhysicalControl::TextEdit { .. } => Err(NativePhysicalControlUnsupported::new(
            "native-physical-control-text-edit-unsupported",
            "egui TextEdit buffer mutation is backend widget state, not a RawInput event; use text/keyboard physical input or add a dedicated text-edit native seam",
        )),
        DebugPhysicalControl::Focus { .. } => Ok(NativePhysicalControlPlan::BackendNativeMutation),
        DebugPhysicalControl::Scroll { .. } => Ok(NativePhysicalControlPlan::BackendNativeMutation),
        DebugPhysicalControl::TextComposition {
            updates, commit, ..
        } => {
            let mut events = Vec::with_capacity(updates.len() + 1);
            for update in updates {
                let cursor_range = match update.cursor_range.as_ref() {
                    Some(range) => Some((
                        char_index_to_byte(&update.preedit_text, range.anchor).ok_or_else(|| {
                            NativePhysicalControlUnsupported::new(
                                "native-physical-control-text-composition-range-invalid",
                                "egui composition cursor ranges must identify Unicode scalar boundaries",
                            )
                        })?,
                        char_index_to_byte(&update.preedit_text, range.focus).ok_or_else(|| {
                            NativePhysicalControlUnsupported::new(
                                "native-physical-control-text-composition-range-invalid",
                                "egui composition cursor ranges must identify Unicode scalar boundaries",
                            )
                        })?,
                    )),
                    None => None,
                };
                events.push(SlipwayDebugInputEvent::Ime(Ime::Preedit(
                    update.preedit_text.clone(),
                    cursor_range,
                )));
            }
            events.push(SlipwayDebugInputEvent::Ime(Ime::Commit(commit.clone())));
            Ok(NativePhysicalControlPlan::Input(events))
        }
    }
}

#[derive(Debug)]
pub(super) enum NativePhysicalControlPlan {
    Input(Vec<SlipwayDebugInputEvent>),
    BackendNativeMutation,
}

#[cfg(test)]
pub(super) fn egui_test_events_for_native_physical_operation(
    operation: &DebugPhysicalControl,
    raw_input: &egui::RawInput,
) -> Result<Vec<egui::Event>, NativePhysicalControlUnsupported> {
    match operation {
        DebugPhysicalControl::Pointer {
            position,
            kind,
            button,
            ..
        } => {
            let pos = egui::pos2(position.x, position.y);
            match kind {
                PointerEventKind::Move => Ok(vec![egui::Event::PointerMoved(pos)]),
                PointerEventKind::Press | PointerEventKind::Release => Ok(vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: match button.unwrap_or(PointerButton::Primary) {
                            PointerButton::Primary => egui::PointerButton::Primary,
                            PointerButton::Secondary => egui::PointerButton::Secondary,
                            PointerButton::Auxiliary => egui::PointerButton::Middle,
                        },
                        pressed: matches!(kind, PointerEventKind::Press),
                        modifiers: raw_input.modifiers,
                    },
                ]),
                PointerEventKind::Cancel => Ok(vec![egui::Event::PointerGone]),
                PointerEventKind::Enter | PointerEventKind::Leave => {
                    Err(NativePhysicalControlUnsupported::new(
                        "native-physical-control-pointer-hover-unsupported",
                        "egui native runner derives pointer enter/leave from pointer movement",
                    ))
                }
            }
        }
        DebugPhysicalControl::Wheel {
            position,
            delta_x,
            delta_y,
        } => Ok(vec![
            egui::Event::PointerMoved(egui::pos2(position.x, position.y)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(*delta_x, *delta_y),
                phase: egui::TouchPhase::Move,
                modifiers: raw_input.modifiers,
            },
        ]),
        DebugPhysicalControl::Text { text, .. } => Ok(vec![egui::Event::Text(text.clone())]),
        DebugPhysicalControl::Keyboard {
            key,
            kind,
            modifiers,
            details,
            ..
        } => {
            let Some(key) = egui::Key::from_name(key) else {
                return Err(NativePhysicalControlUnsupported::new(
                    "native-physical-control-key-unsupported",
                    "egui native runner only supports keyboard keys accepted by egui",
                ));
            };
            Ok(vec![egui::Event::Key {
                key,
                physical_key: Some(key),
                pressed: matches!(kind, KeyEventKind::Press),
                repeat: details.repeat,
                modifiers: egui::Modifiers {
                    alt: modifiers.alt,
                    ctrl: modifiers.control,
                    shift: modifiers.shift,
                    mac_cmd: modifiers.meta,
                    command: modifiers.control || modifiers.meta,
                },
            }])
        }
        DebugPhysicalControl::Command {
            command,
            payload_ref,
            ..
        } if payload_ref.is_none() && command == "copy" => Ok(vec![egui::Event::Copy]),
        DebugPhysicalControl::Command {
            command,
            payload_ref,
            ..
        } if payload_ref.is_none() && command == "cut" => Ok(vec![egui::Event::Cut]),
        DebugPhysicalControl::Command {
            payload_ref: Some(_),
            ..
        } => Err(NativePhysicalControlUnsupported::new(
            "native-physical-control-command-payload-unsupported",
            "egui native command events do not carry Slipway payload data",
        )),
        DebugPhysicalControl::TextComposition {
            updates, commit, ..
        } => {
            let mut events = Vec::with_capacity(updates.len() + 1);
            for update in updates {
                events.push(egui::Event::Ime(egui::ImeEvent::Preedit {
                    text: update.preedit_text.clone(),
                    active_range_chars: update.cursor_range.as_ref().map(|range| std::ops::Range {
                        start: range.anchor,
                        end: range.focus,
                    }),
                }));
            }
            events.push(egui::Event::Ime(egui::ImeEvent::Commit(commit.clone())));
            Ok(events)
        }
        DebugPhysicalControl::Focus { .. } | DebugPhysicalControl::Scroll { .. } => Ok(Vec::new()),
        DebugPhysicalControl::TextEdit { .. } => Err(NativePhysicalControlUnsupported::new(
            "native-physical-control-text-edit-unsupported",
            "egui TextEdit buffer mutation is backend widget state",
        )),
        DebugPhysicalControl::Command { .. } => Err(NativePhysicalControlUnsupported::new(
            "native-physical-control-command-unsupported",
            "egui native command is unsupported",
        )),
    }
}

#[derive(Debug)]
pub(super) struct NativePhysicalControlUnsupported {
    pub code: &'static str,
    pub message: &'static str,
}

impl NativePhysicalControlUnsupported {
    fn new(code: &'static str, message: &'static str) -> Self {
        Self { code, message }
    }
}

pub(super) fn run_slipway_egui_runtime_app_native<W, B, F>(
    title: &str,
    app: SlipwayEguiRuntimeApp<W, B, F>,
) -> eframe::Result<()>
where
    W: SlipwayEguiBackendWidget + 'static,
    W::ExternalState: 'static,
    W::LocalState: Clone + 'static,
    W::AppMessage: 'static,
    B: EguiSlipwayBridge<W> + 'static,
    F: FnMut(&mut W::ExternalState, Vec<W::AppMessage>) + 'static,
{
    let event_loop = EventLoop::<eframe::UserEvent>::with_user_event().build()?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = app;
    app.install_native_debug_proxy(eframe::NativeDebugProxy::new(&event_loop));
    app.mark_native_create_started();
    let eframe_app = eframe::create_native(
        title,
        eframe::NativeOptions {
            renderer: eframe::Renderer::Wgpu,
            ..Default::default()
        },
        Box::new(move |creation_context| {
            let mut app = app;
            app.record_native_create_phase();
            app.ensure_mcp_wake_forwarder(&creation_context.egui_ctx);
            app.prewarm_native_visible_cache(&creation_context.egui_ctx);
            Ok(Box::new(app))
        }),
        &event_loop,
    );
    let mut app = eframe_app;
    event_loop.run_app(&mut app)?;
    Ok(())
}

fn physical_position(position: &Point, pixels_per_point: f32) -> PhysicalPosition<f64> {
    PhysicalPosition::new(
        f64::from(position.x * pixels_per_point),
        f64::from(position.y * pixels_per_point),
    )
}

fn winit_mouse_button(button: PointerButton) -> MouseButton {
    match button {
        PointerButton::Primary => MouseButton::Left,
        PointerButton::Secondary => MouseButton::Right,
        PointerButton::Auxiliary => MouseButton::Middle,
    }
}

fn winit_modifiers(modifiers: Modifiers) -> ModifiersState {
    let mut state = ModifiersState::empty();
    state.set(ModifiersState::ALT, modifiers.alt);
    state.set(ModifiersState::CONTROL, modifiers.control);
    state.set(ModifiersState::SHIFT, modifiers.shift);
    state.set(ModifiersState::SUPER, modifiers.meta);
    state
}

fn winit_key_location(location: slipway_core::KeyLocation) -> KeyLocation {
    match location {
        slipway_core::KeyLocation::Standard | slipway_core::KeyLocation::Unknown => {
            KeyLocation::Standard
        }
        slipway_core::KeyLocation::Left => KeyLocation::Left,
        slipway_core::KeyLocation::Right => KeyLocation::Right,
        slipway_core::KeyLocation::Numpad => KeyLocation::Numpad,
    }
}

fn winit_key_for_native(key: &str) -> Option<(PhysicalKey, Key)> {
    let normalized = key.trim();
    if normalized.chars().count() == 1 {
        let character = normalized.chars().next()?;
        let upper = character.to_ascii_uppercase();
        let code = match upper {
            'A' => KeyCode::KeyA,
            'B' => KeyCode::KeyB,
            'C' => KeyCode::KeyC,
            'D' => KeyCode::KeyD,
            'E' => KeyCode::KeyE,
            'F' => KeyCode::KeyF,
            'G' => KeyCode::KeyG,
            'H' => KeyCode::KeyH,
            'I' => KeyCode::KeyI,
            'J' => KeyCode::KeyJ,
            'K' => KeyCode::KeyK,
            'L' => KeyCode::KeyL,
            'M' => KeyCode::KeyM,
            'N' => KeyCode::KeyN,
            'O' => KeyCode::KeyO,
            'P' => KeyCode::KeyP,
            'Q' => KeyCode::KeyQ,
            'R' => KeyCode::KeyR,
            'S' => KeyCode::KeyS,
            'T' => KeyCode::KeyT,
            'U' => KeyCode::KeyU,
            'V' => KeyCode::KeyV,
            'W' => KeyCode::KeyW,
            'X' => KeyCode::KeyX,
            'Y' => KeyCode::KeyY,
            'Z' => KeyCode::KeyZ,
            '0' => KeyCode::Digit0,
            '1' => KeyCode::Digit1,
            '2' => KeyCode::Digit2,
            '3' => KeyCode::Digit3,
            '4' => KeyCode::Digit4,
            '5' => KeyCode::Digit5,
            '6' => KeyCode::Digit6,
            '7' => KeyCode::Digit7,
            '8' => KeyCode::Digit8,
            '9' => KeyCode::Digit9,
            _ => return None,
        };
        return Some((
            PhysicalKey::Code(code),
            Key::Character(normalized.to_string().into()),
        ));
    }

    let (code, named) = match normalized.to_ascii_lowercase().as_str() {
        "enter" => (KeyCode::Enter, NamedKey::Enter),
        "tab" => (KeyCode::Tab, NamedKey::Tab),
        "space" => (KeyCode::Space, NamedKey::Space),
        "escape" | "esc" => (KeyCode::Escape, NamedKey::Escape),
        "backspace" => (KeyCode::Backspace, NamedKey::Backspace),
        "delete" => (KeyCode::Delete, NamedKey::Delete),
        "arrowup" => (KeyCode::ArrowUp, NamedKey::ArrowUp),
        "arrowdown" => (KeyCode::ArrowDown, NamedKey::ArrowDown),
        "arrowleft" => (KeyCode::ArrowLeft, NamedKey::ArrowLeft),
        "arrowright" => (KeyCode::ArrowRight, NamedKey::ArrowRight),
        "home" => (KeyCode::Home, NamedKey::Home),
        "end" => (KeyCode::End, NamedKey::End),
        "pageup" => (KeyCode::PageUp, NamedKey::PageUp),
        "pagedown" => (KeyCode::PageDown, NamedKey::PageDown),
        _ => return None,
    };
    Some((PhysicalKey::Code(code), Key::Named(named)))
}

fn char_index_to_byte(text: &str, index: usize) -> Option<usize> {
    if index == text.chars().count() {
        return Some(text.len());
    }
    text.char_indices().nth(index).map(|(byte, _)| byte)
}
