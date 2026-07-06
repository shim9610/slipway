use super::*;
use winit::event_loop::{ControlFlow, EventLoop};

pub(super) fn egui_events_for_native_physical_operation(
    operation: &DebugPhysicalControl,
    raw_input: &egui::RawInput,
) -> Result<NativePhysicalControlPlan, NativePhysicalControlUnsupported> {
    match operation {
        DebugPhysicalControl::Pointer {
            position,
            kind,
            button,
            ..
        } => {
            let pos = egui_pos(position);
            let mut events = Vec::new();
            match kind {
                PointerEventKind::Move => events.push(egui::Event::PointerMoved(pos)),
                PointerEventKind::Press | PointerEventKind::Release => {
                    events.push(egui::Event::PointerMoved(pos));
                    events.push(egui::Event::PointerButton {
                        pos,
                        button: button
                            .map(egui_pointer_button_for_native)
                            .unwrap_or(egui::PointerButton::Primary),
                        pressed: matches!(kind, PointerEventKind::Press),
                        modifiers: raw_input.modifiers,
                    });
                }
                PointerEventKind::Cancel => events.push(egui::Event::PointerGone),
                PointerEventKind::Enter | PointerEventKind::Leave => {
                    return Err(NativePhysicalControlUnsupported::new(
                        "native-physical-control-pointer-hover-unsupported",
                        "egui native runner derives pointer enter/leave from pointer movement; request a pointer move to the target position instead",
                    ));
                }
            }
            Ok(NativePhysicalControlPlan::RawInputEvents(events))
        }
        DebugPhysicalControl::Wheel {
            position,
            delta_x,
            delta_y,
        } => Ok(NativePhysicalControlPlan::RawInputEvents(vec![
            egui::Event::PointerMoved(egui_pos(position)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(*delta_x, *delta_y),
                phase: egui::TouchPhase::Move,
                modifiers: raw_input.modifiers,
            },
        ])),
        DebugPhysicalControl::Text { text, .. } => {
            Ok(NativePhysicalControlPlan::RawInputEvents(vec![
                egui::Event::Text(text.clone()),
            ]))
        }
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
                    "egui native runner only supports keyboard keys accepted by egui::Key::from_name",
                ));
            };
            let modifiers = egui_modifiers_for_native(*modifiers);
            Ok(NativePhysicalControlPlan::RawInputEvents(vec![
                egui::Event::Key {
                    key,
                    physical_key: Some(key),
                    pressed: matches!(kind, KeyEventKind::Press),
                    repeat: details.repeat,
                    modifiers,
                },
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
                "copy" => Ok(NativePhysicalControlPlan::RawInputEvents(vec![
                    egui::Event::Copy,
                ])),
                "cut" => Ok(NativePhysicalControlPlan::RawInputEvents(vec![
                    egui::Event::Cut,
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
    }
}

#[derive(Debug)]
pub(super) enum NativePhysicalControlPlan {
    RawInputEvents(Vec<egui::Event>),
    BackendNativeMutation,
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
    app.mark_native_create_started();
    let eframe_app = eframe::create_native(
        title,
        eframe::NativeOptions::default(),
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

fn egui_pos(position: &Point) -> egui::Pos2 {
    egui::pos2(position.x, position.y)
}

fn egui_pointer_button_for_native(button: PointerButton) -> egui::PointerButton {
    match button {
        PointerButton::Primary => egui::PointerButton::Primary,
        PointerButton::Secondary => egui::PointerButton::Secondary,
        PointerButton::Auxiliary => egui::PointerButton::Middle,
    }
}

fn egui_modifiers_for_native(modifiers: Modifiers) -> egui::Modifiers {
    egui::Modifiers {
        alt: modifiers.alt,
        ctrl: modifiers.control,
        shift: modifiers.shift,
        mac_cmd: modifiers.meta,
        command: modifiers.control || modifiers.meta,
    }
}
