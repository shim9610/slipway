use super::*;

#[derive(Debug)]
pub(super) enum NativePhysicalControlPlan {
    BackendNativeMutation,
}

pub(super) fn egui_events_for_native_physical_operation(
    operation: &DebugPhysicalControl,
    _pixels_per_point: f32,
) -> Result<NativePhysicalControlPlan, NativePhysicalControlUnsupported> {
    match operation {
        DebugPhysicalControl::Focus { .. } | DebugPhysicalControl::Scroll { .. } => {
            Ok(NativePhysicalControlPlan::BackendNativeMutation)
        }
        DebugPhysicalControl::Pointer { .. }
        | DebugPhysicalControl::Wheel { .. }
        | DebugPhysicalControl::Text { .. }
        | DebugPhysicalControl::Keyboard { .. }
        | DebugPhysicalControl::Command { .. }
        | DebugPhysicalControl::TextEdit { .. }
        | DebugPhysicalControl::TextComposition { .. } => {
            Err(NativePhysicalControlUnsupported::new(
                "native-physical-control-ingress-unavailable",
                "standard eframe does not expose request-scoped native input injection; use semantic debug control or canonical/offscreen evidence",
            ))
        }
    }
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
                        "egui derives pointer enter/leave from pointer movement",
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
                    "egui test ingress only supports keys accepted by egui",
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
    let mut app = app;
    app.mark_native_create_started();
    eframe::run_native(
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
    )
}
